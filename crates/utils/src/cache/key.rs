use uuid::Uuid;

pub fn activity_feed_cache_key(project_id: Uuid, scope: &str, cursor: Option<&str>) -> String {
    match cursor {
        Some(cursor) if !cursor.is_empty() => {
            format!("activity_feed:{project_id}:{scope}:{cursor}")
        }
        _ => format!("activity_feed:{project_id}:{scope}:root"),
    }
}
