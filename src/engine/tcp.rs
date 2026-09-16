//! Asynchronous TCP traceroute probing engine and concurrency orchestrator.
//! Designed and documented following Australian English conventions.
//!
//! # Execution Paths
//!
//! The engine tries two execution paths in order:
//!
//! 1. **Privileged path** — Requires `CAP_NET_RAW` (or root).
//!    Opens a raw `SOCK_RAW + IPPROTO_TCP` socket, transmits a hand-crafted
//!    TCP SYN packet with the configured TTL, then listens on a raw
//!    `SOCK_RAW + IPPROTO_ICMP` socket (opened **before** transmitting the probe
//!    to avoid a race window) for an ICMP Time Exceeded or Destination
//!    Unreachable reply. The inner IP+TCP header in the ICMP payload is parsed
//!    via `parse_icmp_payload` and the TCP sequence number is validated against
//!    the value embedded in the outgoing SYN to prevent misattribution of
//!    unrelated ICMP traffic.
//!
//! 2. **Unprivileged fallback** — Activated when raw socket creation fails with
//!    `EPERM` (permission denied). Opens a standard `SOCK_STREAM` TCP socket,
//!    sets the TTL via `IP_TTL`, enables `IP_RECVERR` so that ICMP errors are
//!    delivered to the socket error queue, then performs a non-blocking `connect`
//!    to send the SYN. The error queue is read via `recv_from_errqueue` to
//!    determine whether a router replied with ICMP Time Exceeded.

use crate::engine::errqueue::{enable_ip_recverr, recv_from_errqueue};
use crate::engine::hop::{reduce_final_result, TracerouteHop};
use crate::packet::icmp::{parse_icmp_payload, ExtractedProbe};
use crate::packet::tcp::{build_tcp_syn_ipv4, PacketError};
use opentelemetry::trace::Span;
use socket2::{Domain, Protocol, SockAddr, Socket, Type};
use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
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

/// Configuration parameters governing TCP traceroute execution.
#[derive(Debug, Clone)]
pub struct TcpTracerouteConfig {
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
    /// Target port for outgoing TCP SYN probe packets (e.g. 80 or 443).
    pub dest_port: u16,
    /// Optional OpenTelemetry span links to associate with the root trace span.
    pub span_links: Vec<opentelemetry::trace::Link>,
    /// Optional shared semaphore enforcing global concurrency bounds across destinations.
    pub probe_limiter: Option<Arc<Semaphore>>,
}

