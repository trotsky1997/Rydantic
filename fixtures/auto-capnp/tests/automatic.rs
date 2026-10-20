use rd::{capnp, CapnpModel, CapnpSchema, Model};
use rydantic_auto_capnp_fixture::{
    example_user,
    models::{self, nested::Address, Event, Role, User},
};

fn roundtrip<T: CapnpModel + std::fmt::Debug + PartialEq>(value: &T) {
    assert_eq!(
        &T::parse_capnp(&value.dump_capnp().unwrap()).unwrap(),
        value
    );
    assert_eq!(
        &T::parse_capnp_packed(&value.dump_capnp_packed().unwrap()).unwrap(),
        value
    );
}

#[test]
fn generated_protocol_roundtrips_json_native_and_packed_with_deep_containers() {
    let mut user = example_user();
    let mut child = example_user();
    child.name = "Bob".into();
    child.nickname = Some("B".into());
    child.alternative_role = None;
    child.wide_numbers = None;
    child.birthday = None;
    child.balance = None;
    user.children.push(child);
    user.parent = Some(Box::new(example_user()));
    roundtrip(&user);
    assert_eq!(User::parse_json(&user.dump_json().unwrap()).unwrap(), user);
    assert!(models::CAPNP_SCHEMA.contains("avatar @7 :Data"));
    assert!(models::CAPNP_SCHEMA.contains("attachments @8 :List(Data)"));
    assert!(models::CAPNP_SCHEMA.contains("hash @9 :List(UInt8)"));
    assert!(models::CAPNP_SCHEMA.contains("fullName @0 :Text"));
    assert!(models::CAPNP_SCHEMA.contains("wideNumbers"));
    assert_eq!(User::CAPNP_TYPE, "User");
    assert_eq!(Address::CAPNP_TYPE, "NestedAddress");
    assert_eq!(User::capnp_schema(), models::CAPNP_SCHEMA);
}

#[test]
fn generated_native_readers_and_builders_are_public_and_typed() {
    let user = example_user();
    let message = user.to_capnp_message().unwrap();
    let reader = message
        .get_root_as_reader::<models::wire::user::Reader<'_>>()
        .unwrap();
    assert_eq!(reader.get_full_name().unwrap().to_str().unwrap(), "Alice");
    assert_eq!(reader.get_role().unwrap(), models::wire::Role::Admin);
    assert_eq!(reader.get_avatar().unwrap(), [0, 255, 13]);
    assert_eq!(
        reader.get_attachments().unwrap().get(1).unwrap(),
        [0, 128, 255]
    );
    assert_eq!(User::from_capnp_reader(reader).unwrap(), user);
    let mut builder = capnp::message::Builder::new_default();
    builder
        .init_root::<models::wire::number::Builder<'_>>()
        .set_value(u128::MAX.to_string().as_str());
    assert_eq!(
        models::Number::parse_capnp(&capnp::serialize::write_message_to_words(&builder)).unwrap(),
        models::Number(u128::MAX)
    );
}

#[test]
fn root_enums_union_groups_empty_variants_and_tuple_structs_work() {
    roundtrip(&Role::Guest);
    roundtrip(&Role::Admin);
    assert_eq!(Role::Admin.dump_json().unwrap(), "\"administrator\"");
    assert_eq!(Role::CAPNP_TYPE, "RydanticRoleRoot");
    for event in [
        Event::Ping,
        Event::Name("name".into()),
        Event::User(Box::new(example_user())),
        Event::Moved {
            address: Address {
                city: "Rome".into(),
                zip: 12,
            },
            when: u64::MAX,
        },
        Event::Pair("pair".into(), u16::MAX),
        Event::EmptyTuple(),
        Event::EmptyStruct {},
        Event::Local {
            value: "local".into(),
        },
    ] {
        roundtrip(&event);
    }
    roundtrip(&models::Single::Only(u64::MAX));
    roundtrip(&models::Empty);
    roundtrip(&models::Pair("pair".into(), i64::MIN));
    roundtrip(&models::Number(u128::MAX));
    roundtrip(&models::nested::Addresses {
        values: vec![Address {
            city: "Oslo".into(),
            zip: 42,
        }],
    });
}

#[test]
fn native_ingress_runs_normalization_and_checks_with_model_paths() {
    let mut message = example_user().to_capnp_message().unwrap();
    {
        let mut root = message
            .get_root::<models::wire::user::Builder<'_>>()
            .unwrap();
        root.set_full_name(" Alice ");
        root.reborrow().get_address().unwrap().set_city(" Paris ");
    }
    let parsed = User::parse_capnp(&capnp::serialize::write_message_to_words(&message)).unwrap();
    assert_eq!(parsed.name, "Alice");
    assert_eq!(parsed.address.city, "Paris");
    message
        .get_root::<models::wire::user::Builder<'_>>()
        .unwrap()
        .set_age(17);
    let errors =
        User::parse_capnp(&capnp::serialize::write_message_to_words(&message)).unwrap_err();
    assert!(errors
        .errors()
        .iter()
        .any(|error| error.pointer() == "/age"));
    let mut invalid = example_user();
    invalid.name.clear();
    assert!(invalid.dump_capnp().is_err());
}

