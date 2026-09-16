//! Comprehensive integration tests for the `traceroute` binary CLI interface.
//!
//! Designed and documented following Australian English conventions.

use assert_cmd::Command;
use std::io::Write;

#[test]
fn test_cli_generate_command_outputs_yaml() {
    let mut cmd = Command::cargo_bin("traceroute").expect("binary should exist");
    let assert = cmd.arg("generate").assert();
    assert
        .success()
        .stdout(predicates::str::contains("schema-version: 1.0.0"))
        .stdout(predicates::str::contains("first-test-domain.org"))
        .stdout(predicates::str::contains("protocol: tcp"))
        .stdout(predicates::str::contains("opentelemetry:"))
        .stdout(predicates::str::contains("linked-spans: false"))
        .stdout(predicates::str::contains("healthcheck:"));
}

#[test]
fn test_cli_top_level_help() {
    let mut cmd = Command::cargo_bin("traceroute").expect("binary should exist");
    let assert = cmd.arg("--help").assert();
    assert
        .success()
        .stdout(predicates::str::contains(
            "Traceroute with OpenTelemetry distributed tracing",
        ))
        .stdout(predicates::str::contains("udp"))
        .stdout(predicates::str::contains("tcp"))
        .stdout(predicates::str::contains("service"))
        .stdout(predicates::str::contains("generate"));
}

#[test]
fn test_cli_subcommand_helps() {
    // UDP subcommand help
    let mut udp_cmd = Command::cargo_bin("traceroute").expect("binary should exist");
    udp_cmd
        .args(["udp", "--help"])
        .assert()
        .success()
        .stdout(predicates::str::contains("--max-hops"))
        .stdout(predicates::str::contains("--n-queries"))
        .stdout(predicates::str::contains("--parallel-requests"))
        .stdout(predicates::str::contains("--timeout"))
        .stdout(predicates::str::contains("--destination"));

    // TCP subcommand help
    let mut tcp_cmd = Command::cargo_bin("traceroute").expect("binary should exist");
    tcp_cmd
        .args(["tcp", "--help"])
        .assert()
        .success()
        .stdout(predicates::str::contains("--max-hops"))
        .stdout(predicates::str::contains("--n-queries"))
        .stdout(predicates::str::contains("--parallel-requests"))
        .stdout(predicates::str::contains("--timeout"))
        .stdout(predicates::str::contains("--destination"));

    // Service subcommand help
    let mut service_cmd = Command::cargo_bin("traceroute").expect("binary should exist");
    service_cmd
        .args(["service", "--help"])
        .assert()
        .success()
        .stdout(predicates::str::contains("--config-file"))
        .stdout(predicates::str::contains("--validate"));

    // Generate subcommand help
    let mut gen_cmd = Command::cargo_bin("traceroute").expect("binary should exist");
    gen_cmd
        .args(["generate", "--help"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "Generate a sample configuration file",
        ));
}

#[test]
fn test_cli_service_validate_valid_config() {
    let temp_dir = std::env::temp_dir();
    let config_path = temp_dir.join(format!(
        "valid_traceroute_test_config_{}.yaml",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));

    let valid_yaml = r#"schema-version: 1.0.0
destinations:
  - 127.0.0.1
  - localhost
globals:
  protocol: tcp
  max-hops: 30
  number-queries: 3
  parallel-requests: 8
  timeout: 3s
  source-port: 80
  interval: 30s
opentelemetry:
  destination: 127.0.0.1
  tls: false
  port: 4317
  grpc: true
healthcheck:
  path: /_healthcheck
  enabled: true
  port: 8080
"#;

    let mut file = std::fs::File::create(&config_path).expect("failed to create temp config file");
    file.write_all(valid_yaml.as_bytes())
        .expect("failed to write config");

    let mut cmd = Command::cargo_bin("traceroute").expect("binary should exist");
    cmd.args([
        "service",
        "--config-file",
        config_path.to_str().expect("valid path"),
        "--validate",
    ])
    .assert()
    .success()
    .stdout(predicates::str::contains("validated successfully"));

    let _ = std::fs::remove_file(config_path);
}

