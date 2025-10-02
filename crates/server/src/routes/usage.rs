use std::{
    fs::File,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    time::SystemTime,
};

use axum::{Router, response::Json as ResponseJson, routing::get};
use chrono::{DateTime, Timelike, Utc};
use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};
use tokio::task;
use tracing::warn;
use ts_rs::TS;

use crate::{DeploymentImpl, error::ApiError};

use utils::response::ApiResponse;

pub fn router() -> Router<DeploymentImpl> {
    Router::new()
        .route("/usage/codex", get(get_codex_usage))
        .route("/usage/claude-code", get(get_claude_code_usage))
}

#[derive(Debug, Clone, TS, serde::Serialize)]
#[ts(export)]
pub struct CodexUsageSnapshot {
    pub captured_at: String,
    pub rate_limits: CodexUsageRateLimits,
    pub token_usage: Option<CodexTokenUsageInfo>,
}

#[derive(Debug, Clone, Default, TS, serde::Serialize)]
#[ts(export)]
pub struct CodexUsageRateLimits {
    pub primary: Option<CodexUsageWindow>,
    pub secondary: Option<CodexUsageWindow>,
}

#[derive(Debug, Clone, TS, serde::Serialize)]
#[ts(export)]
pub struct CodexUsageWindow {
    pub used_percent: f64,
    #[ts(type = "number | null")]
    pub window_minutes: Option<u64>,
    #[ts(type = "number | null")]
    pub resets_in_seconds: Option<u64>,
}

#[derive(Debug, Clone, TS, serde::Serialize)]
#[ts(export)]
pub struct CodexTokenUsageInfo {
    pub total_token_usage: CodexTokenUsage,
    pub last_token_usage: CodexTokenUsage,
    #[ts(type = "number | null")]
    pub model_context_window: Option<u64>,
}

#[derive(Debug, Clone, TS, serde::Serialize)]
#[ts(export)]
pub struct CodexTokenUsage {
    #[ts(type = "number")]
    pub input_tokens: u64,
    #[ts(type = "number")]
    pub cached_input_tokens: u64,
    #[ts(type = "number")]
    pub output_tokens: u64,
    #[ts(type = "number")]
    pub reasoning_output_tokens: u64,
    #[ts(type = "number")]
    pub total_tokens: u64,
}

pub async fn get_codex_usage()
-> Result<ResponseJson<ApiResponse<Option<CodexUsageSnapshot>>>, ApiError> {
    let snapshot = task::spawn_blocking(collect_codex_usage)
        .await
        .map_err(|err| {
            warn!("failed to join codex usage task: {err}");
            std::io::Error::new(std::io::ErrorKind::Other, "codex usage task failed")
        })??;

    Ok(ResponseJson(ApiResponse::success(snapshot)))
}

fn collect_codex_usage() -> std::io::Result<Option<CodexUsageSnapshot>> {
    let Some(home) = dirs::home_dir() else {
        return Ok(None);
    };

    let sessions_dir = home.join(".codex").join("sessions");
    if !sessions_dir.exists() {
        return Ok(None);
    }

    let mut candidates: Vec<(SystemTime, PathBuf)> = Vec::new();
    for entry in WalkBuilder::new(&sessions_dir)
        .hidden(false)
        .ignore(false)
        .git_ignore(false)
        .git_exclude(false)
        .build()
    {
        let entry = match entry {
            Ok(e) => e,
            Err(err) => {
                warn!("failed to read codex session entry: {err}");
                continue;
            }
        };

        if !entry
            .file_type()
            .map(|file_type| file_type.is_file())
            .unwrap_or(false)
        {
            continue;
        }

        let file_name = entry.file_name().to_string_lossy();
        if !file_name.starts_with("rollout-") || !file_name.ends_with(".jsonl") {
            continue;
        }

        let metadata = match entry.metadata() {
            Ok(data) => data,
            Err(err) => {
                warn!(
                    "failed to read metadata for {}: {err}",
                    entry.path().display()
                );
                continue;
            }
        };

        let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        candidates.push((modified, entry.into_path()));
    }

    if candidates.is_empty() {
        return Ok(None);
    }

    candidates.sort_by(|a, b| b.0.cmp(&a.0));

    let mut latest: Option<(DateTime<Utc>, CodexUsageSnapshot)> = None;

    for (_, path) in candidates {
        match parse_rollout_file(&path) {
            Ok(Some((timestamp, snapshot))) => {
                if latest
                    .as_ref()
                    .map(|(current, _)| timestamp > *current)
                    .unwrap_or(true)
                {
                    latest = Some((timestamp, snapshot));
                }
                break;
            }
            Ok(None) => continue,
            Err(err) => {
                warn!("failed to parse codex rollout {}: {err}", path.display());
            }
        }
    }

    Ok(latest.map(|(_, snapshot)| snapshot))
}

