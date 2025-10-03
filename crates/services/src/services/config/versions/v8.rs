use anyhow::Error;
use executors::{executors::BaseCodingAgent, profile::ExecutorProfileId};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
pub use v7::{EditorConfig, EditorType, NotificationConfig, SoundFile, ThemeMode, UiLanguage};

use crate::services::config::versions::v7;

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub struct GitHubConfig {
    pub pat: Option<String>,
    pub oauth_token: Option<String>,
    pub username: Option<String>,
    pub primary_email: Option<String>,
    pub default_pr_base: Option<String>,
    #[serde(default)]
    pub branch_prefix: Option<String>,
    #[serde(default)]
    pub merge_commit_message_suffix: Option<String>,
}

impl GitHubConfig {
    pub const DEFAULT_BRANCH_PREFIX: &'static str = "vk/";
    pub const DEFAULT_MERGE_COMMIT_SUFFIX: &'static str = "(vibe-kanban {short_id})";

    pub fn token(&self) -> Option<String> {
        self.pat
            .as_deref()
            .or(self.oauth_token.as_deref())
            .map(|s| s.to_string())
    }

    pub fn resolved_branch_prefix(&self) -> String {
        match self.branch_prefix.as_ref() {
            Some(raw) => raw.trim().to_string(),
            None => Self::DEFAULT_BRANCH_PREFIX.to_string(),
        }
    }

    pub fn format_merge_commit_suffix(&self, short_id: &str, task_id: &str) -> Option<String> {
        let template = self.merge_commit_message_suffix.as_ref()?;

        if template.trim().is_empty() {
            return None;
        }

        let mut formatted = template.replace("{short_id}", short_id);
        formatted = formatted.replace("{SHORT_ID}", &short_id.to_uppercase());
        formatted = formatted.replace("{task_id}", task_id);
        formatted = formatted.replace("{TASK_ID}", &task_id.to_uppercase());

        Some(formatted)
    }
}

impl Default for GitHubConfig {
    fn default() -> Self {
        Self {
            pat: None,
            oauth_token: None,
            username: None,
            primary_email: None,
            default_pr_base: Some("main".to_string()),
            branch_prefix: Some(Self::DEFAULT_BRANCH_PREFIX.to_string()),
            merge_commit_message_suffix: Some(Self::DEFAULT_MERGE_COMMIT_SUFFIX.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_config() -> GitHubConfig {
        GitHubConfig {
            branch_prefix: None,
            merge_commit_message_suffix: None,
            ..GitHubConfig::default()
        }
    }

    #[test]
    fn resolved_branch_prefix_uses_default_when_missing() {
        let config = base_config();

        assert_eq!(
            config.resolved_branch_prefix(),
            GitHubConfig::DEFAULT_BRANCH_PREFIX
        );
    }

    #[test]
    fn resolved_branch_prefix_trims_and_preserves_empty() {
        let mut config = base_config();
        config.branch_prefix = Some("  ".into());

        assert_eq!(config.resolved_branch_prefix(), "");

        config.branch_prefix = Some(" greg ".into());
        assert_eq!(config.resolved_branch_prefix(), "greg");
    }

    #[test]
    fn format_merge_commit_suffix_substitutes_placeholders() {
        let mut config = base_config();
        config.merge_commit_message_suffix = Some("(gb {short_id} {TASK_ID})".into());

        let formatted = config
            .format_merge_commit_suffix("abcd", "1234-5678")
            .expect("suffix should be Some");

        assert_eq!(formatted, "(gb abcd 1234-5678)");
    }

    #[test]
    fn format_merge_commit_suffix_returns_none_when_blank() {
        let mut config = base_config();
        config.merge_commit_message_suffix = Some("   ".into());

        assert!(
            config
                .format_merge_commit_suffix("abcd", "1234-5678")
                .is_none()
        );
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub struct ActivityFeedConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "ActivityFeedConfig::default_window")]
    pub window_days: u16,
}

impl ActivityFeedConfig {
    const DEFAULT_WINDOW_DAYS: u16 = 21;

    const fn default_window() -> u16 {
        Self::DEFAULT_WINDOW_DAYS
    }
}

impl Default for ActivityFeedConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            window_days: Self::DEFAULT_WINDOW_DAYS,
        }
    }
}