#[test]
fn test_cli_service_validate_invalid_config() {
    let temp_dir = std::env::temp_dir();
    let config_path = temp_dir.join(format!(
        "invalid_traceroute_test_config_{}.yaml",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));

    let invalid_yaml = r#"schema-version: 99.0.0
destinations:
  - 127.0.0.1
"#;

    let mut file = std::fs::File::create(&config_path).expect("failed to create temp config file");
    file.write_all(invalid_yaml.as_bytes())
        .expect("failed to write config");

    let mut cmd = Command::cargo_bin("traceroute").expect("binary should exist");
    cmd.args([
        "service",
        "--config-file",
        config_path.to_str().expect("valid path"),
        "--validate",
    ])
    .assert()
    .failure();

    let _ = std::fs::remove_file(config_path);
}

#[test]
fn test_cli_service_missing_config_file() {
    let mut cmd = Command::cargo_bin("traceroute").expect("binary should exist");
    cmd.args([
        "service",
        "--config-file",
        "/path/that/definitely/does/not/exist/traceroute.yaml",
        "--validate",
    ])
    .assert()
    .failure();
}

#[test]
fn test_cli_service_daemon_start_output() {
    let temp_dir = std::env::temp_dir();
    let config_path = temp_dir.join(format!(
        "daemon_traceroute_test_config_{}.yaml",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));

    let valid_yaml = r#"schema-version: 1.0.0
destinations:
  - 127.0.0.1
  - localhost
  - test-host.internal
globals:
  protocol: tcp
  max-hops: 30
  number-queries: 3
  parallel-requests: 8
  timeout: 3s
  source-port: 80
  interval: 30s
opentelemetry:
  destination: 127.0.0.1
  tls: false
  port: 4317
  grpc: true
healthcheck:
  path: /_healthcheck
  enabled: false
  port: 8080
"#;

    let mut file = std::fs::File::create(&config_path).expect("failed to create temp config file");
    file.write_all(valid_yaml.as_bytes())
        .expect("failed to write config");

    let bin_path = assert_cmd::cargo::cargo_bin("traceroute");
    let mut child = std::process::Command::new(bin_path)
        .args([
            "service",
            "--config-file",
            config_path.to_str().expect("valid path"),
        ])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("failed to spawn daemon process");

    let stdout = child.stdout.take().expect("child stdout");
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        use std::io::BufRead;
        let reader = std::io::BufReader::new(stdout);
        for line in reader.lines().map_while(Result::ok) {
            if line.contains("Starting traceroute daemon for 3 destination(s)") {
                let _ = tx.send(true);
                return;
            }
        }
        let _ = tx.send(false);
    });

    let found = rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap_or(false);

    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_file(config_path);

    assert!(found, "Daemon should log start message to stdout");
}

#[test]
fn test_cli_udp_command_initialisation() {
    let mut cmd = Command::cargo_bin("traceroute").expect("binary should exist");
    cmd.args([
        "udp",
        "--destination",
        "127.0.0.1",
        "--max-hops",
        "1",
        "--timeout",
        "100ms",
    ])
    .assert()
    .success()
    .stdout(predicates::str::contains("Resolved 127.0.0.1 to 127.0.0.1"));
}

#[test]
fn test_cli_tcp_command_initialisation() {
    let mut cmd = Command::cargo_bin("traceroute").expect("binary should exist");
    cmd.args([
        "tcp",
        "--destination",
        "127.0.0.1",
        "--max-hops",
        "1",
        "--timeout",
        "100ms",
    ])
    .assert()
    .success()
    .stdout(predicates::str::contains("Resolved 127.0.0.1 to 127.0.0.1"));
}

#[test]
fn test_cli_udp_print_results_flag_accepted() {
    let mut cmd = Command::cargo_bin("traceroute").expect("binary should exist");
    cmd.args([
        "udp",
        "--destination",
        "192.0.2.1",
        "--max-hops",
        "1",
        "--timeout",
        "1ms",
        "--print-results",
    ])
    .assert()
    .success();
}

#[test]
fn test_cli_tcp_print_results_flag_accepted() {
    let mut cmd = Command::cargo_bin("traceroute").expect("binary should exist");
    cmd.args([
        "tcp",
        "--destination",
        "192.0.2.1",
        "--max-hops",
        "1",
        "--timeout",
        "1ms",
        "--print-results",
    ])
    .assert()
    .success();
}

