use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, QueryBuilder, Sqlite, SqlitePool, Transaction};
use thiserror::Error;
use ts_rs::TS;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum ProjectRepositoryError {
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error("Validation error: {0}")]
    Validation(String),
    #[error("A repository with this name already exists for the project")]
    DuplicateName,
    #[error("A repository with this path and root already exists for the project")]
    DuplicatePath,
    #[error("Repository not found")]
    NotFound,
    #[error("At least one primary repository is required for each project")]
    PrimaryRequired,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize, TS)]
pub struct ProjectRepository {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    #[ts(type = "string")]
    pub git_repo_path: PathBuf,
    pub root_path: String,
    pub is_primary: bool,
    #[ts(type = "Date")]
    pub created_at: DateTime<Utc>,
    #[ts(type = "Date")]
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, TS)]
pub struct CreateProjectRepository {
    pub name: String,
    pub git_repo_path: String,
    #[serde(default)]
    pub root_path: Option<String>,
    #[serde(default)]
    pub is_primary: bool,
}

#[derive(Debug, Deserialize, TS)]
pub struct UpdateProjectRepository {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub git_repo_path: Option<String>,
    #[serde(default)]
    pub root_path: Option<String>,
    #[serde(default)]
    pub is_primary: Option<bool>,
}

impl ProjectRepository {
    pub async fn list_for_project(
        pool: &SqlitePool,
        project_id: Uuid,
    ) -> Result<Vec<Self>, sqlx::Error> {
        sqlx::query_as!(
            ProjectRepository,
            r#"SELECT id as "id!: Uuid",
                      project_id as "project_id!: Uuid",
                      name,
                      git_repo_path,
                      root_path,
                      is_primary as "is_primary!: bool",
                      created_at as "created_at!: DateTime<Utc>",
                      updated_at as "updated_at!: DateTime<Utc>"
               FROM project_repositories
               WHERE project_id = $1
               ORDER BY is_primary DESC, created_at ASC"#,
            project_id
        )
        .fetch_all(pool)
        .await
    }

