use std::{path::Path, sync::Arc};

use async_trait::async_trait;
use enum_dispatch::enum_dispatch;
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::{
    actions::{
        coding_agent_follow_up::CodingAgentFollowUpRequest,
        coding_agent_initial::CodingAgentInitialRequest, script::ScriptRequest,
    },
    executors::{ExecutorError, SpawnedChild},
    payload::ExecutorPayload,
};
use tokio::process::Command as TokioCommand;
pub mod coding_agent_follow_up;
pub mod coding_agent_initial;
pub mod script;

#[derive(Debug, Clone)]
pub struct ExecutorPayloadEnvelope {
    pub payload: Arc<ExecutorPayload>,
    pub payload_json: Arc<String>,
}

impl ExecutorPayloadEnvelope {
    pub fn try_new(payload: ExecutorPayload) -> Result<Self, serde_json::Error> {
        let json = serde_json::to_string(&payload)?;
        Ok(Self {
            payload: Arc::new(payload),
            payload_json: Arc::new(json),
        })
    }
}

#[enum_dispatch]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(tag = "type")]
pub enum ExecutorActionType {
    CodingAgentInitialRequest,
    CodingAgentFollowUpRequest,
    ScriptRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ExecutorAction {
    pub typ: ExecutorActionType,
    pub next_action: Option<Box<ExecutorAction>>,
}

impl ExecutorAction {
    pub fn new(typ: ExecutorActionType, next_action: Option<Box<ExecutorAction>>) -> Self {
        Self { typ, next_action }
    }

    pub fn typ(&self) -> &ExecutorActionType {
        &self.typ
    }

    pub fn next_action(&self) -> Option<&ExecutorAction> {
        self.next_action.as_deref()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ExecutorSpawnContext<'a> {
    pub current_dir: &'a Path,
    pub task_attempt_id: Option<&'a Uuid>,
    pub payload: &'a ExecutorPayloadEnvelope,
}

impl<'a> ExecutorSpawnContext<'a> {
    pub fn apply_environment(&self, command: &mut TokioCommand) {
        if let Some(attempt_id) = self.task_attempt_id {
            command.env("VIBE_PARENT_TASK_ATTEMPT_ID", attempt_id.to_string());
        }

        command.env("VIBE_EXECUTOR_PAYLOAD", self.payload.payload_json.as_str());

        for (key, value) in &self.payload.payload.env {
            command.env(key, value);
        }
    }
}

#[async_trait]
#[enum_dispatch(ExecutorActionType)]
pub trait Executable {
    async fn spawn(&self, ctx: ExecutorSpawnContext<'_>) -> Result<SpawnedChild, ExecutorError>;
}

#[async_trait]
impl Executable for ExecutorAction {
    async fn spawn(&self, ctx: ExecutorSpawnContext<'_>) -> Result<SpawnedChild, ExecutorError> {
        self.typ.spawn(ctx).await
    }
}
