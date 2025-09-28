use std::time::Duration;

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEnvelope<T> {
    pub payload: T,
    pub etag: String,
    pub expires_at: DateTime<Utc>,
    pub stored_at: DateTime<Utc>,
}

impl<T> CacheEnvelope<T> {
    pub fn new(payload: T, etag: String, ttl: Duration) -> Self {
        let stored_at = Utc::now();
        let expires_at = stored_at
            + ChronoDuration::from_std(ttl).unwrap_or_else(|_| ChronoDuration::seconds(0));

        Self {
            payload,
            etag,
            expires_at,
            stored_at,
        }
    }

    pub fn is_expired(&self) -> bool {
        Utc::now() >= self.expires_at
    }
}
