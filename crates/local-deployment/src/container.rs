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
        draft::{Draft, DraftType},
        execution_process::{
            ExecutionContext, ExecutionProcess, ExecutionProcessRunReason, ExecutionProcessStatus,
        },
        executor_session::ExecutorSession,
        image::TaskImage,
        merge::Merge,
        project::Project,
        project_repository::ProjectRepository,
        task::{Task, TaskStatus},
        task_attempt::TaskAttempt,
        task_attempt_repository::TaskAttemptRepository,
    },
};
use deployment::DeploymentError;
use executors::{
    actions::{Executable, ExecutorAction, ExecutorSpawnContext},
    logs::{
        NormalizedEntryType,
        utils::{
            ConversationPatch,
            patch::{escape_json_pointer_segment, extract_normalized_entry_from_patch},
        },
    },
};
use futures::{FutureExt, StreamExt, TryStreamExt, stream::select};
use notify::RecommendedWatcher;
use notify_debouncer_full::{DebouncedEvent, Debouncer, RecommendedCache};
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
use tokio::{sync::RwLock, task::JoinHandle};
use tokio_util::io::ReaderStream;
use utils::{
    diff::Diff,
    log_msg::LogMsg,
    msg_store::MsgStore,
    text::{git_branch_id, git_branch_name_with_prefix, short_uuid},
};
use uuid::Uuid;

use crate::command;

/// Stream wrapper that owns the filesystem watcher
/// When this stream is dropped, the watcher is automatically cleaned up
struct DiffStreamWithWatcher {
    stream: futures::stream::BoxStream<'static, Result<LogMsg, std::io::Error>>,
    _watcher: Option<Debouncer<RecommendedWatcher, RecommendedCache>>,
}

impl futures::Stream for DiffStreamWithWatcher {
    type Item = Result<LogMsg, std::io::Error>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        // Delegate to inner stream
        std::pin::Pin::new(&mut self.stream).poll_next(cx)
    }
}

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

#[derive(Clone, Debug)]
struct RepositoryInfo {
    id: Uuid,
    name: String,
    root: String,
    root_prefix: Option<String>,
    is_primary: bool,
}

#[derive(Clone, Debug)]
struct RepositoryLookup {
    repos: Vec<RepositoryInfo>,
    primary_index: Option<usize>,
}

impl RepositoryLookup {
    fn from_project_and_attempt(
        project_repositories: &[ProjectRepository],
        attempt_repositories: &[TaskAttemptRepository],
    ) -> Self {
        let attempt_map = attempt_repositories
            .iter()
            .map(|entry| (entry.project_repository_id, entry))
            .collect::<HashMap<_, _>>();

        let mut repos = Vec::new();
        for repo in project_repositories {
            if !attempt_map.is_empty() && !attempt_map.contains_key(&repo.id) {
                continue;
            }

            let is_primary = attempt_map
                .get(&repo.id)
                .map(|entry| entry.is_primary)
                .unwrap_or(repo.is_primary);

            repos.push(RepositoryInfo::new(repo, is_primary));
        }

        if repos.is_empty()
            && let Some(repo) = project_repositories.first()
        {
            repos.push(RepositoryInfo::new(repo, true));
        }

        repos.sort_by(|a, b| b.root.len().cmp(&a.root.len()));
        let primary_index = repos
            .iter()
            .position(|info| info.is_primary)
            .or_else(|| (!repos.is_empty()).then_some(0));

        RepositoryLookup {
            repos,
            primary_index,
        }
    }

    fn annotate_diff(&self, diff: &mut Diff) -> Option<Uuid> {
        let path = diff
            .new_path
            .as_deref()
            .or(diff.old_path.as_deref())
            .map(normalize_diff_path)
            .unwrap_or_default();

        let repo_info = self.match_path(path).or_else(|| self.primary());

        let repo_info = match repo_info {
            Some(info) => info,
            None => {
                diff.repository_id = None;
                diff.repository_name = None;
                diff.repository_root = None;
                return None;
            }
        };

        diff.repository_id = Some(repo_info.id);
        diff.repository_name = Some(repo_info.name.clone());
        diff.repository_root = if repo_info.root.is_empty() {
            None
        } else {
            Some(repo_info.root.clone())
        };

        Some(repo_info.id)
    }

    fn match_path(&self, raw_path: &str) -> Option<&RepositoryInfo> {
        let path = normalize_diff_path(raw_path);
        self.repos.iter().find(|info| info.matches(path))
    }

    fn primary(&self) -> Option<&RepositoryInfo> {
        self.primary_index
            .and_then(|index| self.repos.get(index))
            .or_else(|| self.repos.first())
    }
}

impl RepositoryInfo {
    fn new(repo: &ProjectRepository, is_primary: bool) -> Self {
        let root = normalize_repo_root(&repo.root_path);
        let root_prefix = if root.is_empty() {
            None
        } else {
            Some(format!("{}/", root))
        };

        RepositoryInfo {
            id: repo.id,
            name: repo.name.clone(),
            root,
            root_prefix,
            is_primary,
        }
    }

