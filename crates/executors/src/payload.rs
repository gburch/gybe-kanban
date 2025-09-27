use std::collections::HashMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

/// Context for a repository that will be available inside executor processes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS, JsonSchema)]
pub struct ExecutorRepositoryContext {
    /// Identifier of the project repository.
    pub id: Uuid,
    /// Human-readable repository name.
    pub name: String,
    /// Slugified identifier (lowercase, hyphen-separated) for referencing this repo.
    pub slug: String,
    /// Absolute filesystem path to the worktree that the executor should operate on.
    pub worktree_path: String,
    /// Root path within the repository that should be treated as the execution root.
    pub root_path: String,
    /// Current branch checked out inside the worktree.
    pub branch: Option<String>,
    /// Whether this repository is the primary repository for the task.
    pub is_primary: bool,
}

/// Top-level payload shared with executor processes via environment variables.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS, JsonSchema)]
pub struct ExecutorPayload {
    /// Schema version for the payload; increment when breaking changes are introduced.
    pub version: u32,
    /// Identifier of the running task attempt.
    pub attempt_id: Uuid,
    /// Identifier of the primary repository for this attempt.
    pub primary_repository_id: Uuid,
    /// Repository metadata keyed by repository identifier.
    pub repositories: Vec<ExecutorRepositoryContext>,
    /// Environment variables that should be exported for the executor process.
    pub env: HashMap<String, String>,
}

impl ExecutorPayload {
    pub const CURRENT_VERSION: u32 = 1;
}
