//! Command-line interface and destination address resolution tests.

use clap::Parser;
use std::time::Duration;
use traceroute::cli::{resolve_destination, Cli, Commands};

#[test]
fn test_cli_parse_udp_subcommand() {
    let args = vec![
        "traceroute",
        "udp",
        "--destination",
        "example.com",
        "--max-hops",
        "15",
        "--print-results",
    ];
    let cli = Cli::try_parse_from(args).expect("valid cli arguments");
    match cli.command {
        Commands::Udp(opts) => {
            assert_eq!(opts.destination, "example.com");
            assert_eq!(opts.max_hops, 15);
            assert!(opts.print_results);
            assert_eq!(opts.n_queries, 3);
            assert_eq!(opts.parallel_requests, 16);
            assert_eq!(opts.timeout, Duration::from_secs(2));
            assert_eq!(opts.trace_route_port, 33434);
            assert_eq!(opts.otel_dest, "localhost");
            assert!(!opts.otel_tls);
            assert!(opts.otel_grpc);
            assert_eq!(opts.otel_port, 4317);
        }
        _ => panic!("expected UDP command"),
    }
}

#[test]
fn test_cli_parse_tcp_subcommand() {
    let args = vec![
        "traceroute",
        "tcp",
        "--destination",
        "192.0.2.1",
        "-m",
        "20",
        "-q",
        "5",
        "-N",
        "8",
        "-w",
        "3s",
        "-p",
        "80",
        "--otel-dest",
        "collector.internal",
        "--otel-tls",
        "--otel-port",
        "4318",
    ];
    let cli = Cli::try_parse_from(args).expect("valid TCP cli arguments");
    match cli.command {
        Commands::Tcp(opts) => {
            assert_eq!(opts.destination, "192.0.2.1");
            assert_eq!(opts.max_hops, 20);
            assert_eq!(opts.n_queries, 5);
            assert_eq!(opts.parallel_requests, 8);
            assert_eq!(opts.timeout, Duration::from_secs(3));
            assert_eq!(opts.trace_route_port, 80);
            assert_eq!(opts.otel_dest, "collector.internal");
            assert!(opts.otel_tls);
            assert_eq!(opts.otel_port, 4318);
            assert!(!opts.print_results);
        }
        _ => panic!("expected TCP command"),
    }
}

#[test]
fn test_cli_parse_service_subcommand() {
    let args = vec![
        "traceroute",
        "service",
        "--config-file",
        "/etc/traceroute/config.yaml",
        "--validate",
    ];
    let cli = Cli::try_parse_from(args).expect("valid service cli arguments");
    match cli.command {
        Commands::Service {
            config_file,
            validate,
        } => {
            assert_eq!(config_file, "/etc/traceroute/config.yaml");
            assert!(validate);
        }
        _ => panic!("expected Service command"),
    }
}

#[test]
fn test_cli_parse_generate_subcommand() {
    let args = vec!["traceroute", "generate"];
    let cli = Cli::try_parse_from(args).expect("valid generate cli arguments");
    match cli.command {
        Commands::Generate => {}
        _ => panic!("expected Generate command"),
    }
}

#[tokio::test]
async fn test_resolve_destination_localhost() {
    let ips = resolve_destination("127.0.0.1")
        .await
        .expect("resolve 127.0.0.1");
    assert!(!ips.is_empty());
    assert!(ips.contains(&"127.0.0.1".parse().unwrap()));
}

#[tokio::test]
async fn test_resolve_destination_ipv6_loopback() {
    let ips = resolve_destination("::1").await.expect("resolve ::1");
    assert!(!ips.is_empty());
    assert!(ips.contains(&"::1".parse().unwrap()));
}

#[tokio::test]
async fn test_resolve_destination_invalid() {
    let res = resolve_destination("nonexistent.invalid.example.domain.local").await;
    assert!(res.is_err());
}

#[test]
fn test_cli_parse_otel_linked_spans_flag() {
    let args = vec![
        "traceroute",
        "udp",
        "--destination",
        "example.com",
        "--otel-linked-spans",
    ];
    let cli = Cli::try_parse_from(args).expect("valid cli arguments");
    match cli.command {
        Commands::Udp(opts) => {
            assert!(opts.otel_linked_spans);
        }
        _ => panic!("expected UDP command"),
    }

    let args_default = vec!["traceroute", "tcp", "--destination", "example.com"];
    let cli_default = Cli::try_parse_from(args_default).expect("valid cli arguments default");
    match cli_default.command {
        Commands::Tcp(opts) => {
            assert!(!opts.otel_linked_spans);
        }
        _ => panic!("expected TCP command"),
    }
}