    pub async fn find_by_id(pool: &SqlitePool, id: Uuid) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as!(
            ProjectRepository,
            r#"SELECT id as "id!: Uuid",
                      project_id as "project_id!: Uuid",
                      name,
                      git_repo_path,
                      root_path,
                      is_primary as "is_primary!: bool",
                      created_at as "created_at!: DateTime<Utc>",
                      updated_at as "updated_at!: DateTime<Utc>"
               FROM project_repositories
               WHERE id = $1"#,
            id
        )
        .fetch_optional(pool)
        .await
    }

    pub async fn find_primary(
        pool: &SqlitePool,
        project_id: Uuid,
    ) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as!(
            ProjectRepository,
            r#"SELECT id as "id!: Uuid",
                      project_id as "project_id!: Uuid",
                      name,
                      git_repo_path,
                      root_path,
                      is_primary as "is_primary!: bool",
                      created_at as "created_at!: DateTime<Utc>",
                      updated_at as "updated_at!: DateTime<Utc>"
               FROM project_repositories
               WHERE project_id = $1 AND is_primary = 1"#,
            project_id
        )
        .fetch_optional(pool)
        .await
    }

    pub async fn create(
        pool: &SqlitePool,
        project_id: Uuid,
        data: &CreateProjectRepository,
    ) -> Result<Self, ProjectRepositoryError> {
        if data.name.trim().is_empty() {
            return Err(ProjectRepositoryError::Validation(
                "Repository name cannot be empty".to_string(),
            ));
        }

        if data.git_repo_path.trim().is_empty() {
            return Err(ProjectRepositoryError::Validation(
                "Repository path cannot be empty".to_string(),
            ));
        }

        let normalized_root = normalize_root_path(data.root_path.as_deref());

        let mut tx = pool.begin().await?;

        let name_exists = sqlx::query_scalar!(
            r#"SELECT EXISTS(
                    SELECT 1
                    FROM project_repositories
                    WHERE project_id = $1 AND LOWER(name) = LOWER($2)
                ) as "exists!: bool""#,
            project_id,
            data.name
        )
        .fetch_one(&mut *tx)
        .await?;

        if name_exists {
            return Err(ProjectRepositoryError::DuplicateName);
        }

        let path_exists = sqlx::query_scalar!(
            r#"SELECT EXISTS(
                    SELECT 1
                    FROM project_repositories
                    WHERE project_id = $1 AND git_repo_path = $2 AND root_path = $3
                ) as "exists!: bool""#,
            project_id,
            data.git_repo_path,
            normalized_root
        )
        .fetch_one(&mut *tx)
        .await?;

        if path_exists {
            return Err(ProjectRepositoryError::DuplicatePath);
        }

        if data.is_primary {
            sqlx::query!(
                r#"UPDATE project_repositories
                   SET is_primary = 0,
                       updated_at = datetime('now', 'subsec')
                   WHERE project_id = $1 AND is_primary = 1"#,
                project_id
            )
            .execute(&mut *tx)
            .await?;

            sqlx::query!(
                r#"UPDATE task_attempt_repositories
                   SET is_primary = 0,
                       updated_at = datetime('now', 'subsec')
                   WHERE project_repository_id IN (
                       SELECT id FROM project_repositories WHERE project_id = $1
                   )"#,
                project_id
            )
            .execute(&mut *tx)
            .await?;
        }

        let repo_id = Uuid::new_v4();
        let repository = sqlx::query_as!(
            ProjectRepository,
            r#"INSERT INTO project_repositories (
                    id, project_id, name, git_repo_path, root_path, is_primary
               ) VALUES ($1, $2, $3, $4, $5, $6)
               RETURNING id as "id!: Uuid",
                         project_id as "project_id!: Uuid",
                         name,
                         git_repo_path,
                         root_path,
                         is_primary as "is_primary!: bool",
                         created_at as "created_at!: DateTime<Utc>",
                         updated_at as "updated_at!: DateTime<Utc>""#,
            repo_id,
            project_id,
            data.name,
            data.git_repo_path,
            normalized_root,
            data.is_primary
        )
        .fetch_one(&mut *tx)
        .await?;

        ensure_attempt_memberships(&mut tx, project_id, repository.id, repository.is_primary)
            .await?;
        sync_task_attempt_repository_flags(&mut tx, project_id).await?;

        tx.commit().await?;

        Ok(repository)
    }

    pub async fn update(
        pool: &SqlitePool,
        project_id: Uuid,
        repository_id: Uuid,
        data: &UpdateProjectRepository,
    ) -> Result<Self, ProjectRepositoryError> {
        let mut tx = pool.begin().await?;
        let existing = sqlx::query_as!(
            ProjectRepository,
            r#"SELECT id as "id!: Uuid",
                      project_id as "project_id!: Uuid",
                      name,
                      git_repo_path,
                      root_path,
                      is_primary as "is_primary!: bool",
                      created_at as "created_at!: DateTime<Utc>",
                      updated_at as "updated_at!: DateTime<Utc>"
               FROM project_repositories
               WHERE id = $1"#,
            repository_id
        )
        .fetch_optional(&mut *tx)
        .await?;

        let Some(existing) = existing else {
            return Err(ProjectRepositoryError::NotFound);
        };

        if existing.project_id != project_id {
            return Err(ProjectRepositoryError::NotFound);
        }

        let resolved_name = if let Some(name) = data.name.as_ref() {
            let trimmed = name.trim();
            if trimmed.is_empty() {
                return Err(ProjectRepositoryError::Validation(
                    "Repository name cannot be empty".to_string(),
                ));
            }
            trimmed.to_string()
        } else {
            existing.name.clone()
        };

        let resolved_path = if let Some(path) = data.git_repo_path.as_ref() {
            let trimmed = path.trim();
            if trimmed.is_empty() {
                return Err(ProjectRepositoryError::Validation(
                    "Repository path cannot be empty".to_string(),
                ));
            }
            trimmed.to_string()
        } else {
            existing.git_repo_path.to_string_lossy().to_string()
        };

        let resolved_root = if let Some(root) = data.root_path.as_ref() {
            normalize_root_path(Some(root))
        } else {
            existing.root_path.clone()
        };

        let resolved_primary = data.is_primary.unwrap_or(existing.is_primary);

        if resolved_name.to_lowercase() != existing.name.to_lowercase() {
            let name_exists = sqlx::query_scalar!(
                r#"SELECT EXISTS(
                        SELECT 1
                        FROM project_repositories
                        WHERE project_id = $1 AND LOWER(name) = LOWER($2) AND id != $3
                    ) as "exists!: bool""#,
                project_id,
                resolved_name,
                repository_id
            )
            .fetch_one(&mut *tx)
            .await?;

            if name_exists {
                return Err(ProjectRepositoryError::DuplicateName);
            }
        }

        let existing_path = existing.git_repo_path.to_string_lossy().to_string();
        if existing_path != resolved_path || existing.root_path != resolved_root {
            let path_exists = sqlx::query_scalar!(
                r#"SELECT EXISTS(
                        SELECT 1
                        FROM project_repositories
                        WHERE project_id = $1
                          AND git_repo_path = $2
                          AND root_path = $3
                          AND id != $4
                    ) as "exists!: bool""#,
                project_id,
                resolved_path,
                resolved_root,
                repository_id
            )
            .fetch_one(&mut *tx)
            .await?;

            if path_exists {
                return Err(ProjectRepositoryError::DuplicatePath);
            }
        }

        if !resolved_primary && existing.is_primary {
            let other_primary_exists = sqlx::query_scalar!(
                r#"SELECT EXISTS(
                        SELECT 1
                        FROM project_repositories
                        WHERE project_id = $1 AND id != $2 AND is_primary = 1
                    ) as "exists!: bool""#,
                project_id,
                repository_id
            )
            .fetch_one(&mut *tx)
            .await?;

            if !other_primary_exists {
                return Err(ProjectRepositoryError::PrimaryRequired);
            }
        }

        if resolved_primary {
            sqlx::query!(
                r#"UPDATE project_repositories
                   SET is_primary = 0,
                       updated_at = datetime('now', 'subsec')
                   WHERE project_id = $1 AND id != $2 AND is_primary = 1"#,
                project_id,
                repository_id
            )
            .execute(&mut *tx)
            .await?;

            sqlx::query!(
                r#"UPDATE task_attempt_repositories
                   SET is_primary = 0,
                       updated_at = datetime('now', 'subsec')
                   WHERE project_repository_id IN (
                       SELECT id FROM project_repositories WHERE project_id = $1 AND id != $2
                   )"#,
                project_id,
                repository_id
            )
            .execute(&mut *tx)
            .await?;
        }

        let repository = sqlx::query_as!(
            ProjectRepository,
            r#"UPDATE project_repositories
               SET name = $2,
                   git_repo_path = $3,
                   root_path = $4,
                   is_primary = $5,
                   updated_at = datetime('now', 'subsec')
               WHERE id = $1
               RETURNING id as "id!: Uuid",
                         project_id as "project_id!: Uuid",
                         name,
                         git_repo_path,
                         root_path,
                         is_primary as "is_primary!: bool",
                         created_at as "created_at!: DateTime<Utc>",
                         updated_at as "updated_at!: DateTime<Utc>""#,
            repository_id,
            resolved_name,
            resolved_path,
            resolved_root,
            resolved_primary
        )
        .fetch_one(&mut *tx)
        .await?;

        ensure_attempt_memberships(&mut tx, project_id, repository_id, repository.is_primary)
            .await?;
        sync_task_attempt_repository_flags(&mut tx, project_id).await?;

        tx.commit().await?;

        Ok(repository)
    }

    pub async fn delete(
        pool: &SqlitePool,
        project_id: Uuid,
        repository_id: Uuid,
    ) -> Result<(), ProjectRepositoryError> {
        let mut tx = pool.begin().await?;
        let repository = sqlx::query_as!(
            ProjectRepository,
            r#"SELECT id as "id!: Uuid",
                      project_id as "project_id!: Uuid",
                      name,
                      git_repo_path,
                      root_path,
                      is_primary as "is_primary!: bool",
                      created_at as "created_at!: DateTime<Utc>",
                      updated_at as "updated_at!: DateTime<Utc>"
               FROM project_repositories
               WHERE id = $1"#,
            repository_id
        )
        .fetch_optional(&mut *tx)
        .await?;

        let Some(repository) = repository else {
            return Err(ProjectRepositoryError::NotFound);
        };

        if repository.project_id != project_id {
            return Err(ProjectRepositoryError::NotFound);
        }

        let replacement_primary = if repository.is_primary {
            let candidate = sqlx::query_scalar!(
                r#"SELECT id as "id!: Uuid"
                   FROM project_repositories
                   WHERE project_id = $1 AND id != $2
                   ORDER BY is_primary DESC, created_at ASC
                   LIMIT 1"#,
                project_id,
                repository_id
            )
            .fetch_optional(&mut *tx)
            .await?;

            match candidate {
                Some(id) => Some(id),
                None => return Err(ProjectRepositoryError::PrimaryRequired),
            }
        } else {
            None
        };

        sqlx::query!(
            "DELETE FROM project_repositories WHERE id = $1",
            repository_id
        )
        .execute(&mut *tx)
        .await?;

        if let Some(primary_id) = replacement_primary {
            sqlx::query!(
                r#"UPDATE project_repositories
                   SET is_primary = 1,
                       updated_at = datetime('now', 'subsec')
                   WHERE id = $1"#,
                primary_id
            )
            .execute(&mut *tx)
            .await?;
        }

        sync_task_attempt_repository_flags(&mut tx, project_id).await?;

        tx.commit().await?;

        Ok(())
    }
}

