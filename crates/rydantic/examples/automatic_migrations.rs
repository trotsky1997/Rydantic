//! Upgrade an existing table by changing its Rust model definition.
use rydantic::{sql::MigrationOptions, sqlx::sqlite::SqlitePoolOptions, Model, SqlModel};
mod previous {
    use super::*;
    #[derive(Debug, Model, SqlModel)]
    #[sql(table = "accounts")]
    pub struct Account {
        #[sql(primary_key, auto_increment)]
        pub id: Option<i64>,
        pub name: String,
    }
}
#[derive(Debug, Model, SqlModel)]
#[sql(table = "accounts")]
struct Account {
    #[sql(primary_key, auto_increment)]
    id: Option<i64>,
    #[sql(rename_from = "name", unique)]
    display_name: String,
    #[sql(default = true)]
    enabled: bool,
    note: Option<String>,
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await?;
    previous::Account::migrate(&pool).await?;
    let old = previous::Account {
        id: None,
        name: "Alice".into(),
    }
    .insert(&pool)
    .await?;
    let plan = Account::migration_plan(&pool).await?;
    for change in plan.changes() {
        println!("{}", change.description);
    }
    let report = plan.apply(&pool, MigrationOptions::default()).await?;
    let account = Account::get(&pool, &old.id.unwrap()).await?.unwrap();
    assert_eq!(account.display_name, "Alice");
    assert!(account.enabled);
    assert!(account.note.is_none());
    assert!(Account::migrate(&pool).await?.plan.is_empty());
    println!(
        "Applied revision {:?}: {}",
        report.revision, report.checksum
    );
    Ok(())
}
