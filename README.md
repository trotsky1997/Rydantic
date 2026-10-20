# Rydantic

[![CI](https://github.com/trotsky1997/Rydantic/actions/workflows/ci.yml/badge.svg)](https://github.com/trotsky1997/Rydantic/actions/workflows/ci.yml)

Unified data models for Rust, source candidate **0.7.0-beta.1**, with JSON, native Cap'n Proto, Axum HTTP adapters, and SQLx database models.

Version 0.6 adds automatic database migrations from Rust models and a unified SQL API. SQLite, PostgreSQL and MySQL drivers are independent optional features. One model defines HTTP input, JSON Schema, database columns, migrations and validated CRUD operations.

Version 0.3 introduces `#[rydantic::capnp_models]`: infer a complete `.capnp` schema from Rust models, generate official Rust bindings, and connect each model automatically. No separate schema, generated source, or `build.rs` needs to be maintained.

One `#[derive(Model)]` declaration provides JSON parsing, explicit type coercion, string normalization, constraints, structured errors, Serde serialization and deserialization, and JSON Schema Draft 2020-12.

The workspace contains the `rydantic` runtime, the `rydantic-derive` procedural macros, a renamed-dependency example, and Cap'n Proto integration examples. It has been verified with Rust 1.98.1 and includes Cargo.lock.

## Documentation

- [Project homepage](https://trotsky1997.github.io/Rydantic/)
- [Guides and tutorials](https://qq-261ce204.mintlify.site/introduction) — the full Mintlify documentation
- [Stable 0.6.0 Rust API reference](https://docs.rs/rydantic/0.6.0/rydantic/)
- [Documentation development](docs/README.md#preview-and-validate)

## Release channels

This branch targets **0.7.0-beta.1**, a prerelease with a breaking explicit-derive contract. The latest stable release is **0.6.0**. Check [crates.io](https://crates.io/crates/rydantic/versions) for publication status before installing a candidate; a version in this source tree does not mean it has been published.

The examples below opt in to this beta exactly. For stable installations use `rydantic = "0.6.0"` and the [v0.6.0 documentation](https://github.com/trotsky1997/Rydantic/tree/v0.6.0). See [release notes](https://github.com/trotsky1997/Rydantic/blob/main/CHANGELOG.md) for breaking changes, backup and migration guidance, and promotion criteria.

## Quick start

After the beta is published, add the opt-in crates.io dependency to your Cargo.toml:

```toml
[dependencies]
rydantic = "=0.7.0-beta.1"
```

```rust
use rydantic::{Model, SchemaMode};

#[derive(Debug, Model)]
#[model(extra = "forbid")]
struct User {
    #[model(trim, min_length = 3, max_length = 30)]
    name: String,

    #[model(coerce, ge = 18, le = 120)]
    age: u8,

    #[model(trim, lowercase, email)]
    email: String,

    #[model(default)]
    tags: Vec<String>,

    note: Option<String>,
}

fn main() -> Result<(), rydantic::ValidationErrors> {
    let user = User::parse_json(
        r#"{"name":" Alice ","age":"25","email":" ALICE@EXAMPLE.COM "}"#,
    )?;

    assert_eq!(user.name, "Alice");
    assert_eq!(user.age, 25);
    assert_eq!(user.email, "alice@example.com");
    println!("{}", user.dump_json()?);

    let schema = User::json_schema(SchemaMode::Output);
    assert!(schema.is_object());
    Ok(())
}
```

`Model` also generates Serde's `Serialize` and `Deserialize` implementations. Derive `Model` once per model. Deserialization runs full validation, and serialization revalidates the current object.

```sh
cargo run -p rydantic --example user
cargo run -p rydantic --example tool_call
cargo run -p rydantic --example tool_call -- --schema
cargo run -p rydantic --example tool_call -- '{"kind":"ping"}'
```

## Supported types and rules

Supported types include `String`, borrowed `&str` and `Cow<str>`, `bool`, signed and unsigned 8/16/32/64/128-bit integers, `isize`, `usize`, `f32`, `f64`, `Option<T>`, `Box<T>`, `Vec<T>`, `[T; N]`, string-keyed `BTreeMap` and `HashMap`, `serde_json::Value`, source-preserving `Box<RawValue>`, and nested, recursive, and generic models.

| Field attribute | Behavior |
| --- | --- |
| `trim`, `lowercase`, `uppercase` | Normalize strings through a fixed pipeline |
| `coerce` | Convert numeric strings to numbers, `"true"`/`"false"` to booleans, and numbers/booleans to strings |
| `min_length`, `max_length` | Count Unicode scalar values in strings, collection elements, or map entries |
| `ge`, `gt`, `le`, `lt` | Apply numeric bounds using number literals or exact decimal strings |
| `pattern = "..."` | Use Rust regex syntax; patterns are checked at compile time and cached at runtime |
| `email` | Validate email addresses with `email_address` |
| `default` | Use the field type's `Default::default()` |
| `default = expression` | Use an explicit default; string literals can be used directly for String fields |
| `default_factory = path` | Call a factory whenever the field is missing |
| `required` | Require the field to appear in the input, including Option fields |
| `rename = "..."` | Set both input and output names |
| `validation_alias`, `serialization_alias` | Set input and output names separately |
| `exclude` | Omit the field from output and the output schema; input rules still apply |
| `before = path` | Transform the raw JSON value before type decoding |
| `validate = path` | Run a custom validator after constructing the typed field |
| `description = "..."` | Add a schema description; field documentation comments are also read automatically |
| `flatten` | Merge a named model or string-keyed map into the current JSON object |
| `extra` | Capture unknown fields in a map, together with container-level `extra = "allow"` |
| `async_validate = path` | Await an asynchronous field validator |

Container attributes: `extra = "forbid"` (default), `extra = "ignore"`, `extra = "allow"`, `title = "..."`, `validate = path`, `async_validate = path`, and `tag = "..."` or `untagged` for enums.

The parsing pipeline is: read input or produce a default → before callback → type decoding and explicit coercion → trim → case conversion → field constraints → custom field validation → custom model validation. Defaults follow the same pipeline. Errors from independent fields are collected together; a parent model's custom validator runs after all its fields succeed.

Numeric strings require explicit `coerce`. A JSON number such as `25.0` is accepted for an integer, while `25.5` fails, following JSON Schema's definition of mathematical integers. Range comparisons preserve 128-bit integer precision, and extremely large exponents are processed with bounded work. Floating-point values follow Rust's IEEE 754 representations; NaN, infinity, and values outside the target type's range fail validation.

## Missing fields and null

| Declaration | May be omitted | Accepts null |
| --- | --- | --- |
| `name: String` | No | No |
| `name: Option<String>` | Yes, produces None | Yes |
| `#[model(required)] name: Option<String>` | No | Yes |
| `#[model(default = "guest")] name: String` | Yes, uses the default | No |

## Custom validation

```rust
use rydantic::{Model, ValidationError, ValidationErrors, Value};

fn expand(value: Value) -> Result<Value, ValidationError> {
    if value == "many" { Ok(rydantic::json!(10)) } else { Ok(value) }
}

fn even(value: &u32) -> Result<(), ValidationError> {
    if value.is_multiple_of(2) { Ok(()) }
    else { Err(ValidationError::new("even", "value must be even")) }
}

fn check_window(value: &Window) -> Result<(), ValidationErrors> {
    if value.end >= value.start { Ok(()) }
    else { Err(ValidationError::custom("end must be >= start").at("end").into()) }
}

#[derive(Model)]
#[model(validate = check_window)]
struct Window {
    start: u32,
    #[model(before = expand, validate = even)]
    end: u32,
}
```

Custom error paths are automatically prefixed with the parent path when nested. `ValidationErrors::errors()` returns an error slice, and `to_json()` returns structured JSON. Each error includes `loc`, `code`, `message`, `stage`, and constraint parameters; `pointer()` returns an escaped JSON Pointer.

```json
[
  {
    "loc": ["addresses", 0, "city"],
    "code": "too_short",
    "message": "length must be >= 2",
    "stage": "validate",
    "params": {"actual": 1, "limit": 2}
  }
]
```

## Enums and tool arguments

Enums with unit variants encode as strings. Enums with fields can encode as objects with an explicit discriminator:

```rust
use rydantic::Model;

#[derive(Model)]
#[model(tag = "kind")]
enum ToolCall {
    #[model(rename = "search")]
    Search {
        #[model(min_length = 1)]
        query: String,
        #[model(default = 3, ge = 1, le = 10)]
        limit: u8,
    },
    #[model(rename = "ping")]
    Ping,
}
```

Inputs include `{"kind":"search","query":"Rust"}` and `{"kind":"ping"}`. The generated schema uses `oneOf`, `const`, and explicit required fields.

## Borrowed models

Declarations with lifetimes use the same `Model` derive and parse through `BorrowModel`. The input buffer must outlive the model's use.

```rust
use rydantic::{BorrowModel, Model};
use std::borrow::Cow;

#[derive(Debug, Model)]
struct View<'a> {
    #[model(trim)]
    name: &'a str,
    #[model(trim, lowercase)]
    note: Cow<'a, str>,
}

let input = String::from(r#"{"name":" Alice ","note":" HELLO\nWORLD "}"#);
let view = View::parse_json(&input)?;
assert_eq!(view.name, "Alice");
assert_eq!(view.note, "hello\nworld");
```

`&str` references the original, unescaped JSON string. `trim` preserves the borrow by slicing it. Use `Cow<str>` when decoding escapes or changing case may require allocation. Inputs that require allocation for an `&str` field return `borrowed_string` or `borrowed_transform` errors. A `before` callback produces an owned JSON value and works with `Cow` or owned fields.

Borrowed models support nesting, recursion, generics, multiple lifetimes, collections, flattening, Serde `from_str`/`from_slice`, and the read-only `BorrowValidated` wrapper. `parse_slice` borrows from a byte buffer; reader-based entry points work with owned models.

## Tuple structs and untagged enums

```rust
use rydantic::Model;

#[derive(Debug, Model)]
struct Username(#[model(trim, min_length = 2)] String);

#[derive(Debug, Model)]
struct Point(#[model(ge = 0)] i32, i32);

#[derive(Debug, Model)]
#[model(untagged)]
enum ResultValue {
    Count(#[model(ge = 1)] u32),
    Named { name: String },
    Pair(String, u8),
    Empty,
}
```

Newtypes encode as their inner value, multiple tuple fields encode as fixed-length arrays, unit structs encode as `null`, and empty tuple structs encode as `[]`. Tuple fields support validation and coercion. Names, defaults, flattening, and exclusion apply to named fields.

Untagged enums try variants in declaration order and select the first one that passes synchronous validation. Failure returns `union_no_match` with structured errors for each branch; the schema uses `anyOf`. Asynchronous validation then checks the selected branch. Internally tagged enums use named fields or unit variants.

## Flattening and extra fields

```rust
use rydantic::{Model, Value};
use std::collections::BTreeMap;

#[derive(Debug, Model)]
struct Address { city: String }

#[derive(Debug, Model)]
#[model(extra = "allow")]
struct Person {
    id: u32,
    #[model(flatten)]
    address: Option<Address>,
    #[model(extra)]
    extra: BTreeMap<String, Value>,
}

let person = Person::parse_json(r#"{"id":1,"city":"Paris","score":9}"#)?;
assert_eq!(person.extra["score"], 9);
assert_eq!(person.dump()?["city"], "Paris");
```

Flattening supports named models, `Option`/`Box` wrappers, and string-keyed `BTreeMap`/`HashMap`. Parsing consumes declared fields first, flattened fields next, and finally sends remaining keys to a capture map. Multiple flattened fixed objects are supported, with one field per level allowed to capture unknown keys. `extra = "allow"` requires an explicit `#[model(extra)]` map, whose values are also type-checked.

Optional flattened fields become `None` when all their keys are absent; partially present objects are checked for required inner fields. Empty flattened input can trigger an explicit default. Error locations follow the flattened JSON paths. Input/output names of declared fields are preserved; overlapping extra keys return `field_collision`, and serialization also checks for key collisions when merging objects.

## Dates and Decimal

```toml
rydantic = { version = "=0.7.0-beta.1", features = ["chrono", "decimal"] }
```

The `chrono` feature supports `NaiveDate`, `NaiveTime`, `NaiveDateTime`, `DateTime<Utc>`, and `DateTime<FixedOffset>`, re-exported through `rydantic::chrono`. Their formats are `YYYY-MM-DD`, `HH:MM:SS[.fraction]`, an ISO date-time without a time zone, and RFC 3339 date-times. Validation checks calendar dates, four-digit years, at most nine fractional-second digits, and RFC 3339 time zones. UTC types are converted to UTC.

The `decimal` feature provides `rydantic::rust_decimal::Decimal`, preserving a 96-bit coefficient and up to 28 fractional digits. Input and output use decimal strings by default and preserve scale; explicit `coerce` also accepts JSON numbers. Loss of precision fails validation, and range constraints use exact decimal comparisons.

```rust
use rydantic::{Model, rust_decimal::Decimal};

#[derive(Debug, Model)]
struct Price {
    #[model(ge = "0.1000000000000000000000000001")]
    amount: Decimal,
}
```

Date semantics, Decimal precision, and numeric bounds on string representations are marked as schema-local rules. Run `cargo run -p rydantic --example dates_decimal --features chrono,decimal` for a complete example.

## Asynchronous validation

```rust
use rydantic::{Model, ValidationError};

async fn available(name: &str) -> Result<(), ValidationError> {
    if name == "taken" {
        Err(ValidationError::new("taken", "name is already registered"))
    } else { Ok(()) }
}

#[derive(Debug, Model)]
struct Account {
    #[model(min_length = 2, async_validate = available)]
    name: String,
}

// Inside an async function:
// let account = Account::parse_json_async(input).await?;
// let json = account.dump_json_async().await?;
```

Asynchronous field validators return `Result<(), ValidationError>`; container validators return `Result<(), ValidationErrors>`. After synchronous parsing and validation succeed, asynchronous validators are awaited in field order. Nested models and collections are traversed recursively, and errors from sibling fields are collected together. The runtime uses `Send` futures and works with executors such as Tokio.

Models with asynchronous rules, and their parent models, use `parse_json_async`/`parse_value_async`, `validate_async`, and `dump_async`/`dump_json_async`. Synchronous parsing, Serde, and synchronous dump return `async_validation_required`. Owned models can use `parse_validated_json_async`/`AsyncValidated::new`; their read-only results support Serde serialization. Borrowed models also support asynchronous parsing, validation, and dumping.

Generated asynchronous support requires field types to implement `AsyncCheck`, which requires `Sync`. A synchronous custom type can provide an empty `impl AsyncCheck for MyType {}`; custom types use this trait to define asynchronous traversal. Types with asynchronous rules must expose the corresponding metadata through `JsonType::async_required`/`BorrowType::async_required`. The library itself has no executor dependency. The `asynchronous` example uses Tokio as a development dependency.

## Streaming parsing

```rust
use rydantic::{stream, Model};
use std::io::Cursor;

#[derive(Debug, Model)]
struct Row { id: u32 }

for row in stream::ndjson::<_, Row>(Cursor::new("{\"id\":1}\n{\"id\":2}\n")) {
    println!("{}", row?.id);
}

stream::visit_array::<_, Row, _>(Cursor::new("[{\"id\":1},{\"id\":2}]"), |row| {
    println!("{}", row?.id);
    Ok(())
})?;
```

| Entry point | Input and error handling |
| --- | --- |
| `Model::parse_reader` | Read one JSON document from `Read` |
| `stream::json_stream` | Read whitespace-separated documents; continue after validation errors and stop after JSON syntax errors |
| `stream::ndjson` | Read lines from `BufRead`; skip blank lines, continue after syntax/validation errors, and attach `stream_line` |
| `stream::visit_array` | Visit each element; validation errors include the array index, and a callback error stops reading |
| `stream::borrow_stream`/`visit_borrowed_array` | Borrow fields from `&str`; returned values remain usable while the input buffer lives |
| `stream::documents` | Yield `Box<RawValue>` documents for calls to `parse_json_async` inside the loop |

These entry points use synchronous I/O. Sequence and array visitors materialize only the current record, so memory use scales with that record's size. The caller controls the reader's buffering strategy. Asynchronous applications can integrate at the appropriate I/O layer and await validation for each model. Synchronous typed streams return `async_validation_required` for models with asynchronous rules.

Run the examples:

```sh
cargo run -p rydantic --example borrowed
cargo run -p rydantic --example streaming
cargo run -p rydantic --example asynchronous
```

## Input and output schemas

`SchemaMode::Input` uses input aliases, input requirements, and the extra-field policy. `SchemaMode::Output` uses output aliases, removes excluded fields, and requires every field that is actually emitted. Recursive models use `$defs`/`$ref`, with definition names containing the full Rust type path.

Trimming, coercion, custom functions, and the specific regex/email implementations require Rydantic's runtime. Exported schemas mark these behaviors with `x-rydantic-local-rules`; `schema_report(mode)` returns their locations and rules. Input constraints affected by transformations use a permissive structural description, while output schemas describe the processed data. Literal defaults appear as annotations only in input schemas. Schema generation never executes default factories.

```rust
let report = User::schema_report(rydantic::SchemaMode::Input);
for local in &report.local_rules {
    println!("{}: {}", local.schema_path, local.rule);
}
```

Schemas can describe API documentation and tool arguments. Use model parsing at execution boundaries to run the local rules.

## Runtime dynamic schemas

The optional `dynamic` feature uses `jsonschema` to compile and reuse schemas at runtime. Network and file resolution are disabled in the default dependency configuration; local `$ref` references are supported.

```toml
rydantic = { version = "=0.7.0-beta.1", features = ["dynamic"] }
```

```rust
use rydantic::{json, DynamicModel};

let model = DynamicModel::compile(json!({
    "type":"object",
    "properties":{"age":{"type":"integer","minimum":18}},
    "required":["age"],
    "additionalProperties":false
}))?;

assert!(model.is_valid(&json!({"age":25})));
assert!(model.validate(&json!({"age":17})).is_err());
```

`DynamicModel::compile` rejects schemas containing `x-rydantic-local-rules`. `compile_structural` explicitly opts into standard JSON Schema assertions only. Dynamic validation follows the backend's default format-annotation semantics and preserves the input. Typed models handle default filling and coercion.

## Object lifecycle

Rust code can directly construct or modify ordinary models. Call `validate()` to check the current object; `dump()`/`dump_json()` revalidate it before serialization. These checks preserve field values. Normalization takes place during parsing.

For read-only access, use `User::parse_validated_json(...)` or `Validated::new(user)` to obtain a `Validated<User>` without `DerefMut`. `into_inner()` returns the ordinary model. Custom JsonType implementations with interior mutability are responsible for their own mutation rules.

## Axum and Tokio

Enable the `axum` feature to use Axum 0.8 adapters:

```toml
[dependencies]
rydantic = { version = "=0.7.0-beta.1", features = ["axum"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread", "net"] }
```

```rust
use rydantic::{Model, axum::{Json, Router, routing::post}};

#[derive(Model)]
struct User {
    #[model(trim, min_length = 2)]
    name: String,
    #[model(ge = 18)]
    age: u8,
}

async fn create_user(Json(user): Json<User>) -> Json<User> {
    Json(user)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app = Router::new().route("/users", post(create_user));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;
    rydantic::axum::framework::serve(listener, app).await?;
    Ok(())
}
```

`rydantic::axum::Json<T>` reads raw JSON and applies model normalization and validation. It also supports collections such as `Json<Vec<User>>`. Responses revalidate the model, apply output aliases and exclusions, and set `Content-Type: application/json`. The complete Axum API is re-exported through `rydantic::axum::framework`; `Router` and `routing` are also available directly.

| Failure | HTTP status |
| --- | --- |
| Invalid JSON syntax | 400 |
| Missing or unsupported JSON content type | 415 |
| Request body over Axum's configured limit | 413 |
| Input type or model constraint errors | 422 |
| Invalid model returned by a handler | 500 |

Error bodies use `{"detail": [ValidationError, ...]}` and preserve field paths, codes, stages, and constraint parameters. Axum's `DefaultBodyLimit` applies, and `application/*+json` content types are supported.

For asynchronous validators, extract `AsyncJson<T>` and await output validation explicitly:

```rust
use rydantic::axum::{AsyncJson, Rejection, framework::response::Response};

async fn create_account(
    AsyncJson(account): AsyncJson<Account>,
) -> Result<Response, Rejection> {
    AsyncJson(account).response().await
}
```

Here `Account` is a model with asynchronous validators, as shown in the asynchronous validation section. Tokio provides the asynchronous runtime; Axum supplies the HTTP routing and extraction layer.

## SQLModel-style database models

Use SQLx 0.9 and Tokio with SQLite, PostgreSQL or MySQL. Enable `sqlite` for bundled SQLite, `postgres` for PostgreSQL, or `mysql` for MySQL. Each driver enables the shared `sqlx` integration. Server drivers enable Rustls TLS support and have no SQLite dependency:

```toml
[dependencies]
rydantic = { version = "=0.7.0-beta.1", features = ["axum", "sqlite"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

```rust
use rydantic::{Model, SqlModel, sqlx::sqlite::SqlitePoolOptions};

#[derive(Debug, PartialEq, Model, SqlModel)]
#[sql(table = "users")]
struct User {
    #[sql(primary_key, auto_increment)]
    id: Option<i64>,
    #[model(trim, min_length = 2)]
    #[sql(unique)]
    name: String,
    #[model(ge = 18)]
    #[sql(index)]
    age: u8,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await?;
    User::migrate(&pool).await?;

    let user = User::parse_json(r#"{"name":" Alice ","age":25}"#)?;
    let mut stored = user.insert(&pool).await?;
    let id = stored.id.expect("generated primary key");
    assert_eq!(User::get(&pool, &id).await?.as_ref(), Some(&stored));
    stored.age = 30;
    let updated = stored.update(&pool).await?;
    assert_eq!(updated.age, 30);
    assert!(User::delete(&pool, &id).await?);
    Ok(())
}
```

Derive `Model` and `SqlModel` together. `Model` supplies JSON/Serde/schema/validation behavior; `SqlModel` adds table metadata, a validated SQLx `FromRow` implementation, and asynchronous persistence operations. Each table has one primary key and uses a concrete owned struct with named fields. SQLx is re-exported through `rydantic::sqlx`.

| SQL attribute | Behavior |
| --- | --- |
| `#[sql(table = "users")]` | Set the table name; the default is the struct name in snake_case |
| `#[sql(primary_key)]` | Declare the single primary-key field |
| `#[sql(auto_increment)]` | Generate an `i64` or `Option<i64>` primary key when its value is zero or None |
| `#[sql(rename = "display_name")]` | Set the database column name independently of JSON aliases |
| `#[sql(rename_from = "name")]` | Rename a previous database column while preserving its data |
| `#[sql(default = "pending")]` | Set a constant SQL default and backfill a new required column |
| `#[sql(unique)]` | Generate a database uniqueness constraint |
| `#[sql(index)]` | Generate an index together with the table |
| `#[sql(length = 256)]` | Set a MySQL VARCHAR/VARBINARY bound; otherwise use model `max_length`, or 191 for indexed text/binary columns |
| `#[sql(references = "users.id", on_delete = "cascade")]` | Add a foreign key; deletion policies include `no_action`, `restrict`, `cascade`, and `set_null` (requires a nullable non-key column) |
| `#[sql(skip)]` | Keep a local field outside the table; provide a model default or optional field for row decoding |

SQL identifiers are checked at compile time and quoted in generated statements. CRUD and filter values use bound parameters. Column metadata is exposed as `User::COLUMNS`; `create_table_sql::<DB>()` and `create_index_sql::<DB>()` expose generated DDL for an explicit driver.

### PostgreSQL and MySQL

Enable either driver or both:

```toml
rydantic = { version = "=0.7.0-beta.1", features = ["axum", "postgres", "mysql"] }
```

Keep the same `User` model and CRUD calls. The pool or transaction type selects the driver:

```rust
use rydantic::sqlx::{postgres::PgPoolOptions, mysql::MySqlPoolOptions};

let postgres = PgPoolOptions::new()
    .connect(&std::env::var("POSTGRES_URL")?).await?;
let mysql = MySqlPoolOptions::new()
    .connect(&std::env::var("MYSQL_URL")?).await?;

User::migrate(&postgres).await?;
User::migrate(&mysql).await?;
let user = User::parse_json(r#"{"name":" Alice ","age":25}"#)?;
let pg_user = user.insert(&postgres).await?;
let my_user = user.insert(&mysql).await?;
assert!(User::get(&postgres, &pg_user.id.unwrap()).await?.is_some());
assert!(User::get(&mysql, &my_user.id.unwrap()).await?.is_some());
let users = User::select().limit(20).fetch_all(&postgres).await?;
```

| Backend | Feature | SQLx pool | Generated key |
| --- | --- | --- | --- |
| SQLite | `sqlite` | `SqlitePool` | INTEGER PRIMARY KEY AUTOINCREMENT |
| PostgreSQL | `postgres` | `PgPool` | BIGINT GENERATED BY DEFAULT AS IDENTITY |
| MySQL 8 | `mysql` | `MySqlPool` | BIGINT AUTO_INCREMENT |

Each enabled driver gets a validated SQLx `FromRow` implementation. The adapter uses native parameter binding, identifier quoting and pagination syntax. PostgreSQL and SQLite return written rows with `RETURNING`. MySQL reads the stored row by its key on the same transaction and connection, then validates it before committing; unchanged updates still return the existing row. Omitted keys use the returned insert ID. Explicit zero keys follow MySQL's SQL mode, and the returned model contains the stored key.

Use `User::create_table_sql::<rydantic::sqlx::Postgres>()` and `User::create_index_sql::<rydantic::sqlx::Postgres>()` to preview DDL. Select `Sqlite` or `MySql` as the type parameter for another driver. Schema creation and CRUD reject overlong server identifiers before executing SQL. Generated server index and foreign-key names fit identifier limits.

MySQL creates InnoDB tables with `utf8mb4_0900_bin` collation and inline indexes/foreign keys. Unbounded text/binary fields use LONGTEXT/LONGBLOB. Keys, unique/indexed fields and references use VARCHAR/VARBINARY: `#[sql(length = N)]` overrides the model's `max_length`, with a default of 191 when neither is set. Writes check those bounds before executing SQL and report `sql_length` with a field path. Select bounds that fit the server's row/index limits, and use compatible definitions for referenced columns.

MySQL `create_table` requires a pool and rejects borrowed connections or transactions with `sql_ddl_connection`, preserving caller-owned transactions from DDL's implicit commits. Its indexes are included when creating a new table, so `create_index_sql::<MySql>()` returns an empty list. Use `migrate` to upgrade existing tables from model definitions. PostgreSQL and SQLite schema creation can participate in caller-owned transactions.

Configure production connection options with SQLx's [PgConnectOptions](https://docs.rs/sqlx/0.9.0/sqlx/postgres/struct.PgConnectOptions.html) or [MySqlConnectOptions](https://docs.rs/sqlx/0.9.0/sqlx/mysql/struct.MySqlConnectOptions.html), including the desired TLS verification policy.

### Queries and transactions

```rust
use rydantic::sql::{Compare, Order};

let users = User::select()
    .filter("age", Compare::GreaterOrEqual, &18u8)?
    .order_by("name", Order::Ascending)?
    .limit(20)
    .offset(0)
    .fetch_all(&pool)
    .await?;

let mut transaction = pool.begin().await?;
let stored = user.insert(&mut transaction).await?;
assert!(User::get(&mut transaction, &stored.id.unwrap()).await?.is_some());
transaction.rollback().await?;
```

`where_eq`, comparison filters, ordering, limits, and offsets use checked column names. NULL equality/inequality becomes `IS NULL`/`IS NOT NULL`. Operations accept a pool, mutable connection, or SQLx transaction. Insert/update operations use a transaction or nested savepoint; failed constraints or returned-row validation roll back the write. Missing rows return `None` from `get`, `false` from `delete`, or `SqlError::NotFound` from `update`.

Writes await synchronous and asynchronous model validation before acquiring a connection, then recheck returned rows synchronously before committing. Reads validate stored typed values and release acquired pooled connections before awaiting asynchronous callbacks. Models with asynchronous rules require a pool: insert, update, get and select return `sql_async_connection` immediately for borrowed connections and transactions, which can otherwise hold the only pool slot needed by a callback. Synchronous models support all three connection forms. Normalization runs during JSON/model parsing; persistence checks preserve stored typed values. Skipped fields use the same default preparation, `before` callbacks, and normalization as a missing JSON field. Synchronous SQLx `query_as` uses the generated `FromRow` validator; models with asynchronous rules use the awaiting `SqlModel` entry points.

Portable fields use these native storage declarations:

| Rust field | SQLite | PostgreSQL | MySQL |
| --- | --- | --- | --- |
| Integer types through 64 bits | INTEGER | BIGINT | BIGINT |
| `bool` | INTEGER | BOOLEAN | BOOLEAN |
| `f32`, `f64` | REAL | DOUBLE PRECISION | DOUBLE |
| `String` | TEXT | TEXT | LONGTEXT or bounded VARCHAR |
| `Vec<u8>` | BLOB | BYTEA | LONGBLOB or bounded VARBINARY |
| Dates, Decimal, `i128`, `u128` | TEXT | TEXT | LONGTEXT or bounded VARCHAR |

Unsigned 64-bit values use the common signed 64-bit SQL range; conversions reject overflow. Decimal and 128-bit text storage preserves precision. Comparisons and ordering for these text fields follow the database's text semantics. `Option<T>` uses correctly typed SQL NULL parameters, and `Box<T>` uses its inner representation. Enable `chrono`/`decimal` for the corresponding field types. Identity sequences follow each database's native behavior, including gaps after rolled-back writes.

Custom SQL field representations implement the required `SqlValue::to_parameter` and `from_parameter` conversions through `SqlParameter`. The same codec handles all enabled drivers and filters. `SqlModel` owns table metadata and CRUD; `SqlRow<DB>` owns typed row decoding and column binding. The derive generates both traits without driver-specific compatibility hooks.

`create_table` creates missing tables with their declared indexes. `migrate` also upgrades existing tables from the current model definition. Foreign keys describe database references; application queries load related models. The table layer covers single-column keys and the portable scalar mappings above.

### SQL architecture

The SQL API uses one portable codec and explicit driver features. Select `sqlite`, `postgres` or `mysql` in Cargo.toml. The shared `sqlx` feature provides metadata and generic APIs without a database driver. DDL previews require a database type parameter: `create_table_sql::<DB>()` and `create_index_sql::<DB>()`. Custom codecs implement both portable conversions; row decoding and binding use `SqlRow<DB>`.

### Automatic model migrations

Call `User::migrate(&pool).await?` at startup to create or upgrade its table. Rydantic compares the live SQLite, PostgreSQL or MySQL catalog with `SqlModel` metadata, generates the upgrade, executes it, and verifies the resulting schema. Tables previously created by `create_table` can be upgraded directly. Matching schemas produce a no-op receipt without adding history rows.

For example, evolve an existing `accounts(id, name)` model:

```rust
use rydantic::{Model, SqlModel};

#[derive(Debug, Model, SqlModel)]
#[sql(table = "accounts")]
struct Account {
    #[sql(primary_key, auto_increment)]
    id: Option<i64>,
    #[sql(rename_from = "name", unique, length = 128)]
    display_name: String,
    #[sql(default = true)]
    enabled: bool,
    note: Option<String>,
}

let report = Account::migrate(&pool).await?;
```

The rename preserves existing values. Old rows receive `enabled = true` and `note = NULL`. On MySQL, keep compatible `length` bounds on the earlier model so adding an index does not also narrow LONGTEXT to VARCHAR. Storage narrowing requires the destructive opt-in below.

SQL defaults accept string, boolean, signed 64-bit integer, and finite float literals matching the column storage type. They remain database defaults for future SQL inserts that omit the field. Model parsing defaults and factories are configured independently with `#[model(default = ...)]`. Making an optional field required also backfills existing NULL values from its SQL default. Adding a required column to a populated table without a SQL default returns `migration_data` before changing its schema.

Preview and apply an exact plan:

```rust
use rydantic::sql::MigrationOptions;

let plan = Account::migration_plan(&pool).await?; // Read-only.
for change in plan.changes() {
    println!("{} (destructive: {})", change.description, change.destructive);
}
for statement in plan.statements() {
    println!("{statement}");
}
let report = plan.apply(&pool, MigrationOptions::default()).await?;
```

`apply` checks the catalog again under the migration lock and returns `migration_stale` if it changed after preview. Plans expose immutable statements and changes. `migrate` plans under the lock to serialize concurrent startups. These operations accept pools so they can manage their own connection and transaction lifecycle.

| Model change | Automatic behavior |
| --- | --- |
| New table | Create columns, defaults, keys, indexes and foreign keys |
| Added optional field | Preserve rows and initialize NULL or its SQL default |
| Added required field | Backfill its SQL default; a missing default is allowed on an empty table |
| `rename_from` | Rename a previous column when the new name is absent; the annotation can remain across restarts |
| Nullability/default change | Upgrade constraints/defaults and backfill NULL when a SQL default exists |
| `index` / `unique` change | Add or remove single-column indexes/uniqueness; conflicting data fails validation by the database |
| `references` / `on_delete` change | Add, replace or remove a single-column foreign key |
| MySQL VARCHAR/VARBINARY widening | Upgrade without a destructive opt-in |
| Removed field or other storage type change | Reject by default with `migration_destructive` |

After inspecting a destructive plan, enable its changes for that invocation:

```rust
Account::migrate_with(
    &pool,
    MigrationOptions { allow_destructive: true },
).await?;
```

Storage conversions use database casts and constraints. SQLite checks existing storage classes before type-changing rebuilds and rejects incompatible values. The opt-in permits planned data loss; custom data transformations require separate application logic. Primary-key/identity strategy changes, composite keys/indexes, generated columns and custom foreign-key semantics require an explicit migration. Table renames are outside this per-table API. Apply referenced models before dependent models, and coordinate changes to referenced key types separately.

SQLite uses transactional rebuilds when needed, preserves auto-increment high-water marks and dependent rows, checks foreign-key integrity before commit, and restores the connection's foreign-key setting. Rebuilds reject external triggers, views, CHECK constraints, custom index collations or descending key order, and custom storage options whose definitions are outside the model. Ordinary BINARY ascending indexes remain supported; no-op migrations and native rename-only changes preserve custom indexes without rebuilding them. PostgreSQL uses a transaction, advisory lock and table lock; schema and history roll back together on failure.

MySQL uses a database-scoped advisory lock and requires strict SQL mode. Structural changes are grouped into one ALTER TABLE where possible. MySQL DDL implicitly commits: a sequence containing a NULL backfill or separate creation statements can leave completed work after a later failure. History records `applying`, `applied` or `failed`; interruption can leave an `applying` entry. After correcting the cause, retry `migrate`, which computes remaining work from the live catalog. See [MySQL's DDL transaction rules](https://dev.mysql.com/doc/refman/8.4/en/implicit-commit.html).

`_rydantic_migrations` stores per-table revisions, SHA-256 target checksums, source/target schema snapshots, change summaries and timestamps. SQLite/PostgreSQL record successful upgrades; MySQL also retains failed attempts. Snapshots include SQL defaults. Automatic upgrades cover SQL metadata; application validation rules continue to run through model parsing and CRUD.

Run the complete old-model-to-new-model example:

```sh
cargo run -p rydantic --example automatic_migrations --features sqlite
```

### Complete HTTP and database example

The [database_api example](https://github.com/trotsky1997/Rydantic/blob/main/crates/rydantic/examples/database_api.rs) selects SQLite, PostgreSQL or MySQL from `DATABASE_URL` and uses the same model for validated HTTP input, JSON responses, table creation and CRUD:

```sh
cargo run -p rydantic --example database_api --features axum,sqlite
# Connect the same API to a server database:
DATABASE_URL="$POSTGRES_URL" cargo run -p rydantic --example database_api --features axum,postgres
DATABASE_URL="$MYSQL_URL" cargo run -p rydantic --example database_api --features axum,mysql
# POST /users, GET /users, GET/PUT/DELETE /users/{id}
curl -X POST http://127.0.0.1:3000/users \
  -H 'Content-Type: application/json' \
  -d '{"name":" Alice ","age":25}'

# Run the example without starting a persistent server:
cargo run -p rydantic --example database_api --features axum,sqlite -- --smoke
```

The `sqlite_api` example provides a smaller SQLite-only server. The multi-database example defaults to an in-memory database and only accepts loopback bind addresses. Set `DATABASE_URL='sqlite://app.db?mode=rwc'` for a local database file and `LISTEN_ADDR` for another loopback address or port. With both integrations enabled, `SqlError` implements Axum responses: validation errors return 422, missing rows return 404, database constraint violations return 409, and other driver errors return a generic 500 response.

## Native Cap'n Proto

With the `capnp` feature, models read and build fields in [official Cap'n Proto](https://capnproto.org/) schemas directly. The binary layout interoperates with official implementations in other languages and supports both standard and packed messages.

### Generate a protocol from Rust models

Add one dependency:

```toml
[dependencies]
rydantic = { version = "=0.7.0-beta.1", features = ["capnp"] }
```

Install the official `capnp` compiler when setting up the development environment: `brew install capnp` on macOS, or `sudo apt-get install capnproto` on Debian/Ubuntu. During compilation, the attribute macro invokes that compiler and uses the official `capnpc` generator for Rust bindings. Set `RYDANTIC_CAPNP_COMPILER` to use a specific compiler executable. Bare names (such as `capnp`) use `PATH`; relative paths (such as `./tools/capnp`) are resolved from the compiler process's working directory before entering the temporary schema directory.

```rust
use rydantic::{CapnpModel, CapnpSchema, Model};

#[rydantic::capnp_models(namespace = "my_app.users")]
pub mod models {
    #[derive(rydantic::Model, Debug, PartialEq)]
    pub struct User {
        #[model(trim, min_length = 1)]
        pub name: String,
        #[model(ge = 18)]
        pub age: u8,
        pub role: Role,
        pub nickname: Option<String>,
        pub avatar: Vec<u8>,
    }

    #[derive(rydantic::Model, Debug, PartialEq)]
    pub enum Role { Guest, Admin }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    use models::User;
    let user = User::parse_json(
        r#"{"name":" Alice ","age":25,"role":"Admin","avatar":[0,255]}"#,
    )?;
    let bytes = user.dump_capnp()?;
    assert_eq!(User::parse_capnp(&bytes)?, user);
    let packed = user.dump_capnp_packed()?;
    assert_eq!(User::parse_capnp_packed(&packed)?, user);

    // Export the complete protocol for official generators in other languages.
    User::write_capnp_schema("users.capnp")?;
    Ok(())
}
```

Each model must explicitly derive `rydantic::Model` (or an imported alias). `capnp_models` generates the schema and binding attributes; Rust resolves and runs the derives you write. Other derives such as `Debug` and `PartialEq` are preserved. The module exposes:

- `models::wire`: public Rust Reader, Builder, and Enum bindings from the official generator.
- `models::CAPNP_SCHEMA` and `CAPNP_SCHEMA_ID`: the complete schema text and file ID.
- `CapnpSchema` for each model: `capnp_schema()`, `write_capnp_schema(path)`, and the root type name `CAPNP_TYPE`. Unit enums also support binary roundtrips as root messages through automatically generated root wrapper types.

Default mappings cover nested and recursive models, `Box`, `Option`, `Vec`, fixed arrays, String-keyed `BTreeMap`/`HashMap`, unit enums, and data enums. `Vec<u8>` becomes `Data`; full-range `i128`/`u128`, dates, and Decimal become `Text`, including inside containers. Module-local type aliases, container aliases with concrete type arguments, explicit `use` aliases, and inline submodules work together. Dates and Decimal require the `chrono` and `decimal` features respectively.

Models must use concrete, owned types within the annotated inline module. Data enums still declare their JSON representation: named fields can use `#[model(tag = "kind")]`, while tuple variants use `#[model(untagged)]`. `capnp_name` controls the protocol name independently. `capnp_skip` with a default preserves a local field. JSON aliases, flattening, extra fields, and exclusion continue to control the JSON representation. Unsupported external types can use a default with `capnp_skip`, or the existing-schema integration below.

### Breaking change: explicit Model derives

Version 0.7.0-beta.1 removes automatic `Model` derive insertion inside `capnp_models`. Existing code that relied on it will no longer compile. Add `#[derive(rydantic::Model)]` to every model struct and enum in the annotated module, including nested modules, or add `rydantic::Model` to the model's existing derive list. With a renamed dependency, use that dependency's name instead. Already-explicit derives, including aliases imported outside the annotated module, need no change. An unrelated derive named `Model` does not implement Rydantic's model traits.

This is an intentional source-breaking change. Review and update your application before upgrading; the library does not maintain the old implicit-derive behavior. The derive change alone does not change the generated wire schema.

### Protocol IDs and compatible evolution

The default file ID is derived deterministically from the Cargo package name and the current model module name. For production protocols, set a fixed `namespace = "my_app.users"` or `id = 0x...`. Give distinct namespaces to multiple model modules with the same name in one package. Keep the namespace/ID, model names, and submodule paths stable after publication.

Fields and enum members are numbered from `@0` in declaration order by default. Appending fields preserves existing ordinals. To reorder declarations, pin each wire field or enum member with `#[model(capnp_ordinal = N)]`; ordinals must be unique, contiguous, and start at zero. For example:

```rust
#[rydantic::capnp_models(namespace = "my_app.counters")]
mod models {
    #[derive(rydantic::Model)]
    pub struct Counter {
        #[model(capnp_ordinal = 1)]
        pub count: u64,
        #[model(capnp_ordinal = 0)]
        pub name: String,
    }
}
```

Named and tuple group fields in data enums share the protocol's ordinal space. A variant's `capnp_ordinal` specifies its first field ordinal; fields can also have explicit ordinals. Type changes, field removal, renaming, and union regrouping must follow the [official compatibility rules](https://capnproto.org/language.html#evolving-your-protocol). Export schemas for code review; Rust bindings update automatically with the models.

See [fixtures/auto-capnp](https://github.com/trotsky1997/Rydantic/tree/main/fixtures/auto-capnp) for complete examples and interoperability tests:

```sh
cargo run -p rydantic-auto-capnp-fixture
cargo test -p rydantic-auto-capnp-fixture
```

### Integrate an existing schema

Existing `.capnp` protocols can use explicit bindings:

```toml
[dependencies]
rydantic = { version = "=0.7.0-beta.1", features = ["capnp"] }
capnp = { version = "0.27", features = ["unaligned"] }

[build-dependencies]
capnpc = "0.27"
```

Generate a file ID with `capnp id`, then create `schema/user.capnp`:

```capnp
@0xc886a6cb051d0cab;
struct User {
  name @0 :Text;
  age @1 :UInt16;
}
```

Call the official code generator from `build.rs`; the `capnp` compiler must be installed locally:

```rust
fn main() {
    println!("cargo:rerun-if-changed=schema/user.capnp");
    capnpc::CompilerCommand::new()
        .src_prefix("schema")
        .file("schema/user.capnp")
        .run()
        .expect("generate Cap'n Proto bindings");
}
```

Include the generated bindings at the crate root and connect the model:

```rust
pub mod user_capnp {
    include!(concat!(env!("OUT_DIR"), "/user_capnp.rs"));
}
use rydantic::{CapnpModel, Model};

#[derive(Debug, Model)]
#[model(capnp = "user_capnp::user")]
struct User {
    #[model(trim, min_length = 2)]
    name: String,
    #[model(ge = 18, le = 120)]
    age: u8,
}

fn main() -> Result<(), rydantic::ValidationErrors> {
    let user = User::parse_json(r#"{"name":" Alice ","age":25}"#)?;
    let bytes = user.dump_capnp()?;
    assert_eq!(User::parse_capnp(&bytes)?.name, "Alice");
    let packed = user.dump_capnp_packed()?;
    assert_eq!(User::parse_capnp_packed(&packed)?.age, 25);
    Ok(())
}
```

| Rust model | Native schema field |
| --- | --- |
| Booleans, integers, floats, String | Bool, corresponding numeric types, Text; integer-width conversions check overflow |
| Nested models, Box | The associated model's Struct |
| Vec, fixed arrays | List; fixed arrays check length, with nested lists and struct lists supported |
| Option | A wrapper Struct containing `union { none @0 :Void; some @1 :T; }` |
| String-keyed maps | `List(Entry)` with `key :Text` and `value :T`; duplicate keys are rejected |
| `#[model(capnp_data)] Vec<u8>` | Data |
| `#[model(capnp_text)]` | Text for dates, Decimal, and full-range i128/u128 scalar representations |
| `#[model(capnp_enum)]` fields | Native Enum; the Rust enum declares `#[model(capnp_enum = "wire_capnp::Role")]` |
| Data enums | Native unions with unit, single-field, and named group variants; JSON uses `untagged` or a supported `tag` representation |
| Newtype, tuple, unit structs | Structs containing `value`, `item0`/`item1`, or no fields respectively |

Generated accessors are found using Rust field names by default. `#[model(capnp_name = "fullName")]` selects a protocol field independently; `#[model(capnp_skip, default = "local")]` retains an internal model field. JSON aliases, flattening, extra fields, and exclusion control the JSON representation. The `.capnp` file determines the native field layout.

`parse_capnp` performs model normalization and synchronous validation; `dump_capnp` revalidates the current object. Asynchronous models use `parse_capnp_async`/`dump_capnp_async` and their packed equivalents. `parse_capnp_with_options` accepts the official `ReaderOptions` for traversal and nesting limits; single-message entry points reject trailing data. `from_capnp_reader` accepts an officially generated Reader, and `to_capnp_message` returns a message Builder for use with the official API. `read_capnp`/`write_capnp` are low-level composition interfaces; use the public parsing and output entry points above for full validation.

Native bindings require concrete, owned models. Constructing a Rust model allocates Strings and collections; keep an official Reader when direct message-memory access is needed. Protocol defaults follow Cap'n Proto semantics. Optional values use explicit unions, and `capnp_skip` fields use model defaults. JSON `before` callbacks still receive a field Value, and `capnp_text` mappings for dates and Decimal reuse scalar conversion rules.

See [fixtures/capnp](https://github.com/trotsky1997/Rydantic/tree/main/fixtures/capnp) for a complete schema, officially generated bindings, and an executable example. Run the example and official CLI interoperability tests:

```sh
cargo run -p rydantic-capnp-fixture
cargo test -p rydantic-capnp-fixture --test native
# Requires the system capnp compiler:
cargo test -p rydantic-capnp-fixture --test native -- --include-ignored
# Regenerate and format bindings after changing the example schema:
RYDANTIC_REGENERATE_CAPNP=1 cargo check -p rydantic-capnp-fixture
cargo fmt --all
```

## Implementation scope

Version 0.2 completed the eight follow-up areas listed in the 0.1 README: borrowed fields, tuple structs, untagged unions, flattening, built-in dates/Decimal, asynchronous validators, extra-field preservation, and streaming parsing. It also added 128-bit integers.

Owned models use `JsonType`/`Model`; borrowed models use `BorrowType<'de>`/`BorrowModel<'de>`. Both JSON parsing paths construct models and collections directly from raw field slices in `RawValue`, without building a complete Value tree for the input. Typed defaults are validated through direct traversal as well. Owned models construct Strings as needed; borrowed models can retain string references into the input. Reader entry points buffer the raw text of one document, while streaming entry points retain the current record.

Explicit `Value` fields, `parse_value` inputs, `before` callbacks, and custom types implementing only `decode(Value, …)` use Value through their respective interfaces. Native scalar types reuse lightweight scalar adapters. Custom types can override `JsonType::decode_input` to accept `Input::Borrowed` directly. Arbitrary JSON that requires no traversal can be stored as `Box<RawValue>`, preserving its source text and numeric notation.

`StringType`, `NumericType`, and `LengthType` are independent capability markers; generic helpers must explicitly declare `JsonType` or `BorrowType` bounds. Custom field types also implement `AsyncCheck` and satisfy `Sync`; existing synchronous types can provide an empty `AsyncCheck` implementation. Install with `rydantic = "=0.7.0-beta.1"` from crates.io after publication, or use the matching GitHub v0.7.0-beta.1 tag when available. Workspace members use local path dependencies.

## Validation

```sh
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
cargo check -p rydantic --no-default-features --lib
cargo run -p rydantic-renamed-fixture
```

Tests cover complete parsing/serialization flows; recursive, generic, and borrowed models; missing fields and null; normalization and defaults; exact 128-bit integer boundaries; dates and Decimal; asynchronous validation and cancellation recovery; incremental streaming; error paths; independent JSON Schema validation; and compile-fail cases. The boundary matrix covers 1,400 inputs. The independent validator also checks that successful outputs satisfy their output schemas.

License: MIT.
