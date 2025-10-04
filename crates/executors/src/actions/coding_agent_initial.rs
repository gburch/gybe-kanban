use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{
    actions::{Executable, ExecutorSpawnContext, repo_context::augment_prompt_with_repo_context},
    executors::{ExecutorError, SpawnedChild, StandardCodingAgentExecutor},
    profile::{ExecutorConfigs, ExecutorProfileId},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
pub struct CodingAgentInitialRequest {
    pub prompt: String,
    /// Executor profile specification
    #[serde(alias = "profile_variant_label")]
    // Backwards compatability with ProfileVariantIds, esp stored in DB under ExecutorAction
    pub executor_profile_id: ExecutorProfileId,
}

#[async_trait]
impl Executable for CodingAgentInitialRequest {
    async fn spawn(&self, ctx: &ExecutorSpawnContext<'_>) -> Result<SpawnedChild, ExecutorError> {
        let executor_profile_id = self.executor_profile_id.clone();
        let agent = ExecutorConfigs::get_cached()
            .get_coding_agent(&executor_profile_id)
            .ok_or(ExecutorError::UnknownExecutorType(
                executor_profile_id.to_string(),
            ))?;

        let prompt_with_context = augment_prompt_with_repo_context(&self.prompt, ctx.env);

        agent
            .spawn(ctx.current_dir, &prompt_with_context, ctx.env)
            .await
    }
}
