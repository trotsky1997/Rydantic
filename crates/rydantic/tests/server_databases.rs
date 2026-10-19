#![cfg(any(feature = "postgres", feature = "mysql"))]
use rydantic::{
    sql::{Compare, Dialect, Order, SqlBackend, ValidationConnection},
    sqlx::{self, Pool, QueryBuilder},
    Model, SqlError, SqlModel, ValidationError,
};
#[cfg(feature = "axum")]
#[path = "../examples/database_api.rs"]
#[allow(dead_code)]
mod api;

#[cfg(feature = "axum")]
async fn http_request(
    address: std::net::SocketAddr,
    method: &str,
    path: &str,
    body: &str,
) -> (u16, rydantic::Value) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    stream.write_all(format!("{method} {path} HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).await.unwrap();
    let text = std::str::from_utf8(&bytes).unwrap();
    let (headers, body) = text.split_once("\r\n\r\n").unwrap();
    (
        headers.split_whitespace().nth(1).unwrap().parse().unwrap(),
        if body.is_empty() {
            rydantic::Value::Null
        } else {
            serde_json::from_str(body).unwrap()
        },
    )
}
#[cfg(feature = "axum")]
async fn http_crud<DB: SqlBackend>(pool: &Pool<DB>) {
    api::User::create_table(pool).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = api::router(pool.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let operations = async {
        assert_eq!(
            http_request(address, "POST", "/users", r#"{"name":"A","age":17}"#)
                .await
                .0,
            422
        );
        let name = format!("HTTP-{}", std::process::id());
        let input = rydantic::json!({"name":format!(" {name} "),"age":25}).to_string();
        let (status, body) = http_request(address, "POST", "/users", &input).await;
        assert_eq!(status, 201);
        assert_eq!(body["name"], name);
        let id = body["id"].as_i64().unwrap();
        assert_eq!(api::User::get(pool, &id).await.unwrap().unwrap().name, name);
        let (status, error) = http_request(address, "POST", "/users", &input).await;
        assert_eq!(status, 409);
        assert_eq!(error["detail"][0]["code"], "sql_constraint");
        assert!(!error.to_string().contains(&name));
        let path = format!("/users/{id}");
        assert_eq!(http_request(address, "GET", &path, "").await.0, 200);
        assert_eq!(
            http_request(
                address,
                "PUT",
                &path,
                &rydantic::json!({"name":name,"age":30}).to_string()
            )
            .await
            .0,
            200
        );
        assert_eq!(api::User::get(pool, &id).await.unwrap().unwrap().age, 30);
        assert_eq!(http_request(address, "DELETE", &path, "").await.0, 204);
        assert_eq!(http_request(address, "GET", &path, "").await.0, 404);
    };
    let result = tokio::time::timeout(std::time::Duration::from_secs(15), operations).await;
    server.abort();
    let _ = server.await;
    result.unwrap();
}

#[derive(Debug, PartialEq, Model, SqlModel)]
#[sql(table = "rd05_records")]
struct Record {
    #[sql(primary_key, auto_increment)]
    id: Option<i64>,
    #[sql(unique)]
    #[model(trim, min_length = 2, max_length = 250)]
    name: String,
    #[sql(index)]
    #[model(ge = 18, le = 120)]
    age: u8,
    active: bool,
    note: Option<String>,
    data: Vec<u8>,
    rating: f32,
    #[sql(skip)]
    #[model(default = " Guest ", trim, lowercase)]
    local: String,
}
fn record(name: &str) -> Record {
    Record {
        id: None,
        name: name.into(),
        age: 25,
        active: true,
        note: None,
        data: vec![0, 128, 255],
        rating: 1.25,
        local: "guest".into(),
    }
}
#[derive(Debug, PartialEq, Model, SqlModel)]
#[sql(table = "rd05_wide")]
struct Wide {
    #[sql(primary_key)]
    id: u128,
    signed: i128,
    limited: u64,
    amount: f64,
}
#[derive(Debug, Model, SqlModel)]
#[sql(table = "rd05_cascade")]
struct Cascade {
    #[sql(primary_key)]
    id: i64,
    #[sql(references = "rd05_records.id", on_delete = "cascade", index)]
    parent_id: i64,
}
#[derive(Debug, Model, SqlModel)]
#[sql(table = "rd05_nullable")]
struct Nullable {
    #[sql(primary_key)]
    id: i64,
    #[sql(references = "rd05_records.id", on_delete = "set_null")]
    parent_id: Option<i64>,
}
#[derive(Debug, Model, SqlModel)]
#[sql(table = "rd05_generated")]
struct Generated {
    #[sql(primary_key, auto_increment)]
    #[model(ge = 100)]
    id: Option<i64>,
}
#[derive(Debug, PartialEq, Model, SqlModel)]
#[sql(table = "select")]
struct NaturalKey {
    #[sql(primary_key, rename = "key", length = 256)]
    id: String,
    #[sql(unique, length = 16)]
    token: Vec<u8>,
}
#[derive(Debug, Model, SqlModel)]
#[sql(table = "rd05_short")]
struct Short {
    #[sql(primary_key)]
    id: i64,
    #[sql(length = 3)]
    text: String,
}
#[cfg(all(feature = "chrono", feature = "decimal"))]
#[derive(Debug, PartialEq, Model, SqlModel)]
#[sql(table = "rd05_payments")]
struct Payment {
    #[sql(primary_key)]
    id: i64,
    date: rydantic::chrono::DateTime<rydantic::chrono::FixedOffset>,
    amount: rydantic::rust_decimal::Decimal,
}

#[cfg(feature = "postgres")]
static PG_POOL: std::sync::OnceLock<sqlx::PgPool> = std::sync::OnceLock::new();
#[cfg(feature = "mysql")]
static MY_POOL: std::sync::OnceLock<sqlx::MySqlPool> = std::sync::OnceLock::new();
async fn check_database(source: &str) -> Result<(), ValidationError> {
    let result = match source {
        #[cfg(feature = "postgres")]
        "postgres" => {
            sqlx::query_scalar::<_, i64>("SELECT CAST(1 AS BIGINT)")
                .fetch_one(PG_POOL.get().unwrap())
                .await
        }
        #[cfg(feature = "mysql")]
        "mysql" => {
            sqlx::query_scalar::<_, i64>("SELECT CAST(1 AS SIGNED)")
                .fetch_one(MY_POOL.get().unwrap())
                .await
        }
        _ => return Err(ValidationError::custom("unknown validator source")),
    };
    result
        .map(|_| ())
        .map_err(|_| ValidationError::custom("database callback failed"))
}
#[derive(Debug, Model, SqlModel)]
#[sql(table = "rd05_checked")]
struct Checked {
    #[sql(primary_key)]
    id: i64,
    #[model(async_validate = check_database)]
    source: String,
}

async fn execute<DB: SqlBackend>(pool: &Pool<DB>, statement: String) -> DB::QueryResult {
    let mut connection = pool.acquire().await.unwrap();
    DB::execute(&mut QueryBuilder::new(statement), &mut *connection)
        .await
        .unwrap()
}
async fn exercise<DB: SqlBackend>(pool: &Pool<DB>)
where
    for<'a> &'a mut DB::Connection: ValidationConnection,
{
    for table in [
        "rd05_cascade",
        "rd05_nullable",
        "rd05_records",
        "rd05_wide",
        "rd05_generated",
        "select",
        "rd05_short",
        "rd05_checked",
        "rd05_payments",
    ] {
        execute(
            pool,
            format!("DROP TABLE IF EXISTS {}", DB::DIALECT.quote(table)),
        )
        .await;
    }
    Record::create_table(pool).await.unwrap();
    Record::create_table(pool).await.unwrap();
    let parsed = Record::parse_json(
        r#"{"name":" Alice ","age":25,"active":true,"note":null,"data":[0,128,255],"rating":1.25}"#,
    )
    .unwrap();
    let mut stored = parsed.insert(pool).await.unwrap();
    let id = stored.id.unwrap();
    let supplied_zero = Record {
        id: Some(0),
        ..record("Zero")
    }
    .insert(pool)
    .await
    .unwrap();
    if DB::DIALECT == Dialect::MySql {
        assert!(supplied_zero.id.unwrap() > 0);
    } else {
        assert_eq!(supplied_zero.id, Some(0));
    }
    Record::delete(pool, &supplied_zero.id.unwrap())
        .await
        .unwrap();
    assert_eq!(
        stored,
        Record {
            id: Some(id),
            ..record("Alice")
        }
    );
    assert_eq!(
        Record::get(pool, &id).await.unwrap(),
        Some(Record {
            id: Some(id),
            ..record("Alice")
        })
    );
    stored.age = 30;
    stored.note = Some("updated".into());
    assert_eq!(stored.update(pool).await.unwrap(), stored);
    assert_eq!(stored.update(pool).await.unwrap(), stored);
    assert!(matches!(
        record("Alice").insert(pool).await,
        Err(SqlError::Database(_))
    ));
    let mut invalid = record("invalid");
    invalid.age = 17;
    assert!(matches!(
        invalid.insert(pool).await,
        Err(SqlError::Validation(_))
    ));
    let injection = "x' OR 1=1 --";
    record("Bob").insert(pool).await.unwrap();
    record(injection).insert(pool).await.unwrap();
    assert_eq!(
        Record::select()
            .where_eq("name", &injection.to_owned())
            .unwrap()
            .fetch_all(pool)
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        Record::select()
            .where_eq("note", &Option::<String>::None)
            .unwrap()
            .fetch_all(pool)
            .await
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        Record::select()
            .where_eq("active", &true)
            .unwrap()
            .fetch_all(pool)
            .await
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        Record::select()
            .filter("age", Compare::GreaterOrEqual, &18u8)
            .unwrap()
            .order_by("name", Order::Ascending)
            .unwrap()
            .offset(1)
            .limit(1)
            .fetch_all(pool)
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        Record::select()
            .order_by("name", Order::Ascending)
            .unwrap()
            .offset(1)
            .fetch_all(pool)
            .await
            .unwrap()
            .len(),
        2
    );
    assert!(Record::select()
        .offset(u64::MAX)
        .fetch_all(pool)
        .await
        .is_err());
    assert!(Record::select()
        .where_eq("name OR 1=1", &"invalid".to_owned())
        .is_err());
    let mut transaction = pool.begin().await.unwrap();
    let temporary = record("rollback").insert(&mut transaction).await.unwrap();
    assert!(Record::get(&mut transaction, &temporary.id.unwrap())
        .await
        .unwrap()
        .is_some());
    if DB::DIALECT == Dialect::MySql {
        assert!(matches!(
            Record::create_table(&mut transaction).await,
            Err(SqlError::Validation(_))
        ));
    }
    transaction.rollback().await.unwrap();
    assert!(Record::get(pool, &temporary.id.unwrap())
        .await
        .unwrap()
        .is_none());
    Cascade::create_table(pool).await.unwrap();
    Nullable::create_table(pool).await.unwrap();
    Cascade {
        id: 1,
        parent_id: id,
    }
    .insert(pool)
    .await
    .unwrap();
    Nullable {
        id: 1,
        parent_id: Some(id),
    }
    .insert(pool)
    .await
    .unwrap();
    assert!(matches!(
        Cascade {
            id: 2,
            parent_id: -1
        }
        .insert(pool)
        .await,
        Err(SqlError::Database(_))
    ));
    assert!(Record::delete(pool, &id).await.unwrap());
    assert!(Cascade::select().fetch_all(pool).await.unwrap().is_empty());
    assert!(Nullable::get(pool, &1)
        .await
        .unwrap()
        .unwrap()
        .parent_id
        .is_none());
    assert!(!Record::delete(pool, &id).await.unwrap());
    assert!(matches!(stored.update(pool).await, Err(SqlError::NotFound)));
    Wide::create_table(pool).await.unwrap();
    let wide = Wide {
        id: u128::MAX,
        signed: i128::MIN,
        limited: i64::MAX as u64,
        amount: f64::MAX,
    };
    assert_eq!(wide.insert(pool).await.unwrap(), wide);
    assert_eq!(Wide::get(pool, &u128::MAX).await.unwrap(), Some(wide));
    assert!(matches!(
        Wide {
            id: 2,
            signed: 0,
            limited: u64::MAX,
            amount: 1.0
        }
        .insert(pool)
        .await,
        Err(SqlError::Validation(_))
    ));
    Generated::create_table(pool).await.unwrap();
    assert!(matches!(
        Generated { id: None }.insert(pool).await,
        Err(SqlError::Validation(_))
    ));
    assert!(Generated::select()
        .fetch_all(pool)
        .await
        .unwrap()
        .is_empty());
    NaturalKey::create_table(pool).await.unwrap();
    let natural = NaturalKey {
        id: "quoted-key".into(),
        token: vec![0, 255],
    };
    assert_eq!(natural.insert(pool).await.unwrap(), natural);
    assert_eq!(
        NaturalKey::get(pool, &natural.id).await.unwrap(),
        Some(natural)
    );
    Short::create_table(pool).await.unwrap();
    if DB::DIALECT == Dialect::MySql {
        let Err(SqlError::Validation(error)) = (Short {
            id: 1,
            text: "long".into(),
        })
        .insert(pool)
        .await
        else {
            panic!("expected SQL length error")
        };
        assert_eq!(error.errors()[0].code, "sql_length");
    }
    Checked::create_table(pool).await.unwrap();
    let source = if DB::DIALECT == Dialect::Postgres {
        "postgres"
    } else {
        "mysql"
    };
    let checked = Checked {
        id: 1,
        source: source.into(),
    };
    let checks = async {
        checked.insert(pool).await.unwrap();
        checked.update(pool).await.unwrap();
        Checked::get(pool, &1).await.unwrap().unwrap();
        Checked::select().fetch_all(pool).await.unwrap();
        let mut transaction = pool.begin().await.unwrap();
        for result in [
            checked.insert(&mut transaction).await.map(|_| ()),
            checked.update(&mut transaction).await.map(|_| ()),
            Checked::get(&mut transaction, &1).await.map(|_| ()),
            Checked::select()
                .fetch_all(&mut transaction)
                .await
                .map(|_| ()),
        ] {
            let Err(SqlError::Validation(error)) = result else {
                panic!("expected connection policy error")
            };
            assert_eq!(error.errors()[0].code, "sql_async_connection");
        }
        transaction.rollback().await.unwrap();
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), checks)
        .await
        .unwrap();
    #[cfg(all(feature = "chrono", feature = "decimal"))]
    {
        Payment::create_table(pool).await.unwrap();
        let payment = Payment {
            id: 1,
            date: "2026-10-20T12:34:56.123456789+08:00".parse().unwrap(),
            amount: "12345678901234567890.12345678".parse().unwrap(),
        };
        assert_eq!(payment.insert(pool).await.unwrap(), payment);
        assert_eq!(Payment::get(pool, &1).await.unwrap(), Some(payment));
    }
    execute(
        pool,
        format!("UPDATE {} SET age = 1", DB::DIALECT.quote(Record::TABLE)),
    )
    .await;
    assert!(matches!(
        Record::select().fetch_all(pool).await,
        Err(SqlError::Validation(_))
    ));
}

#[cfg(feature = "postgres")]
#[tokio::test]
#[ignore = "requires RYDANTIC_POSTGRES_URL pointing to a dedicated test database"]
async fn postgres_live_models_crud_and_transactions() {
    let url = std::env::var("RYDANTIC_POSTGRES_URL").expect("set RYDANTIC_POSTGRES_URL");
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .unwrap();
    assert!(PG_POOL.set(pool.clone()).is_ok());
    exercise::<sqlx::Postgres>(&pool).await;
    #[cfg(feature = "axum")]
    http_crud(&pool).await;
    assert!(sqlx::query_as::<_, Record>("SELECT * FROM rd05_records")
        .fetch_all(&pool)
        .await
        .is_err());
    assert!(sqlx::query_as::<_, Checked>("SELECT * FROM rd05_checked")
        .fetch_all(&pool)
        .await
        .is_err());
    sqlx::query("UPDATE rd05_records SET age = 25")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_as::<_, Record>("SELECT * FROM rd05_records")
            .fetch_all(&pool)
            .await
            .unwrap()
            .len(),
        2
    );
    #[cfg(feature = "axum")]
    {
        use axum::response::IntoResponse;
        let error = record("Bob").insert(&pool).await.unwrap_err();
        assert_eq!(error.into_response().status().as_u16(), 409);
    }
    pool.close().await;
}
#[cfg(feature = "mysql")]
#[tokio::test]
#[ignore = "requires RYDANTIC_MYSQL_URL pointing to a dedicated test database"]
async fn mysql_live_models_crud_and_transactions() {
    let url = std::env::var("RYDANTIC_MYSQL_URL").expect("set RYDANTIC_MYSQL_URL");
    let pool = sqlx::mysql::MySqlPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .unwrap();
    assert!(MY_POOL.set(pool.clone()).is_ok());
    exercise::<sqlx::MySql>(&pool).await;
    #[cfg(feature = "axum")]
    http_crud(&pool).await;
    assert!(sqlx::query_as::<_, Record>("SELECT * FROM rd05_records")
        .fetch_all(&pool)
        .await
        .is_err());
    assert!(sqlx::query_as::<_, Checked>("SELECT * FROM rd05_checked")
        .fetch_all(&pool)
        .await
        .is_err());
    sqlx::query("UPDATE rd05_records SET age = 25")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_as::<_, Record>("SELECT * FROM rd05_records")
            .fetch_all(&pool)
            .await
            .unwrap()
            .len(),
        2
    );
    #[cfg(feature = "axum")]
    {
        use axum::response::IntoResponse;
        let error = record("Bob").insert(&pool).await.unwrap_err();
        assert_eq!(error.into_response().status().as_u16(), 409);
    }
    pool.close().await;
}

