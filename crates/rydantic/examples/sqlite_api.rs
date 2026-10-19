//! Run with `cargo run -p rydantic --example sqlite_api --features axum,sqlx`.
use rydantic::{
    axum::{
        framework::{
            extract::{Path, State},
            http::StatusCode,
            routing::get,
        },
        Json, Router,
    },
    sqlx::{sqlite::SqlitePoolOptions, SqlitePool},
    Model, SqlError, SqlModel,
};

#[derive(Debug, PartialEq, Model, SqlModel)]
#[sql(table = "users")]
pub struct User {
    #[sql(primary_key, auto_increment)]
    pub id: Option<i64>,
    #[model(trim, min_length = 2, max_length = 100)]
    #[sql(unique)]
    pub name: String,
    #[model(ge = 18, le = 120)]
    #[sql(index)]
    pub age: u8,
}
pub fn router(pool: SqlitePool) -> Router {
    Router::new()
        .route("/users", get(list).post(create))
        .route("/users/{id}", get(find).put(update).delete(remove))
        .with_state(pool)
}
async fn create(
    State(pool): State<SqlitePool>,
    Json(user): Json<User>,
) -> Result<(StatusCode, Json<User>), SqlError> {
    Ok((StatusCode::CREATED, Json(user.insert(&pool).await?)))
}
async fn list(State(pool): State<SqlitePool>) -> Result<Json<Vec<User>>, SqlError> {
    Ok(Json(User::select().limit(100).fetch_all(&pool).await?))
}
async fn find(State(pool): State<SqlitePool>, Path(id): Path<i64>) -> Result<Json<User>, SqlError> {
    User::get(&pool, &id)
        .await?
        .map(Json)
        .ok_or(SqlError::NotFound)
}
async fn update(
    State(pool): State<SqlitePool>,
    Path(id): Path<i64>,
    Json(mut user): Json<User>,
) -> Result<Json<User>, SqlError> {
    user.id = Some(id);
    Ok(Json(user.update(&pool).await?))
}
async fn remove(
    State(pool): State<SqlitePool>,
    Path(id): Path<i64>,
) -> Result<StatusCode, SqlError> {
    if User::delete(&pool, &id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(SqlError::NotFound)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite::memory:".into());
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await?;
    User::create_table(&pool).await?;
    if std::env::args().any(|arg| arg == "--smoke") {
        use rydantic::axum::framework::{body::Body, http::Request};
        use tower::ServiceExt;
        let response = router(pool)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/users")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"name":" Alice ","age":25}"#))?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::CREATED);
        println!("Validated HTTP -> SQLite -> JSON: OK");
        return Ok(());
    }
    let address = std::env::var("LISTEN_ADDR").unwrap_or_else(|_| "127.0.0.1:3000".into());
    let listener = tokio::net::TcpListener::bind(&address).await?;
    println!("Listening on http://{}", listener.local_addr()?);
    rydantic::axum::framework::serve(listener, router(pool)).await?;
    Ok(())
}
