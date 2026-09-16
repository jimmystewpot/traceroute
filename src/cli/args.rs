//! Command-line argument definitions and structures.
//!
//! Documented following Australian English conventions.

use clap::Args;
use std::time::Duration;

/// Common options shared across traceroute commands.
#[derive(Args, Debug, Clone)]
pub struct CommonTraceOptions {
    /// Maximum number of hops (time-to-live) to traverse.
    #[arg(short = 'm', long, default_value_t = 30, env = "TRACE_MAXHOPS")]
    pub max_hops: u16,

    /// Number of probe queries to send per hop.
    #[arg(short = 'q', long, default_value_t = 3, env = "TRACE_NQUERIES")]
    pub n_queries: u16,

    /// Maximum number of parallel requests in flight.
    #[arg(short = 'N', long, default_value_t = 16, env = "TRACE_PARALLEL")]
    pub parallel_requests: u16,

    /// Response timeout duration before considering a probe dropped.
    #[arg(short = 'w', long, default_value = "2s", env = "TRACE_TIMEOUT", value_parser = humantime::parse_duration)]
    pub timeout: Duration,

    /// Source port or destination port depending on traceroute mode.
    #[arg(short = 'p', long, default_value_t = 33434, env = "TRACE_SRC_PORT")]
    pub trace_route_port: u16,

    /// OpenTelemetry collector destination hostname or IP.
    #[arg(long, default_value = "localhost", env = "TRACE_OTEL_DEST")]
    pub otel_dest: String,

    /// Enable TLS when exporting traces to the OpenTelemetry collector.
    #[arg(long, default_value_t = false, env = "TRACE_OTEL_TLS")]
    pub otel_tls: bool,

    /// Use gRPC for exporting OpenTelemetry traces instead of HTTP.
    #[arg(long, default_value_t = true, env = "TRACE_OTEL_GRPC")]
    pub otel_grpc: bool,

    /// Port of the OpenTelemetry collector endpoint.
    #[arg(long, default_value_t = 4317, env = "TRACE_OTEL_PORT")]
    pub otel_port: u16,

    /// Enable linked spans in OpenTelemetry traces to link consecutive runs
    #[arg(
        long = "otel-linked-spans",
        env = "TRACEROUTE_OTEL_LINKED_SPANS",
        default_value_t = false
    )]
    pub otel_linked_spans: bool,

    /// Target destination address or hostname to trace.
    #[arg(long, env = "TRACE_DESTINATION")]
    pub destination: String,

    /// Print traceroute hop results to stdout.
    #[arg(long, default_value_t = false, env = "TRACE_STDOUT")]
    pub print_results: bool,
}