#[test]
fn schema_previews_use_native_identity_types_quotes_indexes_and_foreign_keys() {
    #[cfg(feature = "postgres")]
    {
        let ddl = Record::create_table_sql_for::<sqlx::Postgres>();
        assert!(ddl.contains("GENERATED BY DEFAULT AS IDENTITY"));
        assert!(ddl.contains("\"active\" BOOLEAN"));
        assert!(ddl.contains("BYTEA"));
        assert!(Record::create_index_sql_for::<sqlx::Postgres>()[0].contains("IF NOT EXISTS"));
    }
    #[cfg(feature = "mysql")]
    {
        let ddl = Record::create_table_sql_for::<sqlx::MySql>();
        assert!(ddl.contains("`id` BIGINT PRIMARY KEY NOT NULL AUTO_INCREMENT"));
        assert!(ddl.contains("`name` VARCHAR(250)"));
        assert!(ddl.contains("INDEX `rydantic_12_rd05_records_age`"));
        assert!(ddl.contains("ENGINE=InnoDB"));
        assert!(Nullable::create_table_sql_for::<sqlx::MySql>().contains(
            "FOREIGN KEY (`parent_id`) REFERENCES `rd05_records` (`id`) ON DELETE SET NULL"
        ));
        assert!(Record::create_index_sql_for::<sqlx::MySql>().is_empty());
    }
}

