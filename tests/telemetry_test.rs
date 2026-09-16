use std::time::Duration;
use tokio::sync::Mutex;
use traceroute::telemetry::baggage::build_traceroute_baggage;
use traceroute::telemetry::init_telemetry;

static TEST_LOCK: Mutex<()> = Mutex::const_new(());

#[test]
fn test_baggage_attributes_creation() {
    let baggage = build_traceroute_baggage("target.example.com", "local-host", 30, "test-xid");
    assert_eq!(
        baggage.get("destination_hostname").map(|v| v.as_str()),
        Some("target.example.com")
    );
    assert_eq!(
        baggage.get("source").map(|v| v.as_str()),
        Some("local-host")
    );
    assert_eq!(baggage.get("max_hops").map(|v| v.as_str()), Some("30"));
    assert_eq!(baggage.get("xid").map(|v| v.as_str()), Some("test-xid"));
}

#[test]
fn test_baggage_reflects_hostname_and_configured_destination() {
    let source_host = hostname::get()
        .map(|h| h.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "localhost".to_string());
    let baggage = traceroute::telemetry::baggage::build_traceroute_baggage(
        "example.com",
        &source_host,
        30,
        "test-xid",
    );
    assert_eq!(
        baggage.get("destination_hostname"),
        Some(&"example.com".to_string())
    );
    assert_eq!(baggage.get("source"), Some(&source_host));
}

#[tokio::test(flavor = "multi_thread")]
async fn test_init_telemetry_provider() {
    let _lock = TEST_LOCK.lock().await;
    let guard_res = init_telemetry("localhost", 4317, true, false, Duration::from_secs(5));
    assert!(guard_res.is_ok());
    let guard = guard_res.unwrap();
    drop(guard);
}

/// Verifies that `init_telemetry` gracefully falls back to a no-op provider when
/// an unreachable OTLP endpoint is provided, rather than returning an error.
/// Uses TEST-NET-1 (RFC 5737) address 192.0.2.1, which is guaranteed to be
/// unreachable in any environment.
#[tokio::test(flavor = "multi_thread")]
async fn test_init_telemetry_unreachable_endpoint() {
    let _lock = TEST_LOCK.lock().await;
    let result = init_telemetry("192.0.2.1", 4317, true, false, Duration::from_millis(100));
    assert!(
        result.is_ok(),
        "init_telemetry must return Ok(_) even when the OTLP endpoint is unreachable; \
         got: {:?}",
        result.err()
    );
    if let Ok(guard) = result {
        drop(guard);
    }
}

