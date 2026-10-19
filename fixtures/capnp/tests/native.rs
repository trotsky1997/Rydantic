use rydantic::{capnp, CapnpModel, Model, ValidationError};
use rydantic_capnp_fixture::{
    example_person, wire_capnp, Address, Empty, Event, Number, Pair, Person, ScalarText,
};

#[test]
fn list_lengths_respect_the_native_29_bit_element_limit() {
    let mut ctx = rydantic::Context::new();
    assert_eq!(
        rydantic::capnp_support::list_len((1 << 29) - 1, &mut ctx),
        Some((1 << 29) - 1)
    );
    assert_eq!(rydantic::capnp_support::list_len(1 << 29, &mut ctx), None);
    let error = ctx.result().unwrap_err();
    assert_eq!(error.errors()[0].code, "capnp_length");
}

#[test]
fn native_and_packed_roundtrip_cover_nested_lists_maps_options_and_scalars() {
    let person = example_person();
    assert_eq!(
        Person::parse_capnp(&person.dump_capnp().unwrap()).unwrap(),
        person
    );
    assert_eq!(
        Person::parse_capnp_packed(&person.dump_capnp_packed().unwrap()).unwrap(),
        person
    );
    let mut other = example_person();
    other.nickname = None;
    other.alternative = Some(Address {
        city: "Oslo".into(),
        zip: 1,
    });
    assert_eq!(
        Person::parse_capnp(&other.dump_capnp().unwrap()).unwrap(),
        other
    );
    // Unaligned slices are accepted by the native runtime feature.
    let bytes = person.dump_capnp().unwrap();
    let mut unaligned = vec![0];
    unaligned.extend_from_slice(&bytes);
    assert_eq!(Person::parse_capnp(&unaligned[1..]).unwrap(), person);
}
#[test]
fn official_readers_and_builders_interoperate_directly() {
    let person = example_person();
    let message = person.to_capnp_message().unwrap();
    let reader = message
        .get_root_as_reader::<wire_capnp::person::Reader<'_>>()
        .unwrap();
    assert_eq!(reader.get_full_name().unwrap().to_str().unwrap(), "Alice");
    assert_eq!(reader.get_avatar().unwrap(), [0, 255, 13]);
    assert_eq!(Person::from_capnp_reader(reader).unwrap(), person);
    let mut message = capnp::message::Builder::new_default();
    message
        .init_root::<wire_capnp::number::Builder<'_>>()
        .set_value(u64::MAX);
    assert_eq!(
        Number::parse_capnp(&capnp::serialize::write_message_to_words(&message)).unwrap(),
        Number(u64::MAX)
    );
}
#[test]
fn native_ingress_normalizes_and_checks_constraints_with_paths() {
    let mut message = example_person().to_capnp_message().unwrap();
    {
        let mut root = message
            .get_root::<wire_capnp::person::Builder<'_>>()
            .unwrap();
        root.set_full_name(" Alice ");
        root.reborrow().get_address().unwrap().set_city(" Paris ");
    }
    let parsed = Person::parse_capnp(&capnp::serialize::write_message_to_words(&message)).unwrap();
    assert_eq!(parsed.name, "Alice");
    assert_eq!(parsed.address.city, "Paris");
    message
        .get_root::<wire_capnp::person::Builder<'_>>()
        .unwrap()
        .set_age(17);
    let error =
        Person::parse_capnp(&capnp::serialize::write_message_to_words(&message)).unwrap_err();
    assert!(error.errors().iter().any(|e| e.pointer() == "/age"));
    let mut person = example_person();
    person.age = 1;
    assert!(person.dump_capnp().is_err());
}
#[test]
fn wire_and_model_numeric_ranges_are_checked() {
    let mut person = example_person();
    person.token = u128::MAX;
    let error = person.dump_capnp().unwrap_err();
    assert_eq!(error.errors()[0].pointer(), "/token");
    assert_eq!(error.errors()[0].code, "capnp_range");
    let mut message = example_person().to_capnp_message().unwrap();
    message
        .get_root::<wire_capnp::person::Builder<'_>>()
        .unwrap()
        .set_age(300);
    let error =
        Person::parse_capnp(&capnp::serialize::write_message_to_words(&message)).unwrap_err();
    assert_eq!(error.errors()[0].pointer(), "/age");
    assert_eq!(error.errors()[0].code, "capnp_range");
}
#[test]
fn unions_newtypes_tuples_and_units_have_native_representations() {
    for event in [
        Event::Ping,
        Event::Name("hello".into()),
        Event::Moved(Address {
            city: "Rome".into(),
            zip: 2,
        }),
        Event::Login {
            user: "alice".into(),
            count: 4,
        },
    ] {
        assert_eq!(
            Event::parse_capnp(&event.dump_capnp().unwrap()).unwrap(),
            event
        );
        assert_eq!(
            Event::parse_capnp_packed(&event.dump_capnp_packed().unwrap()).unwrap(),
            event
        );
    }
    let pair = Pair("pair".into(), -12);
    assert_eq!(
        Pair::parse_capnp(&pair.dump_capnp().unwrap()).unwrap(),
        pair
    );
    assert_eq!(
        Empty::parse_capnp(&Empty.dump_capnp().unwrap()).unwrap(),
        Empty
    );
}
#[test]
fn malformed_truncated_trailing_and_excessive_traversal_inputs_fail() {
    let bytes = example_person().dump_capnp().unwrap();
    for length in [0, 1, 7, bytes.len() - 1] {
        assert!(Person::parse_capnp(&bytes[..length]).is_err());
    }
    let mut extra = bytes.clone();
    extra.extend_from_slice(&[0; 8]);
    assert_eq!(
        Person::parse_capnp(&extra).unwrap_err().errors()[0].code,
        "capnp_trailing"
    );
    let mut packed = example_person().dump_capnp_packed().unwrap();
    packed.push(0);
    assert!(Person::parse_capnp_packed(&packed).is_err());
    let mut options = capnp::message::ReaderOptions::new();
    options.traversal_limit_in_words = Some(1);
    assert!(Person::parse_capnp_with_options(&bytes, options).is_err());
    // A reserved discriminant must become a structured error.
    let mut event = Event::Ping.dump_capnp().unwrap();
    event[16] = 255;
    event[17] = 255;
    assert!(Event::parse_capnp(&event).is_err());
}
#[test]
fn maps_reject_duplicate_keys_and_arrays_reject_wrong_lengths() {
    let mut message = example_person().to_capnp_message().unwrap();
    {
        let mut root = message
            .get_root::<wire_capnp::person::Builder<'_>>()
            .unwrap();
        let mut entries = root.reborrow().init_labels(2);
        for i in 0..2 {
            let mut entry = entries.reborrow().get(i);
            entry.set_key("duplicate");
            entry.set_value("x");
        }
    }
    let error =
        Person::parse_capnp(&capnp::serialize::write_message_to_words(&message)).unwrap_err();
    assert!(error
        .errors()
        .iter()
        .any(|e| e.code == "capnp_duplicate_key" && e.pointer() == "/labels/duplicate"));
    message
        .get_root::<wire_capnp::person::Builder<'_>>()
        .unwrap()
        .init_coordinates(1);
    let error =
        Person::parse_capnp(&capnp::serialize::write_message_to_words(&message)).unwrap_err();
    assert!(error
        .errors()
        .iter()
        .any(|e| e.code == "capnp_length" && e.pointer() == "/coordinates"));
}
#[test]
fn dates_and_decimal_use_explicit_native_text_fields() {
    let value = ScalarText {
        date: rydantic::chrono::NaiveDate::from_ymd_opt(2026, 10, 19).unwrap(),
        amount: "1234567890.0123456789".parse().unwrap(),
    };
    assert_eq!(
        ScalarText::parse_capnp(&value.dump_capnp().unwrap()).unwrap(),
        value
    );
}
async fn available(value: &String) -> Result<(), ValidationError> {
    tokio::task::yield_now().await;
    if value == "taken" {
        Err(ValidationError::new("taken", "name is registered"))
    } else {
        Ok(())
    }
}
#[derive(Debug, Model)]
#[model(capnp = "wire_capnp::async_user")]
struct AsyncUser {
    #[model(trim,async_validate=available)]
    name: String,
}
#[tokio::test]
async fn async_validation_is_required_awaited_and_send() {
    let mut message = capnp::message::Builder::new_default();
    message
        .init_root::<wire_capnp::async_user::Builder<'_>>()
        .set_name(" taken ");
    let bytes = capnp::serialize::write_message_to_words(&message);
    assert_eq!(
        AsyncUser::parse_capnp(&bytes).unwrap_err().errors()[0].code,
        "async_validation_required"
    );
    let error = tokio::spawn(async move { AsyncUser::parse_capnp_async(&bytes).await })
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(error.errors()[0].pointer(), "/name");
    assert_eq!(error.errors()[0].code, "taken");
    let user = AsyncUser {
        name: "free".into(),
    };
    assert!(user.dump_capnp().is_err());
    let bytes = user.dump_capnp_async().await.unwrap();
    assert_eq!(
        AsyncUser::parse_capnp_async(&bytes).await.unwrap().name,
        "free"
    );
    let bytes = user.dump_capnp_packed_async().await.unwrap();
    assert_eq!(
        AsyncUser::parse_capnp_packed_async(&bytes)
            .await
            .unwrap()
            .name,
        "free"
    );
}
#[test]
#[ignore = "requires the official capnp compiler; run explicitly for protocol interop"]
fn official_cli_decodes_rydantic_and_rydantic_decodes_cli_messages() {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let schema = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("schema/wire.capnp");
    let mut child = Command::new("capnp")
        .args(["decode", schema.to_str().unwrap(), "Person"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&example_person().dump_capnp().unwrap())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("fullName = \"Alice\""));
    assert!(text.contains("role = admin"));
    for packed in [false, true] {
        let mut command = Command::new("capnp");
        command.arg("encode");
        if packed {
            command.arg("--packed");
        }
        let mut child = command
            .args([schema.to_str().unwrap(), "Number"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(b"(value = 18446744073709551615)")
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        let value = if packed {
            Number::parse_capnp_packed(&output.stdout)
        } else {
            Number::parse_capnp(&output.stdout)
        }
        .unwrap();
        assert_eq!(value, Number(u64::MAX));
    }
}
