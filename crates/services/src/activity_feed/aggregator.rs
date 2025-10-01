use std::{collections::HashMap, time::Instant};

use chrono::{DateTime, Duration, Utc};
use uuid::Uuid;

use crate::metrics;
use crate::notifications::priority::{self, UrgencyComputationContext, UrgencyLevel};

use super::models::{
    ActivityDomainEvent, ActivityDomainEventKind, ActivityEntityType, ActivityEvent,
    ActivityEventCta, ActivityUrgencyHint,
};

#[derive(Debug, Clone)]
pub struct ActivityAggregatorConfig {
    pub window: Duration,
}

impl Default for ActivityAggregatorConfig {
    fn default() -> Self {
        Self {
            window: Duration::days(21),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ActivityAggregator {
    config: ActivityAggregatorConfig,
}

impl ActivityAggregator {
    pub fn new(config: ActivityAggregatorConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &ActivityAggregatorConfig {
        &self.config
    }

    pub fn window_start(&self, now: DateTime<Utc>) -> DateTime<Utc> {
        now - self.config.window
    }

    pub fn aggregate(
        &self,
        user_id: Option<Uuid>,
        domain_events: Vec<ActivityDomainEvent>,
    ) -> Vec<ActivityEvent> {
        self.aggregate_with_now(user_id, domain_events, Utc::now())
    }

    pub fn aggregate_with_now(
        &self,
        user_id: Option<Uuid>,
        domain_events: Vec<ActivityDomainEvent>,
        now: DateTime<Utc>,
    ) -> Vec<ActivityEvent> {
        let earliest_allowed = self.window_start(now);
        let mut dedup: HashMap<(ActivityEntityType, Uuid), ActivityDomainEvent> = HashMap::new();

        let span = tracing::info_span!("activity_feed.aggregate");
        let _guard = span.enter();
        let aggregation_start = Instant::now();

        for event in domain_events {
            if event.created_at < earliest_allowed {
                continue;
            }

            if !event.visibility.is_visible_to(user_id) {
                continue;
            }

            let key = (event.entity_type, event.entity_id);
            match dedup.entry(key) {
                std::collections::hash_map::Entry::Vacant(slot) => {
                    slot.insert(event);
                }
                std::collections::hash_map::Entry::Occupied(mut existing) => {
                    if event.created_at > existing.get().created_at {
                        existing.insert(event);
                    }
                }
            }
        }

        let mut events: Vec<ActivityEvent> = dedup
            .into_values()
            .map(|event| self.normalize_event(event, now))
            .collect();

        events.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        let elapsed_ms = aggregation_start.elapsed().as_secs_f64() * 1_000.0;
        metrics::record_timing("activity_feed.aggregate.ms", elapsed_ms);

        events
    }

    fn normalize_event(&self, event: ActivityDomainEvent, now: DateTime<Utc>) -> ActivityEvent {
        let ActivityDomainEvent {
            event_id,
            entity_type,
            entity_id,
            project_id,
            headline,
            body,
            actors,
            urgency_hint,
            created_at,
            visibility: _,
            kind,
        } = event;

        let headline = headline.unwrap_or_else(|| self.default_headline(&kind));

        let body = match body {
            Some(text) if !text.trim().is_empty() => Some(text),
            _ => self.default_body(&kind),
        };

        let urgency_level = urgency_hint
            .map(|hint| match hint {
                ActivityUrgencyHint::Low => UrgencyLevel::Low,
                ActivityUrgencyHint::Normal => UrgencyLevel::Normal,
                ActivityUrgencyHint::Elevated => UrgencyLevel::Elevated,
                ActivityUrgencyHint::High => UrgencyLevel::High,
                ActivityUrgencyHint::Critical => UrgencyLevel::Critical,
            })
            .unwrap_or_else(|| self.derive_default_urgency(&kind));

        let recency_hours = (now - created_at).num_hours().max(0) as u32;
        let urgency_score = priority::calculate_score(UrgencyComputationContext {
            level: urgency_level,
            recency_hours,
            entity_type,
        });

        ActivityEvent {
            event_id,
            entity_type,
            entity_id,
            project_id,
            headline,
            body,
            actors,
            cta: self.derive_cta(entity_type, project_id, entity_id, &kind),
            urgency_score,
            created_at,
        }
    }

    fn derive_cta(
        &self,
        entity_type: ActivityEntityType,
        project_id: Uuid,
        entity_id: Uuid,
        kind: &ActivityDomainEventKind,
    ) -> Option<ActivityEventCta> {
        let specific = match (entity_type, kind) {
            (ActivityEntityType::Task, _) => Some(ActivityEventCta {
                label: "Open task".to_string(),
                href: format!("/projects/{}/tasks/{}", project_id, entity_id),
            }),
            (ActivityEntityType::Attempt, ActivityDomainEventKind::Attempt(details)) => {
                Some(ActivityEventCta {
                    label: "View attempt".to_string(),
                    href: format!(
                        "/projects/{}/tasks/{}/attempts/{}",
                        project_id, details.task_id, entity_id
                    ),
                })
            }
            (ActivityEntityType::Deployment, ActivityDomainEventKind::Deployment(details)) => {
                details.url.as_ref().map(|url| ActivityEventCta {
                    label: "Open deployment".to_string(),
                    href: url.clone(),
                })
            }
            _ => None,
        };

        specific.or_else(|| {
            Some(ActivityEventCta {
                label: "Open project".to_string(),
                href: format!("/projects/{}", project_id),
            })
        })
    }

    fn default_headline(&self, kind: &ActivityDomainEventKind) -> String {
        match kind {
            ActivityDomainEventKind::Task(_) => "Task updated".to_string(),
            ActivityDomainEventKind::Attempt(_) => "Task attempt activity".to_string(),
            ActivityDomainEventKind::Comment(_) => "New comment".to_string(),
            ActivityDomainEventKind::Deployment(_) => "Deployment event".to_string(),
        }
    }

    fn default_body(&self, kind: &ActivityDomainEventKind) -> Option<String> {
        match kind {
            ActivityDomainEventKind::Task(details) => details
                .status
                .as_ref()
                .map(|status| format!("Status: {}", status)),
            ActivityDomainEventKind::Attempt(details) => details
                .state
                .as_ref()
                .map(|state| format!("Attempt state: {}", state)),
            ActivityDomainEventKind::Comment(_) => None,
            ActivityDomainEventKind::Deployment(details) => details
                .status
                .as_ref()
                .map(|status| format!("Deployment status: {}", status)),
        }
    }

    fn derive_default_urgency(&self, kind: &ActivityDomainEventKind) -> UrgencyLevel {
        match kind {
            ActivityDomainEventKind::Task(details) => match details
                .status
                .as_deref()
                .map(|status| status.to_ascii_lowercase())
                .as_deref()
            {
                Some("inreview") => UrgencyLevel::High,
                Some("inprogress") => UrgencyLevel::Elevated,
                Some("todo") => UrgencyLevel::Normal,
                Some("cancelled") | Some("done") => UrgencyLevel::Low,
                _ => UrgencyLevel::Normal,
            },
            ActivityDomainEventKind::Attempt(details) => match details
                .state
                .as_deref()
                .map(|state| state.to_ascii_lowercase())
                .as_deref()
            {
                Some("executorfailed") | Some("setupfailed") => UrgencyLevel::Critical,
                Some("executorcomplete") => UrgencyLevel::Normal,
                Some("executorrunning") => UrgencyLevel::Elevated,
                _ => UrgencyLevel::Normal,
            },
            ActivityDomainEventKind::Comment(_) => UrgencyLevel::Normal,
            ActivityDomainEventKind::Deployment(details) => match details
                .status
                .as_deref()
                .map(|status| status.to_ascii_lowercase())
                .as_deref()
            {
                Some("failed") => UrgencyLevel::Critical,
                Some("running") => UrgencyLevel::Elevated,
                Some("succeeded") => UrgencyLevel::Normal,
                _ => UrgencyLevel::Normal,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::activity_feed::models::{
        ActivityDomainEvent, ActivityDomainEventKind, ActivityEventActor, ActivityUrgencyHint,
        ActivityVisibility, AttemptDomainDetails, CommentDomainDetails, DeploymentDomainDetails,
        TaskDomainDetails,
    };

    fn build_event(
        entity_type: ActivityEntityType,
        kind: ActivityDomainEventKind,
        created_at: DateTime<Utc>,
        visibility: ActivityVisibility,
    ) -> ActivityDomainEvent {
        ActivityDomainEvent {
            event_id: Uuid::new_v4(),
            entity_type,
            entity_id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            headline: None,
            body: None,
            actors: vec![ActivityEventActor {
                id: Uuid::new_v4(),
                display_name: "Casey".into(),
            }],
            urgency_hint: None,
            created_at,
            visibility,
            kind,
        }
    }

    #[test]
    fn filters_out_old_events_and_deduplicates() {
        let now = Utc::now();
        let config = ActivityAggregatorConfig {
            window: Duration::days(21),
        };
        let aggregator = ActivityAggregator::new(config);
        let project_id = Uuid::new_v4();
        let entity_id = Uuid::new_v4();

        let mut first = build_event(
            ActivityEntityType::Task,
            ActivityDomainEventKind::Task(TaskDomainDetails {
                status: Some("todo".into()),
            }),
            now - Duration::days(2),
            ActivityVisibility::Public,
        );
        first.project_id = project_id;
        first.entity_id = entity_id;

        let mut second = build_event(
            ActivityEntityType::Task,
            ActivityDomainEventKind::Task(TaskDomainDetails {
                status: Some("inreview".into()),
            }),
            now - Duration::days(1),
            ActivityVisibility::Public,
        );
        second.project_id = project_id;
        second.entity_id = entity_id;

        let mut stale = build_event(
            ActivityEntityType::Task,
            ActivityDomainEventKind::Task(TaskDomainDetails { status: None }),
            now - Duration::days(30),
            ActivityVisibility::Public,
        );
        stale.project_id = project_id;
        stale.entity_id = Uuid::new_v4();

        let events = aggregator.aggregate_with_now(
            Some(Uuid::new_v4()),
            vec![stale, first.clone(), second.clone()],
            now,
        );

        assert_eq!(events.len(), 1, "expected deduplicated events");
        let event = &events[0];
        assert_eq!(event.entity_id, entity_id);
        assert!(
            event.urgency_score >= 60,
            "in-review should yield elevated urgency"
        );
        let cta = event.cta.as_ref().expect("task events should include CTA");
        assert_eq!(cta.label, "Open task");
        assert!(cta.href.ends_with(&entity_id.to_string()));
    }

    #[test]
    fn enforces_visibility_rules() {
        let now = Utc::now();
        let config = ActivityAggregatorConfig {
            window: Duration::days(7),
        };
        let aggregator = ActivityAggregator::new(config);
        let user = Uuid::new_v4();
        let other_user = Uuid::new_v4();

        let mut restricted = build_event(
            ActivityEntityType::Comment,
            ActivityDomainEventKind::Comment(CommentDomainDetails { author_id: None }),
            now - Duration::hours(1),
            ActivityVisibility::Restricted(HashSet::from([user])),
        );
        restricted.project_id = Uuid::new_v4();
        restricted.entity_id = Uuid::new_v4();

        let mut hidden = build_event(
            ActivityEntityType::Attempt,
            ActivityDomainEventKind::Attempt(AttemptDomainDetails {
                task_id: Uuid::new_v4(),
                state: Some("executorfailed".into()),
                executor: None,
            }),
            now - Duration::hours(1),
            ActivityVisibility::Restricted(HashSet::from([other_user])),
        );
        hidden.project_id = restricted.project_id;
        hidden.entity_id = Uuid::new_v4();

        let events =
            aggregator.aggregate_with_now(Some(user), vec![restricted.clone(), hidden], now);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].entity_id, restricted.entity_id);
        assert!(events[0].cta.is_some());
    }

    #[test]
    fn comment_events_fallback_to_project_cta() {
        let now = Utc::now();
        let aggregator = ActivityAggregator::new(ActivityAggregatorConfig::default());
        let project_id = Uuid::new_v4();

        let mut comment = build_event(
            ActivityEntityType::Comment,
            ActivityDomainEventKind::Comment(CommentDomainDetails { author_id: None }),
            now - Duration::minutes(10),
            ActivityVisibility::Public,
        );
        comment.project_id = project_id;

        let events = aggregator.aggregate_with_now(None, vec![comment], now);
        assert_eq!(events.len(), 1);

        let cta = events[0]
            .cta
            .as_ref()
            .expect("comment events should include CTA");
        assert_eq!(cta.label, "Open project");
        assert_eq!(cta.href, format!("/projects/{}", project_id));
    }

    #[test]
    fn applies_explicit_urgency_hint() {
        let now = Utc::now();
        let config = ActivityAggregatorConfig {
            window: Duration::days(7),
        };
        let aggregator = ActivityAggregator::new(config);

        let mut hinted = build_event(
            ActivityEntityType::Deployment,
            ActivityDomainEventKind::Deployment(DeploymentDomainDetails {
                status: Some("running".into()),
                url: None,
            }),
            now - Duration::minutes(5),
            ActivityVisibility::Public,
        );
        hinted.urgency_hint = Some(ActivityUrgencyHint::Critical);

        let events = aggregator.aggregate_with_now(None, vec![hinted], now);
        assert_eq!(events.len(), 1);
        assert!(events[0].urgency_score >= 95);
    }

    #[test]
    fn attempt_events_include_attempt_cta() {
        let now = Utc::now();
        let aggregator = ActivityAggregator::new(ActivityAggregatorConfig::default());
        let task_id = Uuid::new_v4();
        let attempt_id = Uuid::new_v4();
        let project_id = Uuid::new_v4();

        let mut attempt_event = build_event(
            ActivityEntityType::Attempt,
            ActivityDomainEventKind::Attempt(AttemptDomainDetails {
                task_id,
                state: Some("executorrunning".into()),
                executor: None,
            }),
            now - Duration::minutes(5),
            ActivityVisibility::Public,
        );
        attempt_event.entity_id = attempt_id;
        attempt_event.project_id = project_id;

        let events = aggregator.aggregate_with_now(Some(Uuid::new_v4()), vec![attempt_event], now);
        assert_eq!(events.len(), 1);
        let event = &events[0];
        let cta = event
            .cta
            .as_ref()
            .expect("attempt events should include CTA");
        assert_eq!(cta.label, "View attempt");
        assert_eq!(
            cta.href,
            format!(
                "/projects/{}/tasks/{}/attempts/{}",
                project_id, task_id, attempt_id
            )
        );
    }
}
