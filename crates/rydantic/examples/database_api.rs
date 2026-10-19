//! Select a driver with DATABASE_URL; enable the postgres or mysql feature as needed.
use rydantic::{
    axum::{
        framework::{
            extract::{Path, State},
            http::StatusCode,
            routing::get,
        },
        Json, Router,
    },
    sql::SqlBackend,
    sqlx::{sqlite::SqlitePoolOptions, Pool},
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
pub fn router<DB: SqlBackend>(pool: Pool<DB>) -> Router {
    Router::new()
        .route("/users", get(list::<DB>).post(create::<DB>))
        .route(
            "/users/{id}",
            get(find::<DB>).put(update::<DB>).delete(remove::<DB>),
        )
        .with_state(pool)
}
async fn create<DB: SqlBackend>(
    State(pool): State<Pool<DB>>,
    Json(user): Json<User>,
) -> Result<(StatusCode, Json<User>), SqlError> {
    Ok((StatusCode::CREATED, Json(user.insert(&pool).await?)))
}
async fn list<DB: SqlBackend>(State(pool): State<Pool<DB>>) -> Result<Json<Vec<User>>, SqlError> {
    Ok(Json(User::select().limit(100).fetch_all(&pool).await?))
}
async fn find<DB: SqlBackend>(
    State(pool): State<Pool<DB>>,
    Path(id): Path<i64>,
) -> Result<Json<User>, SqlError> {
    User::get(&pool, &id)
        .await?
        .map(Json)
        .ok_or(SqlError::NotFound)
}
async fn update<DB: SqlBackend>(
    State(pool): State<Pool<DB>>,
    Path(id): Path<i64>,
    Json(mut user): Json<User>,
) -> Result<Json<User>, SqlError> {
    user.id = Some(id);
    Ok(Json(user.update(&pool).await?))
}
async fn remove<DB: SqlBackend>(
    State(pool): State<Pool<DB>>,
    Path(id): Path<i64>,
) -> Result<StatusCode, SqlError> {
    if User::delete(&pool, &id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(SqlError::NotFound)
    }
}

async fn run<DB: SqlBackend>(pool: Pool<DB>) -> Result<(), Box<dyn std::error::Error>> {
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
    let address: std::net::SocketAddr = std::env::var("LISTEN_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:3000".into())
        .parse()?;
    if !address.ip().is_loopback() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "the unauthenticated demo requires a loopback LISTEN_ADDR",
        )
        .into());
    }
    let listener = tokio::net::TcpListener::bind(address).await?;
    println!("Listening on http://{}", listener.local_addr()?);
    rydantic::axum::framework::serve(listener, router(pool)).await?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite::memory:".into());
    if url.starts_with("postgres:") || url.starts_with("postgresql:") {
        #[cfg(feature = "postgres")]
        {
            return run(rydantic::sqlx::postgres::PgPoolOptions::new()
                .max_connections(5)
                .connect(&url)
                .await?)
            .await;
        }
        #[cfg(not(feature = "postgres"))]
        {
            return Err("enable the postgres feature for a PostgreSQL DATABASE_URL".into());
        }
    }
    if url.starts_with("mysql:") {
        #[cfg(feature = "mysql")]
        {
            return run(rydantic::sqlx::mysql::MySqlPoolOptions::new()
                .max_connections(5)
                .connect(&url)
                .await?)
            .await;
        }
        #[cfg(not(feature = "mysql"))]
        {
            return Err("enable the mysql feature for a MySQL DATABASE_URL".into());
        }
    }
    run(SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await?)
    .await
}
