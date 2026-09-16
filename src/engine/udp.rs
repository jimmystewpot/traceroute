//! Asynchronous UDP traceroute probing engine and concurrency orchestrator.
//! Designed and documented following Australian English conventions.

use crate::engine::errqueue::{enable_ip_recverr, recv_from_errqueue};
use crate::engine::hop::{reduce_final_result, TracerouteHop};
use opentelemetry::trace::Span;
use socket2::{Domain, Protocol, SockAddr, Socket, Type};
use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;

/// ICMP type 11 — Time Exceeded (IPv4, RFC 792). Emitted by intermediate routers
/// when the packet TTL reaches zero before arriving at the destination.
pub const ICMP_V4_TIME_EXCEEDED: u8 = 11;

/// ICMP type 3 — Destination Unreachable (IPv4, RFC 792). Received when the probe
/// reaches the target host but a port or protocol is inaccessible.
pub const ICMP_V4_DEST_UNREACHABLE: u8 = 3;

/// ICMPv6 type 3 — Time Exceeded (IPv6, RFC 4443). Emitted by intermediate routers
/// when the hop limit reaches zero before arriving at the destination.
pub const ICMP_V6_TIME_EXCEEDED: u8 = 3;

/// ICMPv6 type 1 — Destination Unreachable (IPv6, RFC 4443). Received when the probe
/// reaches the target host but a port is inaccessible.
pub const ICMP_V6_DEST_UNREACHABLE: u8 = 1;

/// Returns expected ICMP `(time_exceeded, dest_unreachable)` message types
/// based on the target IP protocol version (RFC 792 for IPv4, RFC 4443 for IPv6).
#[must_use]
pub fn icmp_types_for_dest(dest: IpAddr) -> (u8, u8) {
    if dest.is_ipv6() {
        (ICMP_V6_TIME_EXCEEDED, ICMP_V6_DEST_UNREACHABLE)
    } else {
        (ICMP_V4_TIME_EXCEEDED, ICMP_V4_DEST_UNREACHABLE)
    }
}

/// Configuration parameters governing UDP traceroute execution.
#[derive(Debug, Clone)]
pub struct UdpTracerouteConfig {
    /// Target destination IP address (IPv4 or IPv6).
    pub destination: IpAddr,
    /// Target destination name, before resolution.
    pub destination_name: Option<String>,
    /// Maximum number of network hops before terminating probe traversal.
    pub max_hops: u16,
    /// Number of distinct probe queries dispatched per hop TTL.
    pub queries_per_hop: u16,
    /// Maximum number of concurrent probe requests permitted in flight.
    pub parallel_requests: u16,
    /// Maximum wait duration for an individual probe before timing out.
    pub timeout: Duration,
    /// Base destination port for outgoing UDP probe packets.
    pub dest_port: u16,
    /// Optional OpenTelemetry span links to associate with the root trace span.
    pub span_links: Vec<opentelemetry::trace::Link>,
    /// Optional shared semaphore enforcing global concurrency bounds across destinations.
    pub probe_limiter: Option<Arc<Semaphore>>,
}

impl PartialEq for UdpTracerouteConfig {
    fn eq(&self, other: &Self) -> bool {
        self.destination == other.destination
            && self.destination_name == other.destination_name
            && self.max_hops == other.max_hops
            && self.queries_per_hop == other.queries_per_hop
            && self.parallel_requests == other.parallel_requests
            && self.timeout == other.timeout
            && self.dest_port == other.dest_port
            && self.span_links == other.span_links
    }
}

