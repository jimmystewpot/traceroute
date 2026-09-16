//! Command-line entrypoint for the traceroute utility.
//!
//! Designed and documented following Australian English conventions.

#![allow(clippy::print_stdout)]
#![warn(clippy::unwrap_used)]
#![warn(clippy::expect_used)]

use clap::Parser;
use std::sync::Arc;
use traceroute::cli::{resolve_destination, Cli, Commands};
use traceroute::config::{generate_sample_config, load_config_from_str};
use traceroute::engine::tcp::{run_tcp_traceroute, TcpTracerouteConfig};
use traceroute::engine::udp::{run_udp_traceroute, UdpTracerouteConfig};
use traceroute::service::health::HealthState;
use traceroute::service::run_daemon;
use traceroute::telemetry::init_telemetry;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialise structured logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    let cli = Cli::parse();
    match cli.command {
        Commands::Generate => {
            let sample = generate_sample_config()?;
            println!("---\n{}\n", sample);
        }

        Commands::Service {
            config_file,
            validate,
        } => {
            let content = std::fs::read_to_string(&config_file)?;
            let cfg = load_config_from_str(&content)?;
            if validate {
                println!(
                    "Configuration file '{}' validated successfully.",
                    std::path::Path::new(&config_file).display()
                );
                return Ok(());
            }
            tracing::info!(
                "Starting traceroute daemon for {} destination(s) using {} protocol",
                cfg.destinations.len(),
                cfg.globals.protocol,
            );
            let state = Arc::new(HealthState::new("opentelemetry-traceroute"));
            let _guard = init_telemetry(
                &cfg.opentelemetry.destination,
                cfg.opentelemetry.port,
                cfg.opentelemetry.grpc,
                cfg.opentelemetry.tls,
                cfg.globals.timeout,
            )?;
            run_daemon(cfg, state).await?;
            tracing::info!("Flushing telemetry buffers and exiting.");
        }

        Commands::Udp(opts) => {
            let dest_ips = resolve_destination(&opts.destination).await?;
            let dest_ip = dest_ips.into_iter().next().ok_or_else(|| {
                anyhow::anyhow!("failed to resolve destination: {}", opts.destination)
            })?;
            tracing::info!("Resolved {} to {}", opts.destination, dest_ip);
            let _guard = init_telemetry(
                &opts.otel_dest,
                opts.otel_port,
                opts.otel_grpc,
                opts.otel_tls,
                opts.timeout,
            )?;
            // Cross-trace span linking is only supported in daemon mode where a persistent
            // span cache is available. One-shot CLI execution always passes empty links.
            let span_links = if opts.otel_linked_spans {
                tracing::warn!(
                    "--otel-linked-spans has no effect in one-shot CLI mode; \
                     use daemon mode for cross-trace linking"
                );
                Vec::new()
            } else {
                Vec::new()
            };
            let config = UdpTracerouteConfig {
                destination: dest_ip,
                destination_name: Some(opts.destination.clone()),
                max_hops: opts.max_hops,
                queries_per_hop: opts.n_queries,
                parallel_requests: opts.parallel_requests,
                timeout: opts.timeout,
                dest_port: opts.trace_route_port,
                span_links,
                probe_limiter: None,
            };
            let hops = run_udp_traceroute(config).await?;
            if opts.print_results {
                print_hop_table(&hops);
            }
        }

        Commands::Tcp(opts) => {
            let dest_ips = resolve_destination(&opts.destination).await?;
            let dest_ip = dest_ips.into_iter().next().ok_or_else(|| {
                anyhow::anyhow!("failed to resolve destination: {}", opts.destination)
            })?;
            tracing::info!("Resolved {} to {}", opts.destination, dest_ip);
            let _guard = init_telemetry(
                &opts.otel_dest,
                opts.otel_port,
                opts.otel_grpc,
                opts.otel_tls,
                opts.timeout,
            )?;
            // Cross-trace span linking is only supported in daemon mode where a persistent
            // span cache is available. One-shot CLI execution always passes empty links.
            let span_links = if opts.otel_linked_spans {
                tracing::warn!(
                    "--otel-linked-spans has no effect in one-shot CLI mode; \
                     use daemon mode for cross-trace linking"
                );
                Vec::new()
            } else {
                Vec::new()
            };
            let config = TcpTracerouteConfig {
                destination: dest_ip,
                destination_name: Some(opts.destination.clone()),
                max_hops: opts.max_hops,
                queries_per_hop: opts.n_queries,
                parallel_requests: opts.parallel_requests,
                timeout: opts.timeout,
                dest_port: opts.trace_route_port,
                span_links,
                probe_limiter: None,
            };
            let hops = run_tcp_traceroute(config).await?;
            if opts.print_results {
                print_hop_table(&hops);
            }
        }
    }
    Ok(())
}

/// Formats and prints traceroute hop results to stdout.
///
/// Each row shows the TTL, the responding router's address (or `*` if no response),
/// and per-query round-trip times in milliseconds.
fn print_hop_table(
    hops: &std::collections::BTreeMap<u16, Vec<traceroute::engine::hop::TracerouteHop>>,
) {
    for (ttl, queries) in hops {
        let addr_display = queries
            .iter()
            .find_map(|h| h.address.map(|a| a.to_string()))
            .unwrap_or_else(|| "*".to_string());
        let rtts: Vec<String> = queries
            .iter()
            .map(|h| {
                h.rtt
                    .map(|r| format!("{:.3} ms", r.as_secs_f64() * 1000.0))
                    .unwrap_or_else(|| "*".to_string())
            })
            .collect();
        println!("{:>3}  {}  {}", ttl, addr_display, rtts.join("  "));
    }
}
