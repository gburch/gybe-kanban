use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::activity_feed::{
    ActivityAggregator, ActivityAggregatorConfig, ActivityDomainEvent, ActivityEvent,
    ActivityVisibility,
};
use crate::services::config::ActivityFeedConfig;

use super::models::{
    ActivityDomainEventKind, ActivityEntityType, ActivityEventActor, ActivityUrgencyHint,
    AttemptDomainDetails, CommentDomainDetails, DeploymentDomainDetails, TaskDomainDetails,
};

#[async_trait]
pub trait ActivityFeedDataSource: Send + Sync {
    async fn fetch_domain_events(
        &self,
        project_id: Uuid,
        since: DateTime<Utc>,
    ) -> Result<Vec<ActivityDomainEvent>>;
}

pub struct ActivityEventRepository<D: ActivityFeedDataSource> {
    data_source: D,
    aggregator: ActivityAggregator,
    enabled: bool,
}

impl<D: ActivityFeedDataSource> ActivityEventRepository<D> {
    pub fn new(data_source: D, aggregator: ActivityAggregator, enabled: bool) -> Self {
        Self {
            data_source,
            aggregator,
            enabled,
        }
    }

    pub async fn list_recent(
        &self,
        project_id: Uuid,
        user_id: Option<Uuid>,
    ) -> Result<Vec<ActivityEvent>> {
        if !self.enabled {
            return Ok(Vec::new());
        }
        let now = Utc::now();
        let since = self.aggregator.window_start(now);
        let domain_events = self
            .data_source
            .fetch_domain_events(project_id, since)
            .await?;
        Ok(self
            .aggregator
            .aggregate_with_now(user_id, domain_events, now))
    }
}

pub struct SqlActivityFeedDataSource {
    pool: SqlitePool,
}

