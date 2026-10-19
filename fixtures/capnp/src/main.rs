use rydantic::CapnpModel;
use rydantic_capnp_fixture::{example_person, Person};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bytes = example_person().dump_capnp()?;
    let person = Person::parse_capnp(&bytes)?;
    assert_eq!(person, example_person());
    println!("Native Cap'n Proto: {} bytes, {}", bytes.len(), person.name);
    Ok(())
}