#[derive(Debug, Model, SqlModel)]
#[sql(table = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")]
struct LongIdentifier {
    #[sql(primary_key)]
    id: i64,
}

fn assert_identifier_error<T>(result: Result<T, SqlError>) {
    let Err(SqlError::Validation(error)) = result else {
        panic!("expected identifier length error")
    };
    assert_eq!(error.errors()[0].code, "sql_identifier_length");
}
#[tokio::test]
async fn server_identifier_limits_are_checked_before_connecting() {
    #[cfg(feature = "postgres")]
    {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://localhost:9/unused")
            .unwrap();
        assert_identifier_error(LongIdentifier::create_table(&pool).await);
        assert_identifier_error(LongIdentifier { id: 1 }.insert(&pool).await);
        assert_identifier_error(LongIdentifier { id: 1 }.update(&pool).await);
        assert_identifier_error(LongIdentifier::get(&pool, &1).await);
        assert_identifier_error(LongIdentifier::delete(&pool, &1).await);
        assert_identifier_error(LongIdentifier::select().fetch_all(&pool).await);
    }
    #[cfg(feature = "mysql")]
    {
        let pool = sqlx::mysql::MySqlPoolOptions::new()
            .connect_lazy("mysql://localhost:9/unused")
            .unwrap();
        assert_identifier_error(LongIdentifier::create_table(&pool).await);
        assert_identifier_error(LongIdentifier { id: 1 }.insert(&pool).await);
        assert_identifier_error(LongIdentifier { id: 1 }.update(&pool).await);
        assert_identifier_error(LongIdentifier::get(&pool, &1).await);
        assert_identifier_error(LongIdentifier::delete(&pool, &1).await);
        assert_identifier_error(LongIdentifier::select().fetch_all(&pool).await);
    }
    let sqlite = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    LongIdentifier::create_table(&sqlite).await.unwrap();
    assert_eq!(
        LongIdentifier { id: 1 }.insert(&sqlite).await.unwrap().id,
        1
    );
}
