use std::{
    collections::{HashMap, HashSet},
    io,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use anyhow::anyhow;
use async_stream::try_stream;
use async_trait::async_trait;
use command_group::AsyncGroupChild;
use db::{
    DBService,
    models::{
        execution_process::{
            ExecutionContext, ExecutionProcess, ExecutionProcessRunReason, ExecutionProcessStatus,
        },
        executor_session::ExecutorSession,
        follow_up_draft::FollowUpDraft,
        image::TaskImage,
        merge::Merge,
        project_repository::ProjectRepository,
        task::{Task, TaskStatus},
        task_attempt::TaskAttempt,
        task_attempt_repository::TaskAttemptRepository,
    },
};
use deployment::DeploymentError;
use executors::payload::{ExecutorPayload, ExecutorRepositoryContext};
use executors::{
    actions::{Executable, ExecutorAction, ExecutorPayloadEnvelope, ExecutorSpawnContext},
    logs::{
        NormalizedEntryType,
        utils::{
            ConversationPatch,
            patch::{escape_json_pointer_segment, extract_normalized_entry_from_patch},
        },
    },
};
use futures::{FutureExt, StreamExt, TryStreamExt, stream::select};
use notify_debouncer_full::DebouncedEvent;
use serde_json::json;
use services::services::{
    analytics::AnalyticsContext,
    config::Config,
    container::{ContainerError, ContainerRef, ContainerService},
    filesystem_watcher,
    git::{Commit, DiffTarget, GitService},
    image::ImageService,
    notification::NotificationService,
    worktree_manager::WorktreeManager,
};
use sqlx::Error as SqlxError;
use tokio::{sync::RwLock, task::JoinHandle};
use tokio_util::io::ReaderStream;
use utils::{
    diff::{Diff, create_unified_diff_hunk},
    log_msg::LogMsg,
    msg_store::MsgStore,
    text::{git_branch_id, short_uuid},
};
use uuid::Uuid;

use crate::command;

#[derive(Clone)]
pub struct LocalContainerService {
    db: DBService,
    child_store: Arc<RwLock<HashMap<Uuid, Arc<RwLock<AsyncGroupChild>>>>>,
    msg_stores: Arc<RwLock<HashMap<Uuid, Arc<MsgStore>>>>,
    config: Arc<RwLock<Config>>,
    git: GitService,
    image_service: ImageService,
    analytics: Option<AnalyticsContext>,
}
struct RepositoryContext {
    project_repo: ProjectRepository,
    worktree_path: PathBuf,
    branch_name: String,
}

impl LocalContainerService {
    // Max cumulative content bytes allowed per diff stream
    const MAX_CUMULATIVE_DIFF_BYTES: usize = 200 * 1024 * 1024; // 200MB

    // Apply stream-level omit policy based on cumulative bytes.
    // If adding this diff's contents exceeds the cap, strip contents and set stats.
    fn apply_stream_omit_policy(
        &self,
        diff: &mut utils::diff::Diff,
        sent_bytes: &Arc<AtomicUsize>,
    ) {
        // Compute size of current diff payload
        let mut size = 0usize;
        if let Some(ref s) = diff.old_content {
            size += s.len();
        }
        if let Some(ref s) = diff.new_content {
            size += s.len();
        }

        if size == 0 {
            return; // nothing to account
        }

        let current = sent_bytes.load(Ordering::Relaxed);
        if current.saturating_add(size) > Self::MAX_CUMULATIVE_DIFF_BYTES {
            // We will omit content for this diff. If we still have both sides loaded
            // (i.e., not already omitted by file-size guards), compute stats for UI.
            if diff.additions.is_none() && diff.deletions.is_none() {
                let old = diff.old_content.as_deref().unwrap_or("");
                let new = diff.new_content.as_deref().unwrap_or("");
                let hunk = create_unified_diff_hunk(old, new);
                let mut add = 0usize;
                let mut del = 0usize;
                for line in hunk.lines() {
                    if let Some(first) = line.chars().next() {
                        if first == '+' {
                            add += 1;
                        } else if first == '-' {
                            del += 1;
                        }
                    }
                }
                diff.additions = Some(add);
                diff.deletions = Some(del);
            }

            diff.old_content = None;
            diff.new_content = None;
            diff.content_omitted = true;
        } else {
            // safe to include; account for it
            let _ = sent_bytes.fetch_add(size, Ordering::Relaxed);
        }
    }
    pub fn new(
        db: DBService,
        msg_stores: Arc<RwLock<HashMap<Uuid, Arc<MsgStore>>>>,
        config: Arc<RwLock<Config>>,
        git: GitService,
        image_service: ImageService,
        analytics: Option<AnalyticsContext>,
    ) -> Self {
        let child_store = Arc::new(RwLock::new(HashMap::new()));

        LocalContainerService {
            db,
            child_store,
            msg_stores,
            config,
            git,
            image_service,
            analytics,
        }
    }

    pub async fn get_child_from_store(&self, id: &Uuid) -> Option<Arc<RwLock<AsyncGroupChild>>> {
        let map = self.child_store.read().await;
        map.get(id).cloned()
    }

    pub async fn add_child_to_store(&self, id: Uuid, exec: AsyncGroupChild) {
        let mut map = self.child_store.write().await;
        map.insert(id, Arc::new(RwLock::new(exec)));
    }

    pub async fn remove_child_from_store(&self, id: &Uuid) {
        let mut map = self.child_store.write().await;
        map.remove(id);
    }

    /// A context is finalized when
    /// - The next action is None (no follow-up actions)
    /// - The run reason is not DevServer
    fn should_finalize(ctx: &ExecutionContext) -> bool {
        ctx.execution_process
            .executor_action()
            .unwrap()
            .next_action
            .is_none()
            && (!matches!(
                ctx.execution_process.run_reason,
                ExecutionProcessRunReason::DevServer
            ))
    }

    /// Finalize task execution by updating status to InReview and sending notifications
    async fn finalize_task(db: &DBService, config: &Arc<RwLock<Config>>, ctx: &ExecutionContext) {
        if let Err(e) = Task::update_status(&db.pool, ctx.task.id, TaskStatus::InReview).await {
            tracing::error!("Failed to update task status to InReview: {e}");
        }
        let notify_cfg = config.read().await.notifications.clone();
        NotificationService::notify_execution_halted(notify_cfg, ctx).await;
    }

    /// Defensively check for externally deleted worktrees and mark them as deleted in the database
    async fn check_externally_deleted_worktrees(db: &DBService) -> Result<(), DeploymentError> {
        let active_attempts = TaskAttempt::find_by_worktree_deleted(&db.pool).await?;
        tracing::debug!(
            "Checking {} active worktrees for external deletion...",
            active_attempts.len()
        );
        for (attempt_id, worktree_path) in active_attempts {
            // Check if worktree directory exists
            if !std::path::Path::new(&worktree_path).exists() {
                // Worktree was deleted externally, mark as deleted in database
                if let Err(e) = TaskAttempt::mark_worktree_deleted(&db.pool, attempt_id).await {
                    tracing::error!(
                        "Failed to mark externally deleted worktree as deleted for attempt {}: {}",
                        attempt_id,
                        e
                    );
                } else {
                    tracing::info!(
                        "Marked externally deleted worktree as deleted for attempt {} (path: {})",
                        attempt_id,
                        worktree_path
                    );
                }
            }
        }
        Ok(())
    }

    /// Find and delete orphaned worktrees that don't correspond to any task attempts
    async fn cleanup_orphaned_worktrees(&self) {
        // Check if orphan cleanup is disabled via environment variable
        if std::env::var("DISABLE_WORKTREE_ORPHAN_CLEANUP").is_ok() {
            tracing::debug!(
                "Orphan worktree cleanup is disabled via DISABLE_WORKTREE_ORPHAN_CLEANUP environment variable"
            );
            return;
        }
        let worktree_base_dir = WorktreeManager::get_worktree_base_dir();
        if !worktree_base_dir.exists() {
            tracing::debug!(
                "Worktree base directory {} does not exist, skipping orphan cleanup",
                worktree_base_dir.display()
            );
            return;
        }
        let entries = match std::fs::read_dir(&worktree_base_dir) {
            Ok(entries) => entries,
            Err(e) => {
                tracing::error!(
                    "Failed to read worktree base directory {}: {}",
                    worktree_base_dir.display(),
                    e
                );
                return;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(e) => {
                    tracing::warn!("Failed to read directory entry: {}", e);
                    continue;
                }
            };
            let path = entry.path();
            // Only process directories
            if !path.is_dir() {
                continue;
            }

            let worktree_path_str = path.to_string_lossy().to_string();
            if let Ok(false) =
                TaskAttempt::container_ref_exists(&self.db().pool, &worktree_path_str).await
            {
                // This is an orphaned worktree - delete it
                tracing::info!("Found orphaned worktree: {}", worktree_path_str);
                if let Err(e) = WorktreeManager::cleanup_worktree(&path, None).await {
                    tracing::error!(
                        "Failed to remove orphaned worktree {}: {}",
                        worktree_path_str,
                        e
                    );
                } else {
                    tracing::info!(
                        "Successfully removed orphaned worktree: {}",
                        worktree_path_str
                    );
                }
            }
        }
    }

    pub async fn cleanup_expired_attempt(
        db: &DBService,
        attempt_id: Uuid,
        worktree_path: PathBuf,
        git_repo_path: PathBuf,
    ) -> Result<(), DeploymentError> {
        WorktreeManager::cleanup_worktree(&worktree_path, Some(&git_repo_path)).await?;
        // Mark worktree as deleted in database after successful cleanup
        TaskAttempt::mark_worktree_deleted(&db.pool, attempt_id).await?;
        tracing::info!("Successfully marked worktree as deleted for attempt {attempt_id}",);
        Ok(())
    }

    pub async fn cleanup_expired_attempts(db: &DBService) -> Result<(), DeploymentError> {
        let expired_attempts = TaskAttempt::find_expired_for_cleanup(&db.pool).await?;
        if expired_attempts.is_empty() {
            tracing::debug!("No expired worktrees found");
            return Ok(());
        }
        tracing::info!(
            "Found {} expired worktrees to clean up",
            expired_attempts.len()
        );
        for (attempt_id, worktree_path, git_repo_path) in expired_attempts {
            Self::cleanup_expired_attempt(
                db,
                attempt_id,
                PathBuf::from(worktree_path),
                PathBuf::from(git_repo_path),
            )
            .await
            .unwrap_or_else(|e| {
                tracing::error!("Failed to clean up expired attempt {attempt_id}: {e}",);
            });
        }
        Ok(())
    }

    pub async fn spawn_worktree_cleanup(&self) {
        let db = self.db.clone();
        let mut cleanup_interval = tokio::time::interval(tokio::time::Duration::from_secs(1800)); // 30 minutes
        self.cleanup_orphaned_worktrees().await;
        tokio::spawn(async move {
            loop {
                cleanup_interval.tick().await;
                tracing::info!("Starting periodic worktree cleanup...");
                Self::check_externally_deleted_worktrees(&db)
                    .await
                    .unwrap_or_else(|e| {
                        tracing::error!("Failed to check externally deleted worktrees: {}", e);
                    });
                Self::cleanup_expired_attempts(&db)
                    .await
                    .unwrap_or_else(|e| {
                        tracing::error!("Failed to clean up expired worktree attempts: {}", e)
                    });
            }
        });
    }

    /// Spawn a background task that polls the child process for completion and
    /// cleans up the execution entry when it exits.
    pub fn spawn_exit_monitor(
        &self,
        exec_id: &Uuid,
        exit_signal: Option<tokio::sync::oneshot::Receiver<()>>,
    ) -> JoinHandle<()> {
        let exec_id = *exec_id;
        let child_store = self.child_store.clone();
        let msg_stores = self.msg_stores.clone();
        let db = self.db.clone();
        let config = self.config.clone();
        let container = self.clone();
        let analytics = self.analytics.clone();

        let mut process_exit_rx = self.spawn_os_exit_watcher(exec_id);

        tokio::spawn(async move {
            let mut exit_signal_future = exit_signal
                .map(|rx| rx.map(|_| ()).boxed()) // wait for signal
                .unwrap_or_else(|| std::future::pending::<()>().boxed()); // no signal, stall forever

            let status_result: std::io::Result<std::process::ExitStatus>;

            // Wait for process to exit, or exit signal from executor
            tokio::select! {
                // Exit signal.
                // Some coding agent processes do not automatically exit after processing the user request; instead the executor
                // signals when processing has finished to gracefully kill the process.
                _ = &mut exit_signal_future => {
                    // Executor signaled completion: kill group and remember to force Completed(0)
                    if let Some(child_lock) = child_store.read().await.get(&exec_id).cloned() {
                        let mut child = child_lock.write().await ;
                        if let Err(err) = command::kill_process_group(&mut child).await {
                            tracing::error!("Failed to kill process group after exit signal: {} {}", exec_id, err);
                        }
                    }
                    status_result = Ok(success_exit_status());
                }
                // Process exit
                exit_status_result = &mut process_exit_rx => {
                    status_result = exit_status_result.unwrap_or_else(|e| Err(std::io::Error::other(e)));
                }
            }

            let (exit_code, status) = match status_result {
                Ok(exit_status) => {
                    let code = exit_status.code().unwrap_or(-1) as i64;
                    let status = if exit_status.success() {
                        ExecutionProcessStatus::Completed
                    } else {
                        ExecutionProcessStatus::Failed
                    };
                    (Some(code), status)
                }
                Err(_) => (None, ExecutionProcessStatus::Failed),
            };

            if !ExecutionProcess::was_killed(&db.pool, exec_id).await
                && let Err(e) =
                    ExecutionProcess::update_completion(&db.pool, exec_id, status, exit_code).await
            {
                tracing::error!("Failed to update execution process completion: {}", e);
            }

            if let Ok(ctx) = ExecutionProcess::load_context(&db.pool, exec_id).await {
                // Update executor session summary if available
                if let Err(e) = container.update_executor_session_summary(&exec_id).await {
                    tracing::warn!("Failed to update executor session summary: {}", e);
                }

                if matches!(
                    ctx.execution_process.status,
                    ExecutionProcessStatus::Completed
                ) && exit_code == Some(0)
                {
                    // Commit changes (if any) and get feedback about whether changes were made
                    let changes_committed = match container.try_commit_changes(&ctx).await {
                        Ok(committed) => committed,
                        Err(e) => {
                            tracing::error!("Failed to commit changes after execution: {}", e);
                            // Treat commit failures as if changes were made to be safe
                            true
                        }
                    };

                    let should_start_next = if matches!(
                        ctx.execution_process.run_reason,
                        ExecutionProcessRunReason::CodingAgent
                    ) {
                        changes_committed
                    } else {
                        true
                    };

                    if should_start_next {
                        // If the process exited successfully, start the next action
                        if let Err(e) = container.try_start_next_action(&ctx).await {
                            tracing::error!("Failed to start next action after completion: {}", e);
                        }
                    } else {
                        tracing::info!(
                            "Skipping cleanup script for task attempt {} - no changes made by coding agent",
                            ctx.task_attempt.id
                        );

                        // Manually finalize task since we're bypassing normal execution flow
                        Self::finalize_task(&db, &config, &ctx).await;
                    }
                }

                if Self::should_finalize(&ctx) {
                    Self::finalize_task(&db, &config, &ctx).await;
                    // After finalization, check if a queued follow-up exists and start it
                    if let Err(e) = container.try_consume_queued_followup(&ctx).await {
                        tracing::error!(
                            "Failed to start queued follow-up for attempt {}: {}",
                            ctx.task_attempt.id,
                            e
                        );
                    }
                }

                // Fire analytics event when CodingAgent execution has finished
                if config.read().await.analytics_enabled == Some(true)
                    && matches!(
                        &ctx.execution_process.run_reason,
                        ExecutionProcessRunReason::CodingAgent
                    )
                    && let Some(analytics) = &analytics
                {
                    analytics.analytics_service.track_event(&analytics.user_id, "task_attempt_finished", Some(json!({
                        "task_id": ctx.task.id.to_string(),
                        "project_id": ctx.task.project_id.to_string(),
                        "attempt_id": ctx.task_attempt.id.to_string(),
                        "execution_success": matches!(ctx.execution_process.status, ExecutionProcessStatus::Completed),
                        "exit_code": ctx.execution_process.exit_code,
                    })));
                }
            }

            // Now that commit/next-action/finalization steps for this process are complete,
            // capture the HEAD OID as the definitive "after" state (best-effort).
            if let Ok(ctx) = ExecutionProcess::load_context(&db.pool, exec_id).await {
                let worktree_dir = container.task_attempt_to_current_dir(&ctx.task_attempt);
                if let Ok(head) = container.git().get_head_info(&worktree_dir)
                    && let Err(e) =
                        ExecutionProcess::update_after_head_commit(&db.pool, exec_id, &head.oid)
                            .await
                {
                    tracing::warn!("Failed to update after_head_commit for {}: {}", exec_id, e);
                }
            }

            // Cleanup msg store
            if let Some(msg_arc) = msg_stores.write().await.remove(&exec_id) {
                msg_arc.push_finished();
                tokio::time::sleep(Duration::from_millis(50)).await; // Wait for the finish message to propogate
                match Arc::try_unwrap(msg_arc) {
                    Ok(inner) => drop(inner),
                    Err(arc) => tracing::error!(
                        "There are still {} strong Arcs to MsgStore for {}",
                        Arc::strong_count(&arc),
                        exec_id
                    ),
                }
            }

            // Cleanup child handle
            child_store.write().await.remove(&exec_id);
        })
    }

    pub fn spawn_os_exit_watcher(
        &self,
        exec_id: Uuid,
    ) -> tokio::sync::oneshot::Receiver<std::io::Result<std::process::ExitStatus>> {
        let (tx, rx) = tokio::sync::oneshot::channel::<std::io::Result<std::process::ExitStatus>>();
        let child_store = self.child_store.clone();
        tokio::spawn(async move {
            loop {
                let child_lock = {
                    let map = child_store.read().await;
                    map.get(&exec_id).cloned()
                };
                if let Some(child_lock) = child_lock {
                    let mut child_handler = child_lock.write().await;
                    match child_handler.try_wait() {
                        Ok(Some(status)) => {
                            let _ = tx.send(Ok(status));
                            break;
                        }
                        Ok(None) => {}
                        Err(e) => {
                            let _ = tx.send(Err(e));
                            break;
                        }
                    }
                } else {
                    let _ = tx.send(Err(io::Error::other(format!(
                        "Child handle missing for {exec_id}"
                    ))));
                    break;
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        });
        rx
    }

    pub fn dir_name_from_task_attempt(attempt_id: &Uuid, task_title: &str) -> String {
        let task_title_id = git_branch_id(task_title);
        format!("{}-{}", short_uuid(attempt_id), task_title_id)
    }

    pub fn git_branch_from_task_attempt(
        branch_prefix: &str,
        attempt_id: &Uuid,
        task_title: &str,
    ) -> String {
        let task_title_id = git_branch_id(task_title);
        let normalized_prefix = {
            let trimmed = branch_prefix.trim();
            if trimmed.is_empty() {
                String::new()
            } else if trimmed.ends_with('/') || trimmed.ends_with('-') || trimmed.ends_with('_') {
                trimmed.to_string()
            } else {
                format!("{trimmed}/")
            }
        };
        let short_id = short_uuid(attempt_id);

        if normalized_prefix.is_empty() {
            format!("{}-{}", short_id, task_title_id)
        } else {
            format!("{}{}-{}", normalized_prefix, short_id, task_title_id)
        }
    }

    fn worktree_path_for_repo(
        attempt_id: &Uuid,
        task_title: &str,
        repo: &ProjectRepository,
    ) -> PathBuf {
        let base_name = LocalContainerService::dir_name_from_task_attempt(attempt_id, task_title);
        let base_dir = WorktreeManager::get_worktree_base_dir();
        if repo.is_primary {
            base_dir.join(base_name)
        } else {
            let slug = LocalContainerService::repo_slug(repo);
            base_dir.join(format!("{base_name}--{slug}"))
        }
    }

    fn repo_slug(repo: &ProjectRepository) -> String {
        let slug = git_branch_id(&repo.name);
        if slug.is_empty() {
            format!("repo-{}", short_uuid(&repo.id))
        } else {
            format!("{}-{}", slug, short_uuid(&repo.id))
        }
    }

    fn repo_env_prefix(slug: &str) -> String {
        let candidate = slug.replace('-', "_").to_ascii_uppercase();
        if candidate.is_empty() {
            "REPO".to_string()
        } else {
            candidate
        }
    }

    async fn build_executor_payload(
        &self,
        task_attempt: &TaskAttempt,
    ) -> Result<ExecutorPayloadEnvelope, ContainerError> {
        let refreshed_attempt = TaskAttempt::find_by_id(&self.db.pool, task_attempt.id)
            .await?
            .ok_or(SqlxError::RowNotFound)?;

        let task = refreshed_attempt
            .parent_task(&self.db.pool)
            .await?
            .ok_or(SqlxError::RowNotFound)?;

        let project_repositories =
            ProjectRepository::list_for_project(&self.db.pool, task.project_id).await?;
        if project_repositories.is_empty() {
            return Err(ContainerError::Other(anyhow!(
                "No repositories configured for task attempt {}",
                task_attempt.id
            )));
        }

        let attempt_repositories =
            TaskAttemptRepository::list_for_attempt(&self.db.pool, refreshed_attempt.id).await?;
        let attempt_repo_lookup: HashMap<Uuid, TaskAttemptRepository> = attempt_repositories
            .into_iter()
            .map(|repo| (repo.project_repository_id, repo))
            .collect();

        let mut repositories_payload = Vec::with_capacity(project_repositories.len());
        let mut env = HashMap::new();
        let mut repo_prefixes = Vec::with_capacity(project_repositories.len());

        let mut primary_repo_id: Option<Uuid> = None;
        let mut primary_env_prefix: Option<String> = None;
        let mut primary_path: Option<String> = None;
        let mut primary_root: Option<String> = None;
        let mut primary_branch: Option<String> = None;
        let mut primary_name: Option<String> = None;

        for repo in project_repositories {
            let attempt_repo = attempt_repo_lookup.get(&repo.id).ok_or_else(|| {
                ContainerError::Other(anyhow!(
                    "Repository metadata missing for project repository {}",
                    repo.id
                ))
            })?;

            let container_path = attempt_repo
                .container_ref
                .clone()
                .or_else(|| {
                    if repo.is_primary {
                        refreshed_attempt.container_ref.clone()
                    } else {
                        None
                    }
                })
                .ok_or_else(|| {
                    ContainerError::Other(anyhow!(
                        "Worktree path missing for repository {}",
                        repo.id
                    ))
                })?;

            let branch = attempt_repo
                .branch
                .clone()
                .or_else(|| refreshed_attempt.branch.clone());
            let branch_for_env = branch.clone().unwrap_or_default();

            let slug = LocalContainerService::repo_slug(&repo);
            let env_prefix = LocalContainerService::repo_env_prefix(&slug);

            if repo.is_primary {
                primary_repo_id = Some(repo.id);
                primary_env_prefix = Some(env_prefix.clone());
                primary_path = Some(container_path.clone());
                primary_root = Some(repo.root_path.clone());
                primary_branch = Some(branch_for_env.clone());
                primary_name = Some(repo.name.clone());
            }

            env.insert(
                format!("VIBE_REPO_{}_PATH", env_prefix),
                container_path.clone(),
            );
            env.insert(
                format!("VIBE_REPO_{}_ROOT", env_prefix),
                repo.root_path.clone(),
            );
            env.insert(format!("VIBE_REPO_{}_BRANCH", env_prefix), branch_for_env);
            env.insert(format!("VIBE_REPO_{}_NAME", env_prefix), repo.name.clone());
            env.insert(
                format!("VIBE_REPO_{}_IS_PRIMARY", env_prefix),
                if repo.is_primary { "1" } else { "0" }.to_string(),
            );
            env.insert(format!("VIBE_REPO_{}_ID", env_prefix), repo.id.to_string());

            repo_prefixes.push(env_prefix.clone());

            repositories_payload.push(ExecutorRepositoryContext {
                id: repo.id,
                name: repo.name.clone(),
                slug,
                worktree_path: container_path,
                root_path: repo.root_path,
                branch,
                is_primary: repo.is_primary,
            });
        }

        let primary_repo_id = primary_repo_id.ok_or_else(|| {
            ContainerError::Other(anyhow!(
                "Primary repository not configured for task attempt {}",
                task_attempt.id
            ))
        })?;

        env.insert(
            "VIBE_EXECUTOR_PAYLOAD_VERSION".to_string(),
            ExecutorPayload::CURRENT_VERSION.to_string(),
        );
        env.insert(
            "VIBE_REPOSITORY_COUNT".to_string(),
            repo_prefixes.len().to_string(),
        );
        env.insert("VIBE_REPOSITORIES".to_string(), repo_prefixes.join(","));
        env.insert(
            "VIBE_TASK_ATTEMPT_ID".to_string(),
            refreshed_attempt.id.to_string(),
        );
        env.insert(
            "VIBE_PRIMARY_REPOSITORY_ID".to_string(),
            primary_repo_id.to_string(),
        );

        if let Some(prefix) = primary_env_prefix {
            env.insert("VIBE_PRIMARY_REPO_PREFIX".to_string(), prefix);
        }
        if let Some(path) = primary_path {
            env.insert("VIBE_PRIMARY_REPO_PATH".to_string(), path);
        }
        if let Some(root) = primary_root {
            env.insert("VIBE_PRIMARY_REPO_ROOT".to_string(), root);
        }
        if let Some(branch) = primary_branch {
            env.insert("VIBE_PRIMARY_REPO_BRANCH".to_string(), branch);
        }
        if let Some(name) = primary_name {
            env.insert("VIBE_PRIMARY_REPO_NAME".to_string(), name);
        }

        let payload = ExecutorPayload {
            version: ExecutorPayload::CURRENT_VERSION,
            attempt_id: refreshed_attempt.id,
            primary_repository_id: primary_repo_id,
            repositories: repositories_payload,
            env,
        };

        ExecutorPayloadEnvelope::try_new(payload).map_err(|e| {
            ContainerError::Other(anyhow!("Failed to serialize executor payload: {}", e))
        })
    }

    async fn resolve_repository_context(
        &self,
        task_attempt: &TaskAttempt,
        repo_id: Option<Uuid>,
    ) -> Result<RepositoryContext, ContainerError> {
        let task = task_attempt
            .parent_task(&self.db.pool)
            .await?
            .ok_or(SqlxError::RowNotFound)?;

        let repositories =
            ProjectRepository::list_for_project(&self.db.pool, task.project_id).await?;
        if repositories.is_empty() {
            return Err(ContainerError::Other(anyhow!(
                "No repositories configured for project {}",
                task.project_id
            )));
        }

        let project_repo = if let Some(id) = repo_id {
            repositories
                .into_iter()
                .find(|repo| repo.id == id)
                .ok_or_else(|| {
                    ContainerError::Other(anyhow!(
                        "Repository {} not found for task attempt {}",
                        id,
                        task_attempt.id
                    ))
                })?
        } else {
            repositories
                .into_iter()
                .find(|repo| repo.is_primary)
                .ok_or_else(|| {
                    ContainerError::Other(anyhow!(
                        "Primary repository not configured for task attempt {}",
                        task_attempt.id
                    ))
                })?
        };

        // Ensure worktrees exist so that repository rows are populated
        let _ = self.ensure_container_exists(task_attempt).await?;

        let attempt_repo = TaskAttemptRepository::find_for_attempt(
            &self.db.pool,
            task_attempt.id,
            project_repo.id,
        )
        .await?
        .ok_or_else(|| {
            ContainerError::Other(anyhow!(
                "Attempt repository entry missing for task attempt {}",
                task_attempt.id
            ))
        })?;

        let container_ref = attempt_repo
            .container_ref
            .clone()
            .or_else(|| {
                if attempt_repo.is_primary {
                    task_attempt.container_ref.clone()
                } else {
                    None
                }
            })
            .ok_or_else(|| {
                ContainerError::Other(anyhow!(
                    "Worktree path missing for repository {}",
                    project_repo.id
                ))
            })?;

        let branch_name = attempt_repo
            .branch
            .clone()
            .or_else(|| task_attempt.branch.clone())
            .ok_or_else(|| {
                ContainerError::Other(anyhow!(
                    "Branch not set for task attempt {}",
                    task_attempt.id
                ))
            })?;

        Ok(RepositoryContext {
            project_repo,
            worktree_path: PathBuf::from(container_ref),
            branch_name,
        })
    }

    fn apply_repository_metadata(diff: &mut Diff, repo: &ProjectRepository) {
        diff.repository_id = Some(repo.id);
        diff.repository_name = Some(repo.name.clone());
        diff.repository_root = Some(repo.root_path.clone());

        if !repo.root_path.is_empty() {
            LocalContainerService::relativize_path(&mut diff.old_path, &repo.root_path);
            LocalContainerService::relativize_path(&mut diff.new_path, &repo.root_path);
        }
    }

    fn relativize_path(path: &mut Option<String>, root: &str) {
        if let Some(value) = path {
            let trimmed = root.trim_matches('/');
            if trimmed.is_empty() {
                return;
            }
            let mut normalized = trimmed.replace('\\', "/");
            if !normalized.ends_with('/') {
                normalized.push('/');
            }
            if let Some(stripped) = value.strip_prefix(&normalized) {
                *value = stripped.to_string();
            } else if value == trimmed {
                *value = String::new();
            }
        }
    }

    async fn track_child_msgs_in_store(&self, id: Uuid, child: &mut AsyncGroupChild) {
        let store = Arc::new(MsgStore::new());

        let out = child.inner().stdout.take().expect("no stdout");
        let err = child.inner().stderr.take().expect("no stderr");

        // Map stdout bytes -> LogMsg::Stdout
        let out = ReaderStream::new(out)
            .map_ok(|chunk| LogMsg::Stdout(String::from_utf8_lossy(&chunk).into_owned()));

        // Map stderr bytes -> LogMsg::Stderr
        let err = ReaderStream::new(err)
            .map_ok(|chunk| LogMsg::Stderr(String::from_utf8_lossy(&chunk).into_owned()));

        // If you have a JSON Patch source, map it to LogMsg::JsonPatch too, then select all three.

        // Merge and forward into the store
        let merged = select(out, err); // Stream<Item = Result<LogMsg, io::Error>>
        let debounced = utils::stream_ext::debounce_logs(merged);
        store.clone().spawn_forwarder(debounced);

        let mut map = self.msg_stores().write().await;
        map.insert(id, store);
    }

    /// Get the worktree path for a task attempt
    #[allow(dead_code)]
    async fn get_worktree_path(
        &self,
        task_attempt: &TaskAttempt,
    ) -> Result<PathBuf, ContainerError> {
        let container_ref = self.ensure_container_exists(task_attempt).await?;
        let worktree_dir = PathBuf::from(&container_ref);

        if !worktree_dir.exists() {
            return Err(ContainerError::Other(anyhow!(
                "Worktree directory not found"
            )));
        }

        Ok(worktree_dir)
    }

    /// Create a diff log stream for merged attempts (never changes) for WebSocket
    fn create_merged_diff_stream(
        &self,
        repo: &ProjectRepository,
        merge_commit_id: &str,
    ) -> Result<futures::stream::BoxStream<'static, Result<LogMsg, std::io::Error>>, ContainerError>
    {
        let diffs = self.git().get_diffs(
            DiffTarget::Commit {
                repo_path: &repo.git_repo_path,
                commit_sha: merge_commit_id,
            },
            None,
        )?;

        let cum = Arc::new(AtomicUsize::new(0));
        let diffs: Vec<_> = diffs
            .into_iter()
            .map(|mut d| {
                self.apply_stream_omit_policy(&mut d, &cum);
                Self::apply_repository_metadata(&mut d, repo);
                d
            })
            .collect();

        let stream = futures::stream::iter(diffs.into_iter().map(|diff| {
            let entry_index = GitService::diff_path(&diff);
            let patch =
                ConversationPatch::add_diff(escape_json_pointer_segment(&entry_index), diff);
            Ok::<_, std::io::Error>(LogMsg::JsonPatch(patch))
        }))
        .chain(futures::stream::once(async {
            Ok::<_, std::io::Error>(LogMsg::Finished)
        }))
        .boxed();

        Ok(stream)
    }

    /// Create a live diff log stream for ongoing attempts for WebSocket
    async fn create_live_diff_stream(
        &self,
        repo_ctx: &RepositoryContext,
        base_commit: &Commit,
    ) -> Result<futures::stream::BoxStream<'static, Result<LogMsg, std::io::Error>>, ContainerError>
    {
        // Get initial snapshot
        let git_service = self.git().clone();
        let initial_diffs = git_service.get_diffs(
            DiffTarget::Worktree {
                worktree_path: &repo_ctx.worktree_path,
                base_commit,
            },
            None,
        )?;

        let cumulative = Arc::new(AtomicUsize::new(0));
        let full_sent = Arc::new(std::sync::RwLock::new(HashSet::<String>::new()));
        let initial_diffs: Vec<_> = initial_diffs
            .into_iter()
            .map(|mut d| {
                self.apply_stream_omit_policy(&mut d, &cumulative);
                Self::apply_repository_metadata(&mut d, &repo_ctx.project_repo);
                d
            })
            .collect();

        {
            let mut guard = full_sent.write().unwrap();
            for d in &initial_diffs {
                if !d.content_omitted {
                    let p = GitService::diff_path(d);
                    guard.insert(p);
                }
            }
        }

        let initial_stream = futures::stream::iter(initial_diffs.into_iter().map(|diff| {
            let entry_index = GitService::diff_path(&diff);
            let patch =
                ConversationPatch::add_diff(escape_json_pointer_segment(&entry_index), diff);
            Ok::<_, std::io::Error>(LogMsg::JsonPatch(patch))
        }))
        .boxed();

        let worktree_path = repo_ctx.worktree_path.clone();
        let base_commit = base_commit.clone();
        let project_repo = repo_ctx.project_repo.clone();

        let live_stream = {
            let git_service = git_service.clone();
            let worktree_path_for_spawn = worktree_path.clone();
            let cumulative = Arc::clone(&cumulative);
            let full_sent = Arc::clone(&full_sent);
            try_stream! {
                let watcher_result = tokio::task::spawn_blocking(move || {
                    filesystem_watcher::async_watcher(worktree_path_for_spawn)
                })
                .await
                .map_err(|e| io::Error::other(format!("Failed to spawn watcher setup: {e}")))?;

                let (_debouncer, mut rx, canonical_worktree_path) = watcher_result
                    .map_err(|e| io::Error::other(e.to_string()))?;

                while let Some(result) = rx.next().await {
                    match result {
                        Ok(events) => {
                            let changed_paths = Self::extract_changed_paths(&events, &canonical_worktree_path, &worktree_path);

                            if !changed_paths.is_empty() {
                                for msg in Self::process_file_changes(
                                    &git_service,
                                    &project_repo,
                                    &worktree_path,
                                    &base_commit,
                                    &changed_paths,
                                    &cumulative,
                                    &full_sent,
                                ).map_err(|e| {
                                    tracing::error!("Error processing file changes: {}", e);
                                    io::Error::other(e.to_string())
                                })? {
                                    yield msg;
                                }
                            }
                        }
                        Err(errors) => {
                            let error_msg = errors.iter()
                                .map(|e| e.to_string())
                                .collect::<Vec<_>>()
                                .join("; ");
                            tracing::error!("Filesystem watcher error: {}", error_msg);
                            Err(io::Error::other(error_msg))?;
                        }
                    }
                }
            }
        }.boxed();

        Ok(select(initial_stream, live_stream).boxed())
    }

    /// Extract changed file paths from filesystem events
    fn extract_changed_paths(
        events: &[DebouncedEvent],
        canonical_worktree_path: &Path,
        worktree_path: &Path,
    ) -> Vec<String> {
        events
            .iter()
            .flat_map(|event| &event.paths)
            .filter_map(|path| {
                path.strip_prefix(canonical_worktree_path)
                    .or_else(|_| path.strip_prefix(worktree_path))
                    .ok()
                    .map(|p| p.to_string_lossy().replace('\\', "/"))
            })
            .filter(|s| !s.is_empty())
            .collect()
    }

    /// Process file changes and generate diff messages (for WS)
    fn process_file_changes(
        git_service: &GitService,
        repo: &ProjectRepository,
        worktree_path: &Path,
        base_commit: &Commit,
        changed_paths: &[String],
        cumulative_bytes: &Arc<AtomicUsize>,
        full_sent_paths: &Arc<std::sync::RwLock<HashSet<String>>>,
    ) -> Result<Vec<LogMsg>, ContainerError> {
        let path_filter: Vec<&str> = changed_paths.iter().map(|s| s.as_str()).collect();

        let current_diffs = git_service.get_diffs(
            DiffTarget::Worktree {
                worktree_path,
                base_commit,
            },
            Some(&path_filter),
        )?;

        let mut msgs = Vec::new();
        let mut files_with_diffs = HashSet::new();

        // Add/update files that have diffs
        for mut diff in current_diffs {
            // Apply stream-level omit policy (affects contents and stats)
            {
                let mut size = 0usize;
                if let Some(ref s) = diff.old_content {
                    size += s.len();
                }
                if let Some(ref s) = diff.new_content {
                    size += s.len();
                }
                if size > 0 {
                    let current = cumulative_bytes.load(Ordering::Relaxed);
                    if current.saturating_add(size)
                        > LocalContainerService::MAX_CUMULATIVE_DIFF_BYTES
                    {
                        if diff.additions.is_none() && diff.deletions.is_none() {
                            let old = diff.old_content.as_deref().unwrap_or("");
                            let new = diff.new_content.as_deref().unwrap_or("");
                            let hunk = create_unified_diff_hunk(old, new);
                            let mut add = 0usize;
                            let mut del = 0usize;
                            for line in hunk.lines() {
                                if let Some(first) = line.chars().next() {
                                    if first == '+' {
                                        add += 1;
                                    } else if first == '-' {
                                        del += 1;
                                    }
                                }
                            }
                            diff.additions = Some(add);
                            diff.deletions = Some(del);
                        }

                        diff.old_content = None;
                        diff.new_content = None;
                        diff.content_omitted = true;
                    } else {
                        let _ = cumulative_bytes.fetch_add(size, Ordering::Relaxed);
                    }
                }
            }

            LocalContainerService::apply_repository_metadata(&mut diff, repo);

            let file_path = GitService::diff_path(&diff);
            files_with_diffs.insert(file_path.clone());

            if diff.content_omitted {
                if full_sent_paths.read().unwrap().contains(&file_path) {
                    continue;
                }
            } else {
                let mut guard = full_sent_paths.write().unwrap();
                guard.insert(file_path.clone());
            }

            let patch = ConversationPatch::add_diff(escape_json_pointer_segment(&file_path), diff);
            msgs.push(LogMsg::JsonPatch(patch));
        }

        // Remove files that changed but no longer have diffs
        for changed_path in changed_paths {
            if !files_with_diffs.contains(changed_path) {
                let patch =
                    ConversationPatch::remove_diff(escape_json_pointer_segment(changed_path));
                msgs.push(LogMsg::JsonPatch(patch));
            }
        }

        Ok(msgs)
    }
}

fn success_exit_status() -> std::process::ExitStatus {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        ExitStatusExt::from_raw(0)
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::ExitStatusExt;
        ExitStatusExt::from_raw(0)
    }
}

#[async_trait]
impl ContainerService for LocalContainerService {
    fn msg_stores(&self) -> &Arc<RwLock<HashMap<Uuid, Arc<MsgStore>>>> {
        &self.msg_stores
    }

    fn db(&self) -> &DBService {
        &self.db
    }

    fn git(&self) -> &GitService {
        &self.git
    }

    fn task_attempt_to_current_dir(&self, task_attempt: &TaskAttempt) -> PathBuf {
        PathBuf::from(task_attempt.container_ref.clone().unwrap_or_default())
    }
    /// Create worktrees for all repositories linked to this task attempt and return the primary path
    async fn create(&self, task_attempt: &TaskAttempt) -> Result<ContainerRef, ContainerError> {
        let task = task_attempt
            .parent_task(&self.db.pool)
            .await?
            .ok_or(SqlxError::RowNotFound)?;

        let project = task
            .parent_project(&self.db.pool)
            .await?
            .ok_or(SqlxError::RowNotFound)?;

        let branch_prefix = {
            let cfg = self.config.read().await;
            cfg.github.resolved_branch_prefix()
        };
        let git_branch_name = LocalContainerService::git_branch_from_task_attempt(
            &branch_prefix,
            &task_attempt.id,
            &task.title,
        );
        let branch_str = git_branch_name.as_str();

        let mut repositories =
            ProjectRepository::list_for_project(&self.db.pool, project.id).await?;
        if repositories.is_empty() {
            return Err(ContainerError::Other(anyhow!(
                "No repositories configured for project {}",
                project.id
            )));
        }

        let mut visited_repo_paths = HashSet::new();
        let mut primary_path: Option<PathBuf> = None;

        for repo in repositories.drain(..) {
            let repo_path = repo.git_repo_path.clone();
            let worktree_path =
                LocalContainerService::worktree_path_for_repo(&task_attempt.id, &task.title, &repo);
            let create_branch = visited_repo_paths.insert(repo_path.clone());

            WorktreeManager::create_worktree(
                &repo_path,
                branch_str,
                &worktree_path,
                &task_attempt.base_branch,
                create_branch,
            )
            .await?;

            let worktree_owned = worktree_path.to_string_lossy().to_string();
            TaskAttemptRepository::upsert_container_ref(
                &self.db.pool,
                task_attempt.id,
                repo.id,
                repo.is_primary,
                Some(worktree_owned.as_str()),
            )
            .await?;
            TaskAttemptRepository::upsert_branch(
                &self.db.pool,
                task_attempt.id,
                repo.id,
                repo.is_primary,
                Some(branch_str),
            )
            .await?;

            if repo.is_primary {
                if let Some(copy_files) = &project.copy_files
                    && !copy_files.trim().is_empty()
                {
                    self.copy_project_files(&repo_path, &worktree_path, copy_files)
                        .await
                        .unwrap_or_else(|e| {
                            tracing::warn!("Failed to copy project files: {}", e);
                        });
                }

                if let Err(e) = self
                    .image_service
                    .copy_images_by_task_to_worktree(&worktree_path, task.id)
                    .await
                {
                    tracing::warn!("Failed to copy task images to worktree: {}", e);
                }

                TaskAttempt::update_container_ref(&self.db.pool, task_attempt.id, &worktree_owned)
                    .await?;
                TaskAttempt::update_branch(&self.db.pool, task_attempt.id, branch_str).await?;
                primary_path = Some(worktree_path.clone());
            }
        }

        let primary_path = primary_path.ok_or_else(|| {
            ContainerError::Other(anyhow!(
                "Primary repository not configured for task attempt {}",
                task_attempt.id
            ))
        })?;

        Ok(primary_path.to_string_lossy().to_string())
    }

    async fn delete_inner(&self, task_attempt: &TaskAttempt) -> Result<(), ContainerError> {
        let task = task_attempt
            .parent_task(&self.db.pool)
            .await?
            .ok_or(SqlxError::RowNotFound)?;

        let project_repositories =
            ProjectRepository::list_for_project(&self.db.pool, task.project_id).await?;
        let attempt_repositories =
            TaskAttemptRepository::list_for_attempt(&self.db.pool, task_attempt.id).await?;

        let repo_lookup: std::collections::HashMap<Uuid, PathBuf> = project_repositories
            .into_iter()
            .map(|repo| (repo.id, repo.git_repo_path))
            .collect();

        for attempt_repo in attempt_repositories {
            let container_path = attempt_repo.container_ref.clone().or_else(|| {
                if attempt_repo.is_primary {
                    task_attempt.container_ref.clone()
                } else {
                    None
                }
            });

            let Some(path) = container_path else {
                continue;
            };

            let worktree_path = PathBuf::from(path);
            let git_repo_path = repo_lookup
                .get(&attempt_repo.project_repository_id)
                .map(PathBuf::as_path);

            if let Err(e) = WorktreeManager::cleanup_worktree(&worktree_path, git_repo_path).await {
                tracing::warn!(
                    "Failed to clean up worktree for task attempt {}: {}",
                    task_attempt.id,
                    e
                );
            }
        }

        Ok(())
    }

    async fn ensure_container_exists(
        &self,
        task_attempt: &TaskAttempt,
    ) -> Result<ContainerRef, ContainerError> {
        let task = task_attempt
            .parent_task(&self.db.pool)
            .await?
            .ok_or(SqlxError::RowNotFound)?;

        let branch_name = task_attempt
            .branch
            .as_ref()
            .ok_or_else(|| ContainerError::Other(anyhow!("Branch not found for task attempt")))?
            .to_owned();

        let project_repositories =
            ProjectRepository::list_for_project(&self.db.pool, task.project_id).await?;
        if project_repositories.is_empty() {
            return Err(ContainerError::Other(anyhow!(
                "No repositories configured for task attempt {}",
                task_attempt.id
            )));
        }

        let mut primary_path: Option<String> = None;

        for repo in project_repositories {
            let worktree_path =
                LocalContainerService::worktree_path_for_repo(&task_attempt.id, &task.title, &repo);

            WorktreeManager::ensure_worktree_exists(
                &repo.git_repo_path,
                &branch_name,
                &worktree_path,
            )
            .await?;

            let worktree_str = worktree_path.to_string_lossy().to_string();

            TaskAttemptRepository::upsert_container_ref(
                &self.db.pool,
                task_attempt.id,
                repo.id,
                repo.is_primary,
                Some(worktree_str.as_str()),
            )
            .await?;
            TaskAttemptRepository::upsert_branch(
                &self.db.pool,
                task_attempt.id,
                repo.id,
                repo.is_primary,
                Some(branch_name.as_str()),
            )
            .await?;

            if repo.is_primary {
                if task_attempt.container_ref.as_deref() != Some(worktree_str.as_str()) {
                    TaskAttempt::update_container_ref(
                        &self.db.pool,
                        task_attempt.id,
                        &worktree_str,
                    )
                    .await?;
                }
                TaskAttempt::update_branch(&self.db.pool, task_attempt.id, &branch_name).await?;
                primary_path = Some(worktree_str.clone());
            }
        }

        let container_ref = primary_path
            .or_else(|| task_attempt.container_ref.clone())
            .ok_or_else(|| {
                ContainerError::Other(anyhow!(
                    "Primary repository not found for task attempt {}",
                    task_attempt.id
                ))
            })?;

        Ok(container_ref)
    }

    async fn is_container_clean(&self, task_attempt: &TaskAttempt) -> Result<bool, ContainerError> {
        if let Some(container_ref) = &task_attempt.container_ref {
            // If container_ref is set, check if the worktree exists
            let path = PathBuf::from(container_ref);
            if path.exists() {
                self.git().is_worktree_clean(&path).map_err(|e| e.into())
            } else {
                return Ok(true); // No worktree means it's clean
            }
        } else {
            return Ok(true); // No container_ref means no worktree, so it's clean
        }
    }

    async fn start_execution_inner(
        &self,
        task_attempt: &TaskAttempt,
        execution_process: &ExecutionProcess,
        executor_action: &ExecutorAction,
    ) -> Result<(), ContainerError> {
        // Ensure worktrees exist and gather payload metadata
        let container_ref = self.ensure_container_exists(task_attempt).await?;
        let current_dir = PathBuf::from(&container_ref);
        let payload_envelope = self.build_executor_payload(task_attempt).await?;

        // Create the child and stream, add to execution tracker
        let spawn_ctx = ExecutorSpawnContext {
            current_dir: &current_dir,
            task_attempt_id: Some(&task_attempt.id),
            payload: &payload_envelope,
        };
        let mut spawned = executor_action.spawn(spawn_ctx).await?;

        self.track_child_msgs_in_store(execution_process.id, &mut spawned.child)
            .await;

        self.add_child_to_store(execution_process.id, spawned.child)
            .await;

        // Spawn unified exit monitor: watches OS exit and optional executor signal
        let _hn = self.spawn_exit_monitor(&execution_process.id, spawned.exit_signal);

        Ok(())
    }

    async fn stop_execution(
        &self,
        execution_process: &ExecutionProcess,
    ) -> Result<(), ContainerError> {
        let child = self
            .get_child_from_store(&execution_process.id)
            .await
            .ok_or_else(|| {
                ContainerError::Other(anyhow!("Child process not found for execution"))
            })?;
        ExecutionProcess::update_completion(
            &self.db.pool,
            execution_process.id,
            ExecutionProcessStatus::Killed,
            None,
        )
        .await?;

        // Kill the child process and remove from the store
        {
            let mut child_guard = child.write().await;
            if let Err(e) = command::kill_process_group(&mut child_guard).await {
                tracing::error!(
                    "Failed to stop execution process {}: {}",
                    execution_process.id,
                    e
                );
                return Err(e);
            }
        }
        self.remove_child_from_store(&execution_process.id).await;

        // Mark the process finished in the MsgStore
        if let Some(msg) = self.msg_stores.write().await.remove(&execution_process.id) {
            msg.push_finished();
        }

        // Update task status to InReview when execution is stopped
        if let Ok(ctx) = ExecutionProcess::load_context(&self.db.pool, execution_process.id).await
            && !matches!(
                ctx.execution_process.run_reason,
                ExecutionProcessRunReason::DevServer
            )
            && let Err(e) =
                Task::update_status(&self.db.pool, ctx.task.id, TaskStatus::InReview).await
        {
            tracing::error!("Failed to update task status to InReview: {e}");
        }

        tracing::debug!(
            "Execution process {} stopped successfully",
            execution_process.id
        );

        // Record after-head commit OID (best-effort)
        if let Ok(ctx) = ExecutionProcess::load_context(&self.db.pool, execution_process.id).await {
            let worktree = self.task_attempt_to_current_dir(&ctx.task_attempt);
            if let Ok(head) = self.git().get_head_info(&worktree) {
                let _ = ExecutionProcess::update_after_head_commit(
                    &self.db.pool,
                    execution_process.id,
                    &head.oid,
                )
                .await;
            }
        }

        Ok(())
    }

    async fn stream_diff(
        &self,
        task_attempt: &TaskAttempt,
        project_repository_id: Option<Uuid>,
    ) -> Result<futures::stream::BoxStream<'static, Result<LogMsg, std::io::Error>>, ContainerError>
    {
        let repo_ctx = self
            .resolve_repository_context(task_attempt, project_repository_id)
            .await?;

        let latest_merge =
            Merge::find_latest_by_task_attempt_id(&self.db.pool, task_attempt.id).await?;

        let is_ahead = if let Ok((ahead, _)) = self.git().get_branch_status(
            &repo_ctx.project_repo.git_repo_path,
            &repo_ctx.branch_name,
            &task_attempt.base_branch,
        ) {
            ahead > 0
        } else {
            false
        };

        let worktree_clean = self
            .git()
            .is_worktree_clean(&repo_ctx.worktree_path)
            .unwrap_or(false);

        if let Some(merge) = &latest_merge
            && let Some(commit) = merge.merge_commit()
            && worktree_clean
            && !is_ahead
        {
            return self.create_merged_diff_stream(&repo_ctx.project_repo, commit.as_str());
        }

        let base_commit = self.git().get_base_commit(
            &repo_ctx.project_repo.git_repo_path,
            &repo_ctx.branch_name,
            &task_attempt.base_branch,
        )?;

        self.create_live_diff_stream(&repo_ctx, &base_commit).await
    }

    async fn try_commit_changes(&self, ctx: &ExecutionContext) -> Result<bool, ContainerError> {
        if !matches!(
            ctx.execution_process.run_reason,
            ExecutionProcessRunReason::CodingAgent | ExecutionProcessRunReason::CleanupScript,
        ) {
            return Ok(false);
        }

        let message = match ctx.execution_process.run_reason {
            ExecutionProcessRunReason::CodingAgent => {
                // Try to retrieve the task summary from the executor session
                // otherwise fallback to default message
                match ExecutorSession::find_by_execution_process_id(
                    &self.db().pool,
                    ctx.execution_process.id,
                )
                .await
                {
                    Ok(Some(session)) if session.summary.is_some() => session.summary.unwrap(),
                    Ok(_) => {
                        tracing::debug!(
                            "No summary found for execution process {}, using default message",
                            ctx.execution_process.id
                        );
                        format!(
                            "Commit changes from coding agent for task attempt {}",
                            ctx.task_attempt.id
                        )
                    }
                    Err(e) => {
                        tracing::debug!(
                            "Failed to retrieve summary for execution process {}: {}",
                            ctx.execution_process.id,
                            e
                        );
                        format!(
                            "Commit changes from coding agent for task attempt {}",
                            ctx.task_attempt.id
                        )
                    }
                }
            }
            ExecutionProcessRunReason::CleanupScript => {
                format!(
                    "Cleanup script changes for task attempt {}",
                    ctx.task_attempt.id
                )
            }
            _ => Err(ContainerError::Other(anyhow::anyhow!(
                "Invalid run reason for commit"
            )))?,
        };

        let container_ref = ctx.task_attempt.container_ref.as_ref().ok_or_else(|| {
            ContainerError::Other(anyhow::anyhow!("Container reference not found"))
        })?;

        tracing::debug!(
            "Committing changes for task attempt {} at path {:?}: '{}'",
            ctx.task_attempt.id,
            &container_ref,
            message
        );

        let changes_committed = self.git().commit(Path::new(container_ref), &message)?;
        Ok(changes_committed)
    }

    /// Copy files from the original project directory to the worktree
    async fn copy_project_files(
        &self,
        source_dir: &Path,
        target_dir: &Path,
        copy_files: &str,
    ) -> Result<(), ContainerError> {
        let files: Vec<&str> = copy_files
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();

        for file_path in files {
            let source_file = source_dir.join(file_path);
            let target_file = target_dir.join(file_path);

            // Create parent directories if needed
            if let Some(parent) = target_file.parent()
                && !parent.exists()
            {
                std::fs::create_dir_all(parent).map_err(|e| {
                    ContainerError::Other(anyhow!("Failed to create directory {parent:?}: {e}"))
                })?;
            }

            // Copy the file
            if source_file.exists() {
                std::fs::copy(&source_file, &target_file).map_err(|e| {
                    ContainerError::Other(anyhow!(
                        "Failed to copy file {source_file:?} to {target_file:?}: {e}"
                    ))
                })?;
                tracing::info!("Copied file {:?} to worktree", file_path);
            } else {
                return Err(ContainerError::Other(anyhow!(
                    "File {source_file:?} does not exist in the project directory"
                )));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use db::models::{
        project::{CreateProject, Project},
        project_repository::ProjectRepository,
        task::{CreateTask, Task},
        task_attempt::{CreateTaskAttempt, TaskAttempt},
        task_attempt_repository::TaskAttemptRepository,
    };
    use executors::executors::BaseCodingAgent;
    use std::{collections::HashMap, path::Path, sync::Arc};
    use tempfile::TempDir;
    use tokio::sync::RwLock;
    use uuid::Uuid;

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var(key).ok();
            // std::env::{set_var, remove_var} are currently unsafe under edition 2024.
            // Wrap the calls so tests can manipulate the environment without leaking state.
            unsafe {
                std::env::set_var(key, value);
            }
            Self { key, previous }
        }

        fn set_from_path(key: &'static str, path: &Path) -> Self {
            Self::set(key, &path.to_string_lossy())
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            unsafe {
                if let Some(prev) = &self.previous {
                    std::env::set_var(self.key, prev);
                } else {
                    std::env::remove_var(self.key);
                }
            }
        }
    }

    #[tokio::test]
    async fn multi_repo_workflow_creates_worktrees_and_payload() {
        let temp = TempDir::new().expect("failed to create tempdir");

        let assets_dir = temp.path().join("assets");
        std::fs::create_dir_all(&assets_dir).unwrap();
        let _assets_guard = EnvGuard::set_from_path("VIBE_ASSETS_DIR", &assets_dir);

        let home_dir = temp.path().join("home");
        std::fs::create_dir_all(&home_dir).unwrap();
        let _home_guard = EnvGuard::set_from_path("HOME", &home_dir);

        let cache_dir = temp.path().join("cache");
        std::fs::create_dir_all(&cache_dir).unwrap();
        let _cache_guard = EnvGuard::set_from_path("XDG_CACHE_HOME", &cache_dir);

        let tmp_dir = temp.path().join("tmp");
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let _tmp_guard = EnvGuard::set_from_path("TMPDIR", &tmp_dir);

        let db = DBService::new().await.expect("failed to create DB");
        let msg_stores = Arc::new(RwLock::new(HashMap::<Uuid, Arc<MsgStore>>::new()));
        let config = Arc::new(RwLock::new(Config::default()));
        let git = GitService::new();
        let image_service = ImageService::new(db.pool.clone()).expect("failed to init image service");
        let container = LocalContainerService::new(
            db.clone(),
            msg_stores,
            config.clone(),
            git.clone(),
            image_service.clone(),
            None,
        );

        let repo_root = temp.path().join("primary_repo");
        git.initialize_repo_with_main_branch(&repo_root)
            .expect("failed to create primary repo");

        let secondary_root = temp.path().join("docs_repo");
        git.initialize_repo_with_main_branch(&secondary_root)
            .expect("failed to create secondary repo");
        std::fs::create_dir_all(secondary_root.join("docs")).unwrap();

        let project_id = Uuid::new_v4();
        let project = Project::create(
            &db.pool,
            &CreateProject {
                name: "Multi Repo Project".to_string(),
                git_repo_path: repo_root.to_string_lossy().to_string(),
                use_existing_repo: true,
                setup_script: None,
                dev_script: None,
                cleanup_script: None,
                copy_files: None,
            },
            project_id,
        )
        .await
        .expect("failed to create project");

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
        .bind(secondary_root.to_string_lossy().to_string())
        .bind("docs")
        .execute(&db.pool)
        .await
        .expect("failed to insert secondary repository");

        let task_id = Uuid::new_v4();
        let task = Task::create(
            &db.pool,
            &CreateTask {
                project_id: project.id,
                title: "QA multi repo flow".to_string(),
                description: None,
                parent_task_attempt: None,
                image_ids: None,
            },
            task_id,
        )
        .await
        .expect("failed to create task");

        let attempt = TaskAttempt::create(
            &db.pool,
            &CreateTaskAttempt {
                executor: BaseCodingAgent::ClaudeCode,
                base_branch: "main".to_string(),
            },
            task.id,
        )
        .await
        .expect("failed to create attempt");

        let container_ref = container
            .create(&attempt)
            .await
            .expect("worktree creation should succeed");
        assert!(Path::new(&container_ref).exists());

        let refreshed_attempt = TaskAttempt::find_by_id(&db.pool, attempt.id)
            .await
            .unwrap()
            .expect("attempt should exist");
        let attempt_repositories = TaskAttemptRepository::list_for_attempt(&db.pool, attempt.id)
            .await
            .expect("attempt repositories should load");
        assert_eq!(attempt_repositories.len(), 2);

        let branch_prefix = config.read().await.github.resolved_branch_prefix();
        let expected_branch = LocalContainerService::git_branch_from_task_attempt(
            &branch_prefix,
            &attempt.id,
            &task.title,
        );
        assert_eq!(
            refreshed_attempt.branch.as_deref(),
            Some(expected_branch.as_str())
        );

        let project_repositories = ProjectRepository::list_for_project(&db.pool, project.id)
            .await
            .expect("project repositories should load");
        assert_eq!(project_repositories.len(), 2);

        for repo in &project_repositories {
            let expected_path = LocalContainerService::worktree_path_for_repo(
                &attempt.id,
                &task.title,
                repo,
            );
            let entry = attempt_repositories
                .iter()
                .find(|r| r.project_repository_id == repo.id)
                .expect("missing attempt repository entry");
            let worktree = entry
                .container_ref
                .as_ref()
                .expect("worktree path should be recorded");
            assert_eq!(
                worktree,
                &expected_path.to_string_lossy().to_string()
            );
            assert_eq!(
                entry.branch.as_deref(),
                Some(expected_branch.as_str())
            );
            if repo.is_primary {
                assert_eq!(worktree, &container_ref);
            } else {
                assert_ne!(worktree, &container_ref);
            }
        }

        let payload_envelope = container
            .build_executor_payload(&refreshed_attempt)
            .await
            .expect("payload generation should succeed");
        let payload = payload_envelope.payload.as_ref();
        assert_eq!(payload.repositories.len(), 2);

        let primary_repo_id = project_repositories
            .iter()
            .find(|repo| repo.is_primary)
            .expect("primary repo missing")
            .id;
        assert_eq!(payload.primary_repository_id, primary_repo_id);

        let prefixes: Vec<&str> = payload
            .env
            .get("VIBE_REPOSITORIES")
            .expect("repo prefixes env missing")
            .split(',')
            .collect();
        assert_eq!(prefixes.len(), 2);

        for repo_ctx in &payload.repositories {
            let prefix = LocalContainerService::repo_env_prefix(&repo_ctx.slug);
            assert_eq!(
                payload
                    .env
                    .get(&format!("VIBE_REPO_{}_ID", prefix))
                    .expect("repo id env missing"),
                &repo_ctx.id.to_string()
            );
            assert_eq!(
                payload
                    .env
                    .get(&format!("VIBE_REPO_{}_PATH", prefix))
                    .expect("repo path env missing"),
                &repo_ctx.worktree_path
            );
        }

        assert_eq!(
            payload
                .env
                .get("VIBE_PRIMARY_REPO_PATH")
                .expect("primary path missing"),
            &container_ref
        );
        assert_eq!(
            payload
                .env
                .get("VIBE_PRIMARY_REPO_BRANCH")
                .expect("primary branch missing"),
            &expected_branch
        );
        assert_eq!(
            payload
                .env
                .get("VIBE_PRIMARY_REPOSITORY_ID")
                .expect("primary id missing"),
            &primary_repo_id.to_string()
        );
    }
}

impl LocalContainerService {
    /// Extract the last assistant message from the MsgStore history
    fn extract_last_assistant_message(&self, exec_id: &Uuid) -> Option<String> {
        // Get the MsgStore for this execution
        let msg_stores = self.msg_stores.try_read().ok()?;
        let msg_store = msg_stores.get(exec_id)?;

        // Get the history and scan in reverse for the last assistant message
        let history = msg_store.get_history();

        for msg in history.iter().rev() {
            if let LogMsg::JsonPatch(patch) = msg {
                // Try to extract a NormalizedEntry from the patch
                if let Some((_, entry)) = extract_normalized_entry_from_patch(patch)
                    && matches!(entry.entry_type, NormalizedEntryType::AssistantMessage)
                {
                    let content = entry.content.trim();
                    if !content.is_empty() {
                        // Truncate to reasonable size (4KB as Oracle suggested)
                        const MAX_SUMMARY_LENGTH: usize = 4096;
                        if content.len() > MAX_SUMMARY_LENGTH {
                            return Some(format!("{}...", &content[..MAX_SUMMARY_LENGTH]));
                        }
                        return Some(content.to_string());
                    }
                }
            }
        }

        None
    }

    /// Update the executor session summary with the final assistant message
    async fn update_executor_session_summary(&self, exec_id: &Uuid) -> Result<(), anyhow::Error> {
        // Check if there's an executor session for this execution process
        let session =
            ExecutorSession::find_by_execution_process_id(&self.db.pool, *exec_id).await?;

        if let Some(session) = session {
            // Only update if summary is not already set
            if session.summary.is_none() {
                if let Some(summary) = self.extract_last_assistant_message(exec_id) {
                    ExecutorSession::update_summary(&self.db.pool, *exec_id, &summary).await?;
                } else {
                    tracing::debug!("No assistant message found for execution {}", exec_id);
                }
            }
        }

        Ok(())
    }

    /// If a queued follow-up draft exists for this attempt and nothing is running,
    /// start it immediately and clear the draft.
    async fn try_consume_queued_followup(
        &self,
        ctx: &ExecutionContext,
    ) -> Result<(), ContainerError> {
        // Only consider CodingAgent/cleanup chains; skip DevServer completions
        if matches!(
            ctx.execution_process.run_reason,
            ExecutionProcessRunReason::DevServer
        ) {
            return Ok(());
        }

        // If anything is running for this attempt, bail
        let procs =
            ExecutionProcess::find_by_task_attempt_id(&self.db.pool, ctx.task_attempt.id, false)
                .await?;
        if procs
            .iter()
            .any(|p| matches!(p.status, ExecutionProcessStatus::Running))
        {
            return Ok(());
        }

        // Load draft and ensure it's eligible
        let Some(draft) =
            FollowUpDraft::find_by_task_attempt_id(&self.db.pool, ctx.task_attempt.id).await?
        else {
            return Ok(());
        };

        if !draft.queued || draft.prompt.trim().is_empty() {
            return Ok(());
        }

        // Atomically acquire sending lock; if not acquired, someone else is sending.
        if !FollowUpDraft::try_mark_sending(&self.db.pool, ctx.task_attempt.id)
            .await
            .unwrap_or(false)
        {
            return Ok(());
        }

        // Ensure worktree exists
        let container_ref = self.ensure_container_exists(&ctx.task_attempt).await?;

        // Get session id
        let Some(session_id) = ExecutionProcess::find_latest_session_id_by_task_attempt(
            &self.db.pool,
            ctx.task_attempt.id,
        )
        .await?
        else {
            tracing::warn!(
                "No session id found for attempt {}. Cannot start queued follow-up.",
                ctx.task_attempt.id
            );
            return Ok(());
        };

        // Get last coding agent process to inherit executor profile
        let Some(latest) = ExecutionProcess::find_latest_by_task_attempt_and_run_reason(
            &self.db.pool,
            ctx.task_attempt.id,
            &ExecutionProcessRunReason::CodingAgent,
        )
        .await?
        else {
            tracing::warn!(
                "No prior CodingAgent process for attempt {}. Cannot start queued follow-up.",
                ctx.task_attempt.id
            );
            return Ok(());
        };

        use executors::actions::ExecutorActionType;
        let initial_executor_profile_id = match &latest.executor_action()?.typ {
            ExecutorActionType::CodingAgentInitialRequest(req) => req.executor_profile_id.clone(),
            ExecutorActionType::CodingAgentFollowUpRequest(req) => req.executor_profile_id.clone(),
            _ => {
                tracing::warn!(
                    "Latest process for attempt {} is not a coding agent; skipping queued follow-up",
                    ctx.task_attempt.id
                );
                return Ok(());
            }
        };

        let executor_profile_id = executors::profile::ExecutorProfileId {
            executor: initial_executor_profile_id.executor,
            variant: draft.variant.clone(),
        };

        // Prepare cleanup action
        let cleanup_action = ctx
            .task
            .parent_project(&self.db.pool)
            .await?
            .and_then(|p| p.cleanup_script)
            .map(|script| {
                Box::new(executors::actions::ExecutorAction::new(
                    executors::actions::ExecutorActionType::ScriptRequest(
                        executors::actions::script::ScriptRequest {
                            script,
                            language: executors::actions::script::ScriptRequestLanguage::Bash,
                            context: executors::actions::script::ScriptContext::CleanupScript,
                        },
                    ),
                    None,
                ))
            });

        // Handle images: associate, copy to worktree, canonicalize prompt
        let mut prompt = draft.prompt.clone();
        if let Some(image_ids) = &draft.image_ids {
            // Associate to task
            let _ = TaskImage::associate_many_dedup(&self.db.pool, ctx.task.id, image_ids).await;

            // Copy to worktree and canonicalize
            let worktree_path = std::path::PathBuf::from(&container_ref);
            if let Err(e) = self
                .image_service
                .copy_images_by_ids_to_worktree(&worktree_path, image_ids)
                .await
            {
                tracing::warn!("Failed to copy images to worktree: {}", e);
            } else {
                prompt = ImageService::canonicalise_image_paths(&prompt, &worktree_path);
            }
        }

        let follow_up_request =
            executors::actions::coding_agent_follow_up::CodingAgentFollowUpRequest {
                prompt,
                session_id,
                executor_profile_id,
            };

        let follow_up_action = executors::actions::ExecutorAction::new(
            executors::actions::ExecutorActionType::CodingAgentFollowUpRequest(follow_up_request),
            cleanup_action,
        );

        // Start the execution
        let _ = self
            .start_execution(
                &ctx.task_attempt,
                &follow_up_action,
                &ExecutionProcessRunReason::CodingAgent,
            )
            .await?;

        // Clear the draft to reflect that it has been consumed
        let _ = FollowUpDraft::clear_after_send(&self.db.pool, ctx.task_attempt.id).await;

        Ok(())
    }
}