/// Creates and configures a UDP socket initialised with the specified Time-To-Live (TTL).
///
/// `IP_RECVERR` (IPv4) or `IPV6_RECVERR` (IPv6) is enabled so that ICMP errors
/// are delivered to the socket error queue.
///
/// The socket is bound to the wildcard address on an ephemeral port assigned by
/// the kernel, which is required for unprivileged ICMP error reception on Linux.
///
/// # Errors
/// Returns an `std::io::Error` if socket creation, option setting, or binding fails.
pub fn create_udp_probe_socket(destination: IpAddr, ttl: u16) -> std::io::Result<Socket> {
    let domain = if destination.is_ipv4() {
        Domain::IPV4
    } else {
        Domain::IPV6
    };

    let socket = Socket::new(domain, Type::DGRAM, Some(Protocol::UDP))?;

    if destination.is_ipv4() {
        socket.set_ttl(ttl as u32)?;
    } else {
        socket.set_unicast_hops_v6(ttl as u32)?;
    }

    // Enable IP_RECVERR / IPV6_RECVERR so that ICMP errors (Time Exceeded,
    // Port Unreachable) are delivered via the error queue rather than dropped.
    // Silently ignore EPERM — the probe will fall back to recv_from timeout behaviour.
    let _ = enable_ip_recverr(&socket);

    // Bind to the wildcard address on an ephemeral port. This is required for the
    // kernel to associate incoming ICMP errors with this socket via the error queue.
    let bind_addr: SocketAddr = if destination.is_ipv4() {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0)
    } else {
        SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 0)
    };
    socket.bind(&SockAddr::from(bind_addr))?;

    Ok(socket)
}

/// Executes a single UDP probe for a given TTL and returns the observed [`TracerouteHop`].
///
/// The probe is executed entirely in a blocking thread via `tokio::task::spawn_blocking`
/// because the underlying socket operations (`recvmsg`, `recv_from`) are blocking calls
/// that must not run on the async executor thread.
///
/// # Flow
/// 1. Create and configure a UDP socket with the appropriate TTL.
/// 2. Configure non-blocking mode and send a minimal probe packet to the destination.
/// 3. Poll using `libc::poll` up to `timeout` for an ICMP Time Exceeded error in
///    the error queue (indicating an intermediate router) or a reply on the socket itself
///    (indicating the destination has been reached).
/// 4. If neither arrives within `timeout`, return a timed-out (failed) hop.
async fn probe_single_hop(
    dest: IpAddr,
    dest_port: u16,
    ttl: u16,
    timeout: Duration,
) -> TracerouteHop {
    // Move all blocking I/O into a dedicated blocking thread.
    let result =
        tokio::task::spawn_blocking(move || execute_blocking_probe(dest, dest_port, ttl, timeout))
            .await;

    match result {
        Ok(hop) => hop,
        Err(join_err) => {
            tracing::error!(ttl, "Blocking probe task panicked: {}", join_err);
            TracerouteHop {
                success: false,
                address: None,
                ttl,
                rtt: None,
            }
        }
    }
}

