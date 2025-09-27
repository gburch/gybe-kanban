use std::collections::HashSet;

use chrono::{DateTime, Utc};
use executors::executors::BaseCodingAgent;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool, Type};
use thiserror::Error;
use ts_rs::TS;
use uuid::Uuid;

use super::{
    execution_process::{ExecutionProcess, ExecutionProcessRunReason, ExecutionProcessStatus},
    project::Project,
    task::Task,
};

#[derive(Debug, Error)]
pub enum TaskAttemptError {
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error("Task not found")]
    TaskNotFound,
    #[error("Project not found")]
    ProjectNotFound,
    #[error("Validation error: {0}")]
    ValidationError(String),
    #[error("Branch not found: {0}")]
    BranchNotFound(String),
}

#[derive(Debug, Clone, Type, Serialize, Deserialize, PartialEq, TS)]
#[sqlx(type_name = "task_attempt_status", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum TaskAttemptStatus {
    SetupRunning,
    SetupComplete,
    SetupFailed,
    ExecutorRunning,
    ExecutorComplete,
    ExecutorFailed,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize, TS)]
pub struct TaskAttempt {
    pub id: Uuid,
    pub task_id: Uuid,                 // Foreign key to Task
    pub container_ref: Option<String>, // Path to a worktree (local), or cloud container id
    pub branch: Option<String>,        // Git branch name for this task attempt
    pub base_branch: String,           // Base branch this attempt is based on
    pub executor: String, // Name of the base coding agent to use ("AMP", "CLAUDE_CODE",
    // "GEMINI", etc.)
    pub worktree_deleted: bool, // Flag indicating if worktree has been cleaned up
    pub setup_completed_at: Option<DateTime<Utc>>, // When setup script was last completed
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// GitHub PR creation parameters
pub struct CreatePrParams<'a> {
    pub attempt_id: Uuid,
    pub task_id: Uuid,
    pub project_id: Uuid,
    pub github_token: &'a str,
    pub title: &'a str,
    pub body: Option<&'a str>,
    pub base_branch: Option<&'a str>,
}

#[derive(Debug, Deserialize, TS)]
pub struct CreateFollowUpAttempt {
    pub prompt: String,
}

/// Context data for resume operations (simplified)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttemptResumeContext {
    pub execution_history: String,
    pub cumulative_diffs: String,
}

#[derive(Debug)]
pub struct TaskAttemptContext {
    pub task_attempt: TaskAttempt,
    pub task: Task,
    pub project: Project,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct CreateTaskAttemptRepository {
    pub project_repository_id: Uuid,
    #[serde(default)]
    pub is_primary: bool,
}

#[derive(Debug, Deserialize, TS)]
pub struct CreateTaskAttempt {
    pub executor: BaseCodingAgent,
    pub base_branch: String,
    #[serde(default)]
    pub repositories: Option<Vec<CreateTaskAttemptRepository>>,
}

impl TaskAttempt {
    pub async fn parent_task(&self, pool: &SqlitePool) -> Result<Option<Task>, sqlx::Error> {
        Task::find_by_id(pool, self.task_id).await
    }

    /// Fetch all task attempts, optionally filtered by task_id. Newest first.
    pub async fn fetch_all(
        pool: &SqlitePool,
        task_id: Option<Uuid>,
    ) -> Result<Vec<Self>, TaskAttemptError> {
        let attempts = match task_id {
            Some(tid) => sqlx::query_as!(
                TaskAttempt,
                r#"SELECT id AS "id!: Uuid",
                              task_id AS "task_id!: Uuid",
                              container_ref,
                              branch,
                              base_branch,
                              executor AS "executor!",
                              worktree_deleted AS "worktree_deleted!: bool",
                              setup_completed_at AS "setup_completed_at: DateTime<Utc>",
                              created_at AS "created_at!: DateTime<Utc>",
                              updated_at AS "updated_at!: DateTime<Utc>"
                       FROM task_attempts
                       WHERE task_id = $1
                       ORDER BY created_at DESC"#,
                tid
            )
            .fetch_all(pool)
            .await
            .map_err(TaskAttemptError::Database)?,
            None => sqlx::query_as!(
                TaskAttempt,
                r#"SELECT id AS "id!: Uuid",
                              task_id AS "task_id!: Uuid",
                              container_ref,
                              branch,
                              base_branch,
                              executor AS "executor!",
                              worktree_deleted AS "worktree_deleted!: bool",
                              setup_completed_at AS "setup_completed_at: DateTime<Utc>",
                              created_at AS "created_at!: DateTime<Utc>",
                              updated_at AS "updated_at!: DateTime<Utc>"
                       FROM task_attempts
                       ORDER BY created_at DESC"#
            )
            .fetch_all(pool)
            .await
            .map_err(TaskAttemptError::Database)?,
        };

        Ok(attempts)
    }

