//! OpenTelemetry baggage builder for traceroute metadata.
//!
//! Constructs metadata baggage key-value pairs associated with the traceroute execution.

use std::collections::HashMap;

/// Constructs metadata baggage key-value pairs associated with the traceroute execution.
pub fn build_traceroute_baggage(
    dest: &str,
    source: &str,
    max_hops: u16,
    xid: &str,
) -> HashMap<String, String> {
    let mut map = HashMap::new();
    map.insert("destination_hostname".into(), dest.to_string());
    map.insert("source".into(), source.to_string());
    map.insert("max_hops".into(), max_hops.to_string());
    map.insert("xid".into(), xid.to_string());
    map
}
