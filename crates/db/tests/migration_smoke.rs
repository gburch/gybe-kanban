use std::{fs, path::PathBuf, str::FromStr};

use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    Row,
};
use tempfile::TempDir;

type TestResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

#[tokio::test]
async fn migrations_apply_to_seed_database() -> TestResult<()> {
    let temp = TempDir::new()?;
    let db_path = temp.path().join("smoke.sqlite");
    let seed_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../dev_assets_seed/db.sqlite");
    fs::copy(&seed_path, &db_path)?;

    let options = SqliteConnectOptions::from_str(&format!("sqlite://{}", db_path.display()))?
        .create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;

    MIGRATOR.run(&pool).await?;

    let project_repo_tables: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='project_repositories'",
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(project_repo_tables, 1, "project_repositories table should exist after migrations");

    let attempt_repo_tables: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='task_attempt_repositories'",
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(attempt_repo_tables, 1, "task_attempt_repositories table should exist after migrations");

    // Verify foreign key relationships are in place for the multi-repo tables
    let fk_rows = sqlx::query(
        "PRAGMA foreign_key_list('task_attempt_repositories')",
    )
    .fetch_all(&pool)
    .await?;
    assert!(
        fk_rows.iter().any(|row| row.get::<String, _>("table") == "project_repositories"),
        "task_attempt_repositories should reference project_repositories"
    );

    Ok(())
}