impl PartialEq for TcpTracerouteConfig {
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

/// Builds a TCP SYN probe packet suitable for transmission towards the destination.
///
/// Used exclusively by the **privileged raw socket path**. In the unprivileged
/// path the kernel constructs the SYN automatically when `connect` is called.
pub fn build_tcp_probe_packet(
    src_ip: Ipv4Addr,
    dst_ip: Ipv4Addr,
    src_port: u16,
    dst_port: u16,
    seq: u32,
    ttl: u8,
) -> Result<Vec<u8>, PacketError> {
    build_tcp_syn_ipv4(src_ip, dst_ip, src_port, dst_port, seq, ttl)
}

/// Discovers the local outbound IPv4 address and an ephemeral source port
/// used to route towards `destination` by creating a dummy connected UDP socket.
///
/// Under POSIX/Linux, connecting a UDP socket performs a kernel route lookup
/// and populates the socket's local address with the source IP chosen for
/// that route, along with a kernel-assigned ephemeral port, without transmitting
/// any packets over the network.
#[must_use]
pub fn discover_outbound_ipv4_and_port(destination: Ipv4Addr) -> Option<(Ipv4Addr, u16)> {
    let dummy = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    dummy
        .connect(SocketAddr::new(IpAddr::V4(destination), 80))
        .ok()?;
    match dummy.local_addr().ok()? {
        SocketAddr::V4(addr) => Some((*addr.ip(), addr.port())),
        _ => None,
    }
}

// ─── Privileged path ─────────────────────────────────────────────────────────

/// Attempts to open a raw `SOCK_RAW + IPPROTO_TCP` socket for the given domain.
///
/// Returns `Ok(socket)` if the caller holds `CAP_NET_RAW` or is root, or
/// `Err` with a permission error otherwise. The caller should treat `Err` as a
/// signal to fall back to the unprivileged `SOCK_STREAM` path.
fn try_open_raw_tcp_socket(domain: Domain) -> std::io::Result<Socket> {
    Socket::new(domain, Type::RAW, Some(Protocol::TCP))
}

/// Attempts to open a raw ICMP socket for the given domain.
///
/// This socket is used to receive ICMP Time Exceeded and Destination Unreachable
/// replies generated by intermediate routers and the destination host in response
/// to the TCP SYN probe.
fn try_open_raw_icmp_socket(domain: Domain, dest: IpAddr) -> std::io::Result<Socket> {
    let protocol = if dest.is_ipv4() {
        Protocol::ICMPV4
    } else {
        Protocol::ICMPV6
    };
    Socket::new(domain, Type::RAW, Some(protocol))
}

/// Parameters for a single privileged probe execution.
///
/// Grouping these avoids exceeding the clippy `too_many_arguments` limit (7)
/// while keeping all probe-correlation fields co-located.
struct PrivilegedProbeParams<'a> {
    /// Pre-opened raw TCP transmit socket with TTL already set.
    tx_socket: &'a Socket,
    /// Pre-opened raw ICMP receive socket (opened before the SYN was sent).
    rx_socket: &'a Socket,
    /// Serialised TCP SYN bytes to transmit.
    syn_bytes: &'a [u8],
    /// Destination IP address.
    dest: IpAddr,
    /// Destination TCP port.
    dest_port: u16,
    /// Source TCP port.
    src_port: u16,
    /// TTL for this probe — used to annotate the returned hop record.
    ttl: u16,
    /// Probe timeout duration.
    timeout: Duration,
    /// Sequence number embedded in the outgoing SYN, used to correlate the
    /// ICMP echo-of-inner-header against our probe and reject unrelated traffic.
    probe_seq: u32,
}