/// Verifies that `start_trace_span` creates a span with a valid `SpanContext`,
/// and `record_hop_span` executes without panics and records hop metrics.
#[tokio::test(flavor = "multi_thread")]
async fn test_trace_and_hop_spans_recording() {
    let _lock = TEST_LOCK.lock().await;
    use opentelemetry::trace::Span;
    use std::net::IpAddr;
    use traceroute::engine::hop::TracerouteHop;
    use traceroute::telemetry::{record_hop_span, start_trace_span};

    let guard = init_telemetry("localhost", 4317, true, false, Duration::from_millis(100))
        .expect("initialising telemetry succeeds");

    let dest: IpAddr = "127.0.0.1".parse().expect("valid IP address");
    let (mut span, cx) = start_trace_span(
        "udp",
        dest,
        30,
        3,
        Vec::new(),
        std::collections::HashMap::new(),
    );
    let span_ctx = span.span_context().clone();
    use opentelemetry::trace::TraceContextExt;
    let full_cx = cx.with_remote_span_context(span_ctx.clone());
    assert!(
        span_ctx.is_valid(),
        "start_trace_span must produce a span with valid SpanContext"
    );

    let hop = TracerouteHop {
        success: true,
        address: Some(dest),
        ttl: 1,
        rtt: Some(Duration::from_millis(5)),
    };
    record_hop_span(1, 0, &hop, Some(&full_cx));

    let failed_hop = TracerouteHop {
        success: false,
        address: None,
        ttl: 2,
        rtt: None,
    };
    record_hop_span(2, 1, &failed_hop, None);

    span.end();

    // Verify linked span creation
    let link = opentelemetry::trace::Link::new(span_ctx.clone(), Vec::new(), 0);
    let (mut linked_span, _linked_cx) = start_trace_span(
        "udp",
        dest,
        30,
        3,
        vec![link],
        std::collections::HashMap::new(),
    );
    assert!(
        linked_span.span_context().is_valid(),
        "linked span must have valid SpanContext"
    );
    linked_span.end();

    drop(guard);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_traceroute_with_span_context_and_links() {
    let _lock = TEST_LOCK.lock().await;
    use std::net::IpAddr;
    use traceroute::engine::tcp::{run_tcp_traceroute_with_span_context, TcpTracerouteConfig};
    use traceroute::engine::udp::{run_udp_traceroute_with_span_context, UdpTracerouteConfig};

    let guard = init_telemetry("localhost", 4317, true, false, Duration::from_millis(100))
        .expect("initialising telemetry succeeds");

    let dest: IpAddr = "127.0.0.1".parse().expect("valid IP address");
    let udp_cfg = UdpTracerouteConfig {
        destination: dest,
        destination_name: None,
        max_hops: 1,
        queries_per_hop: 1,
        parallel_requests: 1,
        timeout: Duration::from_millis(100),
        dest_port: 33434,
        span_links: Vec::new(),
        probe_limiter: None,
    };
    let (_hops, span_ctx) = run_udp_traceroute_with_span_context(udp_cfg)
        .await
        .expect("udp trace succeeds");
    assert!(span_ctx.is_valid());

    let link = opentelemetry::trace::Link::new(span_ctx.clone(), Vec::new(), 0);
    let tcp_cfg = TcpTracerouteConfig {
        destination: dest,
        destination_name: None,
        max_hops: 1,
        queries_per_hop: 1,
        parallel_requests: 1,
        timeout: Duration::from_millis(100),
        dest_port: 80,
        span_links: vec![link],
        probe_limiter: None,
    };
    let (_hops2, span_ctx2) = run_tcp_traceroute_with_span_context(tcp_cfg)
        .await
        .expect("tcp trace succeeds");
    assert!(span_ctx2.is_valid());

    drop(guard);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_telemetry_probe_span_attributes_and_links() {
    let _lock = TEST_LOCK.lock().await;
    use std::net::IpAddr;
    use traceroute::engine::udp::{run_udp_traceroute_with_span_context, UdpTracerouteConfig};

    let guard = init_telemetry("localhost", 4317, true, false, Duration::from_millis(100))
        .expect("initialising telemetry succeeds");

    let dest: IpAddr = "127.0.0.1".parse().expect("valid IP address");
    let udp_cfg = UdpTracerouteConfig {
        destination: dest,
        destination_name: None,
        max_hops: 1,
        queries_per_hop: 2,
        parallel_requests: 2,
        timeout: Duration::from_millis(200),
        dest_port: 33434,
        span_links: Vec::new(),
        probe_limiter: None,
    };

    let (hops, span_ctx) = run_udp_traceroute_with_span_context(udp_cfg)
        .await
        .expect("udp trace succeeds");

    assert!(span_ctx.is_valid(), "Root span context must be valid");
    assert_ne!(
        span_ctx.trace_id(),
        opentelemetry::trace::TraceId::INVALID,
        "Trace ID must be non-zero"
    );
    assert_ne!(
        span_ctx.span_id(),
        opentelemetry::trace::SpanId::INVALID,
        "Span ID must be non-zero"
    );

    // Verify that hop results were gathered and at least one query completed
    let hop1 = hops.get(&1).expect("Hop 1 must be present");
    assert_eq!(hop1.len(), 2, "Expected 2 queries recorded for hop 1");

    // Create a linked span referencing the completed trace context
    let link = opentelemetry::trace::Link::new(span_ctx.clone(), Vec::new(), 0);
    let linked_cfg = UdpTracerouteConfig {
        destination: dest,
        destination_name: None,
        max_hops: 1,
        queries_per_hop: 1,
        parallel_requests: 1,
        timeout: Duration::from_millis(200),
        dest_port: 33434,
        span_links: vec![link],
        probe_limiter: None,
    };

    let (_hops2, linked_ctx) = run_udp_traceroute_with_span_context(linked_cfg)
        .await
        .expect("linked trace succeeds");

    assert!(
        linked_ctx.is_valid(),
        "Linked trace span context must be valid"
    );
    assert_ne!(
        linked_ctx.span_id(),
        span_ctx.span_id(),
        "Linked trace must have distinct span ID"
    );

    drop(guard);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_trace_with_populated_baggage() {
    let _lock = TEST_LOCK.lock().await;
    use opentelemetry::trace::Span;
    use std::net::IpAddr;
    use traceroute::telemetry::start_trace_span;

    let guard = init_telemetry(
        "localhost",
        4317,
        true,
        false,
        std::time::Duration::from_millis(100),
    )
    .expect("initialising telemetry succeeds");

    let dest: IpAddr = "127.0.0.1".parse().unwrap();
    let mut baggage_map = std::collections::HashMap::new();
    baggage_map.insert("test-key".to_string(), "test-value".to_string());

    let (mut span, cx) = start_trace_span("udp", dest, 30, 3, Vec::new(), baggage_map);
    assert!(span.span_context().is_valid());

    use opentelemetry::baggage::BaggageExt;
    let baggage = cx.baggage();
    assert_eq!(
        baggage.get("test-key").map(|v| v.to_string()),
        Some("test-value".to_string())
    );

    span.end();
    drop(guard);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_init_telemetry_with_tls_enabled() {
    let _lock = TEST_LOCK.lock().await;
    // Verifies that initialising telemetry with use_tls = true constructs an https endpoint
    // without failing or panicking when TLS features are active.
    let guard = init_telemetry("127.0.0.1", 4317, true, true, Duration::from_millis(50));
    assert!(guard.is_ok());

    let http_guard = init_telemetry("127.0.0.1", 4318, false, true, Duration::from_millis(50));
    assert!(http_guard.is_ok());
}