    fn matches(&self, path: &str) -> bool {
        if self.root.is_empty() {
            true
        } else if path == self.root {
            true
        } else {
            self.root_prefix
                .as_ref()
                .map(|prefix| path.starts_with(prefix))
                .unwrap_or(false)
        }
    }
}

fn normalize_repo_root(raw: &str) -> String {
    let replaced = raw.replace('\\', "/");
    replaced.trim_matches('/').to_string()
}

fn normalize_diff_path(path: &str) -> &str {
    let path = path.strip_prefix("./").unwrap_or(path);
    path.trim_start_matches('/')
}

impl LocalContainerService {
    // Max cumulative content bytes allowed per diff stream
    const MAX_CUMULATIVE_DIFF_BYTES: usize = 200 * 1024 * 1024; // 200MB

    // Apply stream-level omit policy based on cumulative bytes.
    // If adding this diff's contents exceeds the cap, strip contents and set stats.
    fn apply_stream_omit_policy(
        diff: &mut utils::diff::Diff,
        sent_bytes: &Arc<AtomicUsize>,
        stats_only: bool,
    ) {
        if stats_only {
            Self::omit_diff_contents(diff);
            return;
        }

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
            Self::omit_diff_contents(diff);
        } else {
            // safe to include; account for it
            let _ = sent_bytes.fetch_add(size, Ordering::Relaxed);
        }
    }

    fn omit_diff_contents(diff: &mut utils::diff::Diff) {
        if diff.additions.is_none()
            && diff.deletions.is_none()
            && (diff.old_content.is_some() || diff.new_content.is_some())
        {
            let old = diff.old_content.as_deref().unwrap_or("");
            let new = diff.new_content.as_deref().unwrap_or("");
            let (add, del) = utils::diff::compute_line_change_counts(old, new);
            diff.additions = Some(add);
            diff.deletions = Some(del);
        }

        diff.old_content = None;
        diff.new_content = None;
        diff.content_omitted = true;
    }

    async fn build_executor_env(
        &self,
        task_attempt: &TaskAttempt,
    ) -> Result<HashMap<String, String>, ContainerError> {
        let task = Task::find_by_id(&self.db.pool, task_attempt.task_id)
            .await?
            .ok_or(sqlx::Error::RowNotFound)?;

        let project = Project::find_by_id(&self.db.pool, task.project_id)
            .await?
            .ok_or(sqlx::Error::RowNotFound)?;

        let repositories = ProjectRepository::list_for_project(&self.db.pool, project.id).await?;
        let attempt_repositories =
            TaskAttemptRepository::list_for_attempt(&self.db.pool, task_attempt.id).await?;

        let attempt_map = attempt_repositories
            .into_iter()
            .map(|entry| (entry.project_repository_id, entry))
            .collect::<HashMap<_, _>>();

        Ok(compute_repository_env_map(
            task_attempt,
            &project,
            &repositories,
            &attempt_map,
        ))
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

            if !ExecutionProcess::was_stopped(&db.pool, exec_id).await
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

    fn repo_worktree_suffix(repo: &ProjectRepository) -> String {
        let slug = git_branch_id(&repo.name);
        let suffix = short_uuid(&repo.id);

        if slug.is_empty() {
            format!("repo-{suffix}")
        } else {
            format!("{slug}-{suffix}")
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
    /// Get the project repository path for a task attempt
    /// Create a diff log stream for merged attempts (never changes) for WebSocket
    fn create_merged_diff_stream(
        &self,
        project_repo_path: &Path,
        merge_commit_id: &str,
        stats_only: bool,
        repository_filter: Option<Uuid>,
        repo_lookup: Arc<RepositoryLookup>,
    ) -> Result<DiffStreamWithWatcher, ContainerError> {
        let diffs = self.git().get_diffs(
            DiffTarget::Commit {
                repo_path: project_repo_path,
                commit_sha: merge_commit_id,
            },
            None,
        )?;

        let cum = Arc::new(AtomicUsize::new(0));
        let mut filtered_diffs = Vec::new();
        for mut diff in diffs {
            let repo_match = repo_lookup.annotate_diff(&mut diff);
            if let Some(filter) = repository_filter {
                if repo_match != Some(filter) {
                    continue;
                }
            }

            Self::apply_stream_omit_policy(&mut diff, &cum, stats_only);
            filtered_diffs.push(diff);
        }

        let stream = futures::stream::iter(filtered_diffs.into_iter().map(|diff| {
            let entry_index = GitService::diff_path(&diff);
            let patch =
                ConversationPatch::add_diff(escape_json_pointer_segment(&entry_index), diff);
            Ok::<_, std::io::Error>(LogMsg::JsonPatch(patch))
        }))
        .chain(futures::stream::once(async {
            Ok::<_, std::io::Error>(LogMsg::Finished)
        }))
        .boxed();

        Ok(DiffStreamWithWatcher {
            stream,
            _watcher: None, // Merged diffs are static, no watcher needed
        })
    }

    /// Create a live diff log stream for ongoing attempts for WebSocket
    /// Returns a stream that owns the filesystem watcher - when dropped, watcher is cleaned up
    async fn create_live_diff_stream(
        &self,
        worktree_path: &Path,
        base_commit: &Commit,
        stats_only: bool,
        repository_filter: Option<Uuid>,
        repo_lookup: Arc<RepositoryLookup>,
    ) -> Result<DiffStreamWithWatcher, ContainerError> {
        // Get initial snapshot
        let git_service = self.git().clone();
        let initial_diffs = git_service.get_diffs(
            DiffTarget::Worktree {
                worktree_path,
                base_commit,
            },
            None,
        )?;

        let cumulative = Arc::new(AtomicUsize::new(0));
        let full_sent = Arc::new(std::sync::RwLock::new(HashSet::<String>::new()));
        let mut initial_diffs_vec = Vec::new();
        for mut diff in initial_diffs {
            let repo_match = repo_lookup.annotate_diff(&mut diff);
            if let Some(filter) = repository_filter {
                if repo_match != Some(filter) {
                    continue;
                }
            }

            Self::apply_stream_omit_policy(&mut diff, &cumulative, stats_only);
            initial_diffs_vec.push(diff);
        }

        // Record which paths were sent with full content
        {
            let mut guard = full_sent.write().unwrap();
            for d in &initial_diffs_vec {
                if !d.content_omitted {
                    let p = GitService::diff_path(d);
                    guard.insert(p);
                }
            }
        }

        let initial_stream = futures::stream::iter(initial_diffs_vec.into_iter().map(|diff| {
            let entry_index = GitService::diff_path(&diff);
            let patch =
                ConversationPatch::add_diff(escape_json_pointer_segment(&entry_index), diff);
            Ok::<_, std::io::Error>(LogMsg::JsonPatch(patch))
        }))
        .boxed();

        // Create live update stream
        let worktree_path = worktree_path.to_path_buf();
        let base_commit = base_commit.clone();
        let worktree_path_for_spawn = worktree_path.clone();
        let watcher_result = tokio::task::spawn_blocking(move || {
            filesystem_watcher::async_watcher(worktree_path_for_spawn)
        })
        .await
        .map_err(|e| io::Error::other(format!("Failed to spawn watcher setup: {e}")))?;
        let (debouncer, mut rx, canonical_worktree_path) =
            watcher_result.map_err(|e| io::Error::other(e.to_string()))?;

        let live_stream = {
            let git_service = git_service.clone();
            let cumulative = Arc::clone(&cumulative);
            let full_sent = Arc::clone(&full_sent);
            let repo_lookup = Arc::clone(&repo_lookup);
            try_stream! {
                while let Some(result) = rx.next().await {
                    match result {
                        Ok(events) => {
                            let changed_paths = Self::extract_changed_paths(&events, &canonical_worktree_path, &worktree_path);

                            if !changed_paths.is_empty() {
                                for msg in Self::process_file_changes(
                                    &git_service,
                                    &worktree_path,
                                    &base_commit,
                                    &changed_paths,
                                    &cumulative,
                                    &full_sent,
                                    stats_only,
                                    repo_lookup.as_ref(),
                                    repository_filter,
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

        let combined_stream = initial_stream.chain(live_stream).boxed();

        Ok(DiffStreamWithWatcher {
            stream: combined_stream,
            _watcher: Some(debouncer),
        })
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
        worktree_path: &Path,
        base_commit: &Commit,
        changed_paths: &[String],
        cumulative_bytes: &Arc<AtomicUsize>,
        full_sent_paths: &Arc<std::sync::RwLock<HashSet<String>>>,
        stats_only: bool,
        repo_lookup: &RepositoryLookup,
        repository_filter: Option<Uuid>,
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
            let repo_match = repo_lookup.annotate_diff(&mut diff);
            if let Some(filter) = repository_filter {
                if repo_match != Some(filter) {
                    continue;
                }
            }

            let file_path = GitService::diff_path(&diff);
            files_with_diffs.insert(file_path.clone());
            // Apply stream-level omit policy (affects contents and stats)
            Self::apply_stream_omit_policy(&mut diff, cumulative_bytes, stats_only);

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
            if let Some(filter) = repository_filter {
                let repo_match = repo_lookup.match_path(changed_path).map(|info| info.id);
                if repo_match != Some(filter) {
                    continue;
                }
            }

            if !files_with_diffs.contains(changed_path) {
                let patch =
                    ConversationPatch::remove_diff(escape_json_pointer_segment(changed_path));
                msgs.push(LogMsg::JsonPatch(patch));
            }
        }

        Ok(msgs)
    }
}

fn repo_env_prefix(repo: &ProjectRepository) -> String {
    let base = git_branch_id(&repo.name);
    let suffix = short_uuid(&repo.id);
    let slug = if base.is_empty() {
        format!("repo-{}", suffix)
    } else {
        format!("{base}-{suffix}")
    };

    slug.replace('-', "_").to_uppercase()
}

fn compute_repository_env_map(
    task_attempt: &TaskAttempt,
    project: &Project,
    repositories: &[ProjectRepository],
    attempt_map: &HashMap<Uuid, TaskAttemptRepository>,
) -> HashMap<String, String> {
    let mut env = HashMap::new();

    if repositories.is_empty() {
        let prefix = "PRIMARY".to_string();
        let path = task_attempt
            .container_ref
            .clone()
            .unwrap_or_else(|| project.git_repo_path.to_string_lossy().to_string());

        env.insert("VIBE_REPOSITORY_COUNT".into(), "1".into());
        env.insert("VIBE_REPOSITORIES".into(), prefix.clone());
        env.insert(format!("VIBE_REPO_{}_PATH", prefix), path.clone());
        env.insert(format!("VIBE_REPO_{}_ROOT", prefix), String::new());
        env.insert(
            format!("VIBE_REPO_{}_BRANCH", prefix),
            task_attempt.branch.clone(),
        );
        env.insert(format!("VIBE_REPO_{}_NAME", prefix), project.name.clone());
        env.insert(format!("VIBE_REPO_{}_IS_PRIMARY", prefix), "1".into());
        env.insert("VIBE_PRIMARY_REPO_PREFIX".into(), prefix.clone());
        env.insert("VIBE_PRIMARY_REPO_PATH".into(), path);
        env.insert("VIBE_PRIMARY_REPO_ROOT".into(), String::new());
        env.insert("VIBE_PRIMARY_REPO_NAME".into(), project.name.clone());
        env.insert(
            "VIBE_PRIMARY_REPO_BRANCH".into(),
            task_attempt.branch.clone(),
        );

        return env;
    }

    let mut prefixes = Vec::with_capacity(repositories.len());
    let mut primary_prefix: Option<String> = None;

    for repo in repositories {
        let prefix = repo_env_prefix(repo);
        let attempt_entry = attempt_map.get(&repo.id);

        let repo_path = if repo.is_primary {
            task_attempt
                .container_ref
                .clone()
                .or_else(|| attempt_entry.and_then(|entry| entry.container_ref.clone()))
                .unwrap_or_else(|| repo.git_repo_path.to_string_lossy().to_string())
        } else {
            attempt_entry
                .and_then(|entry| entry.container_ref.clone())
                .unwrap_or_else(|| repo.git_repo_path.to_string_lossy().to_string())
        };

        let branch = attempt_entry
            .and_then(|entry| entry.branch.clone())
            .or_else(|| {
                if repo.is_primary {
                    Some(task_attempt.branch.clone())
                } else {
                    None
                }
            })
            .unwrap_or_default();

        env.insert(format!("VIBE_REPO_{}_PATH", prefix), repo_path.clone());
        env.insert(format!("VIBE_REPO_{}_ROOT", prefix), repo.root_path.clone());
        env.insert(format!("VIBE_REPO_{}_BRANCH", prefix), branch);
        env.insert(format!("VIBE_REPO_{}_NAME", prefix), repo.name.clone());
        env.insert(
            format!("VIBE_REPO_{}_IS_PRIMARY", prefix),
            if repo.is_primary {
                "1".to_string()
            } else {
                "0".to_string()
            },
        );

        if repo.is_primary {
            primary_prefix = Some(prefix.clone());
            env.insert("VIBE_PRIMARY_REPO_PATH".into(), repo_path);
            env.insert("VIBE_PRIMARY_REPO_ROOT".into(), repo.root_path.clone());
            env.insert("VIBE_PRIMARY_REPO_PREFIX".into(), prefix.clone());
            env.insert("VIBE_PRIMARY_REPO_NAME".into(), repo.name.clone());
            let primary_branch = attempt_entry
                .and_then(|entry| entry.branch.clone())
                .unwrap_or_else(|| task_attempt.branch.clone());
            env.insert("VIBE_PRIMARY_REPO_BRANCH".into(), primary_branch);
        }

        prefixes.push(prefix);
    }

    env.insert(
        "VIBE_REPOSITORY_COUNT".into(),
        repositories.len().to_string(),
    );
    env.insert("VIBE_REPOSITORIES".into(), prefixes.join(","));

    if let Some(primary_prefix) = primary_prefix.clone() {
        env.entry("VIBE_PRIMARY_REPO_PREFIX".into())
            .or_insert(primary_prefix);
    } else if let Some(first) = prefixes.first() {
        env.entry("VIBE_PRIMARY_REPO_PREFIX".into())
            .or_insert(first.clone());
    }

    if !env.contains_key("VIBE_PRIMARY_REPO_PATH") {
        let fallback_path = task_attempt
            .container_ref
            .clone()
            .unwrap_or_else(|| project.git_repo_path.to_string_lossy().to_string());
        env.insert("VIBE_PRIMARY_REPO_PATH".into(), fallback_path);
    }

    if !env.contains_key("VIBE_PRIMARY_REPO_ROOT") {
        env.insert("VIBE_PRIMARY_REPO_ROOT".into(), String::new());
    }

    if !env.contains_key("VIBE_PRIMARY_REPO_NAME") && !repositories.is_empty() {
        env.insert(
            "VIBE_PRIMARY_REPO_NAME".into(),
            repositories[0].name.clone(),
        );
    }

    if !env.contains_key("VIBE_PRIMARY_REPO_BRANCH") {
        env.insert(
            "VIBE_PRIMARY_REPO_BRANCH".into(),
            task_attempt.branch.clone(),
        );
    }

    env
}

impl LocalContainerService {
    async fn ensure_repository_container(
        &self,
        task_attempt: &TaskAttempt,
        task: &Task,
        repo: &ProjectRepository,
        attempt_entry: Option<&TaskAttemptRepository>,
    ) -> Result<(String, String), ContainerError> {
        let worktree_dir_name =
            LocalContainerService::dir_name_from_task_attempt(&task_attempt.id, &task.title);
        let base_worktree_dir = WorktreeManager::get_worktree_base_dir();

        let branch_to_use = attempt_entry
            .and_then(|entry| entry.branch.clone())
            .map(|b| b.trim().to_string())
            .filter(|b| !b.is_empty())
            .unwrap_or_else(|| task_attempt.branch.clone());

        let default_path = if repo.is_primary {
            base_worktree_dir.join(&worktree_dir_name)
        } else {
            let suffix = Self::repo_worktree_suffix(repo);
            base_worktree_dir.join(format!("{worktree_dir_name}--{suffix}"))
        };

        let entry_is_primary = attempt_entry
            .map(|entry| entry.is_primary)
            .unwrap_or(repo.is_primary);

        let path_string = attempt_entry
            .and_then(|entry| entry.container_ref.clone())
            .or_else(|| {
                entry_is_primary
                    .then(|| task_attempt.container_ref.clone())
                    .flatten()
            })
            .unwrap_or_else(|| default_path.to_string_lossy().to_string());

        let worktree_path = PathBuf::from(&path_string);

        WorktreeManager::ensure_worktree_exists(
            &repo.git_repo_path,
            &branch_to_use,
            &worktree_path,
        )
        .await?;

        if entry_is_primary
            && task_attempt
                .container_ref
                .as_deref()
                .map(|existing| existing != path_string.as_str())
                .unwrap_or(true)
        {
            TaskAttempt::update_container_ref(&self.db.pool, task_attempt.id, &path_string).await?;
        }

        TaskAttemptRepository::upsert_container_ref(
            &self.db.pool,
            task_attempt.id,
            repo.id,
            entry_is_primary,
            Some(path_string.as_str()),
        )
        .await?;

        TaskAttemptRepository::upsert_branch(
            &self.db.pool,
            task_attempt.id,
            repo.id,
            entry_is_primary,
            Some(branch_to_use.as_str()),
        )
        .await?;

        Ok((path_string, branch_to_use))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::{collections::HashMap, path::PathBuf};

    fn make_project(name: &str, path: &str) -> Project {
        let now = Utc::now();
        Project {
            id: Uuid::new_v4(),
            name: name.to_string(),
            git_repo_path: PathBuf::from(path),
            setup_script: None,
            dev_script: None,
            cleanup_script: None,
            copy_files: None,
            created_at: now,
            updated_at: now,
        }
    }

    fn make_task_attempt(
        project_task_id: Uuid,
        container: Option<&str>,
        branch: &str,
    ) -> TaskAttempt {
        let now = Utc::now();
        TaskAttempt {
            id: Uuid::new_v4(),
            task_id: project_task_id,
            container_ref: container.map(|p| p.to_string()),
            branch: branch.to_string(),
            target_branch: "main".to_string(),
            executor: "CLAUDE_CODE".to_string(),
            worktree_deleted: false,
            setup_completed_at: None,
            created_at: now,
            updated_at: now,
        }
    }

    fn make_repository(
        project_id: Uuid,
        name: &str,
        path: &str,
        root: &str,
        is_primary: bool,
    ) -> ProjectRepository {
        let now = Utc::now();
        ProjectRepository {
            id: Uuid::new_v4(),
            project_id,
            name: name.to_string(),
            git_repo_path: PathBuf::from(path),
            root_path: root.to_string(),
            is_primary,
            created_at: now,
            updated_at: now,
        }
    }

    fn make_attempt_repo(
        task_attempt_id: Uuid,
        project_repository_id: Uuid,
        container: Option<&str>,
        branch: Option<&str>,
        is_primary: bool,
    ) -> TaskAttemptRepository {
        let now = Utc::now();
        TaskAttemptRepository {
            id: Uuid::new_v4(),
            task_attempt_id,
            project_repository_id,
            is_primary,
            container_ref: container.map(|p| p.to_string()),
            branch: branch.map(|b| b.to_string()),
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn compute_env_single_repository() {
        let project = make_project("App", "/tmp/app");
        let task_attempt =
            make_task_attempt(Uuid::new_v4(), Some("/tmp/worktrees/app"), "feature/app");
        let repo = make_repository(project.id, "App", "/tmp/app", "", true);

        let env =
            compute_repository_env_map(&task_attempt, &project, &[repo.clone()], &HashMap::new());

        let prefix = repo_env_prefix(&repo);
        assert_eq!(env.get("VIBE_REPOSITORY_COUNT"), Some(&"1".to_string()));
        assert_eq!(env.get("VIBE_REPOSITORIES"), Some(&prefix));
        assert_eq!(
            env.get(&format!("VIBE_REPO_{}_PATH", prefix)),
            Some(&"/tmp/worktrees/app".to_string())
        );
        assert_eq!(
            env.get(&format!("VIBE_REPO_{}_IS_PRIMARY", prefix)),
            Some(&"1".to_string())
        );
        assert_eq!(
            env.get("VIBE_PRIMARY_REPO_PATH"),
            Some(&"/tmp/worktrees/app".to_string())
        );
        assert_eq!(env.get("VIBE_PRIMARY_REPO_ROOT"), Some(&String::new()));
        assert_eq!(
            env.get("VIBE_PRIMARY_REPO_BRANCH"),
            Some(&"feature/app".to_string())
        );
    }

    #[test]
    fn compute_env_multiple_repositories() {
        let project = make_project("Suite", "/tmp/suite");
        let task_attempt =
            make_task_attempt(Uuid::new_v4(), Some("/tmp/worktrees/suite"), "feature/main");

        let primary_repo = make_repository(project.id, "Suite", "/tmp/suite", "", true);
        let secondary_repo = make_repository(project.id, "Docs", "/tmp/suite", "docs", false);

        let mut attempt_map = HashMap::new();
        attempt_map.insert(
            primary_repo.id,
            make_attempt_repo(
                task_attempt.id,
                primary_repo.id,
                Some("/tmp/worktrees/suite"),
                Some("feature/main"),
                true,
            ),
        );
        attempt_map.insert(
            secondary_repo.id,
            make_attempt_repo(
                task_attempt.id,
                secondary_repo.id,
                Some("/tmp/worktrees/docs"),
                Some("docs-update"),
                false,
            ),
        );

        let env = compute_repository_env_map(
            &task_attempt,
            &project,
            &[primary_repo.clone(), secondary_repo.clone()],
            &attempt_map,
        );

        let primary_prefix = repo_env_prefix(&primary_repo);
        let secondary_prefix = repo_env_prefix(&secondary_repo);
        assert_eq!(env.get("VIBE_REPOSITORY_COUNT"), Some(&"2".to_string()));
        assert_eq!(
            env.get("VIBE_REPOSITORIES"),
            Some(&format!("{},{}", primary_prefix, secondary_prefix))
        );
        assert_eq!(
            env.get(&format!("VIBE_REPO_{}_PATH", secondary_prefix)),
            Some(&"/tmp/worktrees/docs".to_string())
        );
        assert_eq!(env.get("VIBE_PRIMARY_REPO_PREFIX"), Some(&primary_prefix));
        assert_eq!(
            env.get("VIBE_PRIMARY_REPO_PATH"),
            Some(&"/tmp/worktrees/suite".to_string())
        );
        assert_eq!(
            env.get(&format!("VIBE_REPO_{}_BRANCH", secondary_prefix)),
            Some(&"docs-update".to_string())
        );
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

    fn git_branch_from_task_attempt(&self, attempt_id: &Uuid, task_title: &str) -> String {
        let prefix = match tokio::runtime::Handle::try_current() {
            Ok(_) => tokio::task::block_in_place(|| {
                let config = self.config.blocking_read();
                config.github.resolved_branch_prefix()
            }),
            Err(_) => {
                let config = self.config.blocking_read();
                config.github.resolved_branch_prefix()
            }
        };

        git_branch_name_with_prefix(&prefix, attempt_id, task_title)
    }

    fn task_attempt_to_current_dir(&self, task_attempt: &TaskAttempt) -> PathBuf {
        PathBuf::from(task_attempt.container_ref.clone().unwrap_or_default())
    }
    /// Create a container
    async fn create(&self, task_attempt: &TaskAttempt) -> Result<ContainerRef, ContainerError> {
        let task = task_attempt
            .parent_task(&self.db.pool)
            .await?
            .ok_or(sqlx::Error::RowNotFound)?;

        let worktree_dir_name =
            LocalContainerService::dir_name_from_task_attempt(&task_attempt.id, &task.title);
        let base_worktree_dir = WorktreeManager::get_worktree_base_dir();
        let worktree_path = base_worktree_dir.join(&worktree_dir_name);

        let project = task
            .parent_project(&self.db.pool)
            .await?
            .ok_or(sqlx::Error::RowNotFound)?;

        WorktreeManager::create_worktree(
            &project.git_repo_path,
            &task_attempt.branch,
            &worktree_path,
            &task_attempt.target_branch,
            true, // create new branch
        )
        .await?;

        // Copy files specified in the project's copy_files field
        if let Some(copy_files) = &project.copy_files
            && !copy_files.trim().is_empty()
        {
            self.copy_project_files(&project.git_repo_path, &worktree_path, copy_files)
                .await
                .unwrap_or_else(|e| {
                    tracing::warn!("Failed to copy project files: {}", e);
                });
        }

        // Copy task images from cache to worktree
        if let Err(e) = self
            .image_service
            .copy_images_by_task_to_worktree(&worktree_path, task.id)
            .await
        {
            tracing::warn!("Failed to copy task images to worktree: {}", e);
        }

        // Update both container_ref and branch in the database
        TaskAttempt::update_container_ref(
            &self.db.pool,
            task_attempt.id,
            &worktree_path.to_string_lossy(),
        )
        .await?;

        let project_repositories =
            ProjectRepository::list_for_project(&self.db.pool, project.id).await?;
        let attempt_repositories =
            TaskAttemptRepository::list_for_attempt(&self.db.pool, task_attempt.id).await?;
        let attempt_repo_map: HashMap<Uuid, TaskAttemptRepository> = attempt_repositories
            .into_iter()
            .map(|entry| (entry.project_repository_id, entry))
            .collect();

        for repo in project_repositories {
            let attempt_repo = attempt_repo_map.get(&repo.id);

            let branch_to_use = attempt_repo
                .and_then(|entry| entry.branch.clone())
                .map(|b| b.trim().to_string())
                .filter(|b| !b.is_empty())
                .unwrap_or_else(|| task_attempt.branch.clone());

            let repo_worktree_path = if repo.is_primary {
                worktree_path.clone()
            } else {
                attempt_repo
                    .and_then(|entry| entry.container_ref.clone())
                    .map(PathBuf::from)
                    .unwrap_or_else(|| {
                        let suffix = Self::repo_worktree_suffix(&repo);
                        base_worktree_dir.join(format!("{worktree_dir_name}--{suffix}"))
                    })
            };

            if !repo.is_primary {
                WorktreeManager::create_worktree(
                    &repo.git_repo_path,
                    &branch_to_use,
                    &repo_worktree_path,
                    &task_attempt.target_branch,
                    true,
                )
                .await?;
            }

            let path_string = repo_worktree_path.to_string_lossy().to_string();

            TaskAttemptRepository::upsert_container_ref(
                &self.db.pool,
                task_attempt.id,
                repo.id,
                repo.is_primary,
                Some(path_string.as_str()),
            )
            .await?;

            TaskAttemptRepository::upsert_branch(
                &self.db.pool,
                task_attempt.id,
                repo.id,
                repo.is_primary,
                Some(branch_to_use.as_str()),
            )
            .await?;
        }

        Ok(worktree_path.to_string_lossy().to_string())
    }

    async fn delete_inner(&self, task_attempt: &TaskAttempt) -> Result<(), ContainerError> {
        // cleanup the container, here that means deleting the worktree
        let task = task_attempt
            .parent_task(&self.db.pool)
            .await?
            .ok_or(sqlx::Error::RowNotFound)?;
        let git_repo_path = match Project::find_by_id(&self.db.pool, task.project_id).await {
            Ok(Some(project)) => Some(project.git_repo_path.clone()),
            Ok(None) => None,
            Err(e) => {
                tracing::error!("Failed to fetch project {}: {}", task.project_id, e);
                None
            }
        };
        WorktreeManager::cleanup_worktree(
            &PathBuf::from(task_attempt.container_ref.clone().unwrap_or_default()),
            git_repo_path.as_deref(),
        )
        .await
        .unwrap_or_else(|e| {
            tracing::warn!(
                "Failed to clean up worktree for task attempt {}: {}",
                task_attempt.id,
                e
            );
        });
        Ok(())
    }

    async fn ensure_container_exists(
        &self,
        task_attempt: &TaskAttempt,
    ) -> Result<ContainerRef, ContainerError> {
        // Get required context
        let task = task_attempt
            .parent_task(&self.db.pool)
            .await?
            .ok_or(sqlx::Error::RowNotFound)?;

        let project = task
            .parent_project(&self.db.pool)
            .await?
            .ok_or(sqlx::Error::RowNotFound)?;

        let project_repositories =
            ProjectRepository::list_for_project(&self.db.pool, project.id).await?;
        let attempt_repositories =
            TaskAttemptRepository::list_for_attempt(&self.db.pool, task_attempt.id).await?;
        let attempt_repo_map: HashMap<Uuid, TaskAttemptRepository> = attempt_repositories
            .iter()
            .map(|entry| (entry.project_repository_id, entry.clone()))
            .collect();

        let primary_repo = project_repositories
            .iter()
            .find(|repo| {
                attempt_repo_map
                    .get(&repo.id)
                    .map(|entry| entry.is_primary)
                    .unwrap_or(repo.is_primary)
            })
            .cloned()
            .or_else(|| project_repositories.first().cloned())
            .ok_or_else(|| {
                ContainerError::Other(anyhow!(
                    "No repositories configured for project {}",
                    project.id
                ))
            })?;

        let attempt_entry = attempt_repo_map.get(&primary_repo.id);
        let (container_ref, _) = self
            .ensure_repository_container(task_attempt, &task, &primary_repo, attempt_entry)
            .await?;

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
        // Get the worktree path
        let container_ref = self.ensure_container_exists(task_attempt).await?;
        let current_dir = PathBuf::from(&container_ref);

        // Compute environment for executor processes
        let repo_env = self.build_executor_env(task_attempt).await?;

        let spawn_ctx = ExecutorSpawnContext {
            current_dir: &current_dir,
            env: Some(&repo_env),
        };

        // Create the child and stream, add to execution tracker
        let mut spawned = executor_action.spawn(&spawn_ctx).await?;

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
        status: ExecutionProcessStatus,
    ) -> Result<(), ContainerError> {
        let child = self
            .get_child_from_store(&execution_process.id)
            .await
            .ok_or_else(|| {
                ContainerError::Other(anyhow!("Child process not found for execution"))
            })?;
        let exit_code = if status == ExecutionProcessStatus::Completed {
            Some(0)
        } else {
            None
        };

        ExecutionProcess::update_completion(&self.db.pool, execution_process.id, status, exit_code)
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
        stats_only: bool,
        repository_filter: Option<Uuid>,
    ) -> Result<futures::stream::BoxStream<'static, Result<LogMsg, std::io::Error>>, ContainerError>
    {
        let task = task_attempt
            .parent_task(&self.db.pool)
            .await?
            .ok_or(ContainerError::Other(anyhow!("Parent task not found")))?;
        let project = task
            .parent_project(&self.db.pool)
            .await?
            .ok_or(ContainerError::Other(anyhow!("Parent project not found")))?;

        let project_repositories =
            ProjectRepository::list_for_project(&self.db.pool, project.id).await?;
        let attempt_repositories =
            TaskAttemptRepository::list_for_attempt(&self.db.pool, task_attempt.id).await?;
        let attempt_repo_map: HashMap<Uuid, TaskAttemptRepository> = attempt_repositories
            .iter()
            .map(|entry| (entry.project_repository_id, entry.clone()))
            .collect();

        let repo_lookup = Arc::new(RepositoryLookup::from_project_and_attempt(
            &project_repositories,
            &attempt_repositories,
        ));

        let selected_repo = if let Some(repo_id) = repository_filter {
            project_repositories
                .iter()
                .find(|repo| repo.id == repo_id)
                .cloned()
                .ok_or_else(|| {
                    ContainerError::Other(anyhow!(
                        "Repository {} not found for task attempt {}",
                        repo_id,
                        task_attempt.id
                    ))
                })?
        } else {
            project_repositories
                .iter()
                .find(|repo| {
                    attempt_repo_map
                        .get(&repo.id)
                        .map(|entry| entry.is_primary)
                        .unwrap_or(repo.is_primary)
                })
                .cloned()
                .or_else(|| project_repositories.first().cloned())
                .ok_or_else(|| {
                    ContainerError::Other(anyhow!(
                        "No repositories configured for project {}",
                        project.id
                    ))
                })?
        };

        let attempt_entry = attempt_repo_map.get(&selected_repo.id);
        let (container_ref, _) = self
            .ensure_repository_container(task_attempt, &task, &selected_repo, attempt_entry)
            .await?;
        let worktree_path = PathBuf::from(&container_ref);
        let project_repo_path = selected_repo.git_repo_path.clone();

        let latest_merge =
            Merge::find_latest_by_task_attempt_id(&self.db.pool, task_attempt.id).await?;

        let is_ahead = if let Ok((ahead, _)) = self.git().get_branch_status(
            &project_repo_path,
            &task_attempt.branch,
            &task_attempt.target_branch,
        ) {
            ahead > 0
        } else {
            false
        };

        if let Some(merge) = &latest_merge
            && let Some(commit) = merge.merge_commit()
            && self.is_container_clean(task_attempt).await?
            && !is_ahead
        {
            let wrapper = self.create_merged_diff_stream(
                &project_repo_path,
                &commit,
                stats_only,
                repository_filter,
                Arc::clone(&repo_lookup),
            )?;
            return Ok(Box::pin(wrapper));
        }

        let base_commit = self.git().get_base_commit(
            &project_repo_path,
            &task_attempt.branch,
            &task_attempt.target_branch,
        )?;

        let wrapper = self
            .create_live_diff_stream(
                &worktree_path,
                &base_commit,
                stats_only,
                repository_filter,
                repo_lookup,
            )
            .await?;
        Ok(Box::pin(wrapper))
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

        let container_ref = self.ensure_container_exists(&ctx.task_attempt).await?;

        tracing::debug!(
            "Committing changes for task attempt {} at path {:?}: '{}'",
            ctx.task_attempt.id,
            &container_ref,
            message
        );

        let changes_committed = self.git().commit(Path::new(&container_ref), &message)?;
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
        let Some(draft) = Draft::find_by_task_attempt_and_type(
            &self.db.pool,
            ctx.task_attempt.id,
            DraftType::FollowUp,
        )
        .await?
        else {
            return Ok(());
        };

        if !draft.queued || draft.prompt.trim().is_empty() {
            return Ok(());
        }

        // Atomically acquire sending lock; if not acquired, someone else is sending.
        if !Draft::try_mark_sending(&self.db.pool, ctx.task_attempt.id, DraftType::FollowUp)
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
            .and_then(|project| self.cleanup_action(project.cleanup_script));

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
        let _ =
            Draft::clear_after_send(&self.db.pool, ctx.task_attempt.id, DraftType::FollowUp).await;

        Ok(())
    }
}