    /// Load task attempt with full validation - ensures task_attempt belongs to task and task belongs to project
    pub async fn load_context(
        pool: &SqlitePool,
        attempt_id: Uuid,
        task_id: Uuid,
        project_id: Uuid,
    ) -> Result<TaskAttemptContext, TaskAttemptError> {
        // Single query with JOIN validation to ensure proper relationships
        let task_attempt = sqlx::query_as!(
            TaskAttempt,
            r#"SELECT  ta.id                AS "id!: Uuid",
                       ta.task_id           AS "task_id!: Uuid",
                       ta.container_ref,
                       ta.branch,
                       ta.base_branch,
                       ta.executor AS "executor!",
                       ta.worktree_deleted  AS "worktree_deleted!: bool",
                       ta.setup_completed_at AS "setup_completed_at: DateTime<Utc>",
                       ta.created_at        AS "created_at!: DateTime<Utc>",
                       ta.updated_at        AS "updated_at!: DateTime<Utc>"
               FROM    task_attempts ta
               JOIN    tasks t ON ta.task_id = t.id
               JOIN    projects p ON t.project_id = p.id
               WHERE   ta.id = $1 AND t.id = $2 AND p.id = $3"#,
            attempt_id,
            task_id,
            project_id
        )
        .fetch_optional(pool)
        .await?
        .ok_or(TaskAttemptError::TaskNotFound)?;

        // Load task and project (we know they exist due to JOIN validation)
        let task = Task::find_by_id(pool, task_id)
            .await?
            .ok_or(TaskAttemptError::TaskNotFound)?;

        let project = Project::find_by_id(pool, project_id)
            .await?
            .ok_or(TaskAttemptError::ProjectNotFound)?;

        Ok(TaskAttemptContext {
            task_attempt,
            task,
            project,
        })
    }

