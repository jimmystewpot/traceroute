//! Configuration schema and validation unit tests.

use std::time::Duration;
use traceroute::config::{
    generate_sample_config, load_config_from_str, ConfigError, DEFAULT_INTERVAL, DEFAULT_MAX_HOPS,
    DEFAULT_NUM_QUERIES, DEFAULT_PARALLEL_REQUESTS, DEFAULT_SOURCE_PORT, DEFAULT_TIMEOUT,
};

const VALID_CONFIG_YAML: &str = r#"---
schema-version: 1.0.0
destinations:
  - example.com
globals:
  protocol: tcp
  max-hops: 30
  number-queries: 3
  parallel-requests: 16
  timeout: 5s
  source-port: 80
  interval: 60s
opentelemetry:
  destination: "localhost"
  tls: false
  port: 4317
  grpc: true
healthcheck:
  path: "/_healthcheck"
  enabled: true
  port: 8080
"#;

#[test]
fn test_load_valid_config() {
    let cfg = load_config_from_str(VALID_CONFIG_YAML).expect("valid configuration");
    assert_eq!(cfg.schema_version, "1.0.0");
    assert_eq!(cfg.destinations, vec!["example.com"]);
    assert_eq!(cfg.globals.protocol, "tcp");
    assert_eq!(cfg.globals.max_hops, 30);
    assert_eq!(cfg.globals.number_queries, 3);
    assert_eq!(cfg.globals.parallel_requests, 16);
    assert_eq!(cfg.globals.timeout, Duration::from_secs(5));
    assert_eq!(cfg.globals.source_port, 80);
    assert_eq!(cfg.globals.interval, Duration::from_secs(60));
    assert_eq!(cfg.opentelemetry.destination, "localhost");
    assert!(!cfg.opentelemetry.tls);
    assert_eq!(cfg.opentelemetry.port, 4317);
    assert!(cfg.opentelemetry.grpc);
    assert_eq!(cfg.healthcheck.path, "/_healthcheck");
    assert!(cfg.healthcheck.enabled);
    assert_eq!(cfg.healthcheck.port, 8080);
}

#[test]
fn test_reject_invalid_schema_version() {
    let invalid_yaml = VALID_CONFIG_YAML.replace("1.0.0", "2.0.0");
    let err = load_config_from_str(&invalid_yaml);
    assert!(err.is_err());
    match err {
        Err(ConfigError::UnsupportedSchema(version)) => {
            assert_eq!(version, "2.0.0");
        }
        other => panic!("expected UnsupportedSchema error, got {:?}", other),
    }
}

#[test]
fn test_default_values_applied() {
    let yaml_with_defaults = r#"---
schema-version: 1.0.0
destinations:
  - fallback.org
globals:
  protocol: udp
opentelemetry:
  destination: "127.0.0.1"
  tls: true
  port: 4318
  grpc: false
healthcheck:
  path: "/health"
  enabled: false
  port: 9090
"#;
    let cfg = load_config_from_str(yaml_with_defaults).expect("configuration with defaults");
    assert_eq!(cfg.globals.max_hops, DEFAULT_MAX_HOPS);
    assert_eq!(cfg.globals.number_queries, DEFAULT_NUM_QUERIES);
    assert_eq!(cfg.globals.parallel_requests, DEFAULT_PARALLEL_REQUESTS);
    assert_eq!(cfg.globals.timeout, DEFAULT_TIMEOUT);
    assert_eq!(cfg.globals.source_port, DEFAULT_SOURCE_PORT);
    assert_eq!(cfg.globals.interval, DEFAULT_INTERVAL);
}

#[test]
fn test_generate_sample_config_roundtrip() {
    let sample = generate_sample_config().expect("generate sample config");
    assert!(sample.contains("schema-version: '1.0.0'") || sample.contains("schema-version: 1.0.0"));
    let parsed = load_config_from_str(&sample).expect("parse generated sample config");
    assert_eq!(parsed.schema_version, "1.0.0");
    assert_eq!(
        parsed.destinations,
        vec!["first-test-domain.org", "second-test-domain.org"]
    );
    assert_eq!(parsed.globals.protocol, "tcp");
}

#[test]
fn test_reject_malformed_yaml() {
    let malformed = "destinations: [unclosed list";
    let err = load_config_from_str(malformed);
    assert!(err.is_err());
    match err {
        Err(ConfigError::Yaml(_)) => {}
        other => panic!("expected Yaml error, got {:?}", other),
    }
}

#[test]
fn test_reject_empty_destinations() {
    let invalid_yaml =
        VALID_CONFIG_YAML.replace("destinations:\n  - example.com", "destinations: []");
    let err = load_config_from_str(&invalid_yaml);
    assert!(err.is_err());
    match err {
        Err(ConfigError::Validation(msg)) => {
            assert!(msg.contains("destinations list must not be empty"));
        }
        other => panic!("expected Validation error, got {:?}", other),
    }
}

#[test]
fn test_reject_invalid_protocol() {
    let invalid_yaml = VALID_CONFIG_YAML.replace("protocol: tcp", "protocol: quic");
    let err = load_config_from_str(&invalid_yaml);
    assert!(err.is_err());
    match err {
        Err(ConfigError::Validation(msg)) => {
            assert!(msg.contains("unsupported protocol 'quic'"));
        }
        other => panic!("expected Validation error, got {:?}", other),
    }
}

#[test]
fn test_reject_zero_source_port() {
    let invalid_yaml = VALID_CONFIG_YAML.replace("source-port: 80", "source-port: 0");
    let err = load_config_from_str(&invalid_yaml);
    assert!(err.is_err());
    match err {
        Err(ConfigError::Validation(msg)) => {
            assert!(msg.contains("globals.source-port must be between 1 and 65535"));
        }
        other => panic!("expected Validation error, got {:?}", other),
    }
}

#[test]
fn test_reject_zero_healthcheck_port() {
    let invalid_yaml = VALID_CONFIG_YAML.replace("port: 8080", "port: 0");
    let err = load_config_from_str(&invalid_yaml);
    assert!(err.is_err());
    match err {
        Err(ConfigError::Validation(msg)) => {
            assert!(msg.contains("healthcheck.port must be between 1 and 65535"));
        }
        other => panic!("expected Validation error, got {:?}", other),
    }
}

#[test]
fn test_linked_spans_configuration_deserialisation() {
    let yaml_explicit = VALID_CONFIG_YAML.replace("grpc: true", "grpc: true\n  linked-spans: true");
    let cfg = load_config_from_str(&yaml_explicit)
        .expect("valid configuration with linked-spans enabled");
    assert!(cfg.opentelemetry.linked_spans);

    let cfg_default = load_config_from_str(VALID_CONFIG_YAML).expect("valid configuration default");
    assert!(!cfg_default.opentelemetry.linked_spans);
}