fn parse_rollout_file(path: &Path) -> std::io::Result<Option<(DateTime<Utc>, CodexUsageSnapshot)>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let mut best: Option<(DateTime<Utc>, CodexUsageSnapshot)> = None;

    for line in reader.lines() {
        let line = match line {
            Ok(line) => line,
            Err(err) => {
                warn!("failed to read line in {}: {err}", path.display());
                continue;
            }
        };

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let parsed: RolloutLine = match serde_json::from_str(trimmed) {
            Ok(value) => value,
            Err(err) => {
                warn!(
                    "failed to parse rollout JSON line in {}: {err}",
                    path.display()
                );
                continue;
            }
        };

        let RolloutItem::EventMsg(payload) = parsed.item else {
            continue;
        };

        let Some(token_event) = payload.into_token_count() else {
            continue;
        };

        let timestamp = match DateTime::parse_from_rfc3339(&parsed.timestamp) {
            Ok(dt) => dt.with_timezone(&Utc),
            Err(err) => {
                warn!(
                    "failed to parse timestamp '{}' in {}: {err}",
                    parsed.timestamp,
                    path.display()
                );
                continue;
            }
        };

        let rate_limits = token_event
            .rate_limits
            .and_then(RateLimitSnapshot::into_usage_rate_limits)
            .unwrap_or_default();

        let snapshot = CodexUsageSnapshot {
            captured_at: timestamp.to_rfc3339(),
            rate_limits,
            token_usage: token_event.info.map(CodexTokenUsageInfo::from),
        };

        if snapshot.rate_limits.primary.is_none() && snapshot.rate_limits.secondary.is_none() {
            continue;
        }

        if best
            .as_ref()
            .map(|(current, _)| timestamp > *current)
            .unwrap_or(true)
        {
            best = Some((timestamp, snapshot));
        }
    }

    Ok(best)
}

