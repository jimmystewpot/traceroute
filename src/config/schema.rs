//! Configuration schema definitions and validation data structures.
//! Designed and documented following Australian English conventions.

use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;

/// Supported schema version for traceroute configuration.
pub const CURRENT_SCHEMA_VERSION: &str = "1.0.0";

/// Default number of concurrent parallel probe requests.
pub const DEFAULT_PARALLEL_REQUESTS: u16 = 16;

/// Default maximum number of hops to traverse.
pub const DEFAULT_MAX_HOPS: u16 = 60;

/// Default number of probe queries sent to each hop.
pub const DEFAULT_NUM_QUERIES: u16 = 3;

/// Default source or base destination port for probes.
pub const DEFAULT_SOURCE_PORT: u16 = 80;

/// Default interval between recurring traceroute executions.
pub const DEFAULT_INTERVAL: Duration = Duration::from_secs(60);

/// Default probe timeout duration.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

/// Errors encountered during configuration parsing, deserialisation, or validation.
#[derive(Error, Debug)]
pub enum ConfigError {
    /// Failure during YAML parsing or deserialisation.
    #[error("failed to parse YAML: {0}")]
    Yaml(#[from] serde_yaml::Error),

    /// The configuration specifies an unsupported schema version.
    #[error("unknown or unsupported schema version: {0}")]
    UnsupportedSchema(String),

    /// A field or structure failed semantic validation.
    #[error("validation error: {0}")]
    Validation(String),
}

/// Root configuration model deserialised from YAML.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TraceConfig {
    /// Schema version string (must equal [`CURRENT_SCHEMA_VERSION`]).
    #[serde(rename = "schema-version")]
    pub schema_version: String,

    /// List of destination hostnames or IP addresses to probe.
    pub destinations: Vec<String>,

    /// Global parameters governing traceroute execution.
    pub globals: TraceConfigGlobal,

    /// OpenTelemetry distributed tracing exporter parameters.
    pub opentelemetry: TraceConfigOtel,

    /// Health-check HTTP server configuration.
    pub healthcheck: TraceConfigHealthCheck,
}

/// Global settings governing traceroute behaviour.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TraceConfigGlobal {
    /// Transport protocol for probes ("udp" or "tcp").
    pub protocol: String,

    /// Maximum number of hops before terminating the trace.
    #[serde(rename = "max-hops", default = "default_max_hops")]
    pub max_hops: u16,

    /// Number of queries per hop.
    #[serde(rename = "number-queries", default = "default_number_queries")]
    pub number_queries: u16,

    /// Maximum number of parallel requests permitted in flight.
    #[serde(rename = "parallel-requests", default = "default_parallel_requests")]
    pub parallel_requests: u16,

    /// Probe timeout duration.
    #[serde(default = "default_timeout", with = "humantime_serde")]
    pub timeout: Duration,

    /// Source or base port for probes.
    #[serde(rename = "source-port", default = "default_source_port")]
    pub source_port: u16,

    /// Execution interval between recurring daemon runs.
    #[serde(default = "default_interval", with = "humantime_serde")]
    pub interval: Duration,
}

impl Default for TraceConfigGlobal {
    fn default() -> Self {
        Self {
            protocol: "tcp".to_string(),
            max_hops: DEFAULT_MAX_HOPS,
            number_queries: DEFAULT_NUM_QUERIES,
            parallel_requests: DEFAULT_PARALLEL_REQUESTS,
            timeout: DEFAULT_TIMEOUT,
            source_port: DEFAULT_SOURCE_PORT,
            interval: DEFAULT_INTERVAL,
        }
    }
}

fn default_max_hops() -> u16 {
    DEFAULT_MAX_HOPS
}

fn default_number_queries() -> u16 {
    DEFAULT_NUM_QUERIES
}

fn default_parallel_requests() -> u16 {
    DEFAULT_PARALLEL_REQUESTS
}

fn default_timeout() -> Duration {
    DEFAULT_TIMEOUT
}

fn default_source_port() -> u16 {
    DEFAULT_SOURCE_PORT
}

fn default_interval() -> Duration {
    DEFAULT_INTERVAL
}

/// OpenTelemetry exporter configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TraceConfigOtel {
    /// Destination endpoint hostname or IP address.
    pub destination: String,

    /// Whether TLS is enabled for the exporter connection.
    pub tls: bool,

    /// Port number of the OTLP collector.
    pub port: u16,

    /// Whether to export via gRPC (true) or HTTP (false).
    pub grpc: bool,

    /// Whether linked spans across consecutive trace iterations are enabled.
    #[serde(rename = "linked-spans", default)]
    pub linked_spans: bool,
}

/// Health-check HTTP server endpoint configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TraceConfigHealthCheck {
    /// URL path for the health-check handler (e.g. "/_healthcheck").
    pub path: String,

    /// Whether the health-check server is enabled.
    pub enabled: bool,

    /// Listening port for the health-check HTTP server.
    pub port: u16,
}
