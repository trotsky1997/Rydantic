#![cfg(feature = "sqlx")]
use rydantic::{
    sql::{MigrationOptions, SqlBackend},
    sqlx::{self, Pool, QueryBuilder},
    Model, SqlError, SqlModel,
};

#[derive(Debug, Model, SqlModel)]
#[sql(table = "rd06_accounts")]
struct V1 {
    #[sql(primary_key, auto_increment)]
    id: Option<i64>,
    #[sql(length = 128)]
    name: String,
    legacy: Option<String>,
}
#[derive(Debug, Model, SqlModel)]
#[sql(table = "rd06_accounts")]
struct V2 {
    #[sql(primary_key, auto_increment)]
    id: Option<i64>,
    #[sql(rename_from = "name", length = 128, unique)]
    display_name: String,
    legacy: Option<String>,
    #[sql(default = 7)]
    score: i64,
    #[sql(default = true)]
    active: bool,
    #[sql(index, length = 128)]
    bio: Option<String>,
    #[sql(default = "O'Reilly\\reader 中文")]
    source: String,
}
#[derive(Debug, Model, SqlModel)]
#[sql(table = "rd06_accounts")]
struct V3 {
    #[sql(primary_key, auto_increment)]
    id: Option<i64>,
    #[sql(length = 128, index)]
    display_name: String,
    #[sql(default = 9)]
    score: i64,
    #[sql(default = true)]
    active: bool,
    bio: Option<String>,
    #[sql(default = "O'Reilly\\reader 中文")]
    source: String,
}
#[derive(Debug, Model, SqlModel)]
#[sql(table = "rd06_accounts")]
struct MissingDefault {
    #[sql(primary_key, auto_increment)]
    id: Option<i64>,
    #[sql(length = 128)]
    name: String,
    legacy: Option<String>,
    required: String,
}
#[derive(Debug, Model, SqlModel)]
#[sql(table = "rd06_parent")]
struct ParentV1 {
    #[sql(primary_key, auto_increment)]
    id: Option<i64>,
    name: String,
}
#[derive(Debug, Model, SqlModel)]
#[sql(table = "rd06_parent")]
struct ParentV2 {
    #[sql(primary_key, auto_increment)]
    id: Option<i64>,
    name: String,
    #[sql(default = 1)]
    generation: i64,
}
#[derive(Debug, Model, SqlModel)]
#[sql(table = "rd06_child")]
struct ChildV1 {
    #[sql(primary_key)]
    id: i64,
    #[sql(references = "rd06_parent.id", on_delete = "cascade")]
    parent: i64,
}
#[derive(Debug, Model, SqlModel)]
#[sql(table = "rd06_child")]
struct ChildV2 {
    #[sql(primary_key)]
    id: i64,
    #[sql(references = "rd06_parent.id", on_delete = "restrict")]
    parent: i64,
    note: Option<String>,
}
#[derive(Debug, Model, SqlModel)]
#[sql(table = "rd06_values")]
struct Nullable {
    #[sql(primary_key)]
    id: i64,
    #[sql(index, length = 128)]
    value: Option<String>,
}
#[derive(Debug, Model, SqlModel)]
#[sql(table = "rd06_values")]
struct Required {
    #[sql(primary_key)]
    id: i64,
    #[sql(rename_from = "value", index, length = 256, default = "fallback")]
    text: String,
    #[sql(default=-1.5)]
    rating: f64,
    #[sql(default=-7)]
    negative: i64,
    #[sql(default = false)]
    flag: bool,
}
#[derive(Debug, Model, SqlModel)]
#[sql(table = "rd06_types")]
struct Number {
    #[sql(primary_key)]
    id: i64,
    number: i64,
}
#[derive(Debug, Model, SqlModel)]
#[sql(table = "rd06_types")]
struct Text {
    #[sql(primary_key)]
    id: i64,
    number: String,
}
#[derive(Debug, Model, SqlModel)]
#[sql(table = "rd06_self")]
struct SelfReference {
    #[sql(primary_key)]
    id: i64,
    #[sql(unique)]
    code: i64,
    #[sql(references = "rd06_self.code")]
    parent: Option<i64>,
}
#[derive(Debug, Model, SqlModel)]
#[sql(table = "rd06_self")]
struct WithoutReference {
    #[sql(primary_key)]
    id: i64,
    code: i64,
    parent: Option<i64>,
}
#[derive(Debug, Model, SqlModel)]
#[sql(table = "rd06_collision")]
struct CollisionOptional {
    #[sql(primary_key)]
    id: i64,
    #[sql(rename_from = "rd06_collision")]
    legacy: Option<String>,
}
#[derive(Debug, Model, SqlModel)]
#[sql(table = "rd06_collision")]
struct CollisionRequired {
    #[sql(primary_key)]
    id: i64,
    #[sql(rename_from = "legacy")]
    rd06_collision: String,
}
#[derive(Debug, Model, SqlModel)]
#[sql(table = "rd06_collision")]
struct CollisionBackfill {
    #[sql(primary_key)]
    id: i64,
    #[sql(rename_from = "legacy", default = "fallback")]
    rd06_collision: String,
}
async fn execute<DB: SqlBackend>(pool: &Pool<DB>, sql: &str) {
    let mut c = pool.acquire().await.unwrap();
    DB::execute(&mut QueryBuilder::new(sql), &mut c)
        .await
        .unwrap();
}
fn code(error: SqlError, expected: &str) {
    assert!(
        matches!(error,SqlError::Migration{code,..} if code==expected),
        "{error:?}"
    );
}
async fn evolution<DB: SqlBackend>(pool: &Pool<DB>) {
    for table in [
        "rd06_child",
        "rd06_self",
        "rd06_collision",
        "rd06_parent",
        "rd06_accounts",
        "rd06_values",
        "rd06_types",
        "_rydantic_migrations",
    ] {
        execute(
            pool,
            &format!("DROP TABLE IF EXISTS {}", DB::DIALECT.quote(table)),
        )
        .await;
    }
    let plan = V1::migration_plan(pool).await.unwrap();
    assert_eq!(plan.changes().len(), 1);
    assert!(plan.statements()[0].contains("CREATE TABLE"));
    assert_eq!(
        plan.apply(pool, MigrationOptions::default())
            .await
            .unwrap()
            .revision,
        Some(1)
    );
    assert!(V1::migrate(pool).await.unwrap().plan.is_empty());
    let a = V1 {
        id: None,
        name: "Alice".into(),
        legacy: Some("keep".into()),
    }
    .insert(pool)
    .await
    .unwrap();
    let b = V1 {
        id: None,
        name: "Bob".into(),
        legacy: None,
    }
    .insert(pool)
    .await
    .unwrap();
    V1::delete(pool, &b.id.unwrap()).await.unwrap();
    code(
        MissingDefault::migrate(pool).await.unwrap_err(),
        "migration_data",
    );
    assert_eq!(
        V1::get(pool, &a.id.unwrap()).await.unwrap().unwrap().name,
        "Alice"
    );
    let plan = V2::migration_plan(pool).await.unwrap();
    assert!(!plan.is_destructive(), "{plan:#?}");
    let upgraded = plan.apply(pool, MigrationOptions::default()).await.unwrap();
    assert_eq!(upgraded.revision, Some(2));
    assert_eq!(upgraded.checksum.len(), 64);
    let a = V2::get(pool, &a.id.unwrap()).await.unwrap().unwrap();
    assert_eq!(a.display_name, "Alice");
    assert_eq!(a.score, 7);
    assert!(a.active);
    assert_eq!(a.legacy.as_deref(), Some("keep"));
    assert!(a.bio.is_none());
    assert_eq!(a.source, "O'Reilly\\reader 中文");
    assert!(V2::migrate(pool).await.unwrap().plan.is_empty());
    code(
        plan.apply(pool, MigrationOptions::default())
            .await
            .unwrap_err(),
        "migration_stale",
    );
    let next = V2 {
        id: None,
        display_name: "Chris".into(),
        legacy: None,
        score: 8,
        active: false,
        bio: None,
        source: "manual".into(),
    }
    .insert(pool)
    .await
    .unwrap();
    assert!(
        next.id.unwrap() > b.id.unwrap(),
        "identity high-water mark must survive SQLite rebuilds"
    );
    let plan = V3::migration_plan(pool).await.unwrap();
    assert!(plan.is_destructive());
    code(
        V3::migrate(pool).await.unwrap_err(),
        "migration_destructive",
    );
    let report = V3::migrate_with(
        pool,
        MigrationOptions {
            allow_destructive: true,
        },
    )
    .await
    .unwrap();
    assert_eq!(report.revision, Some(3));
    assert_eq!(
        V3::get(pool, &a.id.unwrap()).await.unwrap().unwrap().score,
        7
    );
    assert!(V3::migrate(pool).await.unwrap().plan.is_empty());

    ParentV1::migrate(pool).await.unwrap();
    ChildV1::migrate(pool).await.unwrap();
    let parent = ParentV1 {
        id: None,
        name: "parent".into(),
    }
    .insert(pool)
    .await
    .unwrap();
    ChildV1 {
        id: 1,
        parent: parent.id.unwrap(),
    }
    .insert(pool)
    .await
    .unwrap();
    ParentV2::migrate(pool).await.unwrap();
    assert!(
        ChildV1::get(pool, &1).await.unwrap().is_some(),
        "rebuilding a parent must preserve cascading children"
    );
    ChildV2::migrate(pool).await.unwrap();
    assert!(ParentV2::delete(pool, &parent.id.unwrap()).await.is_err());
    ChildV2::delete(pool, &1).await.unwrap();
    assert!(ParentV2::delete(pool, &parent.id.unwrap()).await.unwrap());
    SelfReference::migrate(pool).await.unwrap();
    SelfReference {
        id: 1,
        code: 1,
        parent: None,
    }
    .insert(pool)
    .await
    .unwrap();
    SelfReference {
        id: 2,
        code: 2,
        parent: Some(1),
    }
    .insert(pool)
    .await
    .unwrap();
    WithoutReference::migrate(pool).await.unwrap();
    assert_eq!(
        WithoutReference::get(pool, &2)
            .await
            .unwrap()
            .unwrap()
            .parent,
        Some(1)
    );
    assert!(WithoutReference::migrate(pool)
        .await
        .unwrap()
        .plan
        .is_empty());

    CollisionOptional::migrate(pool).await.unwrap();
    CollisionOptional {
        id: 1,
        legacy: Some("keep".into()),
    }
    .insert(pool)
    .await
    .unwrap();
    CollisionRequired::migrate(pool).await.unwrap();
    assert_eq!(
        CollisionRequired::get(pool, &1)
            .await
            .unwrap()
            .unwrap()
            .rd06_collision,
        "keep"
    );
    CollisionOptional::migrate(pool).await.unwrap();
    CollisionOptional {
        id: 1,
        legacy: None,
    }
    .update(pool)
    .await
    .unwrap();
    CollisionBackfill::migrate(pool).await.unwrap();
    assert_eq!(
        CollisionBackfill::get(pool, &1)
            .await
            .unwrap()
            .unwrap()
            .rd06_collision,
        "fallback"
    );
    assert!(CollisionBackfill::migrate(pool)
        .await
        .unwrap()
        .plan
        .is_empty());
    let (first, second) = tokio::join!(Nullable::migrate(pool), Nullable::migrate(pool));
    assert_eq!(
        usize::from(first.unwrap().revision.is_some())
            + usize::from(second.unwrap().revision.is_some()),
        1
    );
    Nullable { id: 1, value: None }.insert(pool).await.unwrap();
    Required::migrate(pool).await.unwrap();
    let required = Required::get(pool, &1).await.unwrap().unwrap();
    assert_eq!(required.text, "fallback");
    assert_eq!(required.rating, -1.5);
    assert_eq!(required.negative, -7);
    assert!(!required.flag);
    assert!(Required::migrate(pool).await.unwrap().plan.is_empty());
    Number::migrate(pool).await.unwrap();
    Number { id: 1, number: 123 }.insert(pool).await.unwrap();
    code(
        Text::migrate(pool).await.unwrap_err(),
        "migration_destructive",
    );
    Text::migrate_with(
        pool,
        MigrationOptions {
            allow_destructive: true,
        },
    )
    .await
    .unwrap();
    assert_eq!(Text::get(pool, &1).await.unwrap().unwrap().number, "123");
    Text {
        id: 1,
        number: "not a number".into(),
    }
    .update(pool)
    .await
    .unwrap();
    assert!(Number::migrate_with(
        pool,
        MigrationOptions {
            allow_destructive: true
        }
    )
    .await
    .is_err());
    assert!(Text::migrate(pool).await.unwrap().plan.is_empty());
    assert_eq!(
        Text::get(pool, &1).await.unwrap().unwrap().number,
        "not a number"
    );
}
#[cfg(feature = "sqlite")]
#[tokio::test]
async fn sqlite_model_evolution() {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    evolution(&pool).await;
    let enabled: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(enabled, 1);
    let applied: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM _rydantic_migrations WHERE status='applied'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(applied, 17);
}
#[cfg(feature = "postgres")]
#[tokio::test]
#[ignore = "requires RYDANTIC_POSTGRES_URL pointing at a dedicated test database"]
async fn postgres_model_evolution() {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(3)
        .connect(&std::env::var("RYDANTIC_POSTGRES_URL").unwrap())
        .await
        .unwrap();
    evolution(&pool).await;
}
#[cfg(feature = "mysql")]
#[tokio::test]
#[ignore = "requires RYDANTIC_MYSQL_URL pointing at a dedicated test database"]
async fn mysql_model_evolution() {
    let pool = sqlx::mysql::MySqlPoolOptions::new()
        .max_connections(3)
        .connect(&std::env::var("RYDANTIC_MYSQL_URL").unwrap())
        .await
        .unwrap();
    evolution(&pool).await;
}

