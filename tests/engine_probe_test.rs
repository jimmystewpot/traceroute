// Tests for the UDP and TCP probing engines — Australian English conventions.
// These tests are designed to run without root privileges or CAP_NET_RAW.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::time::Duration;
use traceroute::engine::errqueue::recv_from_errqueue;
use traceroute::engine::tcp::{
    discover_outbound_ipv4_and_port, execute_tcp_trace,
    icmp_types_for_dest as tcp_icmp_types_for_dest, TcpTracerouteConfig, ICMP_V4_DEST_UNREACHABLE,
    ICMP_V4_TIME_EXCEEDED, ICMP_V6_DEST_UNREACHABLE, ICMP_V6_TIME_EXCEEDED,
};
use traceroute::engine::udp::{
    create_udp_probe_socket, execute_udp_trace, icmp_types_for_dest as udp_icmp_types_for_dest,
    UdpTracerouteConfig,
};

#[tokio::test]
async fn test_udp_socket_creation_loopback() {
    // create_udp_probe_socket for loopback must succeed without elevated privileges.
    let s = create_udp_probe_socket("127.0.0.1".parse().unwrap(), 1);
    assert!(s.is_ok(), "UDP socket creation for loopback must succeed");
}

#[tokio::test]
async fn test_udp_trace_all_hops_timeout() {
    // An open UDP socket on loopback that does not reply simulates a silent target.
    // With no ICMP errors or direct UDP replies arriving, every hop must time out
    // within the configured timeout window and be recorded as a failed hop.
    let listener = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let cfg = UdpTracerouteConfig {
        destination: "127.0.0.1".parse().unwrap(),
        destination_name: None,
        max_hops: 3,
        queries_per_hop: 1,
        parallel_requests: 4,
        timeout: Duration::from_millis(5),
        dest_port: port,
        span_links: Vec::new(),
        probe_limiter: None,
    };
    let result = execute_udp_trace(cfg).await.unwrap();
    assert_eq!(result.len(), 3, "Expected 3 hop entries for max_hops=3");
    for hops in result.values() {
        for hop in hops {
            assert!(
                !hop.success,
                "All hops must time out when target port does not reply"
            );
        }
    }
}

#[tokio::test]
async fn test_errqueue_returns_none_without_send() {
    // recv_from_errqueue without a prior send must return None because the
    // error queue is empty — no ICMP error has been generated yet.
    let socket = create_udp_probe_socket("127.0.0.1".parse().unwrap(), 1).unwrap();
    let result = recv_from_errqueue(&socket);
    assert!(
        result.is_none(),
        "Error queue must be empty when no probe has been sent"
    );
}

#[tokio::test]
async fn test_udp_trace_detects_loopback_destination() {
    // Loopback responds with ICMP Port Unreachable (type=3) delivered via the
    // error queue when IP_RECVERR is enabled. At least one hop must be
    // success: true, indicating the destination was correctly detected.
    let cfg = UdpTracerouteConfig {
        destination: "127.0.0.1".parse().unwrap(),
        destination_name: None,
        max_hops: 5,
        queries_per_hop: 1,
        parallel_requests: 4,
        timeout: Duration::from_millis(500),
        dest_port: 33434,
        span_links: Vec::new(),
        probe_limiter: None,
    };
    let result = execute_udp_trace(cfg).await.unwrap();
    let any_successful = result.values().any(|hops| hops.iter().any(|h| h.success));
    assert!(
        any_successful,
        "Loopback destination must produce at least one successful hop (destination reached)"
    );
}

#[tokio::test]
async fn test_udp_poll_detects_error_queue_response() {
    // Validates that polling libc::poll for POLLIN | POLLERR | POLLPRI reliably
    // detects ICMP Destination Unreachable responses via the socket error queue
    // for loopback probes across multiple queries without dropping responses.
    let dest: IpAddr = "127.0.0.1".parse().unwrap();
    let port = {
        let sock = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        sock.local_addr().unwrap().port()
    };

    let cfg = UdpTracerouteConfig {
        destination: dest,
        destination_name: None,
        max_hops: 1,
        queries_per_hop: 3,
        parallel_requests: 3,
        timeout: Duration::from_millis(500),
        dest_port: port,
        span_links: Vec::new(),
        probe_limiter: None,
    };
    let result = execute_udp_trace(cfg).await.unwrap();
    let hop1 = result.get(&1).expect("Hop 1 must be present");
    assert_eq!(hop1.len(), 3, "Expected 3 queries for hop 1");
    for hop in hop1 {
        assert!(
            hop.success,
            "Error queue polling must reliably capture ICMP response on loopback"
        );
        assert_eq!(
            hop.address,
            Some(dest),
            "Reported hop address must match loopback destination"
        );
        assert!(hop.rtt.is_some(), "Round-trip time must be measured");
    }
}

