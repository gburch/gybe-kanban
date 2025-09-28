pub fn record_timing(metric: &str, value_ms: f64) {
    tracing::info!(
        target: "metrics",
        metric = %metric,
        milliseconds = value_ms,
        "metric_timing"
    );
}
