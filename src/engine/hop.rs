use std::collections::BTreeMap;
use std::net::IpAddr;
use std::time::Duration;

/// Represents the measurement outcome for a single probe at a given TTL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TracerouteHop {
    pub success: bool,
    pub address: Option<IpAddr>,
    pub ttl: u16,
    pub rtt: Option<Duration>,
}

/// Reduces preliminary hop measurements by discarding all probes subsequent to reaching the destination.
pub fn reduce_final_result(
    preliminary: BTreeMap<u16, Vec<TracerouteHop>>,
    max_hops: u16,
    dest_ip: IpAddr,
) -> BTreeMap<u16, Vec<TracerouteHop>> {
    let mut final_results = BTreeMap::new();

    for ttl in 1..=max_hops {
        if let Some(probes) = preliminary.get(&ttl) {
            let mut recorded_probes = Vec::new();
            let mut found_final = false;

            for probe in probes {
                if probe.success && probe.address == Some(dest_ip) {
                    found_final = true;
                }
                recorded_probes.push(probe.clone());
            }

            final_results.insert(ttl, recorded_probes);
            if found_final {
                break;
            }
        } else {
            break;
        }
    }

    final_results
}