/// Blocking implementation of a single UDP probe, intended to run inside
/// `tokio::task::spawn_blocking`.
///
/// Returns a [`TracerouteHop`] representing the observed outcome.
fn execute_blocking_probe(
    dest: IpAddr,
    dest_port: u16,
    ttl: u16,
    timeout: Duration,
) -> TracerouteHop {
    let failed_hop = TracerouteHop {
        success: false,
        address: None,
        ttl,
        rtt: None,
    };

    // Create and configure the probe socket.
    let socket = match create_udp_probe_socket(dest, ttl) {
        Ok(s) => s,
        Err(err) => {
            tracing::debug!(ttl, "UDP probe socket creation failed: {}", err);
            return failed_hop;
        }
    };

    // Set socket to non-blocking mode so recv_from and poll do not block indefinitely.
    if let Err(err) = socket.set_nonblocking(true) {
        tracing::debug!(ttl, "Failed to set non-blocking mode: {}", err);
        return failed_hop;
    }

    // Send a minimal 4-byte probe packet to the destination. The TTL set on the
    // socket causes the kernel to emit ICMP Time Exceeded when it reaches zero
    // at an intermediate router.
    let probe_payload = [0u8; 4];
    let dest_sock_addr = SockAddr::from(SocketAddr::new(dest, dest_port));
    let start = Instant::now();
    if let Err(err) = socket.send_to(&probe_payload, &dest_sock_addr) {
        tracing::debug!(ttl, "UDP probe send failed: {}", err);
        return failed_hop;
    }

    let (time_exceeded_type, dest_unreach_type) = icmp_types_for_dest(dest);

    // Poll using libc::poll(2) awaiting socket error queue (POLLERR / POLLPRI)
    // or direct data arrival (POLLIN) within the remaining timeout window.
    let poll_start = Instant::now();
    while poll_start.elapsed() < timeout {
        let remaining = timeout.saturating_sub(poll_start.elapsed());
        let timeout_ms = remaining.as_millis().min(i32::MAX as u128) as libc::c_int;

        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            let fd = socket.as_raw_fd();
            let mut pfd = libc::pollfd {
                fd,
                events: libc::POLLIN | libc::POLLERR | libc::POLLPRI,
                revents: 0,
            };
            let ret = unsafe { libc::poll(&mut pfd, 1, timeout_ms) };
            if ret < 0 {
                let err = std::io::Error::last_os_error();
                if err.raw_os_error() == Some(libc::EINTR) {
                    continue;
                }
                break;
            }
            if ret == 0 {
                break;
            }
        }
        #[cfg(not(unix))]
        {
            std::thread::sleep(remaining);
            break;
        }

        // Read error queue when poll reports activity or error.
        // On Linux with IP_RECVERR enabled, ICMP errors are delivered here.
        if let Some((router_or_dest_ip, icmp_type, icmp_code)) = recv_from_errqueue(&socket) {
            if icmp_type == time_exceeded_type {
                return TracerouteHop {
                    success: true,
                    address: Some(router_or_dest_ip),
                    ttl,
                    rtt: Some(start.elapsed()),
                };
            } else if icmp_type == dest_unreach_type {
                // Destination arrival is signalled by ICMP Port Unreachable from the
                // target itself. Both conditions must hold: the offender IP must match
                // the destination AND the ICMP code must be Port Unreachable.
                // IPv4 Port Unreachable = code 3; IPv6 Port Unreachable = code 4.
                let is_port_unreachable =
                    (dest.is_ipv4() && icmp_code == 3) || (dest.is_ipv6() && icmp_code == 4);

                if router_or_dest_ip == dest && is_port_unreachable {
                    return TracerouteHop {
                        success: true,
                        address: Some(dest),
                        ttl,
                        rtt: Some(start.elapsed()),
                    };
                } else {
                    // Intermediate router or gateway returning Host/Net/Admin unreachable —
                    // report the actual offender IP, not the destination.
                    return TracerouteHop {
                        success: true,
                        address: Some(router_or_dest_ip),
                        ttl,
                        rtt: Some(start.elapsed()),
                    };
                }
            }
        }

        // Try reading direct reply on the socket if POLLIN arrived.
        let mut buf = [std::mem::MaybeUninit::<u8>::uninit(); 128];
        if let Ok((_len, peer_addr)) = socket.recv_from(&mut buf) {
            let addr = peer_addr.as_socket().map(|s| s.ip());
            return TracerouteHop {
                success: true,
                address: addr,
                ttl,
                rtt: Some(start.elapsed()),
            };
        }
    }

    failed_hop
}

