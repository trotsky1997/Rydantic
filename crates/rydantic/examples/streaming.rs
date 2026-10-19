use rydantic::{stream, Model};
use std::io::Cursor;

#[derive(Debug, Model)]
struct Row {
    #[model(ge = 1)]
    id: u32,
}
fn main() -> Result<(), rydantic::ValidationErrors> {
    let input = r#"[{"id":1},{"id":0},{"id":3}]"#;
    let count = stream::visit_array::<_, Row, _>(Cursor::new(input), |item| {
        match item {
            Ok(row) => println!("{}", row.dump_json()?),
            Err(errors) => println!("{}", errors.to_json()),
        }
        Ok(())
    })?;
    assert_eq!(count, 3);
    let lines = "{\"id\":1}\n{oops}\n{\"id\":3}\n";
    let results = stream::ndjson::<_, Row>(Cursor::new(lines));
    for item in results {
        match item {
            Ok(row) => println!("NDJSON: {}", row.id),
            Err(errors) => println!("NDJSON: {}", errors.to_json()),
        }
    }
    Ok(())
}
