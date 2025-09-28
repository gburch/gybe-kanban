use std::{collections::HashMap, time::Duration};

use axum::{
    Extension,
    body::Body,
    extract::{Query, State},
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{ETAG, IF_NONE_MATCH},
    },
    response::{IntoResponse, Response},
};
use db::models::project::Project;
use deployment::Deployment;
use once_cell::sync::Lazy;
use serde::Deserialize;
use services::activity_feed::ActivityEventRepository;
use sha2::{Digest, Sha256};
use tokio::sync::RwLock;
use utils::{
    cache::{CacheEnvelope, key::activity_feed_cache_key},
    response::ApiResponse,
};
use uuid::Uuid;

use crate::{
    DeploymentImpl,
    activity_feed::{
        ActivityFeedResponse, ActivityFeedScope, FEED_PAGE_SIZE, build_feed_response,
        decode_cursor, paginate_events,
    },
    error::ApiError,
};

static FEED_CACHE: Lazy<RwLock<HashMap<String, CacheEnvelope<ActivityFeedResponse>>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

#[derive(Debug, Deserialize)]
pub struct ActivityFeedQuery {
    pub cursor: Option<String>,
    pub scope: Option<ActivityFeedScope>,
}

pub async fn get_activity_feed(
    headers: HeaderMap,
    Extension(project): Extension<Project>,
    State(deployment): State<DeploymentImpl>,
    Query(query): Query<ActivityFeedQuery>,
) -> Result<Response, ApiError> {
    let scope = query.scope.unwrap_or_default();

    if scope == ActivityFeedScope::All && !scope_all_enabled() {
        return Ok(error_response(
            StatusCode::FORBIDDEN,
            "Scope 'all' requires project admin privileges",
        ));
    }

    let user_id = match scope {
        ActivityFeedScope::Mine => Uuid::parse_str(deployment.user_id()).ok(),
        ActivityFeedScope::All => None,
    };

    let config = deployment.config().read().await;
    let repository =
        ActivityEventRepository::from_config(deployment.db().pool.clone(), &config.activity_feed);
    drop(config);

    let cursor = match &query.cursor {
        Some(raw) => match decode_cursor(raw) {
            Ok(cursor) => Some(cursor),
            Err(_) => {
                return Ok(error_response(
                    StatusCode::BAD_REQUEST,
                    "Invalid cursor parameter",
                ));
            }
        },
        None => None,
    };

    let cache_key =
        activity_feed_cache_key(project.id, &scope.to_string(), query.cursor.as_deref());
    let if_none_match = headers
        .get(IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_string());

    if query.cursor.is_none() {
        if let Some(entry) = fetch_cached(&cache_key).await {
            if entry.is_expired() {
                evict_key(&cache_key).await;
            } else {
                if let Some(tag) = &if_none_match {
                    if tag == &entry.etag {
                        return Ok(not_modified_response(&entry.etag));
                    }
                }
                return Ok(success_response(entry.payload.clone(), &entry.etag));
            }
        }
    }

    let events = repository
        .list_recent(project.id, user_id)
        .await
        .map_err(map_anyhow_error)?;
    let (page, next_cursor) = paginate_events(events, cursor, FEED_PAGE_SIZE);
    let response_payload = build_feed_response(page, next_cursor);
    let etag = compute_etag(&response_payload)?;

    if let Some(tag) = &if_none_match {
        if tag == &etag {
            if query.cursor.is_none() {
                store_cache(cache_key, response_payload.clone(), etag.clone()).await;
            }
            return Ok(not_modified_response(&etag));
        }
    }

    if query.cursor.is_none() {
        store_cache(cache_key, response_payload.clone(), etag.clone()).await;
    }

    Ok(success_response(response_payload, &etag))
}

pub async fn invalidate_activity_feed_cache(project_id: Uuid) {
    let mut cache = FEED_CACHE.write().await;
    cache.retain(|key, _| !key.starts_with(&format!("activity_feed:{project_id}")));
}

async fn fetch_cached(key: &str) -> Option<CacheEnvelope<ActivityFeedResponse>> {
    let cache = FEED_CACHE.read().await;
    cache.get(key).cloned()
}

async fn evict_key(key: &str) {
    let mut cache = FEED_CACHE.write().await;
    cache.remove(key);
}

async fn store_cache(key: String, payload: ActivityFeedResponse, etag: String) {
    let ttl = cache_ttl();
    let envelope = CacheEnvelope::new(payload, etag, ttl);
    let mut cache = FEED_CACHE.write().await;
    cache.insert(key, envelope);
}

fn compute_etag(payload: &ActivityFeedResponse) -> Result<String, ApiError> {
    let bytes = serde_json::to_vec(payload).map_err(|err| {
        ApiError::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            err.to_string(),
        ))
    })?;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    Ok(format!("W/\"{:x}\"", hasher.finalize()))
}

fn not_modified_response(etag: &str) -> Response {
    let mut response = Response::builder()
        .status(StatusCode::NOT_MODIFIED)
        .body(Body::empty())
        .expect("failed to build response");
    if let Ok(value) = HeaderValue::from_str(etag) {
        response.headers_mut().insert(ETAG, value);
    }
    response
}

fn success_response(payload: ActivityFeedResponse, etag: &str) -> Response {
    let mut headers = HeaderMap::new();
    if let Ok(value) = HeaderValue::from_str(etag) {
        headers.insert(ETAG, value);
    }
    (
        StatusCode::OK,
        headers,
        axum::response::Json(ApiResponse::<ActivityFeedResponse>::success(payload)),
    )
        .into_response()
}

fn error_response(status: StatusCode, message: &str) -> Response {
    (
        status,
        axum::response::Json(ApiResponse::<()>::error(message)),
    )
        .into_response()
}

fn cache_ttl() -> Duration {
    std::env::var("VIBE_ACTIVITY_FEED_CACHE_TTL")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_secs(30))
}

pub(crate) fn scope_all_enabled() -> bool {
    std::env::var("VIBE_ACTIVITY_FEED_SCOPE_ALL")
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes"
            )
        })
        .unwrap_or(false)
}

fn map_anyhow_error(err: anyhow::Error) -> ApiError {
    ApiError::Io(std::io::Error::new(
        std::io::ErrorKind::Other,
        err.to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cache_roundtrip() {
        unsafe {
            std::env::set_var("VIBE_ACTIVITY_FEED_CACHE_TTL", "5");
        }
        let key = "activity_feed:test";
        let payload = ActivityFeedResponse {
            events: Vec::new(),
            next_cursor: None,
        };

        store_cache(key.to_string(), payload.clone(), "etag-test".to_string()).await;
        let envelope = fetch_cached(key).await.expect("entry stored");
        assert_eq!(envelope.payload, payload);
        assert_eq!(envelope.etag, "etag-test");

        evict_key(key).await;
        unsafe {
            std::env::remove_var("VIBE_ACTIVITY_FEED_CACHE_TTL");
        }
    }

    #[test]
    fn scope_all_flag_respects_env() {
        unsafe {
            std::env::remove_var("VIBE_ACTIVITY_FEED_SCOPE_ALL");
        }
        assert!(!scope_all_enabled());

        unsafe {
            std::env::set_var("VIBE_ACTIVITY_FEED_SCOPE_ALL", "true");
        }
        assert!(scope_all_enabled());

        unsafe {
            std::env::set_var("VIBE_ACTIVITY_FEED_SCOPE_ALL", "0");
        }
        assert!(!scope_all_enabled());

        unsafe {
            std::env::remove_var("VIBE_ACTIVITY_FEED_SCOPE_ALL");
        }
    }
}