#[derive(Debug, Model, SqlModel)]
#[sql(table = "rd06_duplicate")]
struct Plain {
    #[sql(primary_key)]
    id: i64,
    value: String,
}
#[derive(Debug, Model, SqlModel)]
#[sql(table = "rd06_duplicate")]
struct Unique {
    #[sql(primary_key)]
    id: i64,
    #[sql(unique)]
    value: String,
}
#[cfg(feature = "sqlite")]
#[tokio::test]
async fn sqlite_constraint_failure_rolls_back_schema_data_and_history() {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    Plain::create_table(&pool).await.unwrap();
    for id in [1, 2] {
        Plain {
            id,
            value: "same".into(),
        }
        .insert(&pool)
        .await
        .unwrap();
    }
    assert!(Unique::migrate(&pool).await.is_err());
    assert!(Plain::migrate(&pool).await.unwrap().plan.is_empty());
    assert_eq!(Plain::select().fetch_all(&pool).await.unwrap().len(), 2);
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_schema WHERE name='_rydantic_migrations'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 0);
}
#[cfg(feature = "sqlite")]
#[tokio::test]
async fn sqlite_migrations_serialize_concurrent_starts() {
    let file = std::env::temp_dir().join(format!("rydantic-migrate-{}.sqlite", std::process::id()));
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(3)
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(&file)
                .create_if_missing(true),
        )
        .await
        .unwrap();
    let (a, b) = tokio::join!(Plain::migrate(&pool), Plain::migrate(&pool));
    assert_eq!(
        usize::from(a.unwrap().revision.is_some()) + usize::from(b.unwrap().revision.is_some()),
        1
    );
    pool.close().await;
    std::fs::remove_file(file).unwrap();
}
#[cfg(feature = "sqlite")]
#[tokio::test]
async fn sqlite_external_objects_are_preserved() {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    Plain::create_table(&pool).await.unwrap();
    execute(
        &pool,
        "CREATE TRIGGER custom_trigger AFTER INSERT ON rd06_duplicate BEGIN SELECT 1; END",
    )
    .await;
    code(
        Unique::migration_plan(&pool).await.unwrap_err(),
        "migration_unsupported",
    );
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn sqlite_empty_rebuild_preserves_deleted_identity_values() {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    ParentV1::migrate(&pool).await.unwrap();
    let parent = ParentV1 {
        id: None,
        name: "deleted".into(),
    }
    .insert(&pool)
    .await
    .unwrap();
    ParentV1::delete(&pool, &parent.id.unwrap()).await.unwrap();
    ParentV2::migrate(&pool).await.unwrap();
    let next = ParentV2 {
        id: None,
        name: "next".into(),
        generation: 1,
    }
    .insert(&pool)
    .await
    .unwrap();
    assert!(next.id.unwrap() > parent.id.unwrap());
}
