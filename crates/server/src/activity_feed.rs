use std::fmt;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use services::activity_feed::ActivityEvent;
use thiserror::Error;
use ts_rs::TS;
use uuid::Uuid;

pub const FEED_PAGE_SIZE: usize = 25;
pub const ACTION_REQUIRED_THRESHOLD: u8 = 70;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, Hash)]
#[serde(rename_all = "lowercase")]
#[ts(rename_all = "lowercase")]
pub enum ActivityFeedScope {
    Mine,
    All,
}

impl Default for ActivityFeedScope {
    fn default() -> Self {
        ActivityFeedScope::Mine
    }
}

impl fmt::Display for ActivityFeedScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ActivityFeedScope::Mine => write!(f, "mine"),
            ActivityFeedScope::All => write!(f, "all"),
        }
    }
}

#[derive(Debug, Clone, Serialize, TS, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ActivityFeedItemCta {
    pub label: String,
    pub href: String,
}

#[derive(Debug, Clone, Serialize, TS, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ActivityFeedItem {
    pub id: Uuid,
    pub headline: String,
    pub summary: Option<String>,
    pub cta: Option<ActivityFeedItemCta>,
    pub urgency_score: u32,
    pub action_required: bool,
    #[ts(type = "Date")]
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, TS, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ActivityFeedResponse {
    pub events: Vec<ActivityFeedItem>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeedCursor {
    pub created_at: DateTime<Utc>,
    pub event_id: Uuid,
}

#[derive(Debug, Error)]
pub enum CursorDecodeError {
    #[error("invalid cursor encoding")]
    Decode(#[from] base64::DecodeError),
    #[error("invalid cursor utf8")]
    Utf8(#[from] std::str::Utf8Error),
    #[error("invalid cursor format")]
    InvalidFormat,
    #[error("invalid cursor timestamp")]
    InvalidTimestamp,
    #[error("invalid cursor timestamp value")]
    ParseInt(#[from] std::num::ParseIntError),
    #[error("invalid cursor id")]
    InvalidUuid(#[from] uuid::Error),
}

pub fn encode_cursor(event: &ActivityEvent) -> String {
    let timestamp = event.created_at.timestamp_millis();
    let raw = format!("{}:{}", timestamp, event.event_id);
    URL_SAFE_NO_PAD.encode(raw)
}

pub fn decode_cursor(raw: &str) -> Result<FeedCursor, CursorDecodeError> {
    let decoded = URL_SAFE_NO_PAD.decode(raw)?;
    let decoded_str = std::str::from_utf8(&decoded)?;
    let mut segments = decoded_str.splitn(2, ':');
    let timestamp = segments
        .next()
        .ok_or(CursorDecodeError::InvalidFormat)?
        .parse::<i64>()?;
    let event_id_str = segments.next().ok_or(CursorDecodeError::InvalidFormat)?;

    let created_at = DateTime::<Utc>::from_timestamp_millis(timestamp)
        .ok_or(CursorDecodeError::InvalidTimestamp)?;
    let event_id = Uuid::parse_str(event_id_str)?;

    Ok(FeedCursor {
        created_at,
        event_id,
    })
}

pub fn event_is_before_cursor(event: &ActivityEvent, cursor: &FeedCursor) -> bool {
    if event.created_at < cursor.created_at {
        true
    } else if event.created_at == cursor.created_at {
        event.event_id < cursor.event_id
    } else {
        false
    }
}

pub fn event_is_after_cursor(event: &ActivityEvent, cursor: &FeedCursor) -> bool {
    if event.created_at > cursor.created_at {
        true
    } else if event.created_at == cursor.created_at {
        event.event_id > cursor.event_id
    } else {
        false
    }
}

pub fn paginate_events(
    mut events: Vec<ActivityEvent>,
    cursor: Option<FeedCursor>,
    page_size: usize,
) -> (Vec<ActivityEvent>, Option<String>) {
    if let Some(cursor) = cursor {
        events.retain(|event| event_is_before_cursor(event, &cursor));
    }

    let page: Vec<ActivityEvent> = events.iter().take(page_size).cloned().collect();
    let has_more = events.len() > page.len();
    let next_cursor = if has_more {
        page.last().map(encode_cursor)
    } else {
        None
    };

    (page, next_cursor)
}

pub fn map_event_to_item(event: &ActivityEvent) -> ActivityFeedItem {
    ActivityFeedItem {
        id: event.event_id,
        headline: event.headline.clone(),
        summary: event.body.clone().map(|mut body| {
            if body.len() > 280 {
                body.truncate(277);
                body.push_str("...");
            }
            body
        }),
        cta: event.cta.as_ref().map(|cta| ActivityFeedItemCta {
            label: cta.label.clone(),
            href: cta.href.clone(),
        }),
        urgency_score: event.urgency_score as u32,
        action_required: event.urgency_score >= ACTION_REQUIRED_THRESHOLD,
        created_at: event.created_at,
    }
}

pub fn build_feed_response(
    events: Vec<ActivityEvent>,
    next_cursor: Option<String>,
) -> ActivityFeedResponse {
    let items = events.iter().map(map_event_to_item).collect();
    ActivityFeedResponse {
        events: items,
        next_cursor,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn sample_event(ts_offset_secs: i64) -> ActivityEvent {
        ActivityEvent {
            event_id: Uuid::new_v4(),
            entity_type: services::activity_feed::ActivityEntityType::Task,
            entity_id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            headline: "Task updated".to_string(),
            body: Some("A detailed update".to_string()),
            actors: vec![],
            cta: None,
            urgency_score: 75,
            created_at: Utc::now() - Duration::seconds(ts_offset_secs),
        }
    }

    #[test]
    fn map_event_to_item_includes_cta_when_present() {
        let mut event = sample_event(0);
        event.cta = Some(services::activity_feed::ActivityEventCta {
            label: "Open task".to_string(),
            href: "/projects/123/tasks/456".to_string(),
        });

        let item = map_event_to_item(&event);
        let cta = item.cta.expect("CTA should be populated");
        assert_eq!(cta.label, "Open task");
        assert_eq!(cta.href, "/projects/123/tasks/456");
    }

    #[test]
    fn cursor_roundtrip() {
        let event = sample_event(10);
        let encoded = encode_cursor(&event);
        let decoded = decode_cursor(&encoded).expect("cursor should decode");
        assert_eq!(decoded.event_id, event.event_id);
        assert!(decoded.created_at.timestamp_millis() - event.created_at.timestamp_millis() < 2);
    }

    #[test]
    fn pagination_respects_cursor() {
        let mut events: Vec<ActivityEvent> = (0..5).map(sample_event).collect();
        events.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        let first_page = paginate_events(events.clone(), None, 3);
        assert_eq!(first_page.0.len(), 3);
        assert!(first_page.1.is_some());

        let cursor = first_page.1.unwrap();
        let cursor = decode_cursor(&cursor).unwrap();
        let (second_page, _) = paginate_events(events, Some(cursor), 3);
        assert!(second_page.len() <= 2);
    }
}