/// Executes a probe using the **privileged raw socket path**.
///
/// The caller is responsible for opening both the raw TCP transmit socket and
/// the raw ICMP receive socket **before** calling this function, and for setting
/// the TTL on the transmit socket. Passing pre-opened sockets avoids:
///   a) a redundant `Socket::new` call (Finding 2 fix), and
///   b) a race window where an ICMP reply arrives before the listen socket is
///      opened (Finding 1 fix — the ICMP socket must be ready before the SYN).
///
/// The inner IP+TCP header embedded in the ICMP payload is parsed via
/// `parse_icmp_payload` and the extracted TCP sequence number is compared
/// against `params.probe_seq` to reject unrelated ICMP traffic.
///
/// Returns `Some(hop)` if a correlated reply was received, or `None` on
/// timeout or if the reply cannot be attributed to our probe.
fn probe_privileged(params: &PrivilegedProbeParams<'_>) -> Option<TracerouteHop> {
    // Destructure the params, copying scalar (Copy) fields by value to avoid
    // holding references to them through the body of the function.
    let tx_socket = params.tx_socket;
    let rx_socket = params.rx_socket;
    let syn_bytes = params.syn_bytes;
    let dest = params.dest;
    let dest_port = params.dest_port;
    let ttl = params.ttl;
    let timeout = params.timeout;
    let probe_seq = params.probe_seq;
    let dest_sock_addr = SockAddr::from(SocketAddr::new(dest, dest_port));

    // Record the start time immediately before transmitting the SYN.
    let start = Instant::now();
    if let Err(err) = tx_socket.send_to(syn_bytes, &dest_sock_addr) {
        tracing::debug!(ttl, "Raw TCP SYN send failed: {}", err);
        return None;
    }

    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let rx_fd = rx_socket.as_raw_fd();
        let tx_fd = tx_socket.as_raw_fd();

        loop {
            let elapsed = start.elapsed();
            if elapsed >= timeout {
                return None;
            }
            let remaining = timeout - elapsed;
            let timeout_ms = remaining.as_millis().min(i32::MAX as u128) as libc::c_int;

            let mut pfds = [
                libc::pollfd {
                    fd: rx_fd,
                    events: libc::POLLIN,
                    revents: 0,
                },
                libc::pollfd {
                    fd: tx_fd,
                    events: libc::POLLIN,
                    revents: 0,
                },
            ];

            let ret = unsafe { libc::poll(pfds.as_mut_ptr(), 2, timeout_ms) };
            if ret < 0 {
                let err = std::io::Error::last_os_error();
                if err.raw_os_error() == Some(libc::EINTR) {
                    continue;
                }
                break; // Hard error
            }
            if ret == 0 {
                return None; // Timeout
            }

            // Check if ICMP raw socket has a packet (Time Exceeded etc)
            if pfds[0].revents & libc::POLLIN != 0 {
                let mut raw_buf = [std::mem::MaybeUninit::<u8>::uninit(); 512];
                if let Ok((n, peer_addr)) = rx_socket.recv_from(&mut raw_buf) {
                    let rtt = start.elapsed();
                    let router_ip = peer_addr.as_socket().map(|s| s.ip());

                    // SAFETY: recv_from guarantees the first n bytes are initialised.
                    let buf: &[u8] =
                        unsafe { std::slice::from_raw_parts(raw_buf.as_ptr() as *const u8, n) };

                    let inner_data = if dest.is_ipv4() {
                        if buf.is_empty() {
                            continue;
                        }
                        let outer_ihl = (buf[0] & 0x0f) as usize;
                        let offset = (outer_ihl * 4) + 8;
                        if buf.len() <= offset {
                            continue;
                        }
                        &buf[offset..]
                    } else {
                        if buf.len() <= 8 {
                            continue;
                        }
                        &buf[8..]
                    };

                    if let Ok(ExtractedProbe::Tcp { seq }) = parse_icmp_payload(inner_data) {
                        if seq == probe_seq {
                            return Some(TracerouteHop {
                                success: true,
                                address: router_ip,
                                ttl,
                                rtt: Some(rtt),
                            });
                        }
                    }
                }
            }

            // Check if TCP raw socket has a native TCP reply (SYN-ACK or RST) from the target
            if pfds[1].revents & libc::POLLIN != 0 {
                let mut raw_buf = [std::mem::MaybeUninit::<u8>::uninit(); 512];
                if let Ok((n, peer_addr)) = tx_socket.recv_from(&mut raw_buf) {
                    let rtt = start.elapsed();
                    let router_ip = peer_addr.as_socket().map(|s| s.ip());

                    // SAFETY: recv_from guarantees the first n bytes are initialised.
                    let buf: &[u8] =
                        unsafe { std::slice::from_raw_parts(raw_buf.as_ptr() as *const u8, n) };

                    // If IPv4, outer IP header is present on raw TCP receive sockets.
                    let tcp_payload = if dest.is_ipv4() {
                        if buf.is_empty() {
                            continue;
                        }
                        let outer_ihl = (buf[0] & 0x0f) as usize;
                        let offset = outer_ihl * 4;
                        if buf.len() <= offset {
                            continue;
                        }
                        &buf[offset..]
                    } else {
                        buf
                    };

                    // Validate TCP header
                    if tcp_payload.len() >= 20 {
                        // Extract source port (from target) and destination port (our ephemeral port)
                        let src_p = u16::from_be_bytes([tcp_payload[0], tcp_payload[1]]);
                        let dst_p = u16::from_be_bytes([tcp_payload[2], tcp_payload[3]]);
                        let ack_num = u32::from_be_bytes([
                            tcp_payload[8],
                            tcp_payload[9],
                            tcp_payload[10],
                            tcp_payload[11],
                        ]);

                        // We check if it comes from our target port, and acknowledges our SYN.
                        // probe_seq is the SYN sequence number, so SYN-ACK or RST-ACK should have ACK = probe_seq + 1.
                        if router_ip == Some(dest)
                            && src_p == dest_port
                            && dst_p == params.src_port
                            && ack_num == probe_seq.wrapping_add(1)
                        {
                            return Some(TracerouteHop {
                                success: true,
                                address: router_ip,
                                ttl,
                                rtt: Some(rtt),
                            });
                        }
                    }
                }
            }
        }
        None
    }

    #[cfg(not(unix))]
    {
        loop {
            let elapsed = start.elapsed();
            if elapsed >= timeout {
                return None;
            }
            let remaining = timeout - elapsed;

            // Apply the remaining timeout to the ICMP receive socket
            if let Err(err) = rx_socket.set_read_timeout(Some(remaining)) {
                tracing::debug!(ttl, "Failed to set ICMP socket read timeout: {}", err);
                return None;
            }

            // Receive one ICMP datagram from the raw ICMP socket.
            let mut raw_buf = [std::mem::MaybeUninit::<u8>::uninit(); 512];
            let (n, peer_addr) = match rx_socket.recv_from(&mut raw_buf) {
                Ok(pair) => pair,
                Err(_) => {
                    // Timeout or recv error — no reply within the configured window.
                    return None;
                }
            };
            let rtt = start.elapsed();

            // Obtain the router's IP address from the ICMP datagram source.
            let router_ip = peer_addr.as_socket().map(|s| s.ip());

            // Safety: recv_from guarantees that the first `n` elements are initialised.
            let buf: &[u8] =
                unsafe { std::slice::from_raw_parts(raw_buf.as_ptr() as *const u8, n) };

            // The ICMP payload (inner IP+TCP echo) starts after the outer IP header and
            // ICMP header. The outer IPv4 header length is dynamic based on IHL.
            let inner_data = if dest.is_ipv4() {
                if buf.is_empty() {
                    continue;
                }
                let outer_ihl = (buf[0] & 0x0f) as usize;
                let ipv4_icmp_payload_offset = (outer_ihl * 4) + 8;
                if buf.len() <= ipv4_icmp_payload_offset {
                    continue;
                }
                &buf[ipv4_icmp_payload_offset..]
            } else {
                // For IPv6 the raw socket does not prepend the outer IP header; the
                // ICMPv6 header is at byte 0. The inner IP+TCP follows the 8-byte header.
                if buf.len() <= 8 {
                    continue;
                }
                &buf[8..]
            };

            // Parse the inner IP+TCP header to extract the probe's sequence number.
            match parse_icmp_payload(inner_data) {
                Ok(ExtractedProbe::Tcp { seq }) => {
                    if seq == probe_seq {
                        // Sequence numbers match — this reply is correlated to our SYN.
                        return Some(TracerouteHop {
                            success: true,
                            address: router_ip,
                            ttl,
                            rtt: Some(rtt),
                        });
                    }
                }
                Ok(_) | Err(_) => {
                    // Not TCP or parse failed, discard and keep waiting
                }
            }
        }
    }
}