fn normalize_root_path(root_path: Option<&str>) -> String {
    let mut value = root_path.unwrap_or_default().trim().to_string();

    while value.starts_with("./") {
        value = value[2..].trim_start().to_string();
    }

    value = value.trim_matches(|c| "/\\".contains(c)).to_string();

    if value == "." { String::new() } else { value }
}

async fn ensure_attempt_memberships(
    tx: &mut Transaction<'_, Sqlite>,
    project_id: Uuid,
    repository_id: Uuid,
    is_primary: bool,
) -> Result<(), sqlx::Error> {
    let attempt_ids: Vec<Uuid> = sqlx::query_scalar!(
        r#"SELECT ta.id as "id!: Uuid"
           FROM task_attempts ta
           INNER JOIN tasks t ON ta.task_id = t.id
           WHERE t.project_id = $1"#,
        project_id
    )
    .fetch_all(&mut **tx)
    .await?;

    if attempt_ids.is_empty() {
        return Ok(());
    }

    let mut builder = QueryBuilder::new(
        "INSERT INTO task_attempt_repositories (id, task_attempt_id, project_repository_id, is_primary) ",
    );
    builder.push_values(attempt_ids.iter(), |mut row, attempt_id| {
        row.push_bind(Uuid::new_v4());
        row.push_bind(*attempt_id);
        row.push_bind(repository_id);
        row.push_bind(is_primary);
    });
    builder.push(
        " ON CONFLICT(task_attempt_id, project_repository_id) DO UPDATE SET is_primary = excluded.is_primary, updated_at = datetime('now', 'subsec')",
    );

    builder.build().execute(&mut **tx).await?;

    Ok(())
}

