//! Executable native protocol examples, with official capnpc-generated bindings.
pub mod wire_capnp;
use rydantic::Model;
use std::collections::BTreeMap;

#[derive(Debug, PartialEq, Model)]
#[model(capnp = "wire_capnp::address")]
pub struct Address {
    #[model(trim, min_length = 2)]
    pub city: String,
    pub zip: u32,
}
#[derive(Debug, PartialEq, Model)]
#[model(capnp_enum = "wire_capnp::Role")]
pub enum Role {
    User,
    Admin,
}

#[derive(Debug, PartialEq, Model)]
#[model(capnp = "wire_capnp::person")]
pub struct Person {
    pub id: u64,
    #[model(capnp_name = "fullName", trim, min_length = 2)]
    pub name: String,
    #[model(ge = 18, le = 120)]
    pub age: u8,
    pub address: Box<Address>,
    #[model(min_length = 1)]
    pub phones: Vec<String>,
    pub scores: Vec<i64>,
    pub coordinates: [f64; 2],
    pub flags: Vec<bool>,
    #[model(capnp_data)]
    pub avatar: Vec<u8>,
    pub nickname: Option<String>,
    pub labels: BTreeMap<String, String>,
    #[model(capnp_enum)]
    pub role: Role,
    #[model(capnp_text)]
    pub large: u128,
    pub matrix: Vec<Vec<u32>>,
    pub history: Vec<Address>,
    pub alternative: Option<Address>,
    pub token: u128,
    #[model(capnp_skip, default = "local")]
    pub local: String,
}
pub fn example_person() -> Person {
    Person {
        id: 7,
        name: "Alice".into(),
        age: 25,
        address: Box::new(Address {
            city: "Paris".into(),
            zip: 75001,
        }),
        phones: vec!["123".into()],
        scores: vec![-7, 42],
        coordinates: [1.5, -2.0],
        flags: vec![true, false],
        avatar: vec![0, 255, 13],
        nickname: Some("Al".into()),
        labels: BTreeMap::from([("team".into(), "core".into())]),
        role: Role::Admin,
        large: u128::MAX,
        matrix: vec![vec![1, 2], vec![], vec![3]],
        history: vec![Address {
            city: "Rome".into(),
            zip: 1,
        }],
        alternative: None,
        token: u64::MAX as u128,
        local: "local".into(),
    }
}
#[derive(Debug, PartialEq, Model)]
#[model(untagged, capnp = "wire_capnp::event")]
pub enum Event {
    Ping,
    Name(String),
    Moved(Address),
    Login { user: String, count: u32 },
}
#[derive(Debug, PartialEq, Model)]
#[model(capnp = "wire_capnp::number")]
pub struct Number(pub u64);
#[derive(Debug, PartialEq, Model)]
#[model(capnp = "wire_capnp::pair")]
pub struct Pair(pub String, pub i32);
#[derive(Debug, PartialEq, Model)]
#[model(capnp = "wire_capnp::empty")]
pub struct Empty;

#[derive(Debug, PartialEq, Model)]
#[model(capnp = "wire_capnp::scalar_text")]
pub struct ScalarText {
    #[model(capnp_text)]
    pub date: rydantic::chrono::NaiveDate,
    #[model(capnp_text)]
    pub amount: rydantic::rust_decimal::Decimal,
}