    /// Update container reference
    pub async fn update_container_ref(
        pool: &SqlitePool,
        attempt_id: Uuid,
        container_ref: &str,
    ) -> Result<(), sqlx::Error> {
        let now = Utc::now();
        let mut tx = pool.begin().await?;
        sqlx::query!(
            "UPDATE task_attempts SET container_ref = $1, updated_at = $2 WHERE id = $3",
            container_ref,
            now,
            attempt_id
        )
        .execute(&mut *tx)
        .await?;

        let repo_result = sqlx::query!(
            "UPDATE task_attempt_repositories SET container_ref = $2, updated_at = datetime('now', 'subsec') WHERE task_attempt_id = $1 AND is_primary = 1",
            attempt_id,
            container_ref
        )
        .execute(&mut *tx)
        .await?;

        if repo_result.rows_affected() == 0 {
            let repo_row = sqlx::query!(
                r#"SELECT pr.id as "id!: Uuid"
                   FROM task_attempts ta
                   JOIN tasks t ON ta.task_id = t.id
                   JOIN project_repositories pr ON pr.project_id = t.project_id AND pr.is_primary = 1
                   WHERE ta.id = $1"#,
                attempt_id
            )
            .fetch_optional(&mut *tx)
            .await?;

            let project_repo_id = if let Some(row) = repo_row {
                row.id
            } else {
                let project_row = sqlx::query!(
                    r#"SELECT t.project_id as "project_id!: Uuid", p.git_repo_path
                       FROM task_attempts ta
                       JOIN tasks t ON ta.task_id = t.id
                       JOIN projects p ON t.project_id = p.id
                       WHERE ta.id = $1"#,
                    attempt_id
                )
                .fetch_one(&mut *tx)
                .await?;

                let repo_id = Uuid::new_v4();
                sqlx::query!(
                    r#"INSERT INTO project_repositories (id, project_id, name, git_repo_path, root_path, is_primary)
                       VALUES ($1, $2, $3, $4, $5, 1)"#,
                    repo_id,
                    project_row.project_id,
                    "Primary",
                    project_row.git_repo_path,
                    ""
                )
                .execute(&mut *tx)
                .await?;

                repo_id
            };

            let task_repo_id = Uuid::new_v4();
            sqlx::query!(
                r#"INSERT INTO task_attempt_repositories (id, task_attempt_id, project_repository_id, is_primary, container_ref)
                   VALUES ($1, $2, $3, 1, $4)"#,
                task_repo_id,
                attempt_id,
                project_repo_id,
                container_ref
            )
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    pub async fn update_branch(
        pool: &SqlitePool,
        attempt_id: Uuid,
        branch: &str,
    ) -> Result<(), sqlx::Error> {
        let now = Utc::now();
        let mut tx = pool.begin().await?;
        sqlx::query!(
            "UPDATE task_attempts SET branch = $1, updated_at = $2 WHERE id = $3",
            branch,
            now,
            attempt_id
        )
        .execute(&mut *tx)
        .await?;

        let repo_result = sqlx::query!(
            "UPDATE task_attempt_repositories SET branch = $2, updated_at = datetime('now', 'subsec') WHERE task_attempt_id = $1 AND is_primary = 1",
            attempt_id,
            branch
        )
        .execute(&mut *tx)
        .await?;

        if repo_result.rows_affected() == 0 {
            let repo_row = sqlx::query!(
                r#"SELECT pr.id as "id!: Uuid"
                   FROM task_attempts ta
                   JOIN tasks t ON ta.task_id = t.id
                   JOIN project_repositories pr ON pr.project_id = t.project_id AND pr.is_primary = 1
                   WHERE ta.id = $1"#,
                attempt_id
            )
            .fetch_optional(&mut *tx)
            .await?;

            let project_repo_id = if let Some(row) = repo_row {
                row.id
            } else {
                let project_row = sqlx::query!(
                    r#"SELECT t.project_id as "project_id!: Uuid", p.git_repo_path
                       FROM task_attempts ta
                       JOIN tasks t ON ta.task_id = t.id
                       JOIN projects p ON t.project_id = p.id
                       WHERE ta.id = $1"#,
                    attempt_id
                )
                .fetch_one(&mut *tx)
                .await?;

                let repo_id = Uuid::new_v4();
                sqlx::query!(
                    r#"INSERT INTO project_repositories (id, project_id, name, git_repo_path, root_path, is_primary)
                       VALUES ($1, $2, $3, $4, $5, 1)"#,
                    repo_id,
                    project_row.project_id,
                    "Primary",
                    project_row.git_repo_path,
                    ""
                )
                .execute(&mut *tx)
                .await?;

                repo_id
            };

            let task_repo_id = Uuid::new_v4();
            sqlx::query!(
                r#"INSERT INTO task_attempt_repositories (id, task_attempt_id, project_repository_id, is_primary, branch)
                   VALUES ($1, $2, $3, 1, $4)"#,
                task_repo_id,
                attempt_id,
                project_repo_id,
                branch
            )
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    /// Helper function to mark a worktree as deleted in the database
    pub async fn mark_worktree_deleted(
        pool: &SqlitePool,
        attempt_id: Uuid,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            "UPDATE task_attempts SET worktree_deleted = TRUE, container_ref = NULL, updated_at = datetime('now', 'subsec') WHERE id = ?",
            attempt_id
        )
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn find_by_id(pool: &SqlitePool, id: Uuid) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as!(
            TaskAttempt,
            r#"SELECT  id                AS "id!: Uuid",
                       task_id           AS "task_id!: Uuid",
                       container_ref,
                       branch,
                       base_branch,
                       executor AS "executor!",
                       worktree_deleted  AS "worktree_deleted!: bool",
                       setup_completed_at AS "setup_completed_at: DateTime<Utc>",
                       created_at        AS "created_at!: DateTime<Utc>",
                       updated_at        AS "updated_at!: DateTime<Utc>"
               FROM    task_attempts
               WHERE   id = $1"#,
            id
        )
        .fetch_optional(pool)
        .await
    }

    pub async fn find_by_rowid(pool: &SqlitePool, rowid: i64) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as!(
            TaskAttempt,
            r#"SELECT  id                AS "id!: Uuid",
                       task_id           AS "task_id!: Uuid",
                       container_ref,
                       branch,
                       base_branch,
                       executor AS "executor!",
                       worktree_deleted  AS "worktree_deleted!: bool",
                       setup_completed_at AS "setup_completed_at: DateTime<Utc>",
                       created_at        AS "created_at!: DateTime<Utc>",
                       updated_at        AS "updated_at!: DateTime<Utc>"
               FROM    task_attempts
               WHERE   rowid = $1"#,
            rowid
        )
        .fetch_optional(pool)
        .await
    }

    /// Ensure that a task attempt belongs to the supplied project and is currently active.
    /// Returns the attempt if validation succeeds.
    pub async fn ensure_active_for_project(
        pool: &SqlitePool,
        attempt_id: Uuid,
        project_id: Uuid,
    ) -> Result<Self, TaskAttemptError> {
        let attempt = TaskAttempt::find_by_id(pool, attempt_id)
            .await?
            .ok_or(TaskAttemptError::TaskNotFound)?;

        if attempt.worktree_deleted {
            return Err(TaskAttemptError::ValidationError(
                "Parent task attempt has been cleaned up and is no longer active".to_string(),
            ));
        }

        let attempt_task = Task::find_by_id(pool, attempt.task_id)
            .await?
            .ok_or(TaskAttemptError::TaskNotFound)?;

        if attempt_task.project_id != project_id {
            return Err(TaskAttemptError::ValidationError(
                "Parent task attempt belongs to a different project".to_string(),
            ));
        }

        match ExecutionProcess::find_latest_by_task_attempt_and_run_reason(
            pool,
            attempt_id,
            &ExecutionProcessRunReason::CodingAgent,
        )
        .await?
        {
            Some(process) if process.status == ExecutionProcessStatus::Running => {}
            Some(process) => {
                return Err(TaskAttemptError::ValidationError(format!(
                    "Parent task attempt is not active (latest coding agent process status: {:?})",
                    process.status
                )));
            }
            None => {
                return Err(TaskAttemptError::ValidationError(
                    "Parent task attempt does not have an active coding agent process".to_string(),
                ));
            }
        }

        Ok(attempt)
    }

    /// Find the currently running coding-agent attempt for a task, if any.
    pub async fn find_active_coding_agent_for_task(
        pool: &SqlitePool,
        task_id: Uuid,
    ) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as!(
            TaskAttempt,
            r#"SELECT ta.id                  AS "id!: Uuid",
                     ta.task_id            AS "task_id!: Uuid",
                     ta.container_ref,
                     ta.branch,
                     ta.base_branch        AS "base_branch!",
                     ta.executor           AS "executor!",
                     ta.worktree_deleted   AS "worktree_deleted!: bool",
                     ta.setup_completed_at AS "setup_completed_at: DateTime<Utc>",
                     ta.created_at         AS "created_at!: DateTime<Utc>",
                     ta.updated_at         AS "updated_at!: DateTime<Utc>"
              FROM task_attempts ta
              JOIN execution_processes ep ON ep.task_attempt_id = ta.id
             WHERE ta.task_id = $1
               AND ep.run_reason = 'codingagent'
               AND ep.status = 'running'
             ORDER BY ep.created_at DESC
             LIMIT 1"#,
            task_id
        )
        .fetch_optional(pool)
        .await
    }

    /// Find the currently running coding-agent attempt for a project, if any.
    pub async fn find_active_coding_agent_for_project(
        pool: &SqlitePool,
        project_id: Uuid,
    ) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as!(
            TaskAttempt,
            r#"SELECT ta.id                  AS "id!: Uuid",
                     ta.task_id            AS "task_id!: Uuid",
                     ta.container_ref,
                     ta.branch,
                     ta.base_branch        AS "base_branch!",
                     ta.executor           AS "executor!",
                     ta.worktree_deleted   AS "worktree_deleted!: bool",
                     ta.setup_completed_at AS "setup_completed_at: DateTime<Utc>",
                     ta.created_at         AS "created_at!: DateTime<Utc>",
                     ta.updated_at         AS "updated_at!: DateTime<Utc>"
              FROM task_attempts ta
              JOIN tasks t ON ta.task_id = t.id
              JOIN execution_processes ep ON ep.task_attempt_id = ta.id
             WHERE t.project_id = $1
               AND ep.run_reason = 'codingagent'
               AND ep.status = 'running'
             ORDER BY ep.created_at DESC
             LIMIT 1"#,
            project_id
        )
        .fetch_optional(pool)
        .await
    }