impl SqlActivityFeedDataSource {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

impl ActivityEventRepository<SqlActivityFeedDataSource> {
    pub fn from_config(pool: SqlitePool, config: &ActivityFeedConfig) -> Self {
        let data_source = SqlActivityFeedDataSource::new(pool);
        let aggregator_config = ActivityAggregatorConfig {
            window: Duration::days(config.window_days as i64),
        };
        let aggregator = ActivityAggregator::new(aggregator_config);
        Self::new(data_source, aggregator, config.enabled)
    }
}

#[async_trait]
impl ActivityFeedDataSource for SqlActivityFeedDataSource {
    async fn fetch_domain_events(
        &self,
        project_id: Uuid,
        since: DateTime<Utc>,
    ) -> Result<Vec<ActivityDomainEvent>> {
        use db::activity_feed_queries as queries;

        let mut events = Vec::new();

        let tasks = queries::fetch_task_activity(&self.pool, project_id, since).await?;
        for task in tasks {
            let visibility = match task.restricted_to {
                Some(users) if !users.is_empty() => ActivityVisibility::Restricted(users),
                _ => ActivityVisibility::Public,
            };

            events.push(ActivityDomainEvent {
                event_id: task.event_id.unwrap_or(task.entity_id),
                entity_type: ActivityEntityType::Task,
                entity_id: task.entity_id,
                project_id: project_id,
                headline: Some(task.headline.unwrap_or_else(|| task.title.clone())),
                body: task.body,
                actors: task
                    .actors
                    .into_iter()
                    .map(|actor| ActivityEventActor {
                        id: actor.id,
                        display_name: actor.display_name,
                    })
                    .collect(),
                urgency_hint: task.urgency_hint.map(|hint| match hint {
                    queries::UrgencyHint::Low => ActivityUrgencyHint::Low,
                    queries::UrgencyHint::Normal => ActivityUrgencyHint::Normal,
                    queries::UrgencyHint::Elevated => ActivityUrgencyHint::Elevated,
                    queries::UrgencyHint::High => ActivityUrgencyHint::High,
                    queries::UrgencyHint::Critical => ActivityUrgencyHint::Critical,
                }),
                created_at: task.created_at,
                visibility,
                kind: ActivityDomainEventKind::Task(TaskDomainDetails {
                    status: task.status,
                }),
            });
        }

        let attempts = queries::fetch_attempt_activity(&self.pool, project_id, since).await?;
        for attempt in attempts {
            let visibility = match attempt.restricted_to {
                Some(users) if !users.is_empty() => ActivityVisibility::Restricted(users),
                _ => ActivityVisibility::Public,
            };

            events.push(ActivityDomainEvent {
                event_id: attempt.event_id.unwrap_or(attempt.entity_id),
                entity_type: ActivityEntityType::Attempt,
                entity_id: attempt.entity_id,
                project_id,
                headline: attempt.headline,
                body: attempt.body,
                actors: attempt
                    .actors
                    .into_iter()
                    .map(|actor| ActivityEventActor {
                        id: actor.id,
                        display_name: actor.display_name,
                    })
                    .collect(),
                urgency_hint: attempt.urgency_hint.map(|hint| match hint {
                    queries::UrgencyHint::Low => ActivityUrgencyHint::Low,
                    queries::UrgencyHint::Normal => ActivityUrgencyHint::Normal,
                    queries::UrgencyHint::Elevated => ActivityUrgencyHint::Elevated,
                    queries::UrgencyHint::High => ActivityUrgencyHint::High,
                    queries::UrgencyHint::Critical => ActivityUrgencyHint::Critical,
                }),
                created_at: attempt.created_at,
                visibility,
                kind: ActivityDomainEventKind::Attempt(AttemptDomainDetails {
                    state: attempt.state,
                    executor: attempt.executor,
                }),
            });
        }

        let comments = queries::fetch_comment_activity(&self.pool, project_id, since).await?;
        for comment in comments {
            let visibility = match comment.restricted_to {
                Some(users) if !users.is_empty() => ActivityVisibility::Restricted(users),
                _ => ActivityVisibility::Public,
            };

            events.push(ActivityDomainEvent {
                event_id: comment.event_id.unwrap_or(comment.entity_id),
                entity_type: ActivityEntityType::Comment,
                entity_id: comment.entity_id,
                project_id,
                headline: comment.headline,
                body: comment.body,
                actors: comment
                    .actors
                    .into_iter()
                    .map(|actor| ActivityEventActor {
                        id: actor.id,
                        display_name: actor.display_name,
                    })
                    .collect(),
                urgency_hint: comment.urgency_hint.map(|hint| match hint {
                    queries::UrgencyHint::Low => ActivityUrgencyHint::Low,
                    queries::UrgencyHint::Normal => ActivityUrgencyHint::Normal,
                    queries::UrgencyHint::Elevated => ActivityUrgencyHint::Elevated,
                    queries::UrgencyHint::High => ActivityUrgencyHint::High,
                    queries::UrgencyHint::Critical => ActivityUrgencyHint::Critical,
                }),
                created_at: comment.created_at,
                visibility,
                kind: ActivityDomainEventKind::Comment(CommentDomainDetails {
                    author_id: comment.author_id,
                }),
            });
        }

        let deployments = queries::fetch_deployment_activity(&self.pool, project_id, since).await?;
        for deployment in deployments {
            let visibility = match deployment.restricted_to {
                Some(users) if !users.is_empty() => ActivityVisibility::Restricted(users),
                _ => ActivityVisibility::Public,
            };

            events.push(ActivityDomainEvent {
                event_id: deployment.event_id.unwrap_or(deployment.entity_id),
                entity_type: ActivityEntityType::Deployment,
                entity_id: deployment.entity_id,
                project_id,
                headline: deployment.headline,
                body: deployment.body,
                actors: deployment
                    .actors
                    .into_iter()
                    .map(|actor| ActivityEventActor {
                        id: actor.id,
                        display_name: actor.display_name,
                    })
                    .collect(),
                urgency_hint: deployment.urgency_hint.map(|hint| match hint {
                    queries::UrgencyHint::Low => ActivityUrgencyHint::Low,
                    queries::UrgencyHint::Normal => ActivityUrgencyHint::Normal,
                    queries::UrgencyHint::Elevated => ActivityUrgencyHint::Elevated,
                    queries::UrgencyHint::High => ActivityUrgencyHint::High,
                    queries::UrgencyHint::Critical => ActivityUrgencyHint::Critical,
                }),
                created_at: deployment.created_at,
                visibility,
                kind: ActivityDomainEventKind::Deployment(DeploymentDomainDetails {
                    status: deployment.status,
                    url: deployment.url,
                }),
            });
        }

        Ok(events)
    }
}