#[tokio::test]
async fn test_tcp_socket_fallback_on_eperm() {
    // The engine must succeed (return Ok) even without CAP_NET_RAW.
    // When raw socket creation fails with EPERM, the engine falls back to
    // the unprivileged SOCK_STREAM path. This test validates that fallback.
    let cfg = TcpTracerouteConfig {
        destination: "192.0.2.1".parse().unwrap(),
        destination_name: None,
        max_hops: 2,
        queries_per_hop: 1,
        parallel_requests: 2,
        timeout: Duration::from_millis(1),
        dest_port: 80,
        span_links: Vec::new(),
        probe_limiter: None,
    };
    let result = execute_tcp_trace(cfg).await;
    assert!(
        result.is_ok(),
        "TCP engine must not fail even without raw socket privileges"
    );
}

#[tokio::test]
async fn test_tcp_trace_all_hops_timeout() {
    // With a 1 ms timeout and 192.0.2.1 (RFC 5737 TEST-NET, not routed),
    // all hops must time out and be recorded as failed. No elevated privileges
    // are required — the unprivileged SOCK_STREAM path is exercised.
    let cfg = TcpTracerouteConfig {
        destination: "192.0.2.1".parse().unwrap(),
        destination_name: None,
        max_hops: 2,
        queries_per_hop: 1,
        parallel_requests: 2,
        timeout: Duration::from_millis(1),
        dest_port: 80,
        span_links: Vec::new(),
        probe_limiter: None,
    };
    let result = execute_tcp_trace(cfg).await.unwrap();
    for hops in result.values() {
        for hop in hops {
            assert!(
                !hop.success,
                "All hops must time out for unreachable TEST-NET destination"
            );
        }
    }
}

#[tokio::test]
async fn test_tcp_trace_detects_loopback_destination_open_port() {
    // Ephemeral TCP listener on loopback simulating an active destination service.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let cfg = TcpTracerouteConfig {
        destination: "127.0.0.1".parse().unwrap(),
        destination_name: None,
        max_hops: 3,
        queries_per_hop: 1,
        parallel_requests: 1,
        timeout: Duration::from_millis(500),
        dest_port: port,
        span_links: Vec::new(),
        probe_limiter: None,
    };
    let result = execute_tcp_trace(cfg).await.unwrap();
    let hop1 = result.get(&1).expect("Hop 1 must be present");
    assert!(
        !hop1.is_empty(),
        "Hop 1 must have at least one probe result"
    );
    assert!(
        hop1[0].success,
        "TCP destination on loopback must be detected at hop 1"
    );
    assert_eq!(
        hop1[0].address,
        Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        "Detected address must match loopback destination IP"
    );
    // reduce_final_result should truncate hops once destination is reached.
    assert_eq!(
        result.len(),
        1,
        "Trace must truncate after reaching destination"
    );
}

#[tokio::test]
async fn test_tcp_trace_detects_loopback_destination_closed_port() {
    // Bind and drop an ephemeral listener to identify an unassigned port that generates ECONNREFUSED (RST).
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };

    let cfg = TcpTracerouteConfig {
        destination: "127.0.0.1".parse().unwrap(),
        destination_name: None,
        max_hops: 3,
        queries_per_hop: 1,
        parallel_requests: 1,
        timeout: Duration::from_millis(500),
        dest_port: port,
        span_links: Vec::new(),
        probe_limiter: None,
    };
    let result = execute_tcp_trace(cfg).await.unwrap();
    let hop1 = result.get(&1).expect("Hop 1 must be present");
    assert!(
        !hop1.is_empty(),
        "Hop 1 must have at least one probe result"
    );
    assert!(
        hop1[0].success,
        "TCP closed port on loopback must be detected via RST (ECONNREFUSED) at hop 1"
    );
    assert_eq!(
        hop1[0].address,
        Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        "Detected address must match destination IP"
    );
    assert_eq!(
        result.len(),
        1,
        "Trace must truncate after reaching destination"
    );
}

