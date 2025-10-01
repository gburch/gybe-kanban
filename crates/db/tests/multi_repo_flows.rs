use std::{fs, path::PathBuf};

use db::models::{
    project::{CreateProject, Project},
    project_repository::{
        CreateProjectRepository, ProjectRepository, ProjectRepositoryError, UpdateProjectRepository,
    },
    task::{CreateTask, Task},
    task_attempt::{CreateTaskAttempt, CreateTaskAttemptRepository, TaskAttempt},
    task_attempt_repository::TaskAttemptRepository,
};
use executors::executors::BaseCodingAgent;
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use tempfile::TempDir;
use uuid::Uuid;

type TestResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

async fn setup_pool() -> TestResult<(TempDir, SqlitePool)> {
    let seed_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../dev_assets_seed/db.sqlite");
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("multi_repo_test.sqlite");
    fs::copy(seed_path, &db_path)?;

    let options = SqliteConnectOptions::new()
        .filename(&db_path)
        .create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;

    MIGRATOR.run(&pool).await?;

    Ok((temp_dir, pool))
}

async fn seed_project(pool: &SqlitePool, name: &str) -> TestResult<Project> {
    let project_id = Uuid::new_v4();
    let project = Project::create(
        pool,
        &CreateProject {
            name: name.to_string(),
            git_repo_path: format!("/tmp/{project_id}"),
            use_existing_repo: false,
            setup_script: None,
            dev_script: None,
            cleanup_script: None,
            copy_files: None,
        },
        project_id,
    )
    .await?;

    Ok(project)
}

async fn seed_task(pool: &SqlitePool, project: &Project, title: &str) -> TestResult<Task> {
    let task_id = Uuid::new_v4();
    let task = Task::create(
        pool,
        &CreateTask {
            project_id: project.id,
            title: title.to_string(),
            description: None,
            parent_task_attempt: None,
            image_ids: None,
        },
        task_id,
    )
    .await?;

    Ok(task)
}

#[tokio::test]
async fn repository_crud_flow_updates_attempt_metadata() -> TestResult<()> {
    let (_guard, pool) = setup_pool().await?;
    let project = seed_project(&pool, "Multi repo project").await?;
    let task = seed_task(&pool, &project, "Integrate repos").await?;

    let primary = ProjectRepository::find_primary(&pool, project.id)
        .await?
        .expect("primary repository");

    let secondary = ProjectRepository::create(
        &pool,
        project.id,
        &CreateProjectRepository {
            name: "Docs".to_string(),
            git_repo_path: format!("{}/docs", project.git_repo_path.display()),
            root_path: Some("docs".to_string()),
            is_primary: false,
        },
    )
    .await?;

    let attempt = TaskAttempt::create(
        &pool,
        &CreateTaskAttempt {
            executor: BaseCodingAgent::ClaudeCode,
            base_branch: "main".to_string(),
            branch: "feature/test".to_string(),
            repositories: None,
        },
        Uuid::new_v4(),
        task.id,
    )
    .await?;

    let before_switch = TaskAttemptRepository::list_for_attempt(&pool, attempt.id).await?;
    assert_eq!(
        before_switch.len(),
        2,
        "expected both repositories to be linked"
    );
    assert_eq!(
        before_switch
            .iter()
            .filter(|entry| entry.is_primary)
            .count(),
        1,
        "expected exactly one primary before promotion",
    );

    ProjectRepository::update(
        &pool,
        project.id,
        secondary.id,
        &UpdateProjectRepository {
            name: None,
            git_repo_path: None,
            root_path: None,
            is_primary: Some(true),
        },
    )
    .await?;

    let after_switch = TaskAttemptRepository::list_for_attempt(&pool, attempt.id).await?;
    let new_primary = after_switch
        .iter()
        .find(|entry| entry.is_primary)
        .expect("primary attempt repository after switch");
    assert_eq!(
        new_primary.project_repository_id, secondary.id,
        "primary repository selection should follow project primary"
    );

    ProjectRepository::delete(&pool, project.id, primary.id).await?;
    let repos = ProjectRepository::list_for_project(&pool, project.id).await?;
    assert_eq!(repos.len(), 1, "only promoted repository should remain");
    assert!(
        repos[0].is_primary,
        "remaining repository must stay primary"
    );

    Ok(())
}

#[tokio::test]
async fn attempt_explicit_repository_selection_respected() -> TestResult<()> {
    let (_guard, pool) = setup_pool().await?;
    let project = seed_project(&pool, "Explicit selection").await?;
    let task = seed_task(&pool, &project, "Custom attempt").await?;

    let default_primary = ProjectRepository::find_primary(&pool, project.id)
        .await?
        .expect("primary repository");

    let shared_utils = ProjectRepository::create(
        &pool,
        project.id,
        &CreateProjectRepository {
            name: "Shared utils".to_string(),
            git_repo_path: format!("{}/shared", project.git_repo_path.display()),
            root_path: Some("shared".to_string()),
            is_primary: false,
        },
    )
    .await?;

    let attempt = TaskAttempt::create(
        &pool,
        &CreateTaskAttempt {
            executor: BaseCodingAgent::ClaudeCode,
            base_branch: "develop".to_string(),
            branch: "feature/test".to_string(),
            repositories: Some(vec![
                CreateTaskAttemptRepository {
                    project_repository_id: default_primary.id,
                    is_primary: false,
                },
                CreateTaskAttemptRepository {
                    project_repository_id: shared_utils.id,
                    is_primary: true,
                },
            ]),
        },
        Uuid::new_v4(),
        task.id,
    )
    .await?;

    let repositories = TaskAttemptRepository::list_for_attempt(&pool, attempt.id).await?;
    assert_eq!(
        repositories.len(),
        2,
        "explicit selection should persist both repos"
    );

    let primary = repositories
        .iter()
        .find(|entry| entry.is_primary)
        .expect("primary repository for attempt");
    assert_eq!(
        primary.project_repository_id, shared_utils.id,
        "attempt should honour caller primary selection"
    );

    let default_entry = repositories
        .iter()
        .find(|entry| entry.project_repository_id == default_primary.id)
        .expect("default repository entry");
    assert!(
        !default_entry.is_primary,
        "default repo should stay non-primary"
    );

    Ok(())
}

#[tokio::test]
async fn cannot_drop_last_primary_via_delete_in_integration_flow() -> TestResult<()> {
    let (_guard, pool) = setup_pool().await?;
    let project = seed_project(&pool, "Primary guard").await?;

    let primary = ProjectRepository::find_primary(&pool, project.id)
        .await?
        .expect("primary repository");

    let result = ProjectRepository::delete(&pool, project.id, primary.id).await;
    assert!(matches!(
        result,
        Err(ProjectRepositoryError::PrimaryRequired)
    ));

    Ok(())
}
