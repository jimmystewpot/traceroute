//! Command-line interface definitions and destination address resolution.
//!
//! Documented following Australian English conventions.

pub mod args;

use crate::cli::args::CommonTraceOptions;
use clap::{Parser, Subcommand};
use std::net::IpAddr;

/// Top-level command-line parser for traceroute.
#[derive(Parser, Debug)]
#[command(
    name = "traceroute",
    version,
    about = "Traceroute with OpenTelemetry distributed tracing"
)]
pub struct Cli {
    /// Subcommand to execute.
    #[command(subcommand)]
    pub command: Commands,
}

/// Available subcommands supported by the CLI.
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// UDP traceroute mode.
    #[command(about = "UDP traceroute")]
    Udp(CommonTraceOptions),

    /// TCP traceroute mode.
    #[command(about = "TCP traceroute")]
    Tcp(CommonTraceOptions),

    /// Run as a daemon service periodically executing traceroutes.
    #[command(about = "Run as a service daemon")]
    Service {
        /// Configuration file path.
        #[arg(long, env = "TRACE_CFGFILE")]
        config_file: String,

        /// Validate configuration file syntax and structure without executing probes.
        #[arg(long, default_value_t = false)]
        validate: bool,
    },

    /// Generate a sample configuration file and output to stdout.
    #[command(about = "Generate a sample configuration file")]
    Generate,
}

/// Resolves a hostname or IP string into a vector of IP addresses (supporting IPv4 and IPv6).
pub async fn resolve_destination(target: &str) -> anyhow::Result<Vec<IpAddr>> {
    let host_port = if let Ok(ipv6) = target.parse::<std::net::Ipv6Addr>() {
        format!("[{}]:0", ipv6)
    } else {
        format!("{}:0", target)
    };
    let lookup_future = tokio::net::lookup_host(&host_port);
    let socket_addrs = tokio::time::timeout(std::time::Duration::from_secs(5), lookup_future)
        .await
        .map_err(|_| anyhow::anyhow!("DNS resolution timed out for {}", target))??;
    let ips: Vec<IpAddr> = socket_addrs.map(|sa| sa.ip()).collect();
    if ips.is_empty() {
        anyhow::bail!("destination {} failed to resolve", target);
    }
    Ok(ips)
}

/// Resolves destination addresses asynchronously (alias matching specification).
pub async fn parse_destinations(destination: &str) -> anyhow::Result<Vec<IpAddr>> {
    resolve_destination(destination).await
}
