use std::{str::FromStr, sync::Arc};

use db::{
    DBService,
    models::{
        draft::{Draft, DraftType},
        execution_process::ExecutionProcess,
        task::{Task, TaskWithAttemptStatus},
        task_attempt::TaskAttempt,
    },
};
use serde_json::json;
use sqlx::{Error as SqlxError, Sqlite, SqlitePool, decode::Decode, sqlite::SqliteOperation};
use tokio::sync::RwLock;
use utils::msg_store::MsgStore;
use uuid::Uuid;

#[path = "events/patches.rs"]
pub mod patches;
#[path = "events/streams.rs"]
mod streams;
#[path = "events/types.rs"]
pub mod types;

pub use patches::{draft_patch, execution_process_patch, task_attempt_patch, task_patch};
pub use types::{EventError, EventPatch, EventPatchInner, HookTables, RecordTypes};

#[derive(Clone)]
pub struct EventService {
    msg_store: Arc<MsgStore>,
    db: DBService,
    #[allow(dead_code)]
    entry_count: Arc<RwLock<usize>>,
}

impl EventService {
    /// Creates a new EventService that will work with a DBService configured with hooks
    pub fn new(db: DBService, msg_store: Arc<MsgStore>, entry_count: Arc<RwLock<usize>>) -> Self {
        Self {
            msg_store,
            db,
            entry_count,
        }
    }

    async fn push_task_update_for_task(
        pool: &SqlitePool,
        msg_store: Arc<MsgStore>,
        task_id: Uuid,
    ) -> Result<(), SqlxError> {
        if let Some(task) = Task::find_by_id(pool, task_id).await? {
            let tasks = Task::find_by_project_id_with_attempt_status(pool, task.project_id).await?;

            if let Some(task_with_status) = tasks
                .into_iter()
                .find(|task_with_status| task_with_status.id == task_id)
            {
                msg_store.push_patch(task_patch::replace(&task_with_status));
            }
        }

        Ok(())
    }

    async fn push_task_update_for_attempt(
        pool: &SqlitePool,
        msg_store: Arc<MsgStore>,
        attempt_id: Uuid,
    ) -> Result<(), SqlxError> {
        if let Some(attempt) = TaskAttempt::find_by_id(pool, attempt_id).await? {
            Self::push_task_update_for_task(pool, msg_store, attempt.task_id).await?;
        }

        Ok(())
    }

