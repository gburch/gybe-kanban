use std::{collections::HashMap, time::Duration};

use anyhow::Result;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::{
    Extension,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use db::models::project::Project;
use futures_util::{SinkExt, StreamExt, stream::SplitSink};
use serde::{Deserialize, Serialize};
use serde_json::to_string;
use services::activity_feed::ActivityEventRepository;
use tokio::time::interval;
use utils::response::ApiResponse;
use uuid::Uuid;

use deployment::Deployment;

use crate::{
    DeploymentImpl,
    activity_feed::{
        ActivityFeedItem, ActivityFeedScope, FEED_PAGE_SIZE, decode_cursor, event_is_after_cursor,
        map_event_to_item,
    },
    routes::projects::activity_feed::{invalidate_activity_feed_cache, scope_all_enabled},
};

#[derive(Debug, Deserialize)]
pub struct ActivityFeedWsQuery {
    pub cursor: Option<String>,
    pub scope: Option<ActivityFeedScope>,
}

pub async fn project_activity_feed_ws(
    ws: WebSocketUpgrade,
    Extension(project): Extension<Project>,
    State(deployment): State<DeploymentImpl>,
    Query(query): Query<ActivityFeedWsQuery>,
) -> Result<Response, crate::error::ApiError> {
    let scope = query.scope.unwrap_or_default();

    if scope == ActivityFeedScope::All && !scope_all_enabled() {
        return Ok(ws_error_response(
            StatusCode::FORBIDDEN,
            "Scope 'all' requires project admin privileges",
        ));
    }

    Ok(ws.on_upgrade(move |socket| async move {
        if let Err(err) =
            handle_activity_feed_ws(socket, deployment, project.id, scope, query.cursor).await
        {
            tracing::warn!(
                "activity feed websocket closed for project {}: {}",
                project.id,
                err
            );
        }
    }))
}

async fn handle_activity_feed_ws(
    socket: WebSocket,
    deployment: DeploymentImpl,
    project_id: Uuid,
    scope: ActivityFeedScope,
    cursor: Option<String>,
) -> Result<()> {
    let (mut sender, mut receiver) = socket.split();
    tokio::spawn(async move { while let Some(Ok(_)) = receiver.next().await {} });

    let mut cursor = cursor.and_then(|value| match decode_cursor(&value) {
        Ok(cursor) => Some(cursor),
        Err(err) => {
            tracing::warn!("ignoring invalid cursor for websocket resume: {}", err);
            None
        }
    });

    let user_id = match scope {
        ActivityFeedScope::Mine => Uuid::parse_str(deployment.user_id()).ok(),
        ActivityFeedScope::All => None,
    };

    let repository = {
        let config = deployment.config().read().await;
        ActivityEventRepository::from_config(deployment.db().pool.clone(), &config.activity_feed)
    };

    let events = repository.list_recent(project_id, user_id).await?;
    let mut state: HashMap<Uuid, ActivityFeedItem> = events
        .iter()
        .map(|event| {
            let item = map_event_to_item(event);
            (item.id, item)
        })
        .collect();

    let initial_events: Vec<ActivityFeedItem> = if let Some(cursor_value) = cursor.take() {
        events
            .iter()
            .filter(|event| event_is_after_cursor(event, &cursor_value))
            .take(FEED_PAGE_SIZE)
            .map(map_event_to_item)
            .collect()
    } else {
        events
            .iter()
            .take(FEED_PAGE_SIZE)
            .map(map_event_to_item)
            .collect()
    };

    for item in initial_events {
        send_event(
            &mut sender,
            item.id,
            ActivityFeedChangeType::Created,
            Some(item),
        )
        .await?;
    }

    let mut ticker = interval(Duration::from_secs(2));

    loop {
        ticker.tick().await;

        let events = repository.list_recent(project_id, user_id).await?;
        let mut latest: HashMap<Uuid, ActivityFeedItem> = HashMap::with_capacity(events.len());
        for event in events.iter() {
            let item = map_event_to_item(event);
            latest.insert(item.id, item);
        }

        let mut dirty = false;

        for (id, item) in latest.iter() {
            match state.get(id) {
                Some(existing) if existing == item => {}
                Some(_) => {
                    dirty = true;
                    send_event(
                        &mut sender,
                        *id,
                        ActivityFeedChangeType::Updated,
                        Some(item.clone()),
                    )
                    .await?;
                }
                None => {
                    dirty = true;
                    send_event(
                        &mut sender,
                        *id,
                        ActivityFeedChangeType::Created,
                        Some(item.clone()),
                    )
                    .await?;
                }
            }
        }

        for (id, item) in state.iter() {
            if !latest.contains_key(id) {
                dirty = true;
                send_event(
                    &mut sender,
                    *id,
                    ActivityFeedChangeType::Removed,
                    Some(item.clone()),
                )
                .await?;
            }
        }

        if dirty {
            invalidate_activity_feed_cache(project_id).await;
        }

        state = latest;
    }
}

async fn send_event(
    sender: &mut SplitSink<WebSocket, Message>,
    id: Uuid,
    change_type: ActivityFeedChangeType,
    item: Option<ActivityFeedItem>,
) -> Result<()> {
    let message = ActivityFeedWsMessage {
        r#type: "activity_feed.update",
        payload: ActivityFeedWsPayload {
            event: ActivityFeedWsEventChange {
                id,
                change_type,
                event: item,
            },
        },
    };
    let payload = to_string(&message)?;
    sender.send(Message::Text(payload.into())).await?;
    Ok(())
}

fn ws_error_response(status: StatusCode, message: &str) -> Response {
    (
        status,
        axum::response::Json(ApiResponse::<()>::error(message)),
    )
        .into_response()
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
enum ActivityFeedChangeType {
    Created,
    Updated,
    Removed,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ActivityFeedWsMessage {
    r#type: &'static str,
    payload: ActivityFeedWsPayload,
}

#[derive(Serialize)]
struct ActivityFeedWsPayload {
    event: ActivityFeedWsEventChange,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ActivityFeedWsEventChange {
    id: Uuid,
    change_type: ActivityFeedChangeType,
    event: Option<ActivityFeedItem>,
}
