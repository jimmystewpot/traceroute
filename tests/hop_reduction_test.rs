use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;
use traceroute::engine::hop::{reduce_final_result, TracerouteHop};

#[test]
fn test_reduce_final_result_truncates_after_destination() {
    let dest_ip: IpAddr = IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8));
    let mut preliminary: BTreeMap<u16, Vec<TracerouteHop>> = BTreeMap::new();

    // Hop 1: Intermediate router
    preliminary.insert(
        1,
        vec![TracerouteHop {
            success: true,
            address: Some(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1))),
            ttl: 1,
            rtt: Some(Duration::from_millis(5)),
        }],
    );

    // Hop 2: Destination reached
    preliminary.insert(
        2,
        vec![TracerouteHop {
            success: true,
            address: Some(dest_ip),
            ttl: 2,
            rtt: Some(Duration::from_millis(15)),
        }],
    );

    // Hop 3: Extraneous hop that should be discarded
    preliminary.insert(
        3,
        vec![TracerouteHop {
            success: true,
            address: Some(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))),
            ttl: 3,
            rtt: Some(Duration::from_millis(20)),
        }],
    );

    let final_results = reduce_final_result(preliminary, 30, dest_ip);
    assert_eq!(final_results.len(), 2);
    assert!(final_results.contains_key(&1));
    assert!(final_results.contains_key(&2));
    assert!(!final_results.contains_key(&3));
}
