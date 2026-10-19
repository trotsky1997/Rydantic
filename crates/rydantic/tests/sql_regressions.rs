#![cfg(feature = "sqlx")]
use rydantic::{
    sqlx::{self, sqlite::SqlitePoolOptions, SqlitePool},
    Model, SqlError, SqlModel, ValidationError, Value,
};

async fn pool() -> SqlitePool {
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap()
}

mod qualified {
    #[derive(Debug, PartialEq, rydantic::Model, rydantic::SqlModel)]
    pub struct Row {
        #[sql(primary_key)]
        pub id: i64,
        pub name: String,
    }
}
#[tokio::test]
async fn qualified_derives_work_in_nested_modules_without_trait_imports() {
    let db = pool().await;
    qualified::Row::create_table(&db).await.unwrap();
    let row = qualified::Row {
        id: 1,
        name: "Alice".into(),
    };
    assert_eq!(row.insert(&db).await.unwrap(), row);
    assert_eq!(qualified::Row::get(&db, &1).await.unwrap(), Some(row));
}

fn remove_prefix(value: Value) -> Result<Value, ValidationError> {
    match value {
        Value::String(value) => Ok(Value::String(
            value.strip_prefix("draft:").unwrap_or(&value).to_owned(),
        )),
        _ => Err(ValidationError::custom("expected a string")),
    }
}
#[derive(Debug, PartialEq, Model)]
struct Metadata {
    #[model(trim, lowercase)]
    label: String,
}
fn metadata() -> Metadata {
    Metadata {
        label: " NESTED ".into(),
    }
}
#[derive(Debug, PartialEq, Model, SqlModel)]
struct SkippedDefaults {
    #[sql(primary_key, auto_increment)]
    id: Option<i64>,
    #[sql(skip)]
    #[model(default = " Guest ", trim, lowercase)]
    name: String,
    #[sql(skip)]
    #[model(default = "draft: visitor ", before = remove_prefix, trim)]
    prepared: String,
    #[sql(skip)]
    #[model(default_factory = metadata)]
    metadata: Metadata,
}
fn assert_prepared(value: &SkippedDefaults) {
    assert_eq!(value.name, "guest");
    assert_eq!(value.prepared, "visitor");
    assert_eq!(value.metadata.label, "nested");
}
#[tokio::test]
async fn skipped_defaults_follow_the_model_preparation_pipeline_on_every_row_path() {
    let db = pool().await;
    SkippedDefaults::create_table(&db).await.unwrap();
    let parsed = SkippedDefaults::parse_json("{}").unwrap();
    assert_prepared(&parsed);
    let stored = parsed.insert(&db).await.unwrap();
    assert_prepared(&stored);
    assert_prepared(&stored.update(&db).await.unwrap());
    assert_prepared(&SkippedDefaults::get(&db, &1).await.unwrap().unwrap());
    assert_prepared(&SkippedDefaults::select().fetch_all(&db).await.unwrap()[0]);
    let row: SkippedDefaults = sqlx::query_as("SELECT * FROM skipped_defaults")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_prepared(&row);
}

#[derive(Debug, Model, SqlModel)]
struct Parent {
    #[sql(primary_key)]
    id: i64,
}
#[derive(Debug, Model, SqlModel)]
struct Child {
    #[sql(primary_key)]
    id: i64,
    #[sql(references = "parent.id", on_delete = "set_null")]
    parent_id: Option<i64>,
}
#[tokio::test]
async fn set_null_preserves_the_child_after_parent_deletion() {
    let db = pool().await;
    Parent::create_table(&db).await.unwrap();
    Child::create_table(&db).await.unwrap();
    Parent { id: 1 }.insert(&db).await.unwrap();
    Child {
        id: 2,
        parent_id: Some(1),
    }
    .insert(&db)
    .await
    .unwrap();
    Parent::delete(&db, &1).await.unwrap();
    assert_eq!(Child::get(&db, &2).await.unwrap().unwrap().parent_id, None);
}

