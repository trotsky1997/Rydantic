use rd::{CapnpModel, CapnpSchema, Model};
use rydantic_auto_capnp_fixture::{example_user, models::User};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let user = example_user();
    let bytes = user.dump_capnp()?;
    assert_eq!(User::parse_capnp(&bytes)?, user);
    assert_eq!(User::parse_json(&user.dump_json()?)?, user);
    println!(
        "Automatic {} schema, native {} bytes",
        User::CAPNP_TYPE,
        bytes.len()
    );
    Ok(())
}
