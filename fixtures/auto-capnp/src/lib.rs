//! An ordinary consumer with a renamed dependency and no build script or schema files.
#[rd::capnp_models(namespace = "rydantic.examples.models")]
pub mod models {
    use std::collections::{BTreeMap, HashMap};

    pub type Name = String;
    pub type Items<T> = Vec<T>;
    pub type Maybe<T> = Option<T>;

    #[derive(Debug, PartialEq)]
    pub enum Role {
        Guest,
        #[model(rename = "administrator", capnp_name = "admin")]
        Admin,
    }

    #[derive(Debug, PartialEq)]
    pub struct User {
        #[model(trim, min_length = 1, capnp_name = "full_name")]
        pub name: Name,
        #[model(ge = 18)]
        pub age: u8,
        pub role: Role,
        pub address: nested::Address,
        pub nickname: Maybe<Name>,
        pub children: Items<User>,
        pub parent: Option<Box<User>>,
        pub avatar: Vec<u8>,
        pub attachments: Vec<Vec<u8>>,
        pub hash: [u8; 4],
        pub counters: BTreeMap<String, u128>,
        pub tags: HashMap<String, Vec<String>>,
        pub role_history: Vec<Role>,
        pub alternative_role: Option<Role>,
        pub wide_numbers: Option<Vec<u128>>,
        pub birthday: Option<rd::chrono::NaiveDate>,
        pub credits: Vec<rd::rust_decimal::Decimal>,
        pub balance: Option<rd::rust_decimal::Decimal>,
        #[model(capnp_skip, default = "local")]
        pub internal: String,
    }

    pub mod nested {
        #[derive(Debug, PartialEq)]
        pub struct Address {
            #[model(trim)]
            pub city: String,
            pub zip: u32,
        }

        use super::Items as Collection;
        #[derive(Debug, PartialEq)]
        pub struct Addresses {
            pub values: Collection<Address>,
        }
    }

    #[derive(Debug, PartialEq)]
    #[model(untagged)]
    pub enum Event {
        Ping,
        Name(String),
        User(Box<User>),
        Moved {
            address: nested::Address,
            when: u64,
        },
        Pair(String, u16),
        EmptyTuple(),
        EmptyStruct {},
        Local {
            #[model(capnp_skip, default = "local")]
            value: String,
        },
    }

    #[derive(Debug, PartialEq)]
    #[model(untagged)]
    pub enum Single {
        Only(u64),
    }

    #[derive(Debug, PartialEq)]
    pub struct Empty;

    #[derive(Debug, PartialEq)]
    pub struct Number(pub u128);

    #[derive(Debug, PartialEq)]
    pub struct Pair(pub String, pub i64);
}

pub fn example_user() -> models::User {
    use models::{nested::Address, Role, User};
    User {
        name: "Alice".into(),
        age: 25,
        role: Role::Admin,
        address: Address {
            city: "Paris".into(),
            zip: 75000,
        },
        nickname: None,
        children: Vec::new(),
        parent: None,
        avatar: vec![0, 255, 13],
        attachments: vec![vec![], vec![0, 128, 255]],
        hash: [0, 1, 2, 255],
        counters: [("max".into(), u128::MAX)].into(),
        tags: [("lang".into(), vec!["Rust".into()])].into(),
        role_history: vec![Role::Guest, Role::Admin],
        alternative_role: Some(Role::Guest),
        wide_numbers: Some(vec![0, u128::MAX]),
        birthday: Some(rd::chrono::NaiveDate::from_ymd_opt(2000, 1, 2).unwrap()),
        credits: vec![rd::rust_decimal::Decimal::new(12345, 2)],
        balance: Some(rd::rust_decimal::Decimal::MAX),
        internal: "local".into(),
    }
}
