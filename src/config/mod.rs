//! Configuration management, validation, and sample generation.
//! Designed and documented following Australian English conventions.

pub mod schema;

pub use schema::{
    ConfigError, TraceConfig, TraceConfigGlobal, TraceConfigHealthCheck, TraceConfigOtel,
    CURRENT_SCHEMA_VERSION, DEFAULT_INTERVAL, DEFAULT_MAX_HOPS, DEFAULT_NUM_QUERIES,
    DEFAULT_PARALLEL_REQUESTS, DEFAULT_SOURCE_PORT, DEFAULT_TIMEOUT,
};

/// Loads and validates a traceroute configuration from a YAML string.
///
/// Ensures that the YAML deserialises into [`TraceConfig`], that the
/// `schema-version` matches [`CURRENT_SCHEMA_VERSION`], and that semantic
/// validation constraints are met.
pub fn load_config_from_str(s: &str) -> Result<TraceConfig, ConfigError> {
    let config: TraceConfig = serde_yml::from_str(s)?;
    if config.schema_version != CURRENT_SCHEMA_VERSION {
        return Err(ConfigError::UnsupportedSchema(config.schema_version));
    }
    config.validate()?;
    Ok(config)
}

impl TraceConfig {
    /// Validates semantic constraints beyond schema-version and YAML structure.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.destinations.is_empty() {
            return Err(ConfigError::Validation(
                "destinations list must not be empty".into(),
            ));
        }
        if self.globals.protocol != "udp" && self.globals.protocol != "tcp" {
            return Err(ConfigError::Validation(format!(
                "unsupported protocol '{}'; must be 'udp' or 'tcp'",
                self.globals.protocol
            )));
        }
        if self.globals.timeout.is_zero() {
            return Err(ConfigError::Validation(
                "globals.timeout must be greater than zero".into(),
            ));
        }
        if self.globals.interval.is_zero() {
            return Err(ConfigError::Validation(
                "globals.interval must be greater than zero".into(),
            ));
        }
        if self.globals.source_port == 0 {
            return Err(ConfigError::Validation(
                "globals.source-port must be between 1 and 65535".into(),
            ));
        }
        if self.healthcheck.port == 0 {
            return Err(ConfigError::Validation(
                "healthcheck.port must be between 1 and 65535".into(),
            ));
        }
        Ok(())
    }
}

/// Generates a sample YAML configuration string matching schema v1.0.0.
///
/// This produces a pre-populated template suitable for initialisation and
/// customisation by operators.
pub fn generate_sample_config() -> Result<String, ConfigError> {
    let sample = TraceConfig {
        schema_version: CURRENT_SCHEMA_VERSION.to_string(),
        destinations: vec![
            "first-test-domain.org".into(),
            "second-test-domain.org".into(),
        ],
        globals: TraceConfigGlobal {
            protocol: "tcp".into(),
            max_hops: DEFAULT_MAX_HOPS,
            number_queries: DEFAULT_NUM_QUERIES,
            parallel_requests: DEFAULT_PARALLEL_REQUESTS,
            timeout: DEFAULT_TIMEOUT,
            source_port: DEFAULT_SOURCE_PORT,
            interval: DEFAULT_INTERVAL,
        },
        opentelemetry: TraceConfigOtel {
            destination: "127.0.0.1".into(),
            tls: false,
            port: 4317,
            grpc: true,
            linked_spans: false,
        },
        healthcheck: TraceConfigHealthCheck {
            path: "/_healthcheck".into(),
            enabled: true,
            port: 8080,
        },
    };
    Ok(serde_yml::to_string(&sample)?)
}
