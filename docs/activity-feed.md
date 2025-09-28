# Activity Feed Aggregation

The activity feed consolidates recent project-facing events into a normalized
`ActivityEvent` representation. Events are derived from existing domain tables
(tasks, task attempts, comments, and deployments) and are filtered to the 21-day
window defined in configuration.

## Event Normalisation

Each `ActivityEvent` contains:

- `event_id`, `entity_id`, `entity_type`, and `project_id` identifiers.
- A `headline`, optional `body`, and zero or more `actors`.
- An optional primary call-to-action (`cta`) with UI routing hints.
- An urgency score (0-100) that is shared with the notifications system.

Aggregation deduplicates events per entity/type, keeping the newest record in
scope. Visibility rules are honoured by checking the set of permitted user IDs,
and events older than the configured window are dropped eagerly.

## Urgency Scoring

Urgency is derived through the shared `notifications::priority` module. Domain
handlers may supply an `ActivityUrgencyHint`; otherwise the aggregator applies a
fallback heuristic based on entity state (e.g. `inreview`, `executorfailed`). A
recency penalty keeps stale updates from surfacing as high urgency.

## Metrics & Tracing

Each aggregation run emits a `activity_feed.aggregate` tracing span and records
its elapsed time in milliseconds under the `activity_feed.aggregate.ms` metric.
This instrumentation is lightweight and ready for export once a metrics backend
is connected.

## Configuration & Feature Flag

The activity feed is controlled by the `activity_feed` section in the user
configuration (`~/.config/vibe-kanban/config.json`).

```jsonc
{
  "activity_feed": {
    "enabled": true,
    "window_days": 21
  }
}
```

Disabling the flag short-circuits aggregation without requiring a restart.
Changing `window_days` allows experimentation with alternative slices.

## Testing

Unit tests live under `crates/services/src/activity_feed/aggregator.rs`. Run the
focused suite with:

```bash
cargo test --package services -- activity_feed
```

This exercises deduplication, visibility filtering, and urgency hints across the
supported event types.