#[test]
fn exported_schema_and_official_cli_exchange_native_and_packed_messages() {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let directory = tempfile::tempdir().unwrap();
    let schema = directory.path().join("models.capnp");
    User::write_capnp_schema(&schema).unwrap();
    assert_eq!(
        std::fs::read_to_string(&schema).unwrap(),
        models::CAPNP_SCHEMA
    );
    let run = |operation: &str, root: &str, packed: bool, input: &[u8]| {
        let mut command = Command::new("capnp");
        command.arg(operation);
        if packed {
            command.arg("--packed");
        }
        let mut child = command
            .arg(&schema)
            .arg(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(input).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    };
    for packed in [false, true] {
        let user = example_user();
        let bytes = if packed {
            user.dump_capnp_packed().unwrap()
        } else {
            user.dump_capnp().unwrap()
        };
        let decoded = String::from_utf8(run("decode", "User", packed, &bytes)).unwrap();
        assert!(decoded.contains("fullName = \"Alice\""));
        assert!(decoded.contains("role = admin"));
        let encoded = run(
            "encode",
            "Number",
            packed,
            format!("(value = \"{}\")", u128::MAX).as_bytes(),
        );
        let number = if packed {
            models::Number::parse_capnp_packed(&encoded)
        } else {
            models::Number::parse_capnp(&encoded)
        }
        .unwrap();
        assert_eq!(number.0, u128::MAX);
        let encoded = run("encode", Role::CAPNP_TYPE, packed, b"(value = admin)");
        let role = if packed {
            Role::parse_capnp_packed(&encoded)
        } else {
            Role::parse_capnp(&encoded)
        }
        .unwrap();
        assert_eq!(role, Role::Admin);
    }
}

// The same schema identity survives declaration reordering when ordinals are pinned.
#[rd::capnp_models(id = 0xd80e_54b1_1368_67c1)]
mod previous {
    #[derive(rd::Model, Debug, PartialEq)]
    pub enum Role {
        #[model(capnp_ordinal = 0)]
        Guest,
        #[model(capnp_ordinal = 1)]
        Admin,
    }
    #[derive(rd::Model, Debug, PartialEq)]
    #[model(tag = "kind")]
    pub enum Action {
        #[model(capnp_ordinal = 0)]
        Ping,
        #[model(capnp_ordinal = 1)]
        Update { name: String, count: u64 },
    }
    #[derive(rd::Model, Debug, PartialEq)]
    pub struct Record {
        #[model(capnp_ordinal = 0)]
        pub name: String,
        #[model(capnp_ordinal = 1)]
        pub count: u64,
    }
}
mod enclosing {
    #[rd::capnp_models(id = 0xd80e_54b1_1368_67c1)]
    pub mod reordered {
        #[derive(rd::Model, Debug, PartialEq)]
        pub enum Role {
            #[model(capnp_ordinal = 1)]
            Admin,
            #[model(capnp_ordinal = 0)]
            Guest,
        }
        #[derive(rd::Model, Debug, PartialEq)]
        #[model(tag = "kind")]
        pub enum Action {
            Update {
                #[model(capnp_ordinal = 2)]
                count: u64,
                #[model(capnp_ordinal = 1)]
                name: String,
            },
            #[model(capnp_ordinal = 0)]
            Ping,
        }
        #[derive(rd::Model, Debug, PartialEq)]
        pub struct Record {
            #[model(capnp_ordinal = 1)]
            pub count: u64,
            #[model(capnp_ordinal = 0)]
            pub name: String,
        }
    }
    #[rd::capnp_models(id = 0xd80e_54b1_1368_67c1)]
    pub mod extended {
        #[derive(rd::Model, Debug, PartialEq)]
        pub struct Record {
            pub name: String,
            pub count: u64,
            pub note: Option<String>,
        }
    }
}

#[test]
fn pinned_ordinals_and_appended_fields_preserve_protocol_compatibility() {
    use enclosing::{extended, reordered};
    let old = previous::Record {
        name: "stable".into(),
        count: 42,
    };
    let next = reordered::Record {
        name: old.name.clone(),
        count: old.count,
    };
    let bytes = old.dump_capnp().unwrap();
    assert_eq!(bytes, next.dump_capnp().unwrap());
    assert_eq!(reordered::Record::parse_capnp(&bytes).unwrap(), next);
    let newer = extended::Record::parse_capnp(&bytes).unwrap();
    assert_eq!(newer.name, old.name);
    assert_eq!(newer.note, None);
    let newer = extended::Record {
        note: Some("added".into()),
        ..newer
    };
    assert_eq!(
        previous::Record::parse_capnp(&newer.dump_capnp().unwrap()).unwrap(),
        old
    );
    assert_eq!(previous::CAPNP_SCHEMA_ID, reordered::CAPNP_SCHEMA_ID);
    assert_eq!(previous::CAPNP_SCHEMA_ID, extended::CAPNP_SCHEMA_ID);
    for (old, next) in [
        (previous::Role::Guest, reordered::Role::Guest),
        (previous::Role::Admin, reordered::Role::Admin),
    ] {
        assert_eq!(old.dump_capnp().unwrap(), next.dump_capnp().unwrap());
        assert_eq!(
            reordered::Role::parse_capnp(&old.dump_capnp().unwrap()).unwrap(),
            next
        );
    }
    for (old, next) in [
        (previous::Action::Ping, reordered::Action::Ping),
        (
            previous::Action::Update {
                name: "stable".into(),
                count: 42,
            },
            reordered::Action::Update {
                name: "stable".into(),
                count: 42,
            },
        ),
    ] {
        assert_eq!(old.dump_capnp().unwrap(), next.dump_capnp().unwrap());
        assert_eq!(
            reordered::Action::parse_capnp(&old.dump_capnp().unwrap()).unwrap(),
            next
        );
        assert_eq!(
            previous::Action::parse_capnp(&next.dump_capnp().unwrap()).unwrap(),
            old
        );
    }
}