#[derive(Debug, Deserialize, Serialize)]
struct RolloutLine {
    timestamp: String,
    #[serde(flatten)]
    item: RolloutItem,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
enum RolloutItem {
    EventMsg(EventMsg),
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum EventMsg {
    TokenCount(TokenCountEvent),
    #[serde(other)]
    Other,
}

impl EventMsg {
    fn into_token_count(self) -> Option<TokenCountEvent> {
        match self {
            EventMsg::TokenCount(event) => Some(event),
            EventMsg::Other => None,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct TokenCountEvent {
    info: Option<TokenUsageInfo>,
    rate_limits: Option<RateLimitSnapshot>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct TokenUsageInfo {
    total_token_usage: TokenUsage,
    last_token_usage: TokenUsage,
    model_context_window: Option<u64>,
}

impl From<TokenUsageInfo> for CodexTokenUsageInfo {
    fn from(value: TokenUsageInfo) -> Self {
        CodexTokenUsageInfo {
            total_token_usage: value.total_token_usage.into(),
            last_token_usage: value.last_token_usage.into(),
            model_context_window: value.model_context_window,
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct TokenUsage {
    input_tokens: u64,
    cached_input_tokens: u64,
    output_tokens: u64,
    reasoning_output_tokens: u64,
    total_tokens: u64,
}

impl From<TokenUsage> for CodexTokenUsage {
    fn from(value: TokenUsage) -> Self {
        CodexTokenUsage {
            input_tokens: value.input_tokens,
            cached_input_tokens: value.cached_input_tokens,
            output_tokens: value.output_tokens,
            reasoning_output_tokens: value.reasoning_output_tokens,
            total_tokens: value.total_tokens,
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
struct RateLimitSnapshot {
    #[serde(default)]
    primary: Option<RateLimitWindow>,
    #[serde(default)]
    secondary: Option<RateLimitWindow>,
    #[serde(default)]
    primary_used_percent: Option<f64>,
    #[serde(default)]
    secondary_used_percent: Option<f64>,
    #[serde(default)]
    primary_window_minutes: Option<u64>,
    #[serde(default)]
    secondary_window_minutes: Option<u64>,
    #[serde(default)]
    primary_resets_in_seconds: Option<u64>,
    #[serde(default)]
    secondary_resets_in_seconds: Option<u64>,
    #[serde(flatten, default)]
    extra: std::collections::HashMap<String, serde_json::Value>,
}

impl RateLimitSnapshot {
    fn into_usage_rate_limits(self) -> Option<CodexUsageRateLimits> {
        let RateLimitSnapshot {
            primary,
            secondary,
            primary_used_percent,
            secondary_used_percent,
            primary_window_minutes,
            secondary_window_minutes,
            primary_resets_in_seconds,
            secondary_resets_in_seconds,
            extra,
        } = self;

        let primary = primary
            .map(CodexUsageWindow::from)
            .or_else(|| {
                primary_used_percent.map(|used_percent| CodexUsageWindow {
                    used_percent,
                    window_minutes: primary_window_minutes,
                    resets_in_seconds: primary_resets_in_seconds,
                })
            })
            .or_else(|| extract_header_window(&extra, "primary"));

        let secondary = secondary
            .map(CodexUsageWindow::from)
            .or_else(|| {
                secondary_used_percent.map(|used_percent| CodexUsageWindow {
                    used_percent,
                    window_minutes: secondary_window_minutes,
                    resets_in_seconds: secondary_resets_in_seconds,
                })
            })
            .or_else(|| extract_header_window(&extra, "secondary"));

        if primary.is_none() && secondary.is_none() {
            None
        } else {
            Some(CodexUsageRateLimits { primary, secondary })
        }
    }
}

fn extract_header_window(
    extra: &std::collections::HashMap<String, serde_json::Value>,
    prefix: &str,
) -> Option<CodexUsageWindow> {
    let used_percent = extract_f64(extra.get(&format!("x-codex-{}-used-percent", prefix)))?;
    let window_minutes = extract_u64(extra.get(&format!("x-codex-{}-window-minutes", prefix)));
    let resets_in_seconds =
        extract_u64(extra.get(&format!("x-codex-{}-reset-after-seconds", prefix)))
            .or_else(|| extract_u64(extra.get(&format!("x-codex-{}-resets-in-seconds", prefix))));

    Some(CodexUsageWindow {
        used_percent,
        window_minutes,
        resets_in_seconds,
    })
}

fn extract_f64(value: Option<&serde_json::Value>) -> Option<f64> {
    match value? {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => s.parse().ok(),
        serde_json::Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

fn extract_u64(value: Option<&serde_json::Value>) -> Option<u64> {
    match value? {
        serde_json::Value::Number(n) => n.as_u64().or_else(|| n.as_f64().map(|f| f as u64)),
        serde_json::Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct RateLimitWindow {
    used_percent: f64,
    window_minutes: Option<u64>,
    resets_in_seconds: Option<u64>,
}

impl From<RateLimitWindow> for CodexUsageWindow {
    fn from(value: RateLimitWindow) -> Self {
        CodexUsageWindow {
            used_percent: value.used_percent,
            window_minutes: value.window_minutes,
            resets_in_seconds: value.resets_in_seconds,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn parses_latest_token_count_event() {
        let dir = tempdir().unwrap();
        let sessions_dir = dir.path().join("sessions/2025/09/28");
        fs::create_dir_all(&sessions_dir).unwrap();
        let file_path = sessions_dir.join("rollout-2025-09-28T12-00-00-session.jsonl");

        let token_line = RolloutLine {
            timestamp: "2025-09-28T12:00:05.000000Z".to_string(),
            item: RolloutItem::EventMsg(EventMsg::TokenCount(TokenCountEvent {
                info: Some(TokenUsageInfo {
                    total_token_usage: TokenUsage {
                        input_tokens: 100,
                        cached_input_tokens: 10,
                        output_tokens: 50,
                        reasoning_output_tokens: 5,
                        total_tokens: 165,
                    },
                    last_token_usage: TokenUsage {
                        input_tokens: 20,
                        cached_input_tokens: 0,
                        output_tokens: 10,
                        reasoning_output_tokens: 2,
                        total_tokens: 32,
                    },
                    model_context_window: Some(128_000),
                }),
                rate_limits: Some(RateLimitSnapshot {
                    primary: Some(RateLimitWindow {
                        used_percent: 42.0,
                        window_minutes: Some(60),
                        resets_in_seconds: Some(1800),
                    }),
                    secondary: Some(RateLimitWindow {
                        used_percent: 5.0,
                        window_minutes: Some(1),
                        resets_in_seconds: Some(30),
                    }),
                    ..Default::default()
                }),
            })),
        };

        let lines = vec![serde_json::to_string(&token_line).unwrap()];

        fs::write(&file_path, lines.join("\n")).unwrap();

        let result = parse_rollout_file(&file_path).unwrap();
        assert!(result.is_some());
        let (timestamp, snapshot) = result.unwrap();
        assert_eq!(timestamp.to_rfc3339(), "2025-09-28T12:00:05+00:00");
        assert_eq!(
            snapshot.rate_limits.primary.as_ref().unwrap().used_percent,
            42.0
        );
        assert_eq!(
            snapshot
                .rate_limits
                .secondary
                .as_ref()
                .unwrap()
                .window_minutes,
            Some(1)
        );
        assert!(snapshot.token_usage.is_some());
    }

    #[test]
    fn parses_flattened_rate_limit_snapshot() {
        let dir = tempdir().unwrap();
        let sessions_dir = dir.path().join("sessions/2025/09/29");
        fs::create_dir_all(&sessions_dir).unwrap();
        let file_path = sessions_dir.join("rollout-2025-09-29T12-00-00-session.jsonl");

        let json_line = serde_json::json!({
            "timestamp": "2025-09-29T12:00:05.000000Z",
            "type": "event_msg",
            "payload": {
                "type": "token_count",
                "info": {
                    "total_token_usage": {
                        "input_tokens": 200,
                        "cached_input_tokens": 20,
                        "output_tokens": 80,
                        "reasoning_output_tokens": 10,
                        "total_tokens": 310
                    },
                    "last_token_usage": {
                        "input_tokens": 40,
                        "cached_input_tokens": 0,
                        "output_tokens": 20,
                        "reasoning_output_tokens": 4,
                        "total_tokens": 64
                    },
                    "model_context_window": 256000
                },
                "rate_limits": {
                    "primary_used_percent": 12.5,
                    "secondary_used_percent": 2.5,
                    "primary_window_minutes": 300,
                    "secondary_window_minutes": 10080,
                    "primary_resets_in_seconds": 1800,
                    "secondary_resets_in_seconds": 7200
                }
            }
        });

        fs::write(&file_path, format!("{}\n", json_line)).unwrap();

        let result = parse_rollout_file(&file_path).unwrap();
        assert!(
            result.is_some(),
            "flattened snapshot should parse: {result:?}"
        );
        let (_, snapshot) = result.unwrap();

        let primary = snapshot
            .rate_limits
            .primary
            .expect("primary window should exist");
        assert_eq!(primary.used_percent, 12.5);
        assert_eq!(primary.window_minutes, Some(300));
        assert_eq!(primary.resets_in_seconds, Some(1800));

        let secondary = snapshot
            .rate_limits
            .secondary
            .expect("secondary window should exist");
        assert_eq!(secondary.used_percent, 2.5);
        assert_eq!(secondary.window_minutes, Some(10080));
        assert_eq!(secondary.resets_in_seconds, Some(7200));
    }

    #[test]
    fn skips_entries_without_rate_limits() {
        let dir = tempdir().unwrap();
        let sessions_dir = dir.path().join("sessions/2025/10/02");
        fs::create_dir_all(&sessions_dir).unwrap();
        let file_path = sessions_dir.join("rollout-2025-10-02T08-00-00-session.jsonl");

        let lines = [
            serde_json::json!({
                "timestamp": "2025-10-02T08:00:01.000000Z",
                "type": "event_msg",
                "payload": {
                    "type": "token_count",
                    "info": serde_json::Value::Null,
                    "rate_limits": serde_json::Value::Null
                }
            }),
            serde_json::json!({
                "timestamp": "2025-10-02T08:00:02.000000Z",
                "type": "event_msg",
                "payload": {
                    "type": "token_count",
                    "info": serde_json::Value::Null,
                    "rate_limits": {
                        "primary_used_percent": 10.0,
                        "primary_window_minutes": 5,
                        "primary_resets_in_seconds": 30
                    }
                }
            }),
        ];

        let body = lines
            .iter()
            .map(|line| serde_json::to_string(line).unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&file_path, format!("{}\n", body)).unwrap();

        let result = parse_rollout_file(&file_path).unwrap();
        let (_, snapshot) = result.expect("second entry should produce snapshot");
        assert!(snapshot.rate_limits.primary.is_some());
        assert!(snapshot.rate_limits.secondary.is_none());
    }

    #[test]
    fn parses_header_style_rate_limit_snapshot() {
        let dir = tempdir().unwrap();
        let sessions_dir = dir.path().join("sessions/2025/09/30");
        fs::create_dir_all(&sessions_dir).unwrap();
        let file_path = sessions_dir.join("rollout-2025-09-30T12-00-00-session.jsonl");

        let json_line = serde_json::json!({
            "timestamp": "2025-09-30T12:00:05.000000Z",
            "type": "event_msg",
            "payload": {
                "type": "token_count",
                "info": serde_json::Value::Null,
                "rate_limits": {
                    "x-codex-primary-used-percent": "87.5",
                    "x-codex-primary-window-minutes": "15",
                    "x-codex-primary-reset-after-seconds": "120",
                    "x-codex-secondary-used-percent": 5.25,
                    "x-codex-secondary-window-minutes": 60,
                    "x-codex-secondary-reset-after-seconds": 3600
                }
            }
        });

        fs::write(&file_path, format!("{}\n", json_line)).unwrap();

        let result = parse_rollout_file(&file_path).unwrap();
        let (_, snapshot) = result.expect("header style snapshot should parse");

        let primary = snapshot
            .rate_limits
            .primary
            .expect("primary window should exist");
        assert_eq!(primary.used_percent, 87.5);
        assert_eq!(primary.window_minutes, Some(15));
        assert_eq!(primary.resets_in_seconds, Some(120));

        let secondary = snapshot
            .rate_limits
            .secondary
            .expect("secondary window should exist");
        assert!((secondary.used_percent - 5.25).abs() < f64::EPSILON);
        assert_eq!(secondary.window_minutes, Some(60));
        assert_eq!(secondary.resets_in_seconds, Some(3600));
    }
}

// ============================================================================
// Claude Code Usage Tracking
// ============================================================================

#[derive(Debug, Clone, TS, serde::Serialize)]
#[ts(export)]
pub struct ClaudeCodeUsageSnapshot {
    pub captured_at: String,
    pub session_info: ClaudeCodeSessionInfo,
    pub token_usage: ClaudeCodeTokenUsage,
    #[ts(type = "number")]
    pub estimated_limit: u64,
    pub used_percent: f64,
}

#[derive(Debug, Clone, TS, serde::Serialize)]
#[ts(export)]
pub struct ClaudeCodeSessionInfo {
    pub session_id: String,
    pub version: String,
    pub git_branch: Option<String>,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Default, TS, serde::Serialize)]
#[ts(export)]
pub struct ClaudeCodeTokenUsage {
    #[ts(type = "number")]
    pub input_tokens: u64,
    #[ts(type = "number")]
    pub cache_creation_input_tokens: u64,
    #[ts(type = "number")]
    pub cache_read_input_tokens: u64,
    #[ts(type = "number")]
    pub output_tokens: u64,
    #[ts(type = "number")]
    pub total_tokens: u64,
}

pub async fn get_claude_code_usage()
-> Result<ResponseJson<ApiResponse<Option<ClaudeCodeUsageSnapshot>>>, ApiError> {
    // Load config to get the Claude plan
    let config_path = utils::assets::config_path();
    let config = services::services::config::load_config_from_file(&config_path).await;
    let estimated_limit = config.claude_plan.token_limit_per_5h_block();

    let snapshot = task::spawn_blocking(move || collect_claude_code_usage(estimated_limit))
        .await
        .map_err(|err| {
            warn!("failed to join claude code usage task: {err}");
            std::io::Error::new(std::io::ErrorKind::Other, "claude code usage task failed")
        })??;

    Ok(ResponseJson(ApiResponse::success(snapshot)))
}

fn collect_claude_code_usage(
    estimated_limit: u64,
) -> std::io::Result<Option<ClaudeCodeUsageSnapshot>> {
    let Some(home) = dirs::home_dir() else {
        return Ok(None);
    };

    let projects_dir = home.join(".claude").join("projects");
    if !projects_dir.exists() {
        return Ok(None);
    }

    let mut candidates: Vec<(SystemTime, PathBuf)> = Vec::new();
    for entry in WalkBuilder::new(&projects_dir)
        .hidden(false)
        .ignore(false)
        .git_ignore(false)
        .git_exclude(false)
        .build()
    {
        let entry = match entry {
            Ok(e) => e,
            Err(err) => {
                warn!("failed to read claude code project entry: {err}");
                continue;
            }
        };

        if !entry
            .file_type()
            .map(|file_type| file_type.is_file())
            .unwrap_or(false)
        {
            continue;
        }

        let file_name = entry.file_name().to_string_lossy();
        if !file_name.ends_with(".jsonl") {
            continue;
        }

        let metadata = match entry.metadata() {
            Ok(data) => data,
            Err(err) => {
                warn!(
                    "failed to read metadata for {}: {err}",
                    entry.path().display()
                );
                continue;
            }
        };

        let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        candidates.push((modified, entry.into_path()));
    }

    if candidates.is_empty() {
        return Ok(None);
    }

    // Sort by modification time, newest first
    candidates.sort_by(|a, b| b.0.cmp(&a.0));

    let mut latest: Option<(DateTime<Utc>, ClaudeCodeUsageSnapshot)> = None;

    // Check the most recent files
    for (_, path) in candidates.iter().take(20) {
        match parse_claude_code_file(path, estimated_limit) {
            Ok(Some((timestamp, snapshot))) => {
                if latest
                    .as_ref()
                    .map(|(current, _)| timestamp > *current)
                    .unwrap_or(true)
                {
                    latest = Some((timestamp, snapshot));
                }
            }
            Ok(None) => continue,
            Err(err) => {
                warn!("failed to parse claude code log {}: {err}", path.display());
            }
        }
    }

    Ok(latest.map(|(_, snapshot)| snapshot))
}

fn get_five_hour_block_start(timestamp: &DateTime<Utc>) -> DateTime<Utc> {
    let hour = timestamp.hour();
    let block_number = hour / 5;
    let block_start_hour = block_number * 5;

    timestamp
        .date_naive()
        .and_hms_opt(block_start_hour, 0, 0)
        .unwrap()
        .and_utc()
}

fn parse_claude_code_file(
    path: &Path,
    estimated_limit: u64,
) -> std::io::Result<Option<(DateTime<Utc>, ClaudeCodeUsageSnapshot)>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let mut best: Option<(DateTime<Utc>, ClaudeCodeUsageSnapshot)> = None;
    let mut session_info: Option<ClaudeCodeSessionInfo> = None;
    let mut current_block_start: Option<DateTime<Utc>> = None;
    let mut accumulated_usage = ClaudeCodeTokenUsage::default();

    for line in reader.lines() {
        let line = match line {
            Ok(line) => line,
            Err(err) => {
                warn!("failed to read line in {}: {err}", path.display());
                continue;
            }
        };

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let parsed: ClaudeCodeLogLine = match serde_json::from_str(trimmed) {
            Ok(value) => value,
            Err(err) => {
                warn!(
                    "failed to parse claude code JSON line in {}: {err}",
                    path.display()
                );
                continue;
            }
        };

        // Extract session info from first valid entry
        if session_info.is_none() {
            session_info = Some(ClaudeCodeSessionInfo {
                session_id: parsed.session_id.clone(),
                version: parsed.version.clone(),
                git_branch: parsed.git_branch.clone(),
                cwd: parsed.cwd.clone(),
            });
        }

        // Only process assistant messages with usage data
        if parsed.type_field == "assistant" {
            if let Some(message) = parsed.message {
                if let Some(usage) = message.usage {
                    let timestamp = match DateTime::parse_from_rfc3339(&parsed.timestamp) {
                        Ok(dt) => dt.with_timezone(&Utc),
                        Err(err) => {
                            warn!(
                                "failed to parse timestamp '{}' in {}: {err}",
                                parsed.timestamp,
                                path.display()
                            );
                            continue;
                        }
                    };

                    // Determine which 5-hour block this timestamp belongs to
                    let block_start = get_five_hour_block_start(&timestamp);

                    // If we've moved to a new block, reset the accumulated usage
                    if current_block_start.map_or(true, |start| start != block_start) {
                        current_block_start = Some(block_start);
                        accumulated_usage = ClaudeCodeTokenUsage::default();
                    }

                    // Accumulate token usage within the current block
                    accumulated_usage.input_tokens += usage.input_tokens.unwrap_or(0);
                    accumulated_usage.cache_creation_input_tokens +=
                        usage.cache_creation_input_tokens.unwrap_or(0);
                    accumulated_usage.cache_read_input_tokens +=
                        usage.cache_read_input_tokens.unwrap_or(0);
                    accumulated_usage.output_tokens += usage.output_tokens.unwrap_or(0);

                    // Calculate total
                    accumulated_usage.total_tokens =
                        accumulated_usage.input_tokens + accumulated_usage.output_tokens;

                    if let Some(ref info) = session_info {
                        let used_percent = if estimated_limit > 0 {
                            (accumulated_usage.total_tokens as f64 / estimated_limit as f64) * 100.0
                        } else {
                            0.0
                        };

                        let snapshot = ClaudeCodeUsageSnapshot {
                            captured_at: timestamp.to_rfc3339(),
                            session_info: info.clone(),
                            token_usage: accumulated_usage.clone(),
                            estimated_limit,
                            used_percent,
                        };

                        if best
                            .as_ref()
                            .map(|(current, _)| timestamp > *current)
                            .unwrap_or(true)
                        {
                            best = Some((timestamp, snapshot));
                        }
                    }
                }
            }
        }
    }

    Ok(best)
}

#[derive(Debug, Deserialize)]
struct ClaudeCodeLogLine {
    timestamp: String,
    #[serde(rename = "type")]
    type_field: String,
    #[serde(rename = "sessionId")]
    session_id: String,
    version: String,
    #[serde(rename = "gitBranch")]
    git_branch: Option<String>,
    cwd: Option<String>,
    message: Option<ClaudeCodeMessage>,
}

#[derive(Debug, Deserialize)]
struct ClaudeCodeMessage {
    usage: Option<ClaudeCodeUsageData>,
}

#[derive(Debug, Deserialize)]
struct ClaudeCodeUsageData {
    input_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
    output_tokens: Option<u64>,
}

#[cfg(test)]
mod claude_code_tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn parses_claude_code_session() {
        let dir = tempdir().unwrap();
        let projects_dir = dir.path().join("projects/test-project");
        fs::create_dir_all(&projects_dir).unwrap();
        let file_path = projects_dir.join("session.jsonl");

        let lines = vec![
            serde_json::json!({
                "timestamp": "2025-09-30T10:00:00.000Z",
                "type": "user",
                "sessionId": "test-session-123",
                "version": "2.0.0",
                "gitBranch": "main",
                "cwd": "/home/user/project",
                "message": {
                    "role": "user",
                    "content": "Hello"
                }
            }),
            serde_json::json!({
                "timestamp": "2025-09-30T10:00:05.000Z",
                "type": "assistant",
                "sessionId": "test-session-123",
                "version": "2.0.0",
                "gitBranch": "main",
                "cwd": "/home/user/project",
                "message": {
                    "role": "assistant",
                    "usage": {
                        "input_tokens": 100,
                        "cache_creation_input_tokens": 50,
                        "cache_read_input_tokens": 25,
                        "output_tokens": 75
                    }
                }
            }),
            serde_json::json!({
                "timestamp": "2025-09-30T10:00:10.000Z",
                "type": "assistant",
                "sessionId": "test-session-123",
                "version": "2.0.0",
                "gitBranch": "main",
                "cwd": "/home/user/project",
                "message": {
                    "role": "assistant",
                    "usage": {
                        "input_tokens": 50,
                        "cache_creation_input_tokens": 0,
                        "cache_read_input_tokens": 100,
                        "output_tokens": 60
                    }
                }
            }),
        ];

        let content = lines
            .iter()
            .map(|l| serde_json::to_string(l).unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(&file_path, content).unwrap();

        let result = parse_claude_code_file(&file_path, 44_000).unwrap();
        assert!(result.is_some());
        let (timestamp, snapshot) = result.unwrap();
        assert_eq!(timestamp.to_rfc3339(), "2025-09-30T10:00:10+00:00");
        assert_eq!(snapshot.session_info.session_id, "test-session-123");
        assert_eq!(snapshot.session_info.version, "2.0.0");
        assert_eq!(snapshot.session_info.git_branch, Some("main".to_string()));
        assert_eq!(snapshot.token_usage.input_tokens, 150);
        assert_eq!(snapshot.token_usage.cache_creation_input_tokens, 50);
        assert_eq!(snapshot.token_usage.cache_read_input_tokens, 125);
        assert_eq!(snapshot.token_usage.output_tokens, 135);
        assert_eq!(snapshot.token_usage.total_tokens, 285);
    }
}
