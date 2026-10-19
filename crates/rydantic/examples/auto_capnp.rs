use rydantic::{CapnpModel, CapnpSchema, Model};

#[rydantic::capnp_models(namespace = "rydantic.examples.users")]
mod models {
    #[derive(Debug, PartialEq)]
    pub struct User {
        #[model(trim, min_length = 1)]
        pub name: String,
        #[model(ge = 18)]
        pub age: u8,
        pub role: Role,
        pub nickname: Option<String>,
        pub avatar: Vec<u8>,
    }

    #[derive(Debug, PartialEq)]
    pub enum Role {
        Guest,
        Admin,
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let user =
        models::User::parse_json(r#"{"name":" Alice ","age":25,"role":"Admin","avatar":[0,255]}"#)?;
    assert_eq!(models::User::parse_capnp(&user.dump_capnp()?)?, user);
    assert_eq!(
        models::User::parse_capnp_packed(&user.dump_capnp_packed()?)?,
        user
    );
    if let Some(path) = std::env::args_os().nth(1) {
        models::User::write_capnp_schema(path)?;
    }
    println!("{}", models::User::capnp_schema());
    Ok(())
}