static CHECK_POOL: std::sync::OnceLock<SqlitePool> = std::sync::OnceLock::new();
async fn check_database(_: &str) -> Result<(), ValidationError> {
    sqlx::query_scalar::<_, i64>("SELECT 1")
        .fetch_one(CHECK_POOL.get().unwrap())
        .await
        .map(|_| ())
        .map_err(|_| ValidationError::custom("database check failed"))
}
#[derive(Debug, Model, SqlModel)]
struct Checked {
    #[sql(primary_key)]
    id: i64,
    #[model(async_validate = check_database)]
    name: String,
}
fn assert_connection_error(result: Result<(), SqlError>) {
    let Err(SqlError::Validation(errors)) = result else {
        panic!("expected a connection policy error")
    };
    assert_eq!(errors.errors()[0].code, "sql_async_connection");
}
#[tokio::test]
async fn held_connections_reject_async_model_checks_without_waiting_for_the_pool() {
    let db = pool().await;
    CHECK_POOL.set(db.clone()).unwrap();
    Checked::create_table(&db).await.unwrap();
    let value = Checked {
        id: 1,
        name: "Alice".into(),
    };
    value.insert(&db).await.unwrap();
    let operations = async {
        let mut transaction = db.begin().await.unwrap();
        assert_connection_error(value.insert(&mut transaction).await.map(|_| ()));
        assert_connection_error(value.update(&mut transaction).await.map(|_| ()));
        assert_connection_error(Checked::get(&mut transaction, &1).await.map(|_| ()));
        assert_connection_error(
            Checked::select()
                .fetch_all(&mut transaction)
                .await
                .map(|_| ()),
        );
        transaction.rollback().await.unwrap();
        let mut connection = db.acquire().await.unwrap();
        assert_connection_error(value.insert(&mut *connection).await.map(|_| ()));
        assert_connection_error(value.update(&mut *connection).await.map(|_| ()));
        assert_connection_error(Checked::get(&mut *connection, &1).await.map(|_| ()));
        assert_connection_error(
            Checked::select()
                .fetch_all(&mut *connection)
                .await
                .map(|_| ()),
        );
    };
    tokio::time::timeout(std::time::Duration::from_secs(3), operations)
        .await
        .unwrap();
    assert_eq!(Checked::get(&db, &1).await.unwrap().unwrap().name, "Alice");
    db.close().await;
}

#[cfg(feature = "axum")]
#[tokio::test]
async fn every_sqlite_constraint_code_returns_a_safe_http_conflict() {
    use axum::{body::to_bytes, response::IntoResponse};
    let db = pool().await;
    for statement in [
        "CREATE TABLE constraints (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE, n INTEGER CHECK(n > 0), parent_id INTEGER REFERENCES constraints(id))",
        "INSERT INTO constraints VALUES (1, 'initial', 1, NULL)",
        "CREATE TRIGGER deny_sensitive BEFORE INSERT ON constraints WHEN NEW.n = 99 BEGIN SELECT RAISE(ABORT, 'sensitive trigger details'); END",
    ] { sqlx::query(statement).execute(&db).await.unwrap(); }
    for statement in [
        "INSERT INTO constraints VALUES (2, NULL, 1, NULL)",
        "INSERT INTO constraints VALUES (2, 'initial', 1, NULL)",
        "INSERT INTO constraints VALUES (2, 'check', -1, NULL)",
        "INSERT INTO constraints VALUES (2, 'foreign', 1, 999)",
        "INSERT INTO constraints VALUES (2, 'trigger', 99, NULL)",
    ] {
        let error = sqlx::query(statement).execute(&db).await.unwrap_err();
        let response = SqlError::from(error).into_response();
        assert_eq!(response.status().as_u16(), 409);
        let body: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        assert_eq!(body["detail"][0]["code"], "sql_constraint");
        assert_eq!(
            body["detail"][0]["message"],
            "database constraint violation"
        );
        assert!(!body.to_string().contains("sensitive"));
    }
    let error = sqlx::query("SELECT * FROM missing_table")
        .execute(&db)
        .await
        .unwrap_err();
    assert_eq!(SqlError::from(error).into_response().status().as_u16(), 500);
}
