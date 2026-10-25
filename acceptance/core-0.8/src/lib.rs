//! First-party synthetic order service acceptance; not external adoption evidence.
use rydantic::axum::framework::{
    self as axum,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
};
use rydantic::sqlx::{self, SqlitePool};
use rydantic::{Context, Model, ValidationError, Value, json, pipeline::Pipeline};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Debug)]
pub struct Policy {
    pub tenant: &'static str,
    pub maximum: i64,
    pub transforms: AtomicUsize,
    pub prepared: AtomicUsize,
}
impl Policy {
    pub fn new(tenant: &'static str, maximum: i64) -> Self {
        Self {
            tenant,
            maximum,
            transforms: AtomicUsize::new(0),
            prepared: AtomicUsize::new(0),
        }
    }
}
fn normalize_name(value: Value, ctx: &Context) -> Result<Value, ValidationError> {
    ctx.require_data::<Policy>()?
        .transforms
        .fetch_add(1, Ordering::SeqCst);
    match value {
        Value::String(s) => Ok(Value::String(s.trim().to_owned())),
        other => Ok(other),
    }
}
fn within_policy(value: &i64, ctx: &Context) -> Result<(), ValidationError> {
    let policy = ctx.require_data::<Policy>()?;
    if *value > policy.maximum {
        Err(ValidationError::new(
            "policy_limit",
            "Order exceeds tenant limit",
        ))
    } else {
        Ok(())
    }
}
fn even_amount(value: &i64, ctx: &Context) -> Result<(), ValidationError> {
    ctx.require_data::<Policy>()?;
    if value % 2 != 0 {
        Err(ValidationError::new(
            "even_amount",
            "Order must contain pairs",
        ))
    } else {
        Ok(())
    }
}
#[derive(Debug, Model)]
pub struct Order {
    #[model(before_with_context = normalize_name, min_length = 1)]
    pub name: String,
    #[model(ge = 0, validate_with_context = [within_policy, even_amount])]
    pub amount: i64,
}
#[derive(Debug)]
pub struct StoredOrder {
    pub tenant: String,
    pub slug: String,
    pub amount: i64,
}
pub fn prepare(order: Order, policy: &Policy) -> Result<StoredOrder, &'static str> {
    Pipeline::plain(|order: Order, p: &Policy| {
        Ok::<_, &'static str>(StoredOrder {
            tenant: p.tenant.into(),
            slug: order.name.to_lowercase(),
            amount: order.amount,
        })
    })
    .then(|mut order, _: &Policy| {
        order.slug = order
            .slug
            .strip_prefix("order: ")
            .unwrap_or(&order.slug)
            .replace(' ', "-");
        Ok(order)
    })
    .check(|order, _: &Policy| {
        if order.slug == "reserved"
            || order.slug.contains(':')
            || order.slug.contains(' ')
            || order.slug != order.slug.to_lowercase()
        {
            Err("slug_policy")
        } else {
            Ok(())
        }
    })
    .then(|order, p: &Policy| {
        p.prepared.fetch_add(1, Ordering::SeqCst);
        Ok(order)
    })
    .run(order, policy)
}
#[derive(Clone)]
pub struct App {
    pub pool: SqlitePool,
}
impl App {
    pub async fn new() -> Self {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query("CREATE TABLE orders (tenant TEXT NOT NULL, slug TEXT NOT NULL, amount INTEGER NOT NULL, UNIQUE(tenant, slug))").execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE audit (tenant TEXT NOT NULL)")
            .execute(&pool)
            .await
            .unwrap();
        Self { pool }
    }
    pub fn router(&self) -> axum::Router {
        axum::Router::new()
            .route("/orders", post(create))
            .with_state(self.clone())
    }
}
fn reply(status: StatusCode, value: Value) -> axum::response::Response {
    (status, axum::Json(value)).into_response()
}
async fn create(
    State(app): State<App>,
    headers: HeaderMap,
    body: String,
) -> axum::response::Response {
    // Explicit request-local policy; no task/thread global context or caller-supplied limits.
    let policy = match headers.get("x-tenant").and_then(|v| v.to_str().ok()) {
        Some("small") => Policy::new("small", 10),
        Some("large") => Policy::new("large", 100),
        _ => return reply(StatusCode::UNAUTHORIZED, json!({"error":"unknown_tenant"})),
    };
    let policy = Arc::new(policy);
    let order = match Order::parse_json_with_context(&body, policy.clone()) {
        Ok(order) => order,
        Err(errors) => {
            return reply(
                StatusCode::UNPROCESSABLE_ENTITY,
                json!({"codes": errors.errors().iter().map(|e| e.code.clone()).collect::<Vec<_>>()}),
            );
        }
    };
    // Explicit revalidation and output validation; these must not rerun construction transforms.
    if let Err(errors) = order.validate_with_context(policy.clone()) {
        return reply(
            StatusCode::UNPROCESSABLE_ENTITY,
            json!({"error":errors.to_string()}),
        );
    }
    let normalized = order.dump_with_context(policy.clone()).unwrap();
    let stored = match prepare(order, &policy) {
        Ok(order) => order,
        Err(code) => return reply(StatusCode::UNPROCESSABLE_ENTITY, json!({"error":code})),
    };
    let mut tx = app.pool.begin().await.unwrap();
    sqlx::query("INSERT INTO audit (tenant) VALUES (?)")
        .bind(&stored.tenant)
        .execute(&mut *tx)
        .await
        .unwrap();
    let insert = sqlx::query("INSERT INTO orders (tenant, slug, amount) VALUES (?, ?, ?)")
        .bind(&stored.tenant)
        .bind(&stored.slug)
        .bind(stored.amount)
        .execute(&mut *tx)
        .await;
    if insert.is_err() {
        tx.rollback().await.unwrap();
        return reply(StatusCode::CONFLICT, json!({"error":"duplicate_order"}));
    }
    tx.commit().await.unwrap();
    reply(
        StatusCode::CREATED,
        json!({"tenant":stored.tenant,"slug":stored.slug,"order":normalized}),
    )
}

/// Explicit response DTO schema for the actual HTTP JSON envelope.
#[cfg(feature = "openapi-acceptance")]
#[derive(Model)]
pub struct OrderResponse {
    pub tenant: String,
    pub slug: String,
    pub order: Order,
}

/// The application supplies its own route metadata; core only relocates components.
#[cfg(feature = "openapi-acceptance")]
pub fn application_openapi() -> Value {
    use rydantic::{SchemaMode, schema::openapi::OpenApiComponents};
    let mut components = OpenApiComponents::new();
    let input = components
        .insert::<Order>("OrderInput", SchemaMode::Input)
        .unwrap();
    let output = components
        .insert::<OrderResponse>("OrderResponse", SchemaMode::Output)
        .unwrap();
    json!({"openapi":"3.1.1", "info":{"title":"Synthetic order service", "version":"0.0.0"},
        "paths":{"/orders":{"post":{"requestBody":{"content":{"application/json":{"schema":input.reference}}},
        "responses":{"201":{"description":"Created","content":{"application/json":{"schema":output.reference}}},
        "401":{"description":"Unknown tenant"},"409":{"description":"Duplicate order"},"422":{"description":"Validation or slug policy rejection"}}}}},
        "components":{"schemas":components.into_schemas()}})
}