    /// Find task attempts by task_id with project git repo path for cleanup operations
    pub async fn find_by_task_id_with_project(
        pool: &SqlitePool,
        task_id: Uuid,
    ) -> Result<Vec<(Uuid, Option<String>, String)>, sqlx::Error> {
        let records = sqlx::query!(
            r#"
            SELECT ta.id as "attempt_id!: Uuid", ta.container_ref, p.git_repo_path as "git_repo_path!"
            FROM task_attempts ta
            JOIN tasks t ON ta.task_id = t.id
            JOIN projects p ON t.project_id = p.id
            WHERE ta.task_id = $1
            "#,
            task_id
        )
        .fetch_all(pool)
        .await?;

        Ok(records
            .into_iter()
            .map(|r| (r.attempt_id, r.container_ref, r.git_repo_path))
            .collect())
    }

    pub async fn find_by_worktree_deleted(
        pool: &SqlitePool,
    ) -> Result<Vec<(Uuid, String)>, sqlx::Error> {
        let records = sqlx::query!(
        r#"SELECT id as "id!: Uuid", container_ref FROM task_attempts WHERE worktree_deleted = FALSE"#,
        )
        .fetch_all(pool).await?;
        Ok(records
            .into_iter()
            .filter_map(|r| r.container_ref.map(|path| (r.id, path)))
            .collect())
    }

