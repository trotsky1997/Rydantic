use rd::{Model, SchemaMode, SqlModel};

#[derive(Debug, PartialEq, Model)]
struct Message {
    #[model(trim, min_length = 1)]
    text: String,
}

#[derive(Debug, Model, SqlModel)]
struct Record {
    #[sql(primary_key, auto_increment)]
    id: Option<i64>,
    #[sql(default = "seed", rename_from = "previous_text")]
    text: String,
}

fn main() {
    let message = Message::parse_json(r#"{"text":" hello "}"#).unwrap();
    assert_eq!(message.text, "hello");
    assert!(Message::json_schema(SchemaMode::Output).is_object());
    assert!(Record::create_table_sql::<rd::sqlx::Sqlite>().contains("AUTOINCREMENT"));
    assert!(Record::create_table_sql::<rd::sqlx::Postgres>().contains("IDENTITY"));
    assert!(Record::create_table_sql::<rd::sqlx::MySql>().contains("AUTO_INCREMENT"));
    let _router: rd::axum::Router = rd::axum::Router::new();
    println!("{}", message.dump_json().unwrap());
}
