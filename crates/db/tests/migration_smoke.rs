use std::{fs, path::PathBuf, str::FromStr};

use sqlx::{
    Row,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use tempfile::TempDir;

type TestResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

#[tokio::test]
async fn migrations_apply_to_seed_database() -> TestResult<()> {
    let temp = TempDir::new()?;
    let db_path = temp.path().join("smoke.sqlite");
    let seed_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../dev_assets_seed/db.sqlite");
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
    assert_eq!(
        project_repo_tables, 1,
        "project_repositories table should exist after migrations"
    );

    let attempt_repo_tables: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='task_attempt_repositories'",
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(
        attempt_repo_tables, 1,
        "task_attempt_repositories table should exist after migrations"
    );

    // Verify foreign key relationships are in place for the multi-repo tables
    let fk_rows = sqlx::query("PRAGMA foreign_key_list('task_attempt_repositories')")
        .fetch_all(&pool)
        .await?;
    assert!(
        fk_rows
            .iter()
            .any(|row| row.get::<String, _>("table") == "project_repositories"),
        "task_attempt_repositories should reference project_repositories"
    );

    let repo_indexes = sqlx::query("PRAGMA index_list('project_repositories')")
        .fetch_all(&pool)
        .await?;
    let repo_index_names: Vec<String> = repo_indexes
        .iter()
        .map(|row| row.get::<String, _>("name"))
        .collect();
    assert!(
        repo_index_names.contains(&"idx_project_repositories_project_primary".to_string()),
        "primary repository unique index should exist"
    );
    let primary_index = repo_indexes
        .iter()
        .find(|row| row.get::<String, _>("name") == "idx_project_repositories_project_primary")
        .expect("primary repository index metadata");
    assert_eq!(
        primary_index.get::<i64, _>("unique"),
        1,
        "primary repository index must enforce uniqueness",
    );

    let attempt_indexes = sqlx::query("PRAGMA index_list('task_attempt_repositories')")
        .fetch_all(&pool)
        .await?;
    let attempt_index_names: Vec<String> = attempt_indexes
        .iter()
        .map(|row| row.get::<String, _>("name"))
        .collect();
    assert!(
        attempt_index_names.contains(&"idx_task_attempt_repositories_primary".to_string()),
        "task attempt primary index should exist"
    );
    let attempt_primary_index = attempt_indexes
        .iter()
        .find(|row| row.get::<String, _>("name") == "idx_task_attempt_repositories_primary")
        .expect("attempt primary index metadata");
    assert_eq!(
        attempt_primary_index.get::<i64, _>("unique"),
        1,
        "task attempt primary index must enforce uniqueness",
    );

    let project_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM projects")
        .fetch_one(&pool)
        .await?;
    let project_primary_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM project_repositories WHERE is_primary = 1")
            .fetch_one(&pool)
            .await?;
    assert_eq!(
        project_count, project_primary_count,
        "every project should expose exactly one primary repository"
    );

    let attempt_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM task_attempts")
        .fetch_one(&pool)
        .await?;
    let attempt_primary_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM task_attempt_repositories WHERE is_primary = 1")
            .fetch_one(&pool)
            .await?;
    assert_eq!(
        attempt_count, attempt_primary_count,
        "each task attempt should have a primary repository entry"
    );

    Ok(())
}
