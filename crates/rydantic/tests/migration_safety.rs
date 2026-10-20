#![cfg(feature = "sqlite")]
use rydantic::{sqlx, Model, SqlModel};
#[derive(Debug, Model, SqlModel)]
#[sql(table = "names")]
struct Names {
    #[sql(primary_key)]
    id: i64,
    #[sql(unique)]
    name: String,
    note: Option<String>,
}
#[tokio::test]
async fn preserve_custom_index_collation() {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::query("CREATE TABLE names (id INTEGER PRIMARY KEY NOT NULL, name TEXT NOT NULL)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("CREATE UNIQUE INDEX unique_name ON names(name COLLATE NOCASE)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO names VALUES (1, 'Alice')")
        .execute(&pool)
        .await
        .unwrap();
    assert!(sqlx::query("INSERT INTO names VALUES (2, 'alice')")
        .execute(&pool)
        .await
        .is_err());
    let schema_before: Vec<(String, String)> =
        sqlx::query_as("SELECT name, sql FROM sqlite_schema WHERE sql IS NOT NULL ORDER BY name")
            .fetch_all(&pool)
            .await
            .unwrap();
    for result in [
        Names::migration_plan(&pool).await.map(|_| ()),
        Names::migrate(&pool).await.map(|_| ()),
    ] {
        assert!(matches!(
            result,
            Err(rydantic::SqlError::Migration {
                code: "migration_unsupported",
                ..
            })
        ));
    }
    let schema_after: Vec<(String, String)> =
        sqlx::query_as("SELECT name, sql FROM sqlite_schema WHERE sql IS NOT NULL ORDER BY name")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(schema_before, schema_after);
    assert!(sqlx::query("INSERT INTO names VALUES (2, 'alice')")
        .execute(&pool)
        .await
        .is_err());
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM names WHERE name='Alice'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[derive(Debug, Model, SqlModel)]
#[sql(table = "accounts")]
struct RenamedAccount {
    #[sql(primary_key)]
    id: i64,
    #[sql(rename_from = "legacy")]
    accounts: String,
}
#[tokio::test]
async fn rename_preflight_preserves_table_identifier() {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::query("CREATE TABLE accounts (id INTEGER PRIMARY KEY NOT NULL, legacy TEXT)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO accounts VALUES (1, 'present')")
        .execute(&pool)
        .await
        .unwrap();
    RenamedAccount::migrate(&pool).await.unwrap();
    let value: String = sqlx::query_scalar("SELECT accounts FROM accounts WHERE id=1")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(value, "present");
}

#[tokio::test]
async fn ordinary_binary_index_remains_supported() {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::query("CREATE TABLE names (id INTEGER PRIMARY KEY NOT NULL, name TEXT NOT NULL)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("CREATE UNIQUE INDEX unique_name ON names(name COLLATE BINARY ASC)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO names VALUES (1, 'Alice')")
        .execute(&pool)
        .await
        .unwrap();
    Names::migrate(&pool).await.unwrap();
    assert!(
        sqlx::query("INSERT INTO names (id,name) VALUES (2, 'Alice')")
            .execute(&pool)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn descending_index_is_not_silently_recreated_as_ascending() {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::query("CREATE TABLE names (id INTEGER PRIMARY KEY NOT NULL, name TEXT NOT NULL)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("CREATE UNIQUE INDEX unique_name ON names(name DESC)")
        .execute(&pool)
        .await
        .unwrap();
    assert!(matches!(
        Names::migration_plan(&pool).await,
        Err(rydantic::SqlError::Migration {
            code: "migration_unsupported",
            ..
        })
    ));
}

#[tokio::test]
async fn descending_primary_key_preserves_null_identifiers_and_rowids() {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::query("CREATE TABLE names (id INTEGER PRIMARY KEY DESC, name TEXT NOT NULL UNIQUE)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO names (rowid, id, name) VALUES (99, NULL, 'Alice')")
        .execute(&pool)
        .await
        .unwrap();
    let schema_before: Vec<(String, String)> =
        sqlx::query_as("SELECT name, sql FROM sqlite_schema WHERE sql IS NOT NULL ORDER BY name")
            .fetch_all(&pool)
            .await
            .unwrap();
    let result = Names::migrate(&pool).await;
    assert!(
        matches!(
            result,
            Err(rydantic::SqlError::Migration {
                code: "migration_unsupported",
                ..
            })
        ),
        "{result:?}"
    );
    let schema_after: Vec<(String, String)> =
        sqlx::query_as("SELECT name, sql FROM sqlite_schema WHERE sql IS NOT NULL ORDER BY name")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(schema_before, schema_after);
    let ids: (i64, Option<i64>) = sqlx::query_as("SELECT rowid, id FROM names WHERE name='Alice'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(ids, (99, None));
}
