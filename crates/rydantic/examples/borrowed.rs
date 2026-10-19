use rydantic::{BorrowModel, Model};
use std::{borrow::Cow, collections::BTreeMap};

#[derive(Debug, Model)]
struct Address<'a> {
    city: &'a str,
}
#[derive(Debug, Model)]
#[model(extra = "allow")]
struct User<'a> {
    #[model(trim, min_length = 2)]
    name: &'a str,
    #[model(flatten)]
    address: Address<'a>,
    #[model(extra)]
    extra: BTreeMap<String, Cow<'a, str>>,
}
fn main() -> Result<(), rydantic::ValidationErrors> {
    let input = String::from(
        r#"{"name":" Alice ","city":"Paris","plain":"hello","escaped":"hello\nworld"}"#,
    );
    let user = User::parse_json(&input)?;
    assert_eq!(user.name, "Alice");
    assert!(matches!(user.extra["plain"], Cow::Borrowed(_)));
    assert!(matches!(user.extra["escaped"], Cow::Owned(_)));
    println!("{}", user.dump_json()?);
    Ok(())
}