    pub async fn container_ref_exists(
        pool: &SqlitePool,
        container_ref: &str,
    ) -> Result<bool, sqlx::Error> {
        let result = sqlx::query!(
            r#"SELECT EXISTS(SELECT 1 FROM task_attempts WHERE container_ref = ?) as "exists!: bool""#,
            container_ref
        )
        .fetch_one(pool)
        .await?;

        Ok(result.exists)
    }

    /// Find task attempts that are expired (72+ hours since last activity) and eligible for worktree cleanup
    /// Activity includes: execution completion, task attempt updates (including worktree recreation),
    /// and any attempts that are currently in progress
    pub async fn find_expired_for_cleanup(pool: &SqlitePool) -> Result<Vec<Uuid>, sqlx::Error> {
        let records = sqlx::query!(
            r#"
            SELECT ta.id as "attempt_id!: Uuid", ta.container_ref, p.git_repo_path as "git_repo_path!"
            FROM task_attempts ta
            LEFT JOIN execution_processes ep ON ta.id = ep.task_attempt_id AND ep.completed_at IS NOT NULL
            JOIN tasks t ON ta.task_id = t.id
            JOIN projects p ON t.project_id = p.id
            WHERE ta.worktree_deleted = FALSE
                -- Exclude attempts with any running processes (in progress)
                AND ta.id NOT IN (
                    SELECT DISTINCT ep2.task_attempt_id
                    FROM execution_processes ep2
                    WHERE ep2.completed_at IS NULL
                )
            GROUP BY ta.id, ta.container_ref, p.git_repo_path, ta.updated_at
            HAVING datetime('now', '-72 hours') > datetime(
                MAX(
                    CASE
                        WHEN ep.completed_at IS NOT NULL THEN ep.completed_at
                        ELSE ta.updated_at
                    END
                )
            )
            ORDER BY MAX(
                CASE
                    WHEN ep.completed_at IS NOT NULL THEN ep.completed_at
                    ELSE ta.updated_at
                END
            ) ASC
            "#
        )
        .fetch_all(pool)
        .await?;

        Ok(records.into_iter().map(|r| r.attempt_id).collect())
    }

