//! Observability module: structured JSON logging and Prometheus-compatible metrics.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Types reused from the core crate (kept as String aliases for decoupling).
pub type SessionId = String;
pub type FileId = String;

// ---------------------------------------------------------------------------
// Domain types
// ---------------------------------------------------------------------------

/// The kind of transfer event being logged.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum EventType {
    Start,
    ChunkComplete,
    Complete,
    Failed,
    Retry,
}

/// A structured transfer event ready for logging.
///
/// Every event carries `correlation_id`, `session_id`, `event_type`, and
/// `timestamp` (Req 20.1, 20.4).  Failure events additionally carry
/// `file_id`, `failed_chunk_indices`, and `failure_reason` (Req 20.3).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransferEvent {
    pub correlation_id: String,
    pub session_id: SessionId,
    pub event_type: EventType,
    pub timestamp: DateTime<Utc>,
    /// Arbitrary extra details (e.g. chunk index, throughput snapshot).
    #[serde(default)]
    pub details: serde_json::Value,
    // --- failure-specific fields (Req 20.3) ---
    /// Present only on failure events.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_id: Option<FileId>,
    /// Indices of chunks that failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed_chunk_indices: Option<Vec<u64>>,
    /// Human-readable failure reason.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,
}

/// A single metric data point.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MetricPoint {
    pub name: String,
    pub value: f64,
    pub labels: HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// Metrics registry (simple in-memory)
// ---------------------------------------------------------------------------

/// A simple in-memory metrics registry backed by a `HashMap`.
#[derive(Debug, Default)]
pub struct MetricsRegistry {
    gauges: Mutex<HashMap<String, f64>>,
}

impl MetricsRegistry {
    pub fn new() -> Self {
        Self {
            gauges: Mutex::new(HashMap::new()),
        }
    }

    /// Record (accumulate) a metric value.
    pub fn record(&self, name: &str, value: f64) {
        let mut map = self.gauges.lock().unwrap();
        let entry = map.entry(name.to_string()).or_insert(0.0);
        *entry += value;
    }

    /// Retrieve the current accumulated value for a metric.
    pub fn get(&self, name: &str) -> Option<f64> {
        self.gauges.lock().unwrap().get(name).copied()
    }

    /// Render all metrics in Prometheus text exposition format.
    pub fn render_prometheus(&self) -> String {
        let map = self.gauges.lock().unwrap();
        let mut lines: Vec<String> = map
            .iter()
            .map(|(k, v)| format!("{} {}", k, v))
            .collect();
        lines.sort();
        lines.join("\n")
    }

    /// Snapshot of current metrics (useful for testing).
    pub fn snapshot(&self) -> HashMap<String, f64> {
        self.gauges.lock().unwrap().clone()
    }
}

// ---------------------------------------------------------------------------
// ObservabilityModule
// ---------------------------------------------------------------------------

/// Central observability facade providing structured logging and metrics.
pub struct ObservabilityModule {
    /// Collected JSON log lines (also printed to stdout).
    log_entries: Mutex<Vec<String>>,
    /// In-memory metrics registry.
    metrics: Arc<MetricsRegistry>,
}

impl ObservabilityModule {
    /// Create a new `ObservabilityModule` with an empty log buffer and fresh
    /// metrics registry.
    pub fn new() -> Self {
        Self {
            log_entries: Mutex::new(Vec::new()),
            metrics: Arc::new(MetricsRegistry::new()),
        }
    }

    /// Create with a shared metrics registry (useful when the registry is
    /// also handed to the HTTP endpoint).
    pub fn with_registry(registry: Arc<MetricsRegistry>) -> Self {
        Self {
            log_entries: Mutex::new(Vec::new()),
            metrics: registry,
        }
    }

    /// Return a reference to the underlying metrics registry.
    pub fn registry(&self) -> &Arc<MetricsRegistry> {
        &self.metrics
    }

    // -- Logging -----------------------------------------------------------

    /// Log a transfer event as structured JSON (Req 20.1, 20.4).
    ///
    /// The JSON entry always contains `correlation_id`, `event_type`,
    /// `session_id`, `timestamp`, and `details`.
    /// Failure events additionally contain `file_id`, `failed_chunk_indices`,
    /// and `failure_reason` (Req 20.3).
    pub fn log_transfer_event(&self, event: TransferEvent) {
        let json = serde_json::to_string(&event).expect("TransferEvent is always serialisable");
        // Print to stdout for production use
        println!("{}", json);
        // Store for programmatic access / testing
        self.log_entries.lock().unwrap().push(json);
    }

    /// Log a terminal transfer failure with full context (Req 20.3).
    ///
    /// Convenience method that builds a `TransferEvent` with failure-specific
    /// fields and delegates to `log_transfer_event`.
    pub fn log_transfer_failure(
        &self,
        session_id: SessionId,
        file_id: FileId,
        failed_chunks: &[u64],
        reason: &str,
    ) {
        let event = TransferEvent {
            correlation_id: session_id.clone(),
            session_id,
            event_type: EventType::Failed,
            timestamp: Utc::now(),
            details: serde_json::Value::Null,
            file_id: Some(file_id),
            failed_chunk_indices: Some(failed_chunks.to_vec()),
            failure_reason: Some(reason.to_string()),
        };
        self.log_transfer_event(event);
    }

    // -- Metrics -----------------------------------------------------------

    /// Record a metric data point (Req 20.2).
    ///
    /// Supported metric names include (but are not limited to):
    /// `transfer_throughput_bytes`, `transfer_latency_ms`,
    /// `transfer_retry_count`, `transfer_failure_count`.
    pub fn record_metric(&self, metric: MetricPoint) {
        self.metrics.record(&metric.name, metric.value);
    }

    // -- Accessors (mainly for testing) ------------------------------------

    /// Return a clone of all collected log entry strings.
    pub fn log_entries(&self) -> Vec<String> {
        self.log_entries.lock().unwrap().clone()
    }
}

// ---------------------------------------------------------------------------
// Tracing subscriber initialisation (Req 20.1)
// ---------------------------------------------------------------------------

/// Initialise a `tracing-subscriber` with JSON formatting for the Tauri app.
///
/// Call this once during application startup (e.g. in the Tauri `setup` hook)
/// to wire structured JSON logging to stdout.
pub fn init_tracing_subscriber() {
    use tracing_subscriber::{fmt, EnvFilter};

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    fmt()
        .json()
        .with_env_filter(filter)
        .with_target(true)
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true)
        .init();
}

// ---------------------------------------------------------------------------
// Prometheus-compatible HTTP metrics endpoint (Req 11.3)
// ---------------------------------------------------------------------------

/// Build an `axum::Router` that exposes a `/metrics` endpoint returning
/// Prometheus text exposition format.
pub fn metrics_router(registry: Arc<MetricsRegistry>) -> axum::Router {
    use axum::routing::get;

    axum::Router::new().route(
        "/metrics",
        get(move || {
            let reg = Arc::clone(&registry);
            async move { reg.render_prometheus() }
        }),
    )
}