// ─── Unprivileged path ────────────────────────────────────────────────────────

/// Creates and configures an unprivileged TCP stream socket for the given destination.
///
/// Sets the TTL (via `IP_TTL` / `IPV6_UNICAST_HOPS`) and enables `IP_RECVERR`
/// or `IPV6_RECVERR` so ICMP errors are delivered via the socket error queue.
/// The socket is left in blocking mode so that `set_read_timeout` is respected
/// by `recv_from_errqueue`.
///
/// # Errors
/// Returns `std::io::Error` if socket creation or option setting fails.
fn create_tcp_stream_probe_socket(destination: IpAddr, ttl: u16) -> std::io::Result<Socket> {
    let domain = if destination.is_ipv4() {
        Domain::IPV4
    } else {
        Domain::IPV6
    };

    // SOCK_STREAM socket — the OS constructs the SYN automatically on connect.
    let socket = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))?;

    // Set SO_REUSEADDR to allow quick reuse of the ephemeral port after the probe.
    let _ = socket.set_reuse_address(true);

    // Apply the time-to-live so intermediate routers emit ICMP Time Exceeded
    // when the TTL reaches zero.
    if destination.is_ipv4() {
        socket.set_ttl_v4(ttl as u32)?;
    } else {
        socket.set_unicast_hops_v6(ttl as u32)?;
    }

    // Enable IP_RECVERR / IPV6_RECVERR so the kernel delivers ICMP errors
    // (Time Exceeded, Destination Unreachable) through the socket error queue.
    // Silently ignore EPERM — on non-Linux platforms or restricted environments
    // the probe will fall back to a timeout-only behaviour.
    let _ = enable_ip_recverr(&socket);

    Ok(socket)
}

