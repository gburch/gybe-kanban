use std::collections::HashSet;

use chrono::{DateTime, Utc};
use sqlx::{FromRow, Row, SqlitePool};
use uuid::Uuid;

use crate::models::task::TaskStatus;

#[derive(Debug, Clone)]
pub struct ActivityActorRow {
    pub id: Uuid,
    pub display_name: String,
}

#[derive(Debug, Clone, Copy)]
pub enum UrgencyHint {
    Low,
    Normal,
    Elevated,
    High,
    Critical,
}

#[derive(Debug, Clone)]
pub struct TaskActivityRow {
    pub entity_id: Uuid,
    pub event_id: Option<Uuid>,
    pub title: String,
    pub headline: Option<String>,
    pub body: Option<String>,
    pub status: Option<String>,
    pub actors: Vec<ActivityActorRow>,
    pub urgency_hint: Option<UrgencyHint>,
    pub restricted_to: Option<HashSet<Uuid>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct AttemptActivityRow {
    pub entity_id: Uuid,
    pub event_id: Option<Uuid>,
    pub task_id: Uuid,
    pub headline: Option<String>,
    pub body: Option<String>,
    pub state: Option<String>,
    pub executor: Option<String>,
    pub actors: Vec<ActivityActorRow>,
    pub urgency_hint: Option<UrgencyHint>,
    pub restricted_to: Option<HashSet<Uuid>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct CommentActivityRow {
    pub entity_id: Uuid,
    pub event_id: Option<Uuid>,
    pub headline: Option<String>,
    pub body: Option<String>,
    pub author_id: Option<Uuid>,
    pub actors: Vec<ActivityActorRow>,
    pub urgency_hint: Option<UrgencyHint>,
    pub restricted_to: Option<HashSet<Uuid>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct DeploymentActivityRow {
    pub entity_id: Uuid,
    pub event_id: Option<Uuid>,
    pub headline: Option<String>,
    pub body: Option<String>,
    pub status: Option<String>,
    pub url: Option<String>,
    pub actors: Vec<ActivityActorRow>,
    pub urgency_hint: Option<UrgencyHint>,
    pub restricted_to: Option<HashSet<Uuid>>,
    pub created_at: DateTime<Utc>,
}

fn task_status_to_string(status: &TaskStatus) -> &'static str {
    match status {
        TaskStatus::Todo => "todo",
        TaskStatus::InProgress => "inprogress",
        TaskStatus::InReview => "inreview",
        TaskStatus::Done => "done",
        TaskStatus::Cancelled => "cancelled",
    }
}

pub async fn fetch_task_activity(
    pool: &SqlitePool,
    project_id: Uuid,
    since: DateTime<Utc>,
) -> Result<Vec<TaskActivityRow>, sqlx::Error> {
    #[derive(Debug, FromRow)]
    struct TaskRecord {
        id: Uuid,
        title: String,
        description: Option<String>,
        status: TaskStatus,
        updated_at: DateTime<Utc>,
    }

    let records = sqlx::query_as::<_, TaskRecord>(
        "SELECT id, title, description, status, updated_at\n         FROM tasks\n         WHERE project_id = ? AND updated_at >= ?\n         ORDER BY updated_at DESC"
    )
    .bind(project_id)
    .bind(since)
    .fetch_all(pool)
    .await?;

    Ok(records
        .into_iter()
        .map(|rec| TaskActivityRow {
            entity_id: rec.id,
            event_id: None,
            title: rec.title.clone(),
            headline: Some(format!("Task updated: {}", rec.title)),
            body: rec.description.clone(),
            status: Some(task_status_to_string(&rec.status).to_string()),
            actors: Vec::new(),
            urgency_hint: None,
            restricted_to: None,
            created_at: rec.updated_at,
        })
        .collect())
}

pub async fn fetch_attempt_activity(
    pool: &SqlitePool,
    project_id: Uuid,
    since: DateTime<Utc>,
) -> Result<Vec<AttemptActivityRow>, sqlx::Error> {
    #[derive(Debug)]
    struct AttemptRecord {
        id: Uuid,
        task_id: Uuid,
        executor: Option<String>,
        state: Option<String>,
        updated_at: DateTime<Utc>,
    }

    impl<'r> FromRow<'r, sqlx::sqlite::SqliteRow> for AttemptRecord {
        fn from_row(row: &'r sqlx::sqlite::SqliteRow) -> Result<Self, sqlx::Error> {
            Ok(Self {
                id: row.try_get("id")?,
                task_id: row.try_get("task_id")?,
                executor: row.try_get("executor")?,
                state: row.try_get::<Option<String>, _>("state")?,
                updated_at: row.try_get("updated_at")?,
            })
        }
    }

    let records = sqlx::query_as::<_, AttemptRecord>(
        "SELECT ta.id, ta.task_id, ta.executor, ep.status AS state, ta.updated_at\n         FROM task_attempts ta\n         JOIN tasks t ON t.id = ta.task_id\n         LEFT JOIN execution_processes ep ON ep.task_attempt_id = ta.id\n         WHERE t.project_id = ? AND ta.updated_at >= ?\n         ORDER BY ta.updated_at DESC"
    )
    .bind(project_id)
    .bind(since)
    .fetch_all(pool)
    .await?;

    Ok(records
        .into_iter()
        .map(|rec| AttemptActivityRow {
            entity_id: rec.id,
            event_id: None,
            task_id: rec.task_id,
            headline: Some(format!("Attempt updated")),
            body: None,
            state: rec.state.map(|state| state.to_ascii_lowercase()),
            executor: rec.executor,
            actors: Vec::new(),
            urgency_hint: None,
            restricted_to: None,
            created_at: rec.updated_at,
        })
        .collect())
}

pub async fn fetch_comment_activity(
    _pool: &SqlitePool,
    _project_id: Uuid,
    _since: DateTime<Utc>,
) -> Result<Vec<CommentActivityRow>, sqlx::Error> {
    Ok(Vec::new())
}

pub async fn fetch_deployment_activity(
    _pool: &SqlitePool,
    _project_id: Uuid,
    _since: DateTime<Utc>,
) -> Result<Vec<DeploymentActivityRow>, sqlx::Error> {
    Ok(Vec::new())
}
