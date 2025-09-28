use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, TS)]
#[serde(rename_all = "lowercase")]
#[ts(rename_all = "lowercase")]
pub enum ActivityEntityType {
    Task,
    Attempt,
    Comment,
    Deployment,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
pub struct ActivityEventActor {
    pub id: Uuid,
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
pub struct ActivityEventCta {
    pub label: String,
    pub href: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
pub struct ActivityEvent {
    pub event_id: Uuid,
    pub entity_type: ActivityEntityType,
    pub entity_id: Uuid,
    pub project_id: Uuid,
    pub headline: String,
    pub body: Option<String>,
    pub actors: Vec<ActivityEventActor>,
    pub cta: Option<ActivityEventCta>,
    pub urgency_score: u8,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ActivityVisibility {
    Public,
    Restricted(HashSet<Uuid>),
}

impl ActivityVisibility {
    pub fn is_visible_to(&self, user_id: Option<Uuid>) -> bool {
        match (self, user_id) {
            (ActivityVisibility::Public, _) => true,
            (ActivityVisibility::Restricted(allowed), Some(uid)) => allowed.contains(&uid),
            (ActivityVisibility::Restricted(_), None) => false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ActivityDomainEvent {
    pub event_id: Uuid,
    pub entity_type: ActivityEntityType,
    pub entity_id: Uuid,
    pub project_id: Uuid,
    pub headline: Option<String>,
    pub body: Option<String>,
    pub actors: Vec<ActivityEventActor>,
    pub urgency_hint: Option<ActivityUrgencyHint>,
    pub created_at: DateTime<Utc>,
    pub visibility: ActivityVisibility,
    pub kind: ActivityDomainEventKind,
}

#[derive(Debug, Clone)]
pub enum ActivityDomainEventKind {
    Task(TaskDomainDetails),
    Attempt(AttemptDomainDetails),
    Comment(CommentDomainDetails),
    Deployment(DeploymentDomainDetails),
}

#[derive(Debug, Clone)]
pub struct TaskDomainDetails {
    pub status: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AttemptDomainDetails {
    pub task_id: Uuid,
    pub state: Option<String>,
    pub executor: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CommentDomainDetails {
    pub author_id: Option<Uuid>,
}

#[derive(Debug, Clone)]
pub struct DeploymentDomainDetails {
    pub status: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityUrgencyHint {
    Low,
    Normal,
    Elevated,
    High,
    Critical,
}
