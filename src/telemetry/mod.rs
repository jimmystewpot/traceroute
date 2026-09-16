//! OpenTelemetry telemetry initialisation and baggage management.
//!
//! Provides tracer provider initialisation and automatic shutdown behaviour
//! via an RAII TelemetryGuard, following Australian English conventions.

pub mod baggage;

use opentelemetry::baggage::BaggageExt;
use opentelemetry::trace::{Link, Span, Tracer};
use opentelemetry::{global, KeyValue};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{
    runtime,
    trace::{BatchConfig, TracerProvider},
};
use std::net::IpAddr;
use std::time::Duration;

/// RAII guard ensuring tracer provider flushes and shuts down on drop.
pub struct TelemetryGuard {
    pub provider: TracerProvider,
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        let _ = self.provider.shutdown();
    }
}

/// Initialises the OpenTelemetry OTLP tracer provider.
///
/// Builds a gRPC (tonic) or HTTP (reqwest) OTLP exporter depending on `is_grpc`,
/// then installs a batch span processor backed by the Tokio runtime.
///
/// If the exporter or pipeline cannot be initialised — for example, because the
/// collector endpoint is unreachable — a warning is emitted and the function falls
/// back to a no-op `TracerProvider` that silently drops spans.  The fallback path
/// always returns `Ok(TelemetryGuard)` so that callers treat OTLP failure as
/// non-fatal.
pub fn init_telemetry(
    endpoint: &str,
    port: u16,
    is_grpc: bool,
    use_tls: bool,
    timeout: Duration,
) -> anyhow::Result<TelemetryGuard> {
    let scheme = if use_tls { "https" } else { "http" };
    let url = format!("{scheme}://{endpoint}:{port}");

    let provider = if is_grpc {
        build_grpc_provider(&url, timeout)
    } else {
        build_http_provider(&url, timeout)
    };

    opentelemetry::global::set_tracer_provider(provider.clone());
    Ok(TelemetryGuard { provider })
}

/// Builds a `TracerProvider` using the gRPC/tonic OTLP exporter.
///
/// Falls back to a no-op provider on any error, emitting a warning via `tracing`.
fn build_grpc_provider(url: &str, timeout: Duration) -> TracerProvider {
    let pipeline_result = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(
            opentelemetry_otlp::new_exporter()
                .tonic()
                .with_endpoint(url)
                .with_timeout(timeout),
        )
        .with_batch_config(BatchConfig::default())
        .install_batch(runtime::Tokio);

    match pipeline_result {
        Ok(provider) => provider,
        Err(err) => {
            tracing::warn!(
                "OTLP gRPC exporter pipeline failed to initialise ({}); \
                 running without telemetry export",
                err
            );
            TracerProvider::builder().build()
        }
    }
}

/// Builds a `TracerProvider` using the HTTP/reqwest OTLP exporter.
///
/// Falls back to a no-op provider on any error, emitting a warning via `tracing`.
fn build_http_provider(url: &str, timeout: Duration) -> TracerProvider {
    let pipeline_result = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(
            opentelemetry_otlp::new_exporter()
                .http()
                .with_endpoint(url)
                .with_timeout(timeout),
        )
        .with_batch_config(BatchConfig::default())
        .install_batch(runtime::Tokio);

    match pipeline_result {
        Ok(provider) => provider,
        Err(err) => {
            tracing::warn!(
                "OTLP HTTP exporter pipeline failed to initialise ({}); \
                 running without telemetry export",
                err
            );
            TracerProvider::builder().build()
        }
    }
}

/// Initialises and starts a root `"traceroute.trace"` OpenTelemetry span.
///
/// Sets standard attributes describing the traceroute operation, including destination IP,
/// protocol, maximum hops, and queries per hop. If cross-trace links are supplied, they are
/// attached to the span builder before construction.
/// The `baggage_map` is injected into the returned `opentelemetry::Context` as Baggage,
/// which can be propagated to child spans.
#[must_use]
pub fn start_trace_span(
    protocol: &str,
    destination: IpAddr,
    max_hops: u16,
    queries_per_hop: u16,
    links: Vec<Link>,
    baggage_map: std::collections::HashMap<String, String>,
) -> (global::BoxedSpan, opentelemetry::Context) {
    let tracer = global::tracer("traceroute");
    let mut builder = tracer
        .span_builder("traceroute.trace")
        .with_attributes(vec![
            KeyValue::new("destination", destination.to_string()),
            KeyValue::new("protocol", protocol.to_string()),
            KeyValue::new("max_hops", i64::from(max_hops)),
            KeyValue::new("queries_per_hop", i64::from(queries_per_hop)),
        ]);

    if !links.is_empty() {
        builder = builder.with_links(links);
    }

    let cx = opentelemetry::Context::current()
        .with_baggage(baggage_map.into_iter().map(|(k, v)| KeyValue::new(k, v)));

    let span = tracer.build_with_context(builder, &cx);
    (span, cx)
}

/// Records a `"traceroute.hop"` OpenTelemetry span for an individual probe measurement.
///
/// Creates a span capturing hop TTL, query index, reachability status, responding router IP
/// (if identified), and round-trip duration in milliseconds (if measured). If a parent span
/// context is provided, the hop span is parented to that context. The span is ended
/// immediately after recording.
pub fn record_hop_span(
    ttl: u16,
    query_index: u16,
    hop: &crate::engine::hop::TracerouteHop,
    parent_context: Option<&opentelemetry::Context>,
) {
    let tracer = global::tracer("traceroute");
    let mut attributes = vec![
        KeyValue::new("hop.ttl", i64::from(ttl)),
        KeyValue::new("query.index", i64::from(query_index)),
        KeyValue::new("hop.success", hop.success),
    ];

    if let Some(ip) = hop.address {
        attributes.push(KeyValue::new("router.ip", ip.to_string()));
    }

    if let Some(rtt) = hop.rtt {
        attributes.push(KeyValue::new("rtt_ms", rtt.as_secs_f64() * 1000.0));
    }

    let builder = tracer
        .span_builder("traceroute.hop")
        .with_attributes(attributes);

    let mut span = if let Some(parent_cx) = parent_context {
        tracer.build_with_context(builder, parent_cx)
    } else {
        tracer.build(builder)
    };
    span.end();
}
