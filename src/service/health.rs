//! Axum health-check HTTP server endpoint and statistics payload generation.
//! Designed and documented following Australian English conventions.

use axum::{http::StatusCode, routing::get, Json, Router};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Shared live daemon statistics updated by probe tasks and served by the health endpoint.
#[derive(Debug)]
pub struct HealthState {
    /// Name of the traceroute service (set once at startup).
    pub service_name: String,
    /// Hostname of the machine running the daemon (resolved once at startup).
    pub hostname: String,
    /// Count of successfully completed traceroute operations.
    pub successful_traces: AtomicU64,
    /// Count of failed traceroute operations (destination not reached).
    pub unsuccessful_traces: AtomicU64,
    /// Total count of traceroute operations attempted.
    pub total_traces: AtomicU64,
    /// Recent DNS resolution latencies in microseconds (ring buffer, max 100 entries).
    pub dns_latencies_us: Mutex<VecDeque<u64>>,
}

impl HealthState {
    /// Constructs a new [`HealthState`] resolving the system hostname automatically.
    pub fn new(service_name: impl Into<String>) -> Self {
        let hostname = hostname::get()
            .map(|h| h.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "unknown".to_string());
        Self {
            service_name: service_name.into(),
            hostname,
            successful_traces: AtomicU64::new(0),
            unsuccessful_traces: AtomicU64::new(0),
            total_traces: AtomicU64::new(0),
            dns_latencies_us: Mutex::new(VecDeque::with_capacity(100)),
        }
    }

    /// Records a DNS latency sample, evicting the oldest entry beyond 100 samples.
    ///
    /// If the underlying [`Mutex`] has been poisoned by a previous panic, the guard
    /// is safely recovered via [`PoisonError::into_inner`] so that no samples are
    /// silently dropped.
    pub fn record_dns_latency(&self, latency_us: u64) {
        let mut q = match self.dns_latencies_us.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        if q.len() >= 100 {
            q.pop_front();
        }
        q.push_back(latency_us);
    }
}

/// Health-check response payload reflecting the daemon status and statistics.
///
/// Field names preserve compatibility with the original Go service daemon
/// JSON serialisation format.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct HealthCheckResponse {
    /// Overall service status (e.g. "ok").
    pub status: String,

    /// Name of the monitored traceroute service.
    #[serde(rename = "service-name")]
    pub service_name: String,

    /// Hostname where the service daemon is executing.
    pub hostname: String,

    /// RFC 3339 formatted timestamp of the health-check query.
    #[serde(rename = "current-time")]
    pub current_time: String,

    /// Aggregate probe metrics and DNS resolution statistics.
    pub details: HealthCheckDetails,
}

/// Detailed execution statistics for the health-check response.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct HealthCheckDetails {
    /// Number of successfully completed traceroute operations.
    #[serde(rename = "successful-traces")]
    pub successful_traces: u64,

    /// Number of failed traceroute operations.
    #[serde(rename = "unsuccessful-traces")]
    pub unsuccessful_traces: u64,

    /// Total count of traceroute attempts initiated.
    #[serde(rename = "total-traces")]
    pub total_traces: u64,

    /// DNS lookup latencies recorded across probe runs.
    #[serde(rename = "dns-latency")]
    pub dns_latency: Vec<u64>,
}

/// Constructs a [`HealthCheckResponse`] with the provided execution statistics.
///
/// Automatically generates the current timestamp in RFC 3339 format utilising
/// the [`humantime`] crate.
pub fn create_health_payload(
    service_name: &str,
    hostname: &str,
    success: u64,
    fail: u64,
    total: u64,
) -> HealthCheckResponse {
    let now = std::time::SystemTime::now();
    let current_time = humantime::format_rfc3339(now).to_string();

    HealthCheckResponse {
        status: "ok".to_string(),
        service_name: service_name.to_string(),
        hostname: hostname.to_string(),
        current_time,
        details: HealthCheckDetails {
            successful_traces: success,
            unsuccessful_traces: fail,
            total_traces: total,
            dns_latency: Vec::new(),
        },
    }
}

/// Fallback handler for requests to unregistered endpoints.
///
/// Returns HTTP 400 Bad Request to mirror the behaviour of the original
/// Go health-check HTTP server.
async fn invalid_path_handler() -> (StatusCode, &'static str) {
    (StatusCode::BAD_REQUEST, "invalid request\n")
}

/// Creates an Axum [`Router`] serving the health-check payload reflecting live [`HealthState`].
///
/// Unregistered request paths fall back to returning HTTP 400 Bad Request.
pub fn create_health_router(path: &str, state: Arc<HealthState>) -> Router {
    let route_path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };

    Router::new()
        .route(
            &route_path,
            get(move || {
                let s = Arc::clone(&state);
                async move {
                    let now = std::time::SystemTime::now();
                    let current_time = humantime::format_rfc3339(now).to_string();
                    // Recover from a poisoned Mutex via into_inner so that DNS latency
                    // samples are always served rather than silently returning an empty slice.
                    let dns_lat = match s.dns_latencies_us.lock() {
                        Ok(guard) => guard.iter().copied().collect::<Vec<_>>(),
                        Err(poisoned) => poisoned.into_inner().iter().copied().collect::<Vec<_>>(),
                    };
                    Json(HealthCheckResponse {
                        status: "ok".to_string(),
                        service_name: s.service_name.clone(),
                        hostname: s.hostname.clone(),
                        current_time,
                        details: HealthCheckDetails {
                            successful_traces: s.successful_traces.load(Ordering::Relaxed),
                            unsuccessful_traces: s.unsuccessful_traces.load(Ordering::Relaxed),
                            total_traces: s.total_traces.load(Ordering::Relaxed),
                            dns_latency: dns_lat,
                        },
                    })
                }
            }),
        )
        .fallback(invalid_path_handler)
}

/// Runs the health-check HTTP server listening on the specified port.
///
/// Binds to all available local interfaces on `0.0.0.0:<port>`.
pub async fn run_health_server(
    port: u16,
    path: &str,
    state: Arc<HealthState>,
) -> Result<impl std::future::Future<Output = Result<(), std::io::Error>> + Send, std::io::Error> {
    let app = create_health_router(path, state);
    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    Ok(async move {
        axum::serve(listener, app)
            .await
            .map_err(std::io::Error::other)
    })
}

/// Runs the health-check HTTP server with support for graceful shutdown.
///
/// Awaits the provided `shutdown_signal` future before cleanly stopping the listener.
pub async fn run_health_server_with_shutdown<F>(
    port: u16,
    path: &str,
    state: Arc<HealthState>,
    shutdown_signal: F,
) -> Result<impl std::future::Future<Output = Result<(), std::io::Error>> + Send, std::io::Error>
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    let app = create_health_router(path, state);
    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    Ok(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_signal)
            .await
            .map_err(std::io::Error::other)
    })
}
