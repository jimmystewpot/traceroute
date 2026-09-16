//! Integration tests for Axum health-check service and JSON payload formatting.
//! Written following Australian English conventions and strict TDD methodology.

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tower::ServiceExt;
use traceroute::service::health::{
    create_health_payload, create_health_router, run_health_server_with_shutdown,
    HealthCheckResponse, HealthState,
};

#[test]
fn test_healthcheck_json_payload_format() {
    let payload = create_health_payload("opentelemetry-traceroute", "test-host", 10, 2, 12);
    assert_eq!(payload.status, "ok");
    assert_eq!(payload.service_name, "opentelemetry-traceroute");
    assert_eq!(payload.hostname, "test-host");
    assert_eq!(payload.details.total_traces, 12);
    assert_eq!(payload.details.successful_traces, 10);
    assert_eq!(payload.details.unsuccessful_traces, 2);
    assert!(payload.details.dns_latency.is_empty());

    // Verify JSON serialisation matches Go daemon key names exactly.
    let json_val = serde_json::to_value(&payload).unwrap();
    assert_eq!(json_val["status"], "ok");
    assert_eq!(json_val["service-name"], "opentelemetry-traceroute");
    assert_eq!(json_val["hostname"], "test-host");
    assert!(json_val["current-time"].is_string());
    assert_eq!(json_val["details"]["successful-traces"], 10);
    assert_eq!(json_val["details"]["unsuccessful-traces"], 2);
    assert_eq!(json_val["details"]["total-traces"], 12);
    assert!(json_val["details"]["dns-latency"].is_array());
}

#[tokio::test]
async fn test_healthcheck_endpoint_oneshot() {
    let state = Arc::new(HealthState::new("opentelemetry-traceroute"));
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };

    let server_fut =
        traceroute::service::health::run_health_server(port, "/_healthcheck", Arc::clone(&state))
            .await
            .unwrap();
    let server_handle = tokio::spawn(server_fut);

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"))
        .await
        .unwrap();
    stream
        .write_all(b"GET /_healthcheck HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();

    let mut response_buf = Vec::new();
    stream.read_to_end(&mut response_buf).await.unwrap();
    let response_str = String::from_utf8_lossy(&response_buf);
    assert!(response_str.starts_with("HTTP/1.1 200 OK"));
    assert!(response_str.contains("application/json"));

    let body_start = response_str.find("\r\n\r\n").unwrap() + 4;
    let body_str = &response_str[body_start..];

    let resp_payload: HealthCheckResponse = serde_json::from_str(body_str).unwrap();
    assert_eq!(resp_payload.status, "ok");
    assert_eq!(resp_payload.service_name, "opentelemetry-traceroute");
    assert_eq!(resp_payload.hostname, state.hostname);

    server_handle.abort();
}