/// Executes an asynchronous UDP traceroute across all hops up to `max_hops`, returning
/// the reduced hop map alongside the root OpenTelemetry span context.
///
/// Emits an OpenTelemetry root span (`"traceroute.trace"`) and child spans (`"traceroute.hop"`)
/// for each completed probe query. Dispatches individual probe queries governed by a rate-limiting
/// [`Semaphore`] that strictly enforces `parallel_requests` concurrency bounds.
pub async fn run_udp_traceroute_with_span_context(
    config: UdpTracerouteConfig,
) -> anyhow::Result<(
    BTreeMap<u16, Vec<TracerouteHop>>,
    opentelemetry::trace::SpanContext,
)> {
    let dest_name_owned = config.destination.to_string();
    let dest_name = config
        .destination_name
        .as_deref()
        .unwrap_or(&dest_name_owned);
    let source_host = hostname::get()
        .map(|h| h.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "traceroute".to_string());
    let baggage = crate::telemetry::baggage::build_traceroute_baggage(
        dest_name,
        &source_host,
        config.max_hops,
        &uuid::Uuid::new_v4().to_string(),
    );

    let (mut root_span, root_cx) = crate::telemetry::start_trace_span(
        "udp",
        config.destination,
        config.max_hops,
        config.queries_per_hop,
        config.span_links,
        baggage,
    );
    let root_span_context = root_span.span_context().clone();

    use opentelemetry::trace::TraceContextExt;
    let full_cx = root_cx.with_remote_span_context(root_span_context.clone());

    if config.max_hops == 0 {
        root_span.end();
        return Ok((BTreeMap::new(), root_span_context));
    }

    let parallel_capacity = std::cmp::max(1, config.parallel_requests) as usize;
    let semaphore = config
        .probe_limiter
        .clone()
        .unwrap_or_else(|| Arc::new(Semaphore::new(parallel_capacity)));

    let mut preliminary: BTreeMap<u16, Vec<(u16, TracerouteHop)>> = BTreeMap::new();
    let mut reached_destination = false;

    for ttl in 1..=config.max_hops {
        if reached_destination {
            break;
        }

        let mut hop_join_set = tokio::task::JoinSet::new();
        for query_idx in 0..config.queries_per_hop {
            let sem = Arc::clone(&semaphore);
            let dest = config.destination;
            let timeout = config.timeout;
            let dest_port = config.dest_port;

            hop_join_set.spawn(async move {
                let _permit = match sem.acquire().await {
                    Ok(permit) => permit,
                    Err(err) => {
                        tracing::error!("Semaphore permit acquisition failed: {}", err);
                        return (
                            ttl,
                            query_idx,
                            TracerouteHop {
                                success: false,
                                address: None,
                                ttl,
                                rtt: None,
                            },
                        );
                    }
                };

                let hop = probe_single_hop(dest, dest_port, ttl, timeout).await;
                (ttl, query_idx, hop)
            });
        }

        while let Some(join_result) = hop_join_set.join_next().await {
            match join_result {
                Ok((ttl, query_idx, hop)) => {
                    crate::telemetry::record_hop_span(ttl, query_idx, &hop, Some(&full_cx));
                    if hop.success && hop.address == Some(config.destination) {
                        reached_destination = true;
                    }
                    preliminary.entry(ttl).or_default().push((query_idx, hop));
                }
                Err(err) => {
                    tracing::error!("Probe task completed with error: {}", err);
                }
            }
        }
    }

    let mut sorted_results = BTreeMap::new();
    for (ttl, mut probes) in preliminary {
        probes.sort_by_key(|(idx, _)| *idx);
        sorted_results.insert(ttl, probes.into_iter().map(|(_, hop)| hop).collect());
    }

    let reduced = reduce_final_result(sorted_results, config.max_hops, config.destination);
    root_span.end();
    Ok((reduced, root_span_context))
}

/// Executes an asynchronous UDP traceroute across all hops up to `max_hops`.
///
/// Emits an OpenTelemetry root span (`"traceroute.trace"`) and child spans (`"traceroute.hop"`)
/// for each completed probe query. Dispatches individual probe queries governed by a rate-limiting
/// [`Semaphore`] that strictly enforces `parallel_requests` concurrency bounds.
pub async fn execute_udp_trace(
    config: UdpTracerouteConfig,
) -> anyhow::Result<BTreeMap<u16, Vec<TracerouteHop>>> {
    run_udp_traceroute_with_span_context(config)
        .await
        .map(|(hops, _)| hops)
}

/// Convenience alias executing an asynchronous UDP traceroute.
pub async fn run_udp_traceroute(
    config: UdpTracerouteConfig,
) -> anyhow::Result<BTreeMap<u16, Vec<TracerouteHop>>> {
    run_udp_traceroute_with_span_context(config)
        .await
        .map(|(hops, _)| hops)
}