async fn sync_task_attempt_repository_flags(
    tx: &mut Transaction<'_, Sqlite>,
    project_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"UPDATE task_attempt_repositories
           SET is_primary = (
               SELECT pr.is_primary
               FROM project_repositories pr
               WHERE pr.id = task_attempt_repositories.project_repository_id
           ),
               updated_at = datetime('now', 'subsec')
           WHERE project_repository_id IN (
               SELECT id FROM project_repositories WHERE project_id = $1
           )"#,
        project_id
    )
    .execute(&mut **tx)
    .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        project::{CreateProject, Project},
        task::{CreateTask, Task},
        task_attempt::{CreateTaskAttempt, TaskAttempt},
        task_attempt_repository::TaskAttemptRepository,
    };
    use executors::executors::BaseCodingAgent;
    use sqlx::{
        Pool, Sqlite,
        sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    };
    use std::str::FromStr;

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

    async fn seed_project_with_attempt(
        pool: &Pool<Sqlite>,
    ) -> (Project, TaskAttempt, ProjectRepository) {
        let project_id = Uuid::new_v4();
        let project = Project::create(
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
                project_id: project.id,
                title: "Task".to_string(),
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

        let primary = ProjectRepository::find_primary(pool, project.id)
            .await
            .unwrap()
            .unwrap();

        (project, attempt, primary)
    }

    #[tokio::test]
    async fn create_repository_sets_primary_and_attempt_metadata() {
        let pool = setup_pool().await;
        let (project, attempt, _primary) = seed_project_with_attempt(&pool).await;

        let request = CreateProjectRepository {
            name: "Secondary".to_string(),
            git_repo_path: project.git_repo_path.to_string_lossy().to_string(),
            root_path: Some("packages/api".to_string()),
            is_primary: true,
        };

        let created = ProjectRepository::create(&pool, project.id, &request)
            .await
            .expect("create repo");

        assert!(created.is_primary);

        let current_primary = ProjectRepository::find_primary(&pool, project.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(current_primary.id, created.id);

        let attempt_repos = TaskAttemptRepository::list_for_attempt(&pool, attempt.id)
            .await
            .unwrap();
        let primary_entry = attempt_repos
            .iter()
            .find(|entry| entry.is_primary)
            .expect("primary attempt repo");
        assert_eq!(primary_entry.project_repository_id, created.id);
    }

    #[tokio::test]
    async fn update_repository_prevents_dropping_last_primary() {
        let pool = setup_pool().await;
        let (project, _attempt, primary) = seed_project_with_attempt(&pool).await;

        let update = UpdateProjectRepository {
            name: None,
            git_repo_path: None,
            root_path: None,
            is_primary: Some(false),
        };

        let result = ProjectRepository::update(&pool, project.id, primary.id, &update).await;
        assert!(matches!(
            result,
            Err(ProjectRepositoryError::PrimaryRequired)
        ));
    }

    #[tokio::test]
    async fn delete_primary_promotes_fallback() {
        let pool = setup_pool().await;
        let (project, attempt, primary) = seed_project_with_attempt(&pool).await;

        let secondary = ProjectRepository::create(
            &pool,
            project.id,
            &CreateProjectRepository {
                name: "Secondary".to_string(),
                git_repo_path: project.git_repo_path.to_string_lossy().to_string(),
                root_path: Some("apps/client".to_string()),
                is_primary: false,
            },
        )
        .await
        .unwrap();

        ProjectRepository::delete(&pool, project.id, primary.id)
            .await
            .expect("delete primary");

        let new_primary = ProjectRepository::find_primary(&pool, project.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(new_primary.id, secondary.id);

        let attempt_repos = TaskAttemptRepository::list_for_attempt(&pool, attempt.id)
            .await
            .unwrap();
        let primary_entry = attempt_repos
            .iter()
            .find(|entry| entry.is_primary)
            .expect("primary attempt repo");
        assert_eq!(primary_entry.project_repository_id, secondary.id);
    }
}