    /// Creates the hook function that should be used with DBService::new_with_after_connect
    pub fn create_hook(
        msg_store: Arc<MsgStore>,
        entry_count: Arc<RwLock<usize>>,
        db_service: DBService,
    ) -> impl for<'a> Fn(
        &'a mut sqlx::sqlite::SqliteConnection,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<(), sqlx::Error>> + Send + 'a>,
    > + Send
    + Sync
    + 'static {
        move |conn: &mut sqlx::sqlite::SqliteConnection| {
            let msg_store_for_hook = msg_store.clone();
            let entry_count_for_hook = entry_count.clone();
            let db_for_hook = db_service.clone();
            Box::pin(async move {
                let mut handle = conn.lock_handle().await?;
                let runtime_handle = tokio::runtime::Handle::current();
                handle.set_preupdate_hook({
                    let msg_store_for_preupdate = msg_store_for_hook.clone();
                    move |preupdate: sqlx::sqlite::PreupdateHookResult<'_>| {
                        if preupdate.operation != SqliteOperation::Delete {
                            return;
                        }

                        match preupdate.table {
                            "tasks" => {
                                if let Ok(value) = preupdate.get_old_column_value(0)
                                    && let Ok(task_id) = <Uuid as Decode<Sqlite>>::decode(value)
                                {
                                    let patch = task_patch::remove(task_id);
                                    msg_store_for_preupdate.push_patch(patch);
                                }
                            }
                            "task_attempts" => {
                                if let Ok(value) = preupdate.get_old_column_value(0)
                                    && let Ok(attempt_id) = <Uuid as Decode<Sqlite>>::decode(value)
                                {
                                    let patch = task_attempt_patch::remove(attempt_id);
                                    msg_store_for_preupdate.push_patch(patch);
                                }
                            }
                            "execution_processes" => {
                                if let Ok(value) = preupdate.get_old_column_value(0)
                                    && let Ok(process_id) = <Uuid as Decode<Sqlite>>::decode(value)
                                {
                                    let patch = execution_process_patch::remove(process_id);
                                    msg_store_for_preupdate.push_patch(patch);
                                }
                            }
                            "drafts" => {
                                let draft_type = preupdate
                                    .get_old_column_value(2)
                                    .ok()
                                    .and_then(|val| <String as Decode<Sqlite>>::decode(val).ok())
                                    .and_then(|s| DraftType::from_str(&s).ok());
                                let task_attempt_id = preupdate
                                    .get_old_column_value(1)
                                    .ok()
                                    .and_then(|val| <Uuid as Decode<Sqlite>>::decode(val).ok());

                                if let (Some(draft_type), Some(task_attempt_id)) =
                                    (draft_type, task_attempt_id)
                                {
                                    let patch = match draft_type {
                                        DraftType::FollowUp => {
                                            draft_patch::follow_up_clear(task_attempt_id)
                                        }
                                        DraftType::Retry => {
                                            draft_patch::retry_clear(task_attempt_id)
                                        }
                                    };
                                    msg_store_for_preupdate.push_patch(patch);
                                }
                            }
                            _ => {}
                        }
                    }
                });

                handle.set_update_hook(move |hook: sqlx::sqlite::UpdateHookResult<'_>| {
                    let runtime_handle = runtime_handle.clone();
                    let entry_count_for_hook = entry_count_for_hook.clone();
                    let msg_store_for_hook = msg_store_for_hook.clone();
                    let db = db_for_hook.clone();

                    if let Ok(table) = HookTables::from_str(hook.table) {
                        let rowid = hook.rowid;
                        runtime_handle.spawn(async move {
                            let record_type: RecordTypes = match (table, hook.operation.clone()) {
                                (HookTables::Tasks, SqliteOperation::Delete)
                                | (HookTables::TaskAttempts, SqliteOperation::Delete)
                                | (HookTables::ExecutionProcesses, SqliteOperation::Delete)
                                | (HookTables::Drafts, SqliteOperation::Delete) => {
                                    // Deletions handled in preupdate hook for reliable data capture
                                    return;
                                }
                                (HookTables::Tasks, _) => {
                                    match Task::find_by_rowid(&db.pool, rowid).await {
                                        Ok(Some(task)) => RecordTypes::Task(task),
                                        Ok(None) => RecordTypes::DeletedTask {
                                            rowid,
                                            project_id: None,
                                            task_id: None,
                                        },
                                        Err(e) => {
                                            tracing::error!("Failed to fetch task: {:?}", e);
                                            return;
                                        }
                                    }
                                }
                                (HookTables::TaskAttempts, _) => {
                                    match TaskAttempt::find_by_rowid(&db.pool, rowid).await {
                                        Ok(Some(attempt)) => RecordTypes::TaskAttempt(attempt),
                                        Ok(None) => RecordTypes::DeletedTaskAttempt {
                                            rowid,
                                            task_id: None,
                                        },
                                        Err(e) => {
                                            tracing::error!(
                                                "Failed to fetch task_attempt: {:?}",
                                                e
                                            );
                                            return;
                                        }
                                    }
                                }
                                (HookTables::ExecutionProcesses, _) => {
                                    match ExecutionProcess::find_by_rowid(&db.pool, rowid).await {
                                        Ok(Some(process)) => RecordTypes::ExecutionProcess(process),
                                        Ok(None) => RecordTypes::DeletedExecutionProcess {
                                            rowid,
                                            task_attempt_id: None,
                                            process_id: None,
                                        },
                                        Err(e) => {
                                            tracing::error!(
                                                "Failed to fetch execution_process: {:?}",
                                                e
                                            );
                                            return;
                                        }
                                    }
                                }
                                (HookTables::Drafts, _) => {
                                    match Draft::find_by_rowid(&db.pool, rowid).await {
                                        Ok(Some(draft)) => match draft.draft_type {
                                            DraftType::FollowUp => RecordTypes::Draft(draft),
                                            DraftType::Retry => RecordTypes::RetryDraft(draft),
                                        },
                                        Ok(None) => RecordTypes::DeletedDraft {
                                            rowid,
                                            draft_type: DraftType::Retry,
                                            task_attempt_id: None,
                                        },
                                        Err(e) => {
                                            tracing::error!("Failed to fetch draft: {:?}", e);
                                            return;
                                        }
                                    }
                                }
                            };

                            let db_op: &str = match hook.operation {
                                SqliteOperation::Insert => "insert",
                                SqliteOperation::Delete => "delete",
                                SqliteOperation::Update => "update",
                                SqliteOperation::Unknown(_) => "unknown",
                            };

                            // Handle task-related operations with direct patches
                            match &record_type {
                                RecordTypes::Task(task) => {
                                    let fetched = Task::find_by_project_id_with_attempt_status(
                                        &db.pool,
                                        task.project_id,
                                    )
                                    .await
                                    .ok()
                                    .and_then(|task_list| {
                                        task_list.into_iter().find(|t| t.id == task.id)
                                    });

                                    let (task_with_status, is_fallback) = if let Some(found) = fetched {
                                        (found, false)
                                    } else {
                                        (
                                            TaskWithAttemptStatus {
                                                task: task.clone(),
                                                has_in_progress_attempt: false,
                                                has_running_dev_server: false,
                                                has_merged_attempt: false,
                                                last_attempt_failed: false,
                                                executor: String::new(),
                                            },
                                            true,
                                        )
                                    };

                                    let patch = match hook.operation {
                                        SqliteOperation::Insert => task_patch::add(&task_with_status),
                                        SqliteOperation::Update => {
                                            task_patch::replace(&task_with_status)
                                        }
                                        _ => task_patch::replace(&task_with_status), // fallback
                                    };

                                    if is_fallback {
                                        tracing::debug!(
                                            task_id = %task.id,
                                            op = ?hook.operation,
                                            "using fallback task patch for websocket stream"
                                        );
                                    }

                                    msg_store_for_hook.push_patch(patch);
                                    return;
                                }
                                // Draft updates: emit direct patches used by the follow-up draft stream
                                RecordTypes::Draft(draft) => {
                                    let patch = draft_patch::follow_up_replace(draft);
                                    msg_store_for_hook.push_patch(patch);
                                    return;
                                }
                                RecordTypes::RetryDraft(draft) => {
                                    let patch = draft_patch::retry_replace(draft);
                                    msg_store_for_hook.push_patch(patch);
                                    return;
                                }
                                RecordTypes::DeletedDraft { draft_type, task_attempt_id: Some(id), .. } => {
                                    let patch = match draft_type {
                                        DraftType::FollowUp => draft_patch::follow_up_clear(*id),
                                        DraftType::Retry => draft_patch::retry_clear(*id),
                                    };
                                    msg_store_for_hook.push_patch(patch);
                                    return;
                                }
                                RecordTypes::DeletedTask {
                                    task_id: Some(task_id),
                                    ..
                                } => {
                                    let patch = task_patch::remove(*task_id);
                                    msg_store_for_hook.push_patch(patch);
                                    return;
                                }
                                RecordTypes::TaskAttempt(attempt) => {
                                    // Task attempts should update the parent task with fresh data
                                    if let Ok(Some(task)) =
                                        Task::find_by_id(&db.pool, attempt.task_id).await
                                        && let Ok(task_list) =
                                            Task::find_by_project_id_with_attempt_status(
                                                &db.pool,
                                                task.project_id,
                                            )
                                            .await
                                        && let Some(task_with_status) =
                                            task_list.into_iter().find(|t| t.id == attempt.task_id)
                                    {
                                        let patch = task_patch::replace(&task_with_status);
                                        msg_store_for_hook.push_patch(patch);
                                        return;
                                    }
                                }
                                RecordTypes::DeletedTaskAttempt {
                                    task_id: Some(task_id),
                                    ..
                                } => {
                                    // Task attempt deletion should update the parent task with fresh data
                                    if let Ok(Some(task)) =
                                        Task::find_by_id(&db.pool, *task_id).await
                                        && let Ok(task_list) =
                                            Task::find_by_project_id_with_attempt_status(
                                                &db.pool,
                                                task.project_id,
                                            )
                                            .await
                                        && let Some(task_with_status) =
                                            task_list.into_iter().find(|t| t.id == *task_id)
                                    {
                                        let patch = task_patch::replace(&task_with_status);
                                        msg_store_for_hook.push_patch(patch);
                                        return;
                                    }
                                }
                                RecordTypes::ExecutionProcess(process) => {
                                    let patch = match hook.operation {
                                        SqliteOperation::Insert => {
                                            execution_process_patch::add(process)
                                        }
                                        SqliteOperation::Update => {
                                            execution_process_patch::replace(process)
                                        }
                                        _ => execution_process_patch::replace(process), // fallback
                                    };
                                    msg_store_for_hook.push_patch(patch);

                                    if let Err(err) = EventService::push_task_update_for_attempt(
                                        &db.pool,
                                        msg_store_for_hook.clone(),
                                        process.task_attempt_id,
                                    )
                                    .await
                                    {
                                        tracing::error!(
                                            "Failed to push task update after execution process change: {:?}",
                                            err
                                        );
                                    }

                                    return;
                                }
                                RecordTypes::DeletedExecutionProcess {
                                    process_id: Some(process_id),
                                    task_attempt_id,
                                    ..
                                } => {
                                    let patch = execution_process_patch::remove(*process_id);
                                    msg_store_for_hook.push_patch(patch);

                                    if let Some(task_attempt_id) = task_attempt_id
                                        && let Err(err) =
                                            EventService::push_task_update_for_attempt(
                                                &db.pool,
                                                msg_store_for_hook.clone(),
                                                *task_attempt_id,
                                            )
                                            .await
                                        {
                                            tracing::error!(
                                                "Failed to push task update after execution process removal: {:?}",
                                                err
                                            );
                                        }

                                    return;
                                }
                                _ => {}
                            }

                            // Fallback: use the old entries format for other record types
                            let next_entry_count = {
                                let mut entry_count = entry_count_for_hook.write().await;
                                *entry_count += 1;
                                *entry_count
                            };

                            let event_patch: EventPatch = EventPatch {
                                op: "add".to_string(),
                                path: format!("/entries/{next_entry_count}"),
                                value: EventPatchInner {
                                    db_op: db_op.to_string(),
                                    record: record_type,
                                },
                            };

                            let patch =
                                serde_json::from_value(json!([
                                    serde_json::to_value(event_patch).unwrap()
                                ]))
                                .unwrap();

                            msg_store_for_hook.push_patch(patch);
                        });
                    }
                });

                Ok(())
            })
        }
    }

    pub fn msg_store(&self) -> &Arc<MsgStore> {
        &self.msg_store
    }
}