    pub async fn clear_container_ref(
        pool: &SqlitePool,
        attempt_id: Uuid,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            "UPDATE task_attempts SET container_ref = NULL, updated_at = datetime('now', 'subsec') WHERE id = ?",
            attempt_id
        )
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn create(
        pool: &SqlitePool,
        data: &CreateTaskAttempt,
        task_id: Uuid,
    ) -> Result<Self, TaskAttemptError> {
        let attempt_id = Uuid::new_v4();
        let mut tx = pool.begin().await?;
        let task_row = sqlx::query!(
            r#"SELECT project_id as "project_id!: Uuid" FROM tasks WHERE id = $1"#,
            task_id
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(TaskAttemptError::TaskNotFound)?;

        let attempt = sqlx::query_as!(
            TaskAttempt,
            r#"INSERT INTO task_attempts (id, task_id, container_ref, branch, base_branch, executor, worktree_deleted, setup_completed_at)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
               RETURNING id as "id!: Uuid", task_id as "task_id!: Uuid", container_ref, branch, base_branch, executor as "executor!",  worktree_deleted as "worktree_deleted!: bool", setup_completed_at as "setup_completed_at: DateTime<Utc>", created_at as "created_at!: DateTime<Utc>", updated_at as "updated_at!: DateTime<Utc>""#,
            attempt_id,
            task_id,
            Option::<String>::None,
            Option::<String>::None,
            data.base_branch,
            data.executor,
            false,
            Option::<DateTime<Utc>>::None
        )
        .fetch_one(&mut *tx)
        .await?;

        #[derive(Clone)]
        struct RepoRow {
            id: Uuid,
            is_primary: bool,
        }

        let mut project_repositories: Vec<RepoRow> = sqlx::query!(
            r#"SELECT id as "id!: Uuid", is_primary as "is_primary!: bool"
               FROM project_repositories
               WHERE project_id = $1
               ORDER BY is_primary DESC, created_at ASC"#,
            task_row.project_id
        )
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .map(|row| RepoRow {
            id: row.id,
            is_primary: row.is_primary,
        })
        .collect();

        if project_repositories.is_empty() {
            let project = sqlx::query!(
                r#"SELECT git_repo_path FROM projects WHERE id = $1"#,
                task_row.project_id
            )
            .fetch_one(&mut *tx)
            .await?;

            let repo_id = Uuid::new_v4();
            sqlx::query!(
                r#"INSERT INTO project_repositories (id, project_id, name, git_repo_path, root_path, is_primary)
                   VALUES ($1, $2, $3, $4, $5, 1)"#,
                repo_id,
                task_row.project_id,
                "Primary",
                project.git_repo_path,
                ""
            )
            .execute(&mut *tx)
            .await?;

            project_repositories.push(RepoRow {
                id: repo_id,
                is_primary: true,
            });
        }

        let project_primary_id = project_repositories
            .iter()
            .find(|repo| repo.is_primary)
            .map(|repo| repo.id);
        let project_repo_ids: HashSet<Uuid> =
            project_repositories.iter().map(|repo| repo.id).collect();

        let selected_repositories: Vec<RepoRow> = if let Some(selection) = &data.repositories {
            if selection.is_empty() {
                project_repositories.clone()
            } else {
                let mut seen = HashSet::new();
                let mut explicit_primary_count = 0;
                let mut rows = Vec::with_capacity(selection.len());

                for repo in selection {
                    if !project_repo_ids.contains(&repo.project_repository_id) {
                        return Err(TaskAttemptError::ValidationError(format!(
                            "Repository {} is not configured for this project",
                            repo.project_repository_id
                        )));
                    }

                    if !seen.insert(repo.project_repository_id) {
                        return Err(TaskAttemptError::ValidationError(
                            "Duplicate repository selection".to_string(),
                        ));
                    }

                    if repo.is_primary {
                        explicit_primary_count += 1;
                    }

                    rows.push(RepoRow {
                        id: repo.project_repository_id,
                        is_primary: repo.is_primary,
                    });
                }

                if rows.is_empty() {
                    return Err(TaskAttemptError::ValidationError(
                        "At least one repository must be selected".to_string(),
                    ));
                }

                if explicit_primary_count > 1 {
                    return Err(TaskAttemptError::ValidationError(
                        "Only one repository may be marked as primary".to_string(),
                    ));
                }

                if explicit_primary_count == 0 {
                    if let Some(primary_id) = project_primary_id {
                        if let Some(entry) = rows.iter_mut().find(|row| row.id == primary_id) {
                            entry.is_primary = true;
                        } else if rows.len() == 1 {
                            rows[0].is_primary = true;
                        }
                    } else if rows.len() == 1 {
                        rows[0].is_primary = true;
                    }
                }

                let final_primary_count = rows.iter().filter(|row| row.is_primary).count();
                if final_primary_count != 1 {
                    return Err(TaskAttemptError::ValidationError(
                        "One repository must be marked as primary".to_string(),
                    ));
                }

                rows
            }
        } else {
            project_repositories.clone()
        };

        if selected_repositories.is_empty() {
            return Err(TaskAttemptError::ValidationError(
                "At least one repository must be selected".to_string(),
            ));
        }

        if selected_repositories
            .iter()
            .filter(|repo| repo.is_primary)
            .count()
            != 1
        {
            return Err(TaskAttemptError::ValidationError(
                "One repository must be marked as primary".to_string(),
            ));
        }

        for repo in selected_repositories {
            let task_repo_id = Uuid::new_v4();
            sqlx::query!(
                r#"INSERT INTO task_attempt_repositories (id, task_attempt_id, project_repository_id, is_primary)
                   VALUES ($1, $2, $3, $4)"#,
                task_repo_id,
                attempt.id,
                repo.id,
                repo.is_primary
            )
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;

        Ok(attempt)
    }

    pub async fn update_base_branch(
        pool: &SqlitePool,
        attempt_id: Uuid,
        new_base_branch: &str,
    ) -> Result<(), TaskAttemptError> {
        sqlx::query!(
            "UPDATE task_attempts SET base_branch = $1, updated_at = datetime('now') WHERE id = $2",
            new_base_branch,
            attempt_id,
        )
        .execute(pool)
        .await?;

        Ok(())
    }

    pub async fn resolve_container_ref(
        pool: &SqlitePool,
        container_ref: &str,
    ) -> Result<(Uuid, Uuid, Uuid), sqlx::Error> {
        let result = sqlx::query!(
            r#"SELECT ta.id as "attempt_id!: Uuid",
                      ta.task_id as "task_id!: Uuid",
                      t.project_id as "project_id!: Uuid"
               FROM task_attempts ta
               JOIN tasks t ON ta.task_id = t.id
               WHERE ta.container_ref = ?"#,
            container_ref
        )
        .fetch_optional(pool)
        .await?
        .ok_or(sqlx::Error::RowNotFound)?;

        Ok((result.attempt_id, result.task_id, result.project_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        execution_process::{
            CreateExecutionProcess, ExecutionProcess, ExecutionProcessRunReason,
            ExecutionProcessStatus,
        },
        project::{CreateProject, Project},
        project_repository::ProjectRepository,
        task::{CreateTask, Task},
        task_attempt_repository::TaskAttemptRepository,
    };
    use executors::{
        actions::{
            ExecutorAction, ExecutorActionType, coding_agent_initial::CodingAgentInitialRequest,
        },
        executors::BaseCodingAgent,
        profile::ExecutorProfileId,
    };
    use sqlx::{
        Pool, Sqlite,
        sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    };
    use std::str::FromStr;
    use uuid::Uuid;

    async fn setup_pool() -> Pool<Sqlite> {
        let options = SqliteConnectOptions::from_str("sqlite::memory:")
            .unwrap()
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    async fn seed_active_attempt(pool: &Pool<Sqlite>) -> (Uuid, TaskAttempt, ExecutionProcess) {
        let project_id = Uuid::new_v4();
        Project::create(
            pool,
            &CreateProject {
                name: "Test Project".to_string(),
                git_repo_path: format!("/tmp/{}", project_id),
                use_existing_repo: false,
                setup_script: None,
                dev_script: None,
                cleanup_script: None,
                copy_files: None,
            },
            project_id,
        )
        .await
        .unwrap();

        let task_id = Uuid::new_v4();
        let task = Task::create(
            pool,
            &CreateTask {
                project_id,
                title: "Parent Task".to_string(),
                description: None,
                parent_task_attempt: None,
                image_ids: None,
            },
            task_id,
        )
        .await
        .unwrap();

        let attempt = TaskAttempt::create(
            pool,
            &CreateTaskAttempt {
                executor: BaseCodingAgent::ClaudeCode,
                base_branch: "main".to_string(),
                repositories: None,
            },
            task.id,
        )
        .await
        .unwrap();

        let executor_action = ExecutorAction::new(
            ExecutorActionType::CodingAgentInitialRequest(CodingAgentInitialRequest {
                prompt: "bootstrap".to_string(),
                executor_profile_id: ExecutorProfileId::new(BaseCodingAgent::ClaudeCode),
            }),
            None,
        );

        let process = ExecutionProcess::create(
            pool,
            &CreateExecutionProcess {
                task_attempt_id: attempt.id,
                executor_action,
                run_reason: ExecutionProcessRunReason::CodingAgent,
            },
            Uuid::new_v4(),
            None,
        )
        .await
        .unwrap();

        (project_id, attempt, process)
    }

    #[tokio::test]
    async fn ensure_active_for_project_accepts_running_attempt() {
        let pool = setup_pool().await;
        let (project_id, attempt, _process) = seed_active_attempt(&pool).await;

        let result = TaskAttempt::ensure_active_for_project(&pool, attempt.id, project_id)
            .await
            .expect("active attempt should validate");

        assert_eq!(result.id, attempt.id);
    }

    #[tokio::test]
    async fn find_active_coding_agent_for_task_returns_running_attempt() {
        let pool = setup_pool().await;
        let (_project_id, attempt, _process) = seed_active_attempt(&pool).await;

        let found = TaskAttempt::find_active_coding_agent_for_task(&pool, attempt.task_id)
            .await
            .expect("query should succeed")
            .expect("expected active attempt");

        assert_eq!(found.id, attempt.id);
    }

    #[tokio::test]
    async fn find_active_coding_agent_for_task_skips_completed_attempt() {
        let pool = setup_pool().await;
        let (_project_id, attempt, process) = seed_active_attempt(&pool).await;

        ExecutionProcess::update_completion(
            &pool,
            process.id,
            ExecutionProcessStatus::Completed,
            Some(0),
        )
        .await
        .unwrap();

        let found = TaskAttempt::find_active_coding_agent_for_task(&pool, attempt.task_id)
            .await
            .expect("query should succeed");

        assert!(found.is_none(), "completed attempts should not be returned");
    }

    #[tokio::test]
    async fn find_active_coding_agent_for_project_returns_running_attempt() {
        let pool = setup_pool().await;
        let (project_id, attempt, _process) = seed_active_attempt(&pool).await;

        let found = TaskAttempt::find_active_coding_agent_for_project(&pool, project_id)
            .await
            .expect("query should succeed")
            .expect("expected active attempt");

        assert_eq!(found.id, attempt.id);
    }

    #[tokio::test]
    async fn find_active_coding_agent_for_project_skips_completed_attempt() {
        let pool = setup_pool().await;
        let (project_id, _attempt, process) = seed_active_attempt(&pool).await;

        ExecutionProcess::update_completion(
            &pool,
            process.id,
            ExecutionProcessStatus::Completed,
            Some(0),
        )
        .await
        .unwrap();

        let found = TaskAttempt::find_active_coding_agent_for_project(&pool, project_id)
            .await
            .expect("query should succeed");

        assert!(found.is_none(), "completed attempts should not be returned");
    }

    #[tokio::test]
    async fn ensure_active_for_project_rejects_inactive_attempt() {
        let pool = setup_pool().await;
        let (project_id, attempt, process) = seed_active_attempt(&pool).await;

        ExecutionProcess::update_completion(
            &pool,
            process.id,
            ExecutionProcessStatus::Completed,
            Some(0),
        )
        .await
        .unwrap();

        let err = TaskAttempt::ensure_active_for_project(&pool, attempt.id, project_id)
            .await
            .expect_err("completed attempt should not validate");

        match err {
            TaskAttemptError::ValidationError(msg) => {
                assert!(msg.contains("not active"));
            }
            other => panic!("unexpected error variant: {:?}", other),
        }
    }

    #[tokio::test]
    async fn ensure_active_for_project_rejects_wrong_project() {
        let pool = setup_pool().await;
        let (_project_id, attempt, _process) = seed_active_attempt(&pool).await;

        let other_project_id = Uuid::new_v4();
        Project::create(
            &pool,
            &CreateProject {
                name: "Other".to_string(),
                git_repo_path: format!("/tmp/{}", other_project_id),
                use_existing_repo: false,
                setup_script: None,
                dev_script: None,
                cleanup_script: None,
                copy_files: None,
            },
            other_project_id,
        )
        .await
        .unwrap();

        let err = TaskAttempt::ensure_active_for_project(&pool, attempt.id, other_project_id)
            .await
            .expect_err("attempt from another project should fail");

        match err {
            TaskAttemptError::ValidationError(msg) => {
                assert!(msg.contains("different project"));
            }
            other => panic!("unexpected error variant: {:?}", other),
        }
    }

    #[tokio::test]
    async fn create_populates_attempt_repositories_for_multi_repo_projects() {
        let pool = setup_pool().await;

        let project_id = Uuid::new_v4();
        let project = Project::create(
            &pool,
            &CreateProject {
                name: "Multi Repo".to_string(),
                git_repo_path: "/tmp/multi-repo".to_string(),
                use_existing_repo: true,
                setup_script: None,
                dev_script: None,
                cleanup_script: None,
                copy_files: None,
            },
            project_id,
        )
        .await
        .unwrap();

        let secondary_repo_id = Uuid::new_v4();
        sqlx::query(
            r#"INSERT INTO project_repositories (
                    id,
                    project_id,
                    name,
                    git_repo_path,
                    root_path,
                    is_primary
                )
                VALUES (?, ?, ?, ?, ?, 0)"#,
        )
        .bind(secondary_repo_id)
        .bind(project.id)
        .bind("Docs")
        .bind("/tmp/multi-repo")
        .bind("docs")
        .execute(&pool)
        .await
        .unwrap();

        let repositories = ProjectRepository::list_for_project(&pool, project.id)
            .await
            .unwrap();
        assert_eq!(
            repositories.len(),
            2,
            "expected both repositories to be registered"
        );

        let task_id = Uuid::new_v4();
        let task = Task::create(
            &pool,
            &CreateTask {
                project_id: project.id,
                title: "Test task".to_string(),
                description: None,
                parent_task_attempt: None,
                image_ids: None,
            },
            task_id,
        )
        .await
        .unwrap();

        let attempt = TaskAttempt::create(
            &pool,
            &CreateTaskAttempt {
                executor: BaseCodingAgent::ClaudeCode,
                base_branch: "main".to_string(),
                repositories: None,
            },
            task.id,
        )
        .await
        .unwrap();

        let attempt_repositories = TaskAttemptRepository::list_for_attempt(&pool, attempt.id)
            .await
            .unwrap();

        assert_eq!(attempt_repositories.len(), 2);
        assert_eq!(
            attempt_repositories
                .iter()
                .filter(|repo| repo.is_primary)
                .count(),
            1,
            "expected exactly one primary repository entry"
        );

        let secondary_entry = attempt_repositories
            .iter()
            .find(|repo| repo.project_repository_id == secondary_repo_id)
            .expect("secondary repository entry should exist");
        assert!(!secondary_entry.is_primary);
        assert!(secondary_entry.container_ref.is_none());
        assert!(secondary_entry.branch.is_none());
    }
}
