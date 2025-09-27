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

#[derive(Debug, Clone)]
pub struct TaskAttemptWorktreeRef {
    pub task_attempt_id: Uuid,
    pub project_repository_id: Uuid,
    pub is_primary: bool,
    pub container_ref: String,
}

#[derive(Debug, Clone)]
pub struct TaskAttemptRepositoryWithRepo {
    pub task_attempt_id: Uuid,
    pub project_repository_id: Uuid,
    pub is_primary: bool,
    pub container_ref: Option<String>,
    pub branch: Option<String>,
    pub git_repo_path: String,
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

    pub async fn list_active_worktrees(
        pool: &SqlitePool,
    ) -> Result<Vec<TaskAttemptWorktreeRef>, sqlx::Error> {
        let records = sqlx::query!(
            r#"
            SELECT
                tar.task_attempt_id       AS "task_attempt_id!: Uuid",
                tar.project_repository_id AS "project_repository_id!: Uuid",
                tar.is_primary            AS "is_primary!: bool",
                tar.container_ref         AS container_ref
            FROM task_attempt_repositories tar
            JOIN task_attempts ta ON ta.id = tar.task_attempt_id
            WHERE ta.worktree_deleted = 0
              AND tar.container_ref IS NOT NULL
            "#
        )
        .fetch_all(pool)
        .await?;

        Ok(records
            .into_iter()
            .filter_map(|r| {
                r.container_ref.map(|container_ref| TaskAttemptWorktreeRef {
                    task_attempt_id: r.task_attempt_id,
                    project_repository_id: r.project_repository_id,
                    is_primary: r.is_primary,
                    container_ref,
                })
            })
            .collect())
    }

    pub async fn list_for_attempt_with_repo(
        pool: &SqlitePool,
        attempt_id: Uuid,
    ) -> Result<Vec<TaskAttemptRepositoryWithRepo>, sqlx::Error> {
        let records = sqlx::query!(
            r#"
            SELECT
                tar.task_attempt_id       AS "task_attempt_id!: Uuid",
                tar.project_repository_id AS "project_repository_id!: Uuid",
                tar.is_primary            AS "is_primary!: bool",
                tar.container_ref         AS container_ref,
                tar.branch                AS branch,
                pr.git_repo_path          AS git_repo_path
            FROM task_attempt_repositories tar
            JOIN project_repositories pr ON pr.id = tar.project_repository_id
            WHERE tar.task_attempt_id = $1
            "#,
            attempt_id
        )
        .fetch_all(pool)
        .await?;

        Ok(records
            .into_iter()
            .map(|r| TaskAttemptRepositoryWithRepo {
                task_attempt_id: r.task_attempt_id,
                project_repository_id: r.project_repository_id,
                is_primary: r.is_primary,
                container_ref: r.container_ref,
                branch: r.branch,
                git_repo_path: r.git_repo_path,
            })
            .collect())
    }

    pub async fn clear_container_ref(
        pool: &SqlitePool,
        attempt_id: Uuid,
        project_repository_id: Uuid,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"UPDATE task_attempt_repositories
               SET container_ref = NULL,
                   updated_at = datetime('now', 'subsec')
             WHERE task_attempt_id = $1 AND project_repository_id = $2"#,
            attempt_id,
            project_repository_id
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