/// Executes a probe using the **unprivileged SOCK_STREAM fallback path**.
///
/// Opens a TCP stream socket, enables `IP_RECVERR` / `IPV6_RECVERR`, sets non-blocking
/// mode for the `connect` call (which transmits the SYN), and waits using `poll(2)`
/// for connection completion (`Ok(())`), destination reset (`ECONNREFUSED`), or an
/// ICMP error from intermediate routers or the destination delivered to the socket
/// error queue.
fn probe_unprivileged(dest: IpAddr, dest_port: u16, ttl: u16, timeout: Duration) -> TracerouteHop {
    let failed_hop = TracerouteHop {
        success: false,
        address: None,
        ttl,
        rtt: None,
    };

    // Create and configure the probe socket.
    let socket = match create_tcp_stream_probe_socket(dest, ttl) {
        Ok(s) => s,
        Err(err) => {
            tracing::debug!(ttl, "TCP stream probe socket creation failed: {}", err);
            return failed_hop;
        }
    };

    // Set non-blocking mode so that connect returns immediately (EINPROGRESS).
    // This allows us to record the start time precisely before the SYN is sent.
    if let Err(err) = socket.set_nonblocking(true) {
        tracing::debug!(ttl, "Failed to set non-blocking mode: {}", err);
        return failed_hop;
    }

    let dest_sock_addr = SockAddr::from(SocketAddr::new(dest, dest_port));
    let (time_exceeded_type, dest_unreach_type) = icmp_types_for_dest(dest);

    // Record the start time immediately before the SYN is dispatched.
    let start = Instant::now();

    // Perform a non-blocking connect. This transmits the TCP SYN packet.
    match socket.connect(&dest_sock_addr) {
        Ok(()) => {
            // Immediate synchronous connection established (e.g. loopback open port).
            return TracerouteHop {
                success: true,
                address: Some(dest),
                ttl,
                rtt: Some(start.elapsed()),
            };
        }
        Err(ref err) if err.raw_os_error() == Some(libc::ECONNREFUSED) => {
            // Immediate connection refused (e.g. loopback closed port receiving RST).
            return TracerouteHop {
                success: true,
                address: Some(dest),
                ttl,
                rtt: Some(start.elapsed()),
            };
        }
        Err(ref err)
            if err.kind() == std::io::ErrorKind::WouldBlock
                || err.raw_os_error() == Some(libc::EINPROGRESS) =>
        {
            // Expected asynchronous connect progress — proceed to poll.
        }
        Err(err) => {
            // Genuine connect failure (network unreachable, permission denied, etc.).
            // Check error queue once before exiting in case an ICMP error arrived.
            tracing::debug!(ttl, "TCP connect failed on non-blocking socket: {}", err);
            if let Some((router_or_dest_ip, icmp_type, icmp_code)) = recv_from_errqueue(&socket) {
                if icmp_type == time_exceeded_type {
                    return TracerouteHop {
                        success: true,
                        address: Some(router_or_dest_ip),
                        ttl,
                        rtt: Some(start.elapsed()),
                    };
                } else if icmp_type == dest_unreach_type {
                    // Both conditions must hold: offender IP matches destination AND
                    // the ICMP code is Port Unreachable (IPv4 = 3, IPv6 = 4).
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
                        // Intermediate router returning unreachable — report actual offender IP.
                        return TracerouteHop {
                            success: true,
                            address: Some(router_or_dest_ip),
                            ttl,
                            rtt: Some(start.elapsed()),
                        };
                    }
                }
            }
            return failed_hop;
        }
    }

    // Wait for socket writability or an error queue notification using poll(2).
    let poll_start = Instant::now();
    while poll_start.elapsed() < timeout {
        let remaining = timeout.saturating_sub(poll_start.elapsed());

        #[cfg(unix)]
        let is_writable = {
            use std::os::unix::io::AsRawFd;
            let fd = socket.as_raw_fd();
            let mut pfd = libc::pollfd {
                fd,
                events: libc::POLLOUT | libc::POLLIN | libc::POLLPRI,
                revents: 0,
            };
            let timeout_ms = remaining.as_millis().min(i32::MAX as u128) as libc::c_int;

            // SAFETY: pfd is a valid stack-allocated pollfd referencing our active socket descriptor.
            let ret = unsafe { libc::poll(&mut pfd, 1, timeout_ms) };
            if ret < 0 {
                let err = std::io::Error::last_os_error();
                if err.raw_os_error() == Some(libc::EINTR) {
                    continue;
                }
                tracing::debug!(ttl, "libc::poll encountered error: {}", err);
                break;
            }
            if ret == 0 {
                // Poll timeout elapsed without events.
                break;
            }
            pfd.revents & libc::POLLOUT != 0
        };
        #[cfg(not(unix))]
        let is_writable: bool = {
            std::thread::sleep(remaining);
            break;
        };

        // Check if an ICMP error arrived via the socket error queue.
        if let Some((router_or_dest_ip, icmp_type, icmp_code)) = recv_from_errqueue(&socket) {
            if icmp_type == time_exceeded_type {
                return TracerouteHop {
                    success: true,
                    address: Some(router_or_dest_ip),
                    ttl,
                    rtt: Some(start.elapsed()),
                };
            } else if icmp_type == dest_unreach_type {
                // Both conditions must hold: offender IP matches destination AND
                // the ICMP code is Port Unreachable (IPv4 = 3, IPv6 = 4).
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
                    // Intermediate router returning unreachable — report actual offender IP.
                    return TracerouteHop {
                        success: true,
                        address: Some(router_or_dest_ip),
                        ttl,
                        rtt: Some(start.elapsed()),
                    };
                }
            }
        }

        // Only inspect socket error status if the socket became writable (POLLOUT).
        // A non-blocking connect in progress has SO_ERROR == 0 before completion;
        // checking take_error without POLLOUT risks false-positive connection detection.
        if is_writable {
            match socket.take_error() {
                Ok(None) => {
                    // Connection established successfully — destination reached.
                    return TracerouteHop {
                        success: true,
                        address: Some(dest),
                        ttl,
                        rtt: Some(start.elapsed()),
                    };
                }
                Ok(Some(err))
                    if err.raw_os_error() == Some(libc::ECONNREFUSED)
                        || err.raw_os_error() == Some(libc::ECONNRESET) =>
                {
                    // Destination replied with RST (port closed) — destination reached.
                    return TracerouteHop {
                        success: true,
                        address: Some(dest),
                        ttl,
                        rtt: Some(start.elapsed()),
                    };
                }
                Ok(Some(err)) => {
                    tracing::debug!(ttl, "TCP socket error after poll: {}", err);
                    // Check error queue one more time before terminating.
                    if let Some((router_or_dest_ip, icmp_type, icmp_code)) =
                        recv_from_errqueue(&socket)
                    {
                        if icmp_type == time_exceeded_type {
                            return TracerouteHop {
                                success: true,
                                address: Some(router_or_dest_ip),
                                ttl,
                                rtt: Some(start.elapsed()),
                            };
                        } else if icmp_type == dest_unreach_type {
                            let is_port_unreachable = (dest.is_ipv4() && icmp_code == 3)
                                || (dest.is_ipv6() && icmp_code == 4);
                            if router_or_dest_ip == dest && is_port_unreachable {
                                return TracerouteHop {
                                    success: true,
                                    address: Some(dest),
                                    ttl,
                                    rtt: Some(start.elapsed()),
                                };
                            } else {
                                return TracerouteHop {
                                    success: true,
                                    address: Some(router_or_dest_ip),
                                    ttl,
                                    rtt: Some(start.elapsed()),
                                };
                            }
                        }
                    }
                    break;
                }
                Err(err) => {
                    tracing::debug!(
                        ttl,
                        "Failed to retrieve socket error via take_error: {}",
                        err
                    );
                    break;
                }
            }
        }
    }

    // Final check on error queue before recording a timed-out hop.
    if let Some((router_or_dest_ip, icmp_type, icmp_code)) = recv_from_errqueue(&socket) {
        if icmp_type == time_exceeded_type {
            return TracerouteHop {
                success: true,
                address: Some(router_or_dest_ip),
                ttl,
                rtt: Some(start.elapsed()),
            };
        } else if icmp_type == dest_unreach_type {
            // Both conditions must hold: offender IP matches destination AND
            // the ICMP code is Port Unreachable (IPv4 = 3, IPv6 = 4).
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
                // Intermediate router returning unreachable — report actual offender IP.
                return TracerouteHop {
                    success: true,
                    address: Some(router_or_dest_ip),
                    ttl,
                    rtt: Some(start.elapsed()),
                };
            }
        }
    }

    failed_hop
}

