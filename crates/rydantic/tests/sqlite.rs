#![cfg(feature = "sqlx")]
use rydantic::sqlx::{self, sqlite::SqlitePoolOptions, Row, SqlitePool};
use rydantic::{
    sql::{Compare, Order},
    Model, SqlError, SqlModel,
};

#[derive(Debug, PartialEq, Model, SqlModel)]
#[sql(table = "users")]
struct User {
    #[sql(primary_key, auto_increment)]
    id: Option<i64>,
    #[model(trim, min_length = 2)]
    #[sql(unique)]
    name: String,
    #[model(ge = 18, le = 120)]
    #[sql(index)]
    age: u8,
    note: Option<String>,
    avatar: Vec<u8>,
    #[sql(skip)]
    #[model(default = "local")]
    internal: String,
}
async fn pool() -> SqlitePool {
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap()
}
fn user(name: &str) -> User {
    User {
        id: None,
        name: name.into(),
        age: 25,
        note: None,
        avatar: vec![0, 255],
        internal: "local".into(),
    }
}

#[tokio::test]
async fn schema_crud_indexes_aliases_and_native_values_roundtrip() {
    let db = pool().await;
    User::create_table(&db).await.unwrap();
    User::create_table(&db).await.unwrap();
    let parsed = User::parse_json(r#"{"name":" Alice ","age":25,"avatar":[0,255]}"#).unwrap();
    let mut stored = parsed.insert(&db).await.unwrap();
    assert_eq!(stored.id, Some(1));
    assert_eq!(stored.name, "Alice");
    assert_eq!(parsed.id, None);
    assert_eq!(
        User::get(&db, &1).await.unwrap(),
        Some(user_with_id("Alice", 1))
    );
    stored.age = 30;
    stored.note = Some("edited".into());
    stored = stored.update(&db).await.unwrap();
    assert_eq!(User::get(&db, &1).await.unwrap(), Some(stored));
    let rows = sqlx::query("PRAGMA index_list(users)")
        .fetch_all(&db)
        .await
        .unwrap();
    assert!(rows
        .iter()
        .any(|row| row.get::<String, _>("name").contains("age")));
    let decoded: User = sqlx::query_as("SELECT * FROM users WHERE id = 1")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(decoded.note.as_deref(), Some("edited"));
    assert!(User::delete(&db, &1).await.unwrap());
    assert!(!User::delete(&db, &1).await.unwrap());
    assert!(User::get(&db, &1).await.unwrap().is_none());
    assert!(matches!(
        user_with_id("missing", 999).update(&db).await,
        Err(SqlError::NotFound)
    ));
}
fn user_with_id(name: &str, id: i64) -> User {
    User {
        id: Some(id),
        ..user(name)
    }
}

#[tokio::test]
async fn invalid_models_and_duplicate_keys_cannot_persist() {
    let db = pool().await;
    User::create_table(&db).await.unwrap();
    let mut invalid = user("bad");
    invalid.age = 17;
    assert!(matches!(
        invalid.insert(&db).await,
        Err(SqlError::Validation(_))
    ));
    assert_eq!(User::select().fetch_all(&db).await.unwrap().len(), 0);
    user("Alice").insert(&db).await.unwrap();
    assert!(matches!(
        user("Alice").insert(&db).await,
        Err(SqlError::Database(_))
    ));
    assert_eq!(User::select().fetch_all(&db).await.unwrap().len(), 1);
    let mut invalid = user("Bob");
    invalid.id = Some(1);
    invalid.age = 17;
    assert!(invalid.update(&db).await.is_err());
    assert_eq!(User::get(&db, &1).await.unwrap().unwrap().age, 25);
    sqlx::query("UPDATE users SET age = 1 WHERE id = 1")
        .execute(&db)
        .await
        .unwrap();
    assert!(matches!(
        User::get(&db, &1).await,
        Err(SqlError::Validation(_))
    ));
    assert!(sqlx::query_as::<_, User>("SELECT * FROM users")
        .fetch_one(&db)
        .await
        .is_err());
}
#[tokio::test]
async fn filters_bind_parameters_validate_columns_and_paginate() {
    let db = pool().await;
    User::create_table(&db).await.unwrap();
    for name in ["Alice", "Bob", "x' OR 1=1 --"] {
        user(name).insert(&db).await.unwrap();
    }
    let values = User::select()
        .where_eq("name", &"x' OR 1=1 --".to_owned())
        .unwrap()
        .fetch_all(&db)
        .await
        .unwrap();
    assert_eq!(values.len(), 1);
    assert!(matches!(
        User::select().where_eq("name; DROP TABLE users", &1i64),
        Err(SqlError::InvalidColumn(_))
    ));
    assert!(User::select().where_eq("name", &25i64).is_err());
    let values = User::select()
        .filter("age", Compare::GreaterOrEqual, &18u8)
        .unwrap()
        .order_by("name", Order::Ascending)
        .unwrap()
        .limit(1)
        .offset(1)
        .fetch_all(&db)
        .await
        .unwrap();
    assert_eq!(values[0].name, "Bob");
    assert_eq!(
        User::select()
            .where_eq("note", &Option::<String>::None)
            .unwrap()
            .fetch_all(&db)
            .await
            .unwrap()
            .len(),
        3
    );
    assert!(User::select()
        .offset(u64::MAX)
        .fetch_all(&db)
        .await
        .is_err());
}
#[tokio::test]
async fn operations_compose_with_connections_and_outer_transactions() {
    let db = pool().await;
    User::create_table(&db).await.unwrap();
    let mut tx = db.begin().await.unwrap();
    let stored = user("Alice").insert(&mut tx).await.unwrap();
    assert_eq!(
        User::get(&mut tx, &stored.id.unwrap()).await.unwrap(),
        Some(stored)
    );
    tx.rollback().await.unwrap();
    assert!(User::select().fetch_all(&db).await.unwrap().is_empty());
    let mut connection = db.acquire().await.unwrap();
    user("Bob").insert(&mut *connection).await.unwrap();
    assert_eq!(
        User::select()
            .fetch_all(&mut *connection)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[derive(Debug, PartialEq, Model, SqlModel)]
struct Wide {
    #[sql(primary_key)]
    id: String,
    unsigned: u128,
    signed: i128,
    limited: u64,
}
#[tokio::test]
async fn wide_integers_are_exact_and_sqlite_integer_overflow_is_rejected() {
    let db = pool().await;
    Wide::create_table(&db).await.unwrap();
    let mut model = Wide {
        id: "key".into(),
        unsigned: u128::MAX,
        signed: i128::MIN,
        limited: i64::MAX as u64,
    };
    let stored = model.insert(&db).await.unwrap();
    assert_eq!(model, stored);
    model.id = "overflow".into();
    model.limited = u64::MAX;
    let error = model.insert(&db).await.unwrap_err();
    let SqlError::Validation(error) = error else {
        panic!("expected validation error")
    };
    assert_eq!(error.errors()[0].pointer(), "/limited");
    assert_eq!(Wide::select().fetch_all(&db).await.unwrap().len(), 1);
}

#[derive(Debug, Model, SqlModel)]
struct Post {
    #[sql(primary_key, auto_increment)]
    id: Option<i64>,
    #[sql(references = "users.id", on_delete = "cascade", index)]
    user_id: i64,
    title: String,
}
#[tokio::test]
async fn foreign_keys_and_cascade_are_enforced_by_sqlite() {
    let db = pool().await;
    User::create_table(&db).await.unwrap();
    Post::create_table(&db).await.unwrap();
    let parent = user("Alice").insert(&db).await.unwrap();
    Post {
        id: None,
        user_id: parent.id.unwrap(),
        title: "post".into(),
    }
    .insert(&db)
    .await
    .unwrap();
    assert!(Post {
        id: None,
        user_id: 999,
        title: "invalid".into()
    }
    .insert(&db)
    .await
    .is_err());
    User::delete(&db, &parent.id.unwrap()).await.unwrap();
    assert!(Post::select().fetch_all(&db).await.unwrap().is_empty());
}
