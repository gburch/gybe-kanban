use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use ts_rs::TS;
use uuid::Uuid;

#[derive(Debug, Clone, FromRow, Serialize, Deserialize, TS)]
pub struct TaskAttemptRepository {
    pub id: Uuid,
    pub task_attempt_id: Uuid,
    pub project_repository_id: Uuid,
    pub is_primary: bool,
    pub container_ref: Option<String>,
    pub branch: Option<String>,
    #[ts(type = "Date")]
    pub created_at: DateTime<Utc>,
    #[ts(type = "Date")]
    pub updated_at: DateTime<Utc>,
}

impl TaskAttemptRepository {
    pub async fn list_for_attempt(
        pool: &SqlitePool,
        attempt_id: Uuid,
    ) -> Result<Vec<Self>, sqlx::Error> {
        sqlx::query_as!(
            TaskAttemptRepository,
            r#"SELECT id as "id!: Uuid",
                      task_attempt_id as "task_attempt_id!: Uuid",
                      project_repository_id as "project_repository_id!: Uuid",
                      is_primary as "is_primary!: bool",
                      container_ref,
                      branch,
                      created_at as "created_at!: DateTime<Utc>",
                      updated_at as "updated_at!: DateTime<Utc>"
               FROM task_attempt_repositories
               WHERE task_attempt_id = $1
               ORDER BY is_primary DESC, created_at ASC"#,
            attempt_id
        )
        .fetch_all(pool)
        .await
    }

    pub async fn find_for_attempt(
        pool: &SqlitePool,
        attempt_id: Uuid,
        project_repository_id: Uuid,
    ) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as!(
            TaskAttemptRepository,
            r#"SELECT id as "id!: Uuid",
                      task_attempt_id as "task_attempt_id!: Uuid",
                      project_repository_id as "project_repository_id!: Uuid",
                      is_primary as "is_primary!: bool",
                      container_ref,
                      branch,
                      created_at as "created_at!: DateTime<Utc>",
                      updated_at as "updated_at!: DateTime<Utc>"
               FROM task_attempt_repositories
               WHERE task_attempt_id = $1 AND project_repository_id = $2"#,
            attempt_id,
            project_repository_id
        )
        .fetch_optional(pool)
        .await
    }

    pub async fn upsert_container_ref(
        pool: &SqlitePool,
        attempt_id: Uuid,
        project_repository_id: Uuid,
        is_primary: bool,
        container_ref: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        let id = Uuid::new_v4();
        sqlx::query!(
            r#"INSERT INTO task_attempt_repositories (
                    id,
                    task_attempt_id,
                    project_repository_id,
                    is_primary,
                    container_ref
                )
                VALUES ($1, $2, $3, $4, $5)
                ON CONFLICT(task_attempt_id, project_repository_id)
                DO UPDATE SET
                    container_ref = excluded.container_ref,
                    is_primary = excluded.is_primary,
                    updated_at = datetime('now', 'subsec')"#,
            id,
            attempt_id,
            project_repository_id,
            is_primary,
            container_ref
        )
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn upsert_branch(
        pool: &SqlitePool,
        attempt_id: Uuid,
        project_repository_id: Uuid,
        is_primary: bool,
        branch: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        let id = Uuid::new_v4();
        sqlx::query!(
            r#"INSERT INTO task_attempt_repositories (
                    id,
                    task_attempt_id,
                    project_repository_id,
                    is_primary,
                    branch
                )
                VALUES ($1, $2, $3, $4, $5)
                ON CONFLICT(task_attempt_id, project_repository_id)
                DO UPDATE SET
                    branch = excluded.branch,
                    is_primary = excluded.is_primary,
                    updated_at = datetime('now', 'subsec')"#,
            id,
            attempt_id,
            project_repository_id,
            is_primary,
            branch
        )
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn find_primary_for_attempt(
        pool: &SqlitePool,
        attempt_id: Uuid,
    ) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as!(
            TaskAttemptRepository,
            r#"SELECT id as "id!: Uuid",
                      task_attempt_id as "task_attempt_id!: Uuid",
                      project_repository_id as "project_repository_id!: Uuid",
                      is_primary as "is_primary!: bool",
                      container_ref,
                      branch,
                      created_at as "created_at!: DateTime<Utc>",
                      updated_at as "updated_at!: DateTime<Utc>"
               FROM task_attempt_repositories
               WHERE task_attempt_id = $1 AND is_primary = 1"#,
            attempt_id
        )
        .fetch_optional(pool)
        .await
    }
}