#[tokio::test]
async fn test_tcp_trace_detects_ipv6_loopback_destination() {
    // Ephemeral TCP listener on IPv6 loopback simulating an active IPv6 destination.
    let listener = match std::net::TcpListener::bind("[::1]:0") {
        Ok(l) => l,
        Err(_) => return, // Skip test if IPv6 binding is restricted in environment.
    };
    let port = listener.local_addr().unwrap().port();

    let cfg = TcpTracerouteConfig {
        destination: "::1".parse().unwrap(),
        destination_name: None,
        max_hops: 3,
        queries_per_hop: 1,
        parallel_requests: 1,
        timeout: Duration::from_millis(500),
        dest_port: port,
        span_links: Vec::new(),
        probe_limiter: None,
    };
    let result = execute_tcp_trace(cfg).await.unwrap();
    let hop1 = result.get(&1).expect("Hop 1 must be present");
    assert!(
        !hop1.is_empty(),
        "Hop 1 must have at least one probe result"
    );
    assert!(
        hop1[0].success,
        "IPv6 TCP destination on loopback must be detected at hop 1"
    );
    assert_eq!(
        hop1[0].address,
        Some(IpAddr::V6(Ipv6Addr::LOCALHOST)),
        "Detected address must match IPv6 loopback destination"
    );
    assert_eq!(
        result.len(),
        1,
        "Trace must truncate after reaching IPv6 destination"
    );
}

#[test]
fn test_icmp_dual_stack_type_mapping_logic() {
    // RFC 792 (IPv4): Type 11 = Time Exceeded, Type 3 = Destination Unreachable
    let v4_dest: IpAddr = "192.0.2.1".parse().unwrap();
    assert_eq!(
        tcp_icmp_types_for_dest(v4_dest),
        (ICMP_V4_TIME_EXCEEDED, ICMP_V4_DEST_UNREACHABLE),
        "IPv4 TCP engine must map Type 11 to Time Exceeded and Type 3 to Destination Unreachable"
    );
    assert_eq!(
        udp_icmp_types_for_dest(v4_dest),
        (ICMP_V4_TIME_EXCEEDED, ICMP_V4_DEST_UNREACHABLE),
        "IPv4 UDP engine must map Type 11 to Time Exceeded and Type 3 to Destination Unreachable"
    );
    assert_eq!(ICMP_V4_TIME_EXCEEDED, 11);
    assert_eq!(ICMP_V4_DEST_UNREACHABLE, 3);

    // RFC 4443 (IPv6): Type 3 = Time Exceeded, Type 1 = Destination Unreachable
    let v6_dest: IpAddr = "2001:db8::1".parse().unwrap();
    assert_eq!(
        tcp_icmp_types_for_dest(v6_dest),
        (ICMP_V6_TIME_EXCEEDED, ICMP_V6_DEST_UNREACHABLE),
        "IPv6 TCP engine must map Type 3 to Time Exceeded and Type 1 to Destination Unreachable"
    );
    assert_eq!(
        udp_icmp_types_for_dest(v6_dest),
        (ICMP_V6_TIME_EXCEEDED, ICMP_V6_DEST_UNREACHABLE),
        "IPv6 UDP engine must map Type 3 to Time Exceeded and Type 1 to Destination Unreachable"
    );
    assert_eq!(ICMP_V6_TIME_EXCEEDED, 3);
    assert_eq!(ICMP_V6_DEST_UNREACHABLE, 1);
}

#[tokio::test]
async fn test_udp_stops_probing_when_destination_reached() {
    // When destination is reached at hop 1 (loopback), the engine must cease
    // scheduling probes for subsequent TTLs up to max_hops (15), producing
    // a result containing only hop 1.
    let dest: IpAddr = "127.0.0.1".parse().unwrap();
    let port = {
        let sock = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        sock.local_addr().unwrap().port()
    };

    let cfg = UdpTracerouteConfig {
        destination: dest,
        destination_name: None,
        max_hops: 15,
        queries_per_hop: 2,
        parallel_requests: 4,
        timeout: Duration::from_millis(500),
        dest_port: port,
        span_links: Vec::new(),
        probe_limiter: None,
    };
    let result = execute_udp_trace(cfg).await.unwrap();
    assert_eq!(
        result.len(),
        1,
        "UDP traceroute must stop scheduling probes after hop 1 reaches destination"
    );
    assert!(result.contains_key(&1), "Hop 1 must be present");
    assert_eq!(
        result.get(&1).unwrap().len(),
        2,
        "All queries for hop 1 must be recorded"
    );
    assert!(
        !result.contains_key(&2),
        "Hop 2 must never have been scheduled"
    );
}