impl From<v7::GitHubConfig> for GitHubConfig {
    fn from(old: v7::GitHubConfig) -> Self {
        Self {
            pat: old.pat,
            oauth_token: old.oauth_token,
            username: old.username,
            primary_email: old.primary_email,
            default_pr_base: old.default_pr_base,
            branch_prefix: Some(Self::DEFAULT_BRANCH_PREFIX.to_string()),
            merge_commit_message_suffix: Some(Self::DEFAULT_MERGE_COMMIT_SUFFIX.to_string()),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub struct Config {
    pub config_version: String,
    pub theme: ThemeMode,
    pub executor_profile: ExecutorProfileId,
    pub disclaimer_acknowledged: bool,
    pub onboarding_acknowledged: bool,
    pub github_login_acknowledged: bool,
    pub telemetry_acknowledged: bool,
    pub notifications: NotificationConfig,
    pub editor: EditorConfig,
    pub github: GitHubConfig,
    pub analytics_enabled: Option<bool>,
    pub workspace_dir: Option<String>,
    pub last_app_version: Option<String>,
    pub show_release_notes: bool,
    #[serde(default)]
    pub language: UiLanguage,
    #[serde(default)]
    pub activity_feed: ActivityFeedConfig,
}

impl Config {
    pub fn from_previous_version(raw_config: &str) -> Result<Self, Error> {
        let old_config = match serde_json::from_str::<v7::Config>(raw_config) {
            Ok(cfg) => cfg,
            Err(e) => {
                tracing::error!("❌ Failed to parse config: {}", e);
                tracing::error!("   at line {}, column {}", e.line(), e.column());
                return Err(e.into());
            }
        };

        Ok(Self {
            config_version: "v8".to_string(),
            theme: old_config.theme,
            executor_profile: old_config.executor_profile,
            disclaimer_acknowledged: old_config.disclaimer_acknowledged,
            onboarding_acknowledged: old_config.onboarding_acknowledged,
            github_login_acknowledged: old_config.github_login_acknowledged,
            telemetry_acknowledged: old_config.telemetry_acknowledged,
            notifications: old_config.notifications,
            editor: old_config.editor,
            github: GitHubConfig::from(old_config.github),
            analytics_enabled: old_config.analytics_enabled,
            workspace_dir: old_config.workspace_dir,
            last_app_version: old_config.last_app_version,
            show_release_notes: old_config.show_release_notes,
            language: old_config.language,
            activity_feed: ActivityFeedConfig::default(),
        })
    }
}

impl From<String> for Config {
    fn from(raw_config: String) -> Self {
        if let Ok(config) = serde_json::from_str::<Config>(&raw_config)
            && config.config_version == "v8"
        {
            return config;
        }

        match Self::from_previous_version(&raw_config) {
            Ok(config) => {
                tracing::info!("Config upgraded to v8");
                config
            }
            Err(e) => {
                tracing::warn!("Config migration failed: {}, using default", e);
                Self::default()
            }
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            config_version: "v8".to_string(),
            theme: ThemeMode::System,
            executor_profile: ExecutorProfileId::new(BaseCodingAgent::ClaudeCode),
            disclaimer_acknowledged: false,
            onboarding_acknowledged: false,
            github_login_acknowledged: false,
            telemetry_acknowledged: false,
            notifications: NotificationConfig::default(),
            editor: EditorConfig::default(),
            github: GitHubConfig::default(),
            analytics_enabled: None,
            workspace_dir: None,
            last_app_version: None,
            show_release_notes: false,
            language: UiLanguage::default(),
            activity_feed: ActivityFeedConfig::default(),
        }
    }
}
