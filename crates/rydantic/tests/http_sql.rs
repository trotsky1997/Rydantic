#![cfg(all(feature = "axum", feature = "sqlx"))]
#[path = "../examples/sqlite_api.rs"]
#[allow(dead_code)]
mod api;
use rydantic::{sqlx::sqlite::SqlitePoolOptions, Model, SqlModel};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn http(
    address: std::net::SocketAddr,
    method: &str,
    path: &str,
    body: &str,
) -> (u16, rydantic::Value) {
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    stream.write_all(format!("{method} {path} HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).await.unwrap();
    let text = std::str::from_utf8(&bytes).unwrap();
    let (headers, body) = text.split_once("\r\n\r\n").unwrap();
    let status = headers.split_whitespace().nth(1).unwrap().parse().unwrap();
    (
        status,
        if body.is_empty() {
            rydantic::Value::Null
        } else {
            serde_json::from_str(body).unwrap()
        },
    )
}
#[tokio::test]
async fn real_tcp_http_server_runs_validated_sqlite_crud_on_tokio() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    api::User::create_table(&pool).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = api::router(pool.clone());
    let server = tokio::spawn(async move {
        rydantic::axum::framework::serve(listener, app)
            .await
            .unwrap()
    });
    let run = async {
        let (status, body) = http(address, "POST", "/users", r#"{"name":"A","age":17}"#).await;
        assert_eq!(status, 422);
        assert_eq!(body["detail"].as_array().unwrap().len(), 2);
        assert!(api::User::select()
            .fetch_all(&pool)
            .await
            .unwrap()
            .is_empty());
        let (status, body) =
            http(address, "POST", "/users", r#"{"name":" Alice ","age":25}"#).await;
        assert_eq!(status, 201);
        assert_eq!(body, rydantic::json!({"id":1,"name":"Alice","age":25}));
        let (status, body) = http(address, "GET", "/users/1", "").await;
        assert_eq!(status, 200);
        assert_eq!(body["name"], "Alice");
        let (status, body) = http(address, "POST", "/users", r#"{"name":"Alice","age":25}"#).await;
        assert_eq!(status, 409);
        assert_eq!(body["detail"][0]["code"], "sql_constraint");
        assert_eq!(body["detail"][0]["loc"], rydantic::json!([]));
        assert_eq!(body["detail"][0]["stage"], "validate");
        assert!(!body.to_string().contains("UNIQUE"));
        let (status, body) = http(address, "PUT", "/users/1", r#"{"name":" Bob ","age":30}"#).await;
        assert_eq!(status, 200);
        assert_eq!(body["name"], "Bob");
        assert_eq!(api::User::get(&pool, &1).await.unwrap().unwrap().age, 30);
        let (status, body) = http(address, "GET", "/users", "").await;
        assert_eq!(status, 200);
        assert_eq!(body.as_array().unwrap().len(), 1);
        assert!(api::User::json_schema(rydantic::SchemaMode::Output).is_object());
        assert_eq!(http(address, "DELETE", "/users/1", "").await.0, 204);
        assert_eq!(http(address, "GET", "/users/1", "").await.0, 404);
    };
    let result = tokio::time::timeout(std::time::Duration::from_secs(15), run).await;
    server.abort();
    result.unwrap();
}