#[tokio::test]
async fn test_healthcheck_unregistered_path_bad_request() {
    let state = Arc::new(HealthState::new("test-service"));
    let app = create_health_router("/_healthcheck", Arc::clone(&state));

    let req = Request::builder()
        .uri("/unregistered/path")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_healthcheck_custom_path() {
    let state = Arc::new(HealthState::new("test-service"));
    let app = create_health_router("/custom/status", Arc::clone(&state));

    let req = Request::builder()
        .uri("/custom/status")
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let req_wrong = Request::builder()
        .uri("/_healthcheck")
        .body(Body::empty())
        .unwrap();

    let response_wrong = app.oneshot(req_wrong).await.unwrap();
    assert_eq!(response_wrong.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_healthcheck_path_without_leading_slash() {
    let state = Arc::new(HealthState::new("test-service"));
    let app = create_health_router("status", Arc::clone(&state));

    let req = Request::builder()
        .uri("/status")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_run_health_server_with_graceful_shutdown() {
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    let state = Arc::new(HealthState::new("opentelemetry-traceroute"));
    let server_handle = tokio::spawn(async move {
        let fut =
            run_health_server_with_shutdown(port, "/_healthcheck", Arc::clone(&state), async {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap();
        fut.await
    });

    // Brief delay to ensure listener has bound.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Send HTTP GET request via standard TCP stream.
    let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"))
        .await
        .unwrap();
    stream
        .write_all(b"GET /_healthcheck HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();

    let mut response_buf = Vec::new();
    stream.read_to_end(&mut response_buf).await.unwrap();
    let response_str = String::from_utf8_lossy(&response_buf);

    assert!(response_str.starts_with("HTTP/1.1 200 OK"));
    assert!(response_str.contains("\"service-name\":\"opentelemetry-traceroute\""));

    // Signal graceful shutdown.
    let _ = shutdown_tx.send(());
    let server_result = server_handle.await.unwrap();
    assert!(server_result.is_ok());
}

#[tokio::test]
async fn test_healthcheck_reflects_live_counters() {
    let state = Arc::new(HealthState::new("test"));
    state.successful_traces.store(5, Ordering::Relaxed);
    state.unsuccessful_traces.store(2, Ordering::Relaxed);
    state.total_traces.store(7, Ordering::Relaxed);
    state.record_dns_latency(450);
    state.record_dns_latency(780);

    let app = create_health_router("/_healthcheck", Arc::clone(&state));
    let req = Request::builder()
        .uri("/_healthcheck")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let resp_payload: HealthCheckResponse = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(resp_payload.status, "ok");
    assert_eq!(resp_payload.service_name, "test");
    assert_eq!(resp_payload.hostname, state.hostname);
    assert!(!resp_payload.hostname.is_empty());
    assert_eq!(resp_payload.details.successful_traces, 5);
    assert_eq!(resp_payload.details.unsuccessful_traces, 2);
    assert_eq!(resp_payload.details.total_traces, 7);
    assert_eq!(resp_payload.details.dns_latency, vec![450, 780]);

    // Also assert raw JSON field format
    let json_val: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(json_val["service-name"], "test");
    assert_eq!(json_val["hostname"], state.hostname);
    assert!(!json_val["hostname"].as_str().unwrap_or("").is_empty());
    assert_eq!(json_val["details"]["successful-traces"], 5);
    assert_eq!(json_val["details"]["unsuccessful-traces"], 2);
    assert_eq!(json_val["details"]["total-traces"], 7);
    assert_eq!(
        json_val["details"]["dns-latency"],
        serde_json::json!([450, 780])
    );
}

#[test]
fn test_health_state_dns_latency_ring_buffer_capacity() {
    let state = HealthState::new("ring-buffer-test");
    for i in 1..=105 {
        state.record_dns_latency(i);
    }
    let latencies = state
        .dns_latencies_us
        .lock()
        .unwrap()
        .iter()
        .copied()
        .collect::<Vec<_>>();
    assert_eq!(latencies.len(), 100);
    // Oldest 5 entries (1..=5) should have been popped; entries should be 6..=105
    assert_eq!(latencies[0], 6);
    assert_eq!(latencies[99], 105);
}

#[tokio::test]
async fn test_run_daemon_single_iteration_and_cancel() {
    let config_yaml = r#"
schema-version: "1.0.0"
destinations:
  - "127.0.0.1"
globals:
  protocol: "udp"
  max-hops: 1
  number-queries: 1
  parallel-requests: 1
  timeout: 50ms
  source-port: 33434
  interval: 10s
opentelemetry:
  destination: "localhost"
  tls: false
  port: 4317
  grpc: true
healthcheck:
  path: "/_healthcheck"
  enabled: false
  port: 8080
"#;
    let cfg = traceroute::config::load_config_from_str(config_yaml).unwrap();
    let state = Arc::new(HealthState::new("daemon-test"));

    // Run daemon with a brief timeout so it performs the initial tick and then stops.
    let _ = tokio::time::timeout(
        std::time::Duration::from_millis(200),
        traceroute::service::run_daemon(cfg, Arc::clone(&state)),
    )
    .await;

    // After the initial tick, at least one trace attempt has been recorded.
    assert!(state.total_traces.load(Ordering::Relaxed) >= 1);
}

#[tokio::test]
async fn test_run_daemon_spawns_health_server() {
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let config_yaml = format!(
        r#"
schema-version: "1.0.0"
destinations:
  - "127.0.0.1"
globals:
  protocol: "udp"
  max-hops: 1
  number-queries: 1
  parallel-requests: 1
  timeout: 50ms
  source-port: 33434
  interval: 10s
opentelemetry:
  destination: "localhost"
  tls: false
  port: 4317
  grpc: true
healthcheck:
  path: "/_healthcheck"
  enabled: true
  port: {port}
"#
    );
    let cfg = traceroute::config::load_config_from_str(&config_yaml).unwrap();
    let state = Arc::new(HealthState::new("daemon-health-test"));

    let daemon_handle = tokio::spawn(traceroute::service::run_daemon(cfg, Arc::clone(&state)));

    // Brief delay to allow health server to bind and start listening.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Send HTTP GET request via standard TCP stream to verify the health server started by run_daemon.
    let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"))
        .await
        .unwrap();
    stream
        .write_all(b"GET /_healthcheck HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();

    let mut response_buf = Vec::new();
    stream.read_to_end(&mut response_buf).await.unwrap();
    let response_str = String::from_utf8_lossy(&response_buf);
    assert!(response_str.starts_with("HTTP/1.1 200 OK"));
    assert!(response_str.contains("\"service-name\":\"daemon-health-test\""));

    daemon_handle.abort();
}

#[tokio::test]
async fn test_run_daemon_tcp_single_iteration_and_cancel() {
    let config_yaml = r#"
schema-version: "1.0.0"
destinations:
  - "127.0.0.1"
globals:
  protocol: "tcp"
  max-hops: 1
  number-queries: 1
  parallel-requests: 1
  timeout: 50ms
  source-port: 80
  interval: 10s
opentelemetry:
  destination: "localhost"
  tls: false
  port: 4317
  grpc: true
healthcheck:
  path: "/_healthcheck"
  enabled: false
  port: 8080
"#;
    let cfg = traceroute::config::load_config_from_str(config_yaml).unwrap();
    let state = Arc::new(HealthState::new("daemon-tcp-test"));

    let _ = tokio::time::timeout(
        std::time::Duration::from_millis(200),
        traceroute::service::run_daemon(cfg, Arc::clone(&state)),
    )
    .await;

    assert!(state.total_traces.load(Ordering::Relaxed) >= 1);
}

#[tokio::test]
async fn test_run_daemon_graceful_drain_on_shutdown() {
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let config_yaml = format!(
        r#"
schema-version: "1.0.0"
destinations:
  - "127.0.0.1"
globals:
  protocol: "udp"
  max-hops: 1
  number-queries: 1
  parallel-requests: 1
  timeout: 50ms
  source-port: 33434
  interval: 10s
opentelemetry:
  destination: "localhost"
  tls: false
  port: 4317
  grpc: true
healthcheck:
  path: "/_healthcheck"
  enabled: true
  port: {port}
"#
    );
    let cfg = traceroute::config::load_config_from_str(&config_yaml).unwrap();
    let state = Arc::new(HealthState::new("daemon-graceful-shutdown-test"));

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    let state_clone = Arc::clone(&state);
    let daemon_handle = tokio::spawn(async move {
        traceroute::service::run_daemon_with_shutdown(cfg, state_clone, async {
            let _ = shutdown_rx.await;
        })
        .await
    });

    // Brief delay to allow health server to bind and start listening.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Verify health server is actively serving.
    let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"))
        .await
        .unwrap();
    stream
        .write_all(b"GET /_healthcheck HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut response_buf = Vec::new();
    stream.read_to_end(&mut response_buf).await.unwrap();
    let response_str = String::from_utf8_lossy(&response_buf);
    assert!(response_str.starts_with("HTTP/1.1 200 OK"));

    // Trigger graceful shutdown.
    let _ = shutdown_tx.send(());

    // Await daemon completion and ensure it returns Ok(()).
    let daemon_result = daemon_handle.await.unwrap();
    assert!(daemon_result.is_ok());

    // Verify health server port is released (TCP connection is refused).
    let connect_res = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}")).await;
    assert!(connect_res.is_err());

    // Verify in-flight traces completed and updated state.total_traces.
    assert!(state.total_traces.load(Ordering::Relaxed) >= 1);
}

#[tokio::test]
async fn test_run_daemon_with_linked_spans_enabled() {
    let config_yaml = r#"
schema-version: "1.0.0"
destinations:
  - "127.0.0.1"
globals:
  protocol: "udp"
  max-hops: 1
  number-queries: 1
  parallel-requests: 1
  timeout: 50ms
  source-port: 33434
  interval: 100ms
opentelemetry:
  destination: "localhost"
  tls: false
  port: 4317
  grpc: true
  linked-spans: true
healthcheck:
  path: "/_healthcheck"
  enabled: false
  port: 8080
"#;
    let cfg = traceroute::config::load_config_from_str(config_yaml).unwrap();
    assert!(cfg.opentelemetry.linked_spans);
    let state = Arc::new(HealthState::new("daemon-linked-spans-test"));

    let _ = tokio::time::timeout(
        std::time::Duration::from_millis(250),
        traceroute::service::run_daemon(cfg, Arc::clone(&state)),
    )
    .await;

    assert!(state.total_traces.load(Ordering::Relaxed) >= 1);
}

#[test]
fn test_health_state_poisoned_lock_recovery() {
    let state = Arc::new(HealthState::new("test-service"));
    let state_clone = Arc::clone(&state);

    // Intentionally panic inside a thread holding the lock to poison it.
    let _ = std::panic::catch_unwind(|| {
        let _guard = state_clone.dns_latencies_us.lock().unwrap();
        panic!("simulated panic to poison mutex");
    });

    assert!(state.dns_latencies_us.is_poisoned());

    // record_dns_latency must safely recover and record the sample.
    state.record_dns_latency(12345);

    let _router = create_health_router("/healthz", Arc::clone(&state));
    // Verify the sample was recorded despite the poisoned mutex.
    let samples = match state.dns_latencies_us.lock() {
        Ok(g) => g.iter().copied().collect::<Vec<_>>(),
        Err(p) => p.into_inner().iter().copied().collect::<Vec<_>>(),
    };
    assert_eq!(samples, vec![12345]);
}
