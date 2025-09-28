# Project Activity Feed

The activity feed surfaces the most recent project events in a normalized JSON
format. Consumers can either page the REST endpoint or subscribe to the
WebSocket channel for live updates.

## REST

`GET /api/projects/:projectId/activity_feed`

### Query Parameters

- `cursor` *(optional)* – opaque string returned by previous requests to page
  older results.
- `scope` *(optional)* – `mine` (default) filters to events visible to the
  current user, `all` requires admin privileges and returns every event.

### Behaviour

- Results are returned in batches of 25, ordered by `createdAt` descending.
- The response includes an `ETag`. Repeat the call with `If-None-Match` to take
  advantage of the Redis-backed cache. The TTL is configurable via
  `VIBE_ACTIVITY_FEED_CACHE_TTL` (seconds, defaults to 30).
- Requests with `scope=all` are rejected with **403 Forbidden** unless the
  environment variable `VIBE_ACTIVITY_FEED_SCOPE_ALL` is set to one of
  `true|1|yes`.

### Example Response

```json
{
  "success": true,
  "data": {
    "events": [
      {
        "id": "0f5b8441-20aa-4e9f-8b1d-7a45d4e8887e",
        "headline": "Task updated",
        "summary": "Status: inreview",
        "cta": null,
        "urgencyScore": 78,
        "actionRequired": true,
        "createdAt": "2025-09-28T06:15:12.973Z"
      }
    ],
    "nextCursor": "MTY5NTg1MjEyOTczOjBmNWI4NDQxLTIwYWEtNGU5Zi04YjFkLTdhNDVkNGU4ODg3ZQ"
  }
}
```

## WebSocket

`WS /api/projects/:projectId/activity_feed`

### Query Parameters

- `cursor` *(optional)* – resume token; events newer than this cursor are
  replayed on connect.
- `scope` *(optional)* – mirrors the REST parameter and is subject to the same
  admin check.

### Message Format

Every update is delivered as a JSON object with the shape below. `changeType`
will be one of `created`, `updated`, or `removed`. For removals the `event`
object contains the last known payload so clients can reconcile local state.

```json
{
  "type": "activity_feed.update",
  "payload": {
    "event": {
      "id": "0f5b8441-20aa-4e9f-8b1d-7a45d4e8887e",
      "changeType": "created",
      "event": {
        "headline": "Task updated",
        "summary": "Status: inreview",
        "cta": null,
        "urgencyScore": 78,
        "actionRequired": true,
        "createdAt": "2025-09-28T06:15:12.973Z"
      }
    }
  }
}
```

The connection polls for changes every two seconds and broadcasts deltas to all
subscribers. Cache entries are invalidated automatically whenever an update is
emitted so subsequent REST calls observe the fresh payload.
