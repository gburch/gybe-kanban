use std::{
    fs::File,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    time::SystemTime,
};

use axum::{Router, response::Json as ResponseJson, routing::get};
use chrono::{DateTime, Utc};
use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};
use tokio::task;
use tracing::warn;
use ts_rs::TS;

use crate::{DeploymentImpl, error::ApiError};

use utils::response::ApiResponse;

pub fn router() -> Router<DeploymentImpl> {
    Router::new().route("/usage/codex", get(get_codex_usage))
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
    _extra: std::collections::HashMap<String, serde_json::Value>,
}

impl RateLimitSnapshot {
    fn into_usage_rate_limits(self) -> Option<CodexUsageRateLimits> {
        let primary = self.primary.map(CodexUsageWindow::from).or_else(|| {
            self.primary_used_percent
                .map(|used_percent| CodexUsageWindow {
                    used_percent,
                    window_minutes: self.primary_window_minutes,
                    resets_in_seconds: self.primary_resets_in_seconds,
                })
        });

        let secondary = self.secondary.map(CodexUsageWindow::from).or_else(|| {
            self.secondary_used_percent
                .map(|used_percent| CodexUsageWindow {
                    used_percent,
                    window_minutes: self.secondary_window_minutes,
                    resets_in_seconds: self.secondary_resets_in_seconds,
                })
        });

        if primary.is_none() && secondary.is_none() {
            None
        } else {
            Some(CodexUsageRateLimits { primary, secondary })
        }
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
}