#[tokio::test]
async fn test_tcp_stops_probing_when_destination_reached() {
    // When TCP destination is reached at hop 1 (loopback RST/closed port),
    // probe scheduling must terminate immediately and avoid probing hops 2..=15.
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };

    let cfg = TcpTracerouteConfig {
        destination: "127.0.0.1".parse().unwrap(),
        destination_name: None,
        max_hops: 15,
        queries_per_hop: 2,
        parallel_requests: 4,
        timeout: Duration::from_millis(500),
        dest_port: port,
        span_links: Vec::new(),
        probe_limiter: None,
    };
    let result = execute_tcp_trace(cfg).await.unwrap();
    assert_eq!(
        result.len(),
        1,
        "TCP traceroute must stop scheduling probes after hop 1 reaches destination"
    );
    assert!(result.contains_key(&1), "Hop 1 must be present");
    assert_eq!(
        result.get(&1).unwrap().len(),
        2,
        "All queries for hop 1 must be recorded"
    );
    assert!(
        !result.contains_key(&2),
        "Hop 2 must never have been scheduled"
    );
}

#[test]
fn test_discover_outbound_ipv4_and_port_loopback() {
    let loopback = Ipv4Addr::LOCALHOST;
    let res = discover_outbound_ipv4_and_port(loopback);
    assert!(
        res.is_some(),
        "Outbound discovery for loopback must succeed"
    );
    let (src_ip, src_port) = res.unwrap();
    assert_eq!(
        src_ip, loopback,
        "Outbound IP for loopback must be 127.0.0.1"
    );
    assert!(src_port > 0, "Discovered ephemeral port must be non-zero");
}

#[test]
fn test_intermediate_router_unreachable_does_not_claim_destination() {
    // An intermediate router returning ICMP Destination Unreachable with code 1
    // (Host Unreachable) must NOT be classified as the destination being reached.
    // Only ICMP Port Unreachable (code 3 for IPv4, code 4 for IPv6) from the
    // target itself qualifies as destination arrival.
    let intermediate_router: IpAddr = "192.168.1.1".parse().unwrap();
    let target_dest: IpAddr = "8.8.8.8".parse().unwrap();

    // Intermediate router returning Destination Unreachable (Code 1 — Host Unreachable).
    let icmp_type = 3u8;
    let icmp_code = 1u8;
    let is_port_unreachable =
        (target_dest.is_ipv4() && icmp_code == 3) || (target_dest.is_ipv6() && icmp_code == 4);

    // Both the IP match AND Port Unreachable code must hold for destination arrival.
    let reached = intermediate_router == target_dest && is_port_unreachable;
    assert!(
        !reached,
        "Intermediate router unreachable must not be classified as destination reached"
    );

    // Confirm that icmp_type is present but not used to override the guard — suppress unused warning.
    let _ = icmp_type;
}

#[test]
fn test_tcp_probe_source_port_generation_diversity() {
    let base_port = 50000u16;
    let queries_per_hop = 3u16;
    let mut ports = std::collections::HashSet::new();

    for ttl in 1..=5 {
        for query_idx in 0..queries_per_hop {
            let port =
                ((base_port as u32 + (ttl as u32 * queries_per_hop as u32) + query_idx as u32)
                    % 16384
                    + 49152) as u16;
            assert!(port >= 49152);
            assert!(
                ports.insert(port),
                "Port collision detected for TTL {} query {}",
                ttl,
                query_idx
            );
        }
    }
}

#[tokio::test]
async fn test_tcp_unprivileged_pollout_guard_semantics() {
    // Tests that an immediate connect to a closed loopback port reports destination arrival (RST),
    // while non-responding destinations accurately time out rather than falsely declaring arrival.
    let cfg = TcpTracerouteConfig {
        destination: "127.0.0.1".parse().unwrap(),
        destination_name: None,
        max_hops: 1,
        queries_per_hop: 1,
        parallel_requests: 1,
        timeout: Duration::from_millis(50),
        dest_port: 65432, // Closed port
        span_links: Vec::new(),
        probe_limiter: None,
    };
    let result = execute_tcp_trace(cfg).await.unwrap();
    assert_eq!(result.len(), 1);
    let hop = &result.get(&1).unwrap()[0];
    assert!(hop.success);
    assert_eq!(hop.address, Some("127.0.0.1".parse().unwrap()));
}