#[test]
fn test_cli_missing_subcommand_fails() {
    let mut cmd = Command::cargo_bin("traceroute").expect("binary should exist");
    cmd.assert().failure();
}

#[test]
fn test_cli_udp_missing_destination_fails() {
    let mut cmd = Command::cargo_bin("traceroute").expect("binary should exist");
    cmd.arg("udp").assert().failure();
}

#[test]
fn test_cli_udp_linked_spans_flag_accepted() {
    let mut cmd = Command::cargo_bin("traceroute").expect("binary should exist");
    cmd.args([
        "udp",
        "--destination",
        "192.0.2.1",
        "--max-hops",
        "1",
        "--timeout",
        "5ms",
        "--otel-linked-spans",
    ])
    .assert()
    .success();
}

#[test]
fn test_cli_tcp_linked_spans_flag_accepted() {
    let mut cmd = Command::cargo_bin("traceroute").expect("binary should exist");
    cmd.args([
        "tcp",
        "--destination",
        "192.0.2.1",
        "--max-hops",
        "1",
        "--timeout",
        "5ms",
        "--otel-linked-spans",
    ])
    .assert()
    .success();
}

#[test]
fn test_cli_service_daemon_graceful_shutdown_logging() {
    let temp_dir = std::env::temp_dir();
    let config_path = temp_dir.join(format!(
        "daemon_shutdown_test_config_{}.yaml",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let free_port = listener.local_addr().expect("local addr").port();
    drop(listener);

    let valid_yaml = format!(
        r#"schema-version: 1.0.0
destinations:
  - 127.0.0.1
globals:
  protocol: tcp
  max-hops: 30
  number-queries: 3
  parallel-requests: 8
  timeout: 3s
  source-port: 80
  interval: 30s
opentelemetry:
  destination: 127.0.0.1
  tls: false
  port: 4317
  grpc: true
  linked-spans: false
healthcheck:
  path: /_healthcheck
  enabled: true
  port: {free_port}
"#
    );

    let mut file = std::fs::File::create(&config_path).expect("failed to create temp config file");
    file.write_all(valid_yaml.as_bytes())
        .expect("failed to write config");

    let bin_path = assert_cmd::cargo::cargo_bin("traceroute");
    let mut child = std::process::Command::new(bin_path)
        .args([
            "service",
            "--config-file",
            config_path.to_str().expect("valid path"),
        ])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("failed to spawn daemon process");

    let stdout = child.stdout.take().expect("child stdout");
    let (tx_started, rx_started) = std::sync::mpsc::channel();
    let (tx_shutdown, rx_shutdown) = std::sync::mpsc::channel();

    std::thread::spawn(move || {
        use std::io::BufRead;
        let reader = std::io::BufReader::new(stdout);
        let mut started = false;
        let mut draining = false;
        let mut finished = false;
        let mut flushed = false;

        for line in reader.lines().map_while(Result::ok) {
            if line.contains("Health server listening on port") {
                started = true;
                let _ = tx_started.send(true);
            }
            if line.contains("Shutdown signal received; draining in-flight traces...") {
                draining = true;
            }
            if line.contains("All in-flight traces completed. Daemon shut down cleanly.") {
                finished = true;
            }
            if line.contains("Flushing telemetry buffers and exiting.") {
                flushed = true;
            }
        }
        let _ = tx_shutdown.send((started, draining, finished, flushed));
    });

    let started = rx_started
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap_or(false);
    assert!(
        started,
        "Daemon should log initialisation message to stdout"
    );

    // Allow the event loop to enter select! and register the signal handler
    std::thread::sleep(std::time::Duration::from_millis(150));

    // Send SIGINT to test clean termination behaviour
    unsafe {
        libc::kill(child.id() as i32, libc::SIGINT);
    }

    let status = child.wait().expect("child process wait");
    assert!(
        status.success(),
        "Daemon process should exit with status 0 upon SIGINT"
    );

    let (was_started, was_draining, was_finished, was_flushed) = rx_shutdown
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap_or((false, false, false, false));

    let _ = std::fs::remove_file(config_path);

    assert!(was_started, "Expected daemon start log");
    assert!(was_draining, "Expected draining log upon shutdown signal");
    assert!(was_finished, "Expected clean daemon shutdown log");
    assert!(was_flushed, "Expected telemetry flushing log prior to exit");
}