// ─── Single-hop probe dispatcher ─────────────────────────────────────────────

/// Dispatches a single TCP probe for the given TTL.
///
/// First attempts the **privileged raw socket path**. If raw socket creation
/// succeeds (i.e. the process holds `CAP_NET_RAW` or is root), the pre-opened
/// raw TCP and ICMP sockets are passed directly to `probe_privileged` — no
/// duplicate socket opens occur. If raw socket creation fails with `EPERM` the
/// engine transparently falls back to the **unprivileged SOCK_STREAM path**.
///
/// All blocking socket operations are executed inside `tokio::task::spawn_blocking`
/// so they do not stall the async executor.
async fn probe_single_hop(
    dest: IpAddr,
    dest_port: u16,
    ttl: u16,
    timeout: Duration,
    src_v4: Ipv4Addr,
    src_port: u16,
) -> TracerouteHop {
    let result = tokio::task::spawn_blocking(move || {
        let domain = if dest.is_ipv4() {
            Domain::IPV4
        } else {
            Domain::IPV6
        };

        // ── Privileged path ──────────────────────────────────────────────────
        // Attempt raw socket probe first. If the process holds CAP_NET_RAW or
        // runs as root, this path will succeed and produce the most accurate
        // RTT measurements with probe-correlated ICMP validation.
        //
        // The ICMP listen socket is opened first (before the SYN is sent) to
        // eliminate the race window where a reply arrives before the socket is
        // ready. The same pre-opened raw TCP socket is passed to probe_privileged
        // to avoid a redundant Socket::new call.
        if let IpAddr::V4(dst_v4) = dest {
            // Attempt to open the raw TCP socket. On failure (EPERM) fall through
            // to the unprivileged path without any retry.
            if let Ok(tx_socket) = try_open_raw_tcp_socket(domain) {
                // Open the raw ICMP listen socket BEFORE setting TTL or sending
                // the SYN, ensuring no reply can be missed.
                match try_open_raw_icmp_socket(domain, dest) {
                    Ok(rx_socket) => {
                        // Set the TTL on the transmit socket so that intermediate
                        // routers emit ICMP Time Exceeded when it reaches zero.
                        let ttl_set_ok = if dest.is_ipv4() {
                            tx_socket.set_ttl_v4(ttl as u32)
                        } else {
                            tx_socket.set_unicast_hops_v6(ttl as u32)
                        }
                        .is_ok();

                        if ttl_set_ok {
                            // Generate a random TCP sequence number for probe
                            // correlation. The same value is embedded in the SYN
                            // packet and used to validate the ICMP inner header.
                            let probe_seq: u32 = rand::random();

                            match build_tcp_probe_packet(
                                src_v4, dst_v4, src_port, dest_port, probe_seq, ttl as u8,
                            ) {
                                Ok(syn_bytes) => {
                                    if let Some(hop) = probe_privileged(&PrivilegedProbeParams {
                                        tx_socket: &tx_socket,
                                        rx_socket: &rx_socket,
                                        syn_bytes: &syn_bytes,
                                        dest,
                                        dest_port,
                                        src_port,
                                        ttl,
                                        timeout,
                                        probe_seq,
                                    }) {
                                        return hop;
                                    }
                                    // Privileged probe timed out — return failed hop.
                                    return TracerouteHop {
                                        success: false,
                                        address: None,
                                        ttl,
                                        rtt: None,
                                    };
                                }
                                Err(err) => {
                                    tracing::debug!(ttl, "Failed to build TCP SYN packet: {}", err);
                                    return TracerouteHop {
                                        success: false,
                                        address: None,
                                        ttl,
                                        rtt: None,
                                    };
                                }
                            }
                        }
                    }
                    Err(err) => {
                        tracing::debug!(ttl, "Raw ICMP socket creation failed: {}", err);
                        return TracerouteHop {
                            success: false,
                            address: None,
                            ttl,
                            rtt: None,
                        };
                    }
                }
            }
            // If try_open_raw_tcp_socket returned Err (EPERM) we arrive here
            // and drop directly into the unprivileged path below.
        }
        // IPv6 destination: raw packet building is not implemented, so we
        // always use the unprivileged SOCK_STREAM path for IPv6.

        // ── Unprivileged fallback path ────────────────────────────────────────
        // Taken when any of the following conditions hold:
        //   a) Raw TCP socket creation failed with EPERM (no CAP_NET_RAW).
        //   b) Raw ICMP socket creation failed.
        //   c) The destination is IPv6 (raw IPv6 packet building is not
        //      implemented in this engine).
        probe_unprivileged(dest, dest_port, ttl, timeout)
    })
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

// ─── Orchestrator ─────────────────────────────────────────────────────────────

/// Executes an asynchronous TCP traceroute across all hops up to `max_hops`, returning
/// the reduced hop map alongside the root OpenTelemetry span context.
///
/// Emits an OpenTelemetry root span (`"traceroute.trace"`) and child spans (`"traceroute.hop"`)
/// for each completed probe query. Dispatches individual probe queries governed by a rate-limiting
/// [`Semaphore`] that strictly enforces `parallel_requests` concurrency bounds.
pub async fn run_tcp_traceroute_with_span_context(
    config: TcpTracerouteConfig,
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
        "tcp",
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

    // Discover outbound IP and ephemeral port once per trace to avoid redundant UDP socket allocations.
    let (src_v4, src_port) = if let IpAddr::V4(dst_v4) = config.destination {
        discover_outbound_ipv4_and_port(dst_v4).unwrap_or((Ipv4Addr::UNSPECIFIED, 0))
    } else {
        (Ipv4Addr::UNSPECIFIED, 0)
    };

    for ttl in 1..=config.max_hops {
        if reached_destination {
            break;
        }

        let mut hop_join_set = tokio::task::JoinSet::new();
        for query_idx in 0..config.queries_per_hop {
            let sem = Arc::clone(&semaphore);
            let timeout = config.timeout;
            let dest = config.destination;
            let dest_port = config.dest_port;
            let queries_per_hop = config.queries_per_hop;

            let query_src_port = if src_port != 0 {
                ((src_port as u32 + (ttl as u32 * queries_per_hop as u32) + query_idx as u32)
                    % 16384
                    + 49152) as u16
            } else {
                0
            };

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

                let hop =
                    probe_single_hop(dest, dest_port, ttl, timeout, src_v4, query_src_port).await;
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

/// Executes an asynchronous TCP traceroute across all hops up to `max_hops`.
///
/// Emits an OpenTelemetry root span (`"traceroute.trace"`) and child spans (`"traceroute.hop"`)
/// for each completed probe query. Dispatches individual probe queries governed by a rate-limiting
/// [`Semaphore`] that strictly enforces `parallel_requests` concurrency bounds.
pub async fn execute_tcp_trace(
    config: TcpTracerouteConfig,
) -> anyhow::Result<BTreeMap<u16, Vec<TracerouteHop>>> {
    run_tcp_traceroute_with_span_context(config)
        .await
        .map(|(hops, _)| hops)
}

/// Convenience alias executing an asynchronous TCP traceroute.
pub async fn run_tcp_traceroute(
    config: TcpTracerouteConfig,
) -> anyhow::Result<BTreeMap<u16, Vec<TracerouteHop>>> {
    run_tcp_traceroute_with_span_context(config)
        .await
        .map(|(hops, _)| hops)
}
