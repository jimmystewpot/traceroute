use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use traceroute::engine::tcp::{execute_tcp_trace, run_tcp_traceroute, TcpTracerouteConfig};
use traceroute::engine::udp::{execute_udp_trace, run_udp_traceroute, UdpTracerouteConfig};

#[tokio::test]
async fn test_rate_limiter_semaphore_bounds() {
    let sem = Arc::new(Semaphore::new(4));
    let active_count = Arc::new(AtomicUsize::new(0));
    let max_active = Arc::new(AtomicUsize::new(0));
    let mut handles = Vec::new();

    for _ in 0..10 {
        let sem_clone = Arc::clone(&sem);
        let active_clone = Arc::clone(&active_count);
        let max_clone = Arc::clone(&max_active);
        handles.push(tokio::spawn(async move {
            let _permit = sem_clone.acquire().await.unwrap();
            let current = active_clone.fetch_add(1, Ordering::SeqCst) + 1;
            max_clone.fetch_max(current, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(10)).await;
            active_clone.fetch_sub(1, Ordering::SeqCst);
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    assert!(max_active.load(Ordering::SeqCst) <= 4);
}

#[tokio::test]
async fn test_execute_udp_trace_structure_and_bounds() {
    let config = UdpTracerouteConfig {
        destination: IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)), // TEST-NET-1 — all hops time out
        destination_name: None,
        max_hops: 5,
        queries_per_hop: 3,
        parallel_requests: 2,
        timeout: Duration::from_millis(50),
        dest_port: 33434,
        span_links: Vec::new(),
        probe_limiter: None,
    };

    let results = execute_udp_trace(config).await.expect("trace succeeds");
    assert_eq!(results.len(), 5);
    for ttl in 1..=5 {
        let hops = results.get(&ttl).expect("hop exists");
        assert_eq!(hops.len(), 3);
        for hop in hops {
            assert_eq!(hop.ttl, ttl);
        }
    }
}

#[tokio::test]
async fn test_execute_tcp_trace_structure_and_bounds() {
    let config = TcpTracerouteConfig {
        destination: IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)), // TEST-NET-1 — all hops time out
        destination_name: None,
        max_hops: 4,
        queries_per_hop: 2,
        parallel_requests: 3,
        timeout: Duration::from_millis(50),
        dest_port: 80,
        span_links: Vec::new(),
        probe_limiter: None,
    };

    let results = execute_tcp_trace(config).await.expect("trace succeeds");
    assert_eq!(results.len(), 4);
    for ttl in 1..=4 {
        let hops = results.get(&ttl).expect("hop exists");
        assert_eq!(hops.len(), 2);
        for hop in hops {
            assert_eq!(hop.ttl, ttl);
        }
    }
}

#[tokio::test]
async fn test_run_trace_aliases() {
    let udp_config = UdpTracerouteConfig {
        destination: IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)), // TEST-NET-1 — all hops time out
        destination_name: None,
        max_hops: 2,
        queries_per_hop: 1,
        parallel_requests: 4,
        timeout: Duration::from_millis(20),
        dest_port: 33434,
        span_links: Vec::new(),
        probe_limiter: None,
    };
    let udp_res = run_udp_traceroute(udp_config)
        .await
        .expect("udp alias succeeds");
    assert_eq!(udp_res.len(), 2);

    let tcp_config = TcpTracerouteConfig {
        destination: IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)), // TEST-NET-1 — all hops time out
        destination_name: None,
        max_hops: 2,
        queries_per_hop: 1,
        parallel_requests: 4,
        timeout: Duration::from_millis(20),
        dest_port: 443,
        span_links: Vec::new(),
        probe_limiter: None,
    };
    let tcp_res = run_tcp_traceroute(tcp_config)
        .await
        .expect("tcp alias succeeds");
    assert_eq!(tcp_res.len(), 2);
}

#[tokio::test]
async fn test_execute_trace_edge_cases() {
    // max_hops = 0
    let zero_hops_config = UdpTracerouteConfig {
        destination: IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
        destination_name: None,
        max_hops: 0,
        queries_per_hop: 3,
        parallel_requests: 4,
        timeout: Duration::from_millis(10),
        dest_port: 33434,
        span_links: Vec::new(),
        probe_limiter: None,
    };
    let results = execute_udp_trace(zero_hops_config)
        .await
        .expect("zero hops succeeds");
    assert!(results.is_empty());

    // parallel_requests = 0 fallback
    let zero_parallel_config = TcpTracerouteConfig {
        destination: IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)), // TEST-NET-1 — all hops time out
        destination_name: None,
        max_hops: 2,
        queries_per_hop: 1,
        parallel_requests: 0,
        timeout: Duration::from_millis(10),
        dest_port: 80,
        span_links: Vec::new(),
        probe_limiter: None,
    };
    let results = execute_tcp_trace(zero_parallel_config)
        .await
        .expect("zero parallel succeeds");
    assert_eq!(results.len(), 2);

    // IPv6 destination — use documentation prefix (RFC 3849), which is unreachable,
    // so all hops time out and the hop count equals max_hops.
    let ipv6_config = UdpTracerouteConfig {
        destination: IpAddr::V6(Ipv6Addr::new(0x2001, 0x0db8, 0, 0, 0, 0, 0, 1)),
        destination_name: None,
        max_hops: 2,
        queries_per_hop: 2,
        parallel_requests: 2,
        timeout: Duration::from_millis(10),
        dest_port: 33434,
        span_links: Vec::new(),
        probe_limiter: None,
    };
    let results = execute_udp_trace(ipv6_config).await.expect("ipv6 succeeds");
    assert_eq!(results.len(), 2);
}

#[tokio::test]
async fn test_shared_probe_limiter_bounds_across_multiple_traces() {
    let shared_limiter = Arc::new(Semaphore::new(2));
    let mut handles = Vec::new();

    for _ in 0..3 {
        let limiter = Arc::clone(&shared_limiter);
        handles.push(tokio::spawn(async move {
            let config = UdpTracerouteConfig {
                destination: IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)),
                destination_name: None,
                max_hops: 2,
                queries_per_hop: 2,
                parallel_requests: 10, // overridden by probe_limiter
                timeout: Duration::from_millis(50),
                dest_port: 33434,
                span_links: Vec::new(),
                probe_limiter: Some(limiter),
            };
            execute_udp_trace(config).await
        }));
    }

    for h in handles {
        let res = h.await.expect("join handle succeeds");
        assert!(res.is_ok(), "Trace with shared probe limiter must succeed");
        assert_eq!(res.unwrap().len(), 2);
    }
}
