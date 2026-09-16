use etherparse::{PacketBuilder, TcpHeaderSlice};
use std::net::Ipv4Addr;
use traceroute::packet::icmp::{
    parse_icmp_payload, parse_icmp_time_exceeded_payload, ExtractedProbe, IcmpExtractedProbe,
};
use traceroute::packet::quic::generate_quic_initial_packet;
use traceroute::packet::tcp::{build_tcp_syn_ipv4, build_tcp_syn_packet};

#[test]
fn test_quic_packet_length_and_header() {
    let packet = generate_quic_initial_packet();
    assert_eq!(packet.len(), 1200);
    assert_eq!(&packet[0..5], &[0xCA, 0xFF, 0xFF, 0xFF, 0xFF]);
    assert_eq!(packet[5], 10); // Destination Connection ID length
    assert_eq!(packet[16], 4); // Source Connection ID length
}

#[test]
fn test_tcp_syn_packet_generation() {
    let src = Ipv4Addr::new(192, 168, 1, 100);
    let dst = Ipv4Addr::new(8, 8, 8, 8);
    let packet = build_tcp_syn_ipv4(src, dst, 49152, 443, 1000, 64).expect("valid tcp syn");
    assert!(!packet.is_empty());
    assert_eq!(packet.len(), 20); // Standard TCP header without options is 20 bytes

    // Parse header and verify SYN flag and non-zero checksum
    let slice = TcpHeaderSlice::from_slice(&packet).expect("valid tcp header slice");
    assert_eq!(slice.source_port(), 49152);
    assert_eq!(slice.destination_port(), 443);
    assert_eq!(slice.sequence_number(), 1000);
    assert!(slice.syn());
    assert!(!slice.ack());
    assert_ne!(slice.checksum(), 0);

    // Verify alias produces identical output
    let alias_packet =
        build_tcp_syn_packet(src, dst, 49152, 443, 1000, 64).expect("valid tcp syn via alias");
    assert_eq!(packet, alias_packet);
}

#[test]
fn test_parse_icmp_payload_udp() {
    let builder = PacketBuilder::ipv4([192, 168, 1, 100], [8, 8, 8, 8], 64).udp(54321, 33434);
    let mut payload = Vec::new();
    builder
        .write(&mut payload, &[1, 2, 3, 4])
        .expect("write packet");

    let probe = parse_icmp_payload(&payload).expect("parse udp probe");
    assert_eq!(probe, ExtractedProbe::Udp { src_port: 54321 });

    // Test alias function and type alias
    let alias_probe: IcmpExtractedProbe =
        parse_icmp_time_exceeded_payload(&payload).expect("parse via alias");
    assert_eq!(alias_probe, ExtractedProbe::Udp { src_port: 54321 });
}

#[test]
fn test_parse_icmp_payload_tcp() {
    let builder =
        PacketBuilder::ipv4([192, 168, 1, 100], [8, 8, 8, 8], 64).tcp(49152, 443, 99999, 14600);
    let mut payload = Vec::new();
    builder.write(&mut payload, &[]).expect("write packet");

    let probe = parse_icmp_payload(&payload).expect("parse tcp probe");
    assert_eq!(probe, ExtractedProbe::Tcp { seq: 99999 });
}

#[test]
fn test_parse_icmp_payload_truncated_tcp() {
    // RFC 792 ICMP payload contains IP header (20 bytes) + first 8 bytes of TCP header
    let builder =
        PacketBuilder::ipv4([192, 168, 1, 100], [8, 8, 8, 8], 64).tcp(49152, 443, 123456, 14600);
    let mut full_packet = Vec::new();
    builder.write(&mut full_packet, &[]).expect("write packet");

    // Truncate to IPv4 header (20 bytes) + 8 bytes of TCP (total 28 bytes)
    let truncated = &full_packet[..28];
    let probe = parse_icmp_payload(truncated).expect("parse truncated tcp probe");
    assert_eq!(probe, ExtractedProbe::Tcp { seq: 123456 });
}

#[test]
fn test_parse_icmp_payload_ipv6_udp() {
    let builder = PacketBuilder::ipv6(
        [0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
        [0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2],
        64,
    )
    .udp(60000, 33434);
    let mut payload = Vec::new();
    builder
        .write(&mut payload, &[10, 20, 30])
        .expect("write packet");

    let probe = parse_icmp_payload(&payload).expect("parse ipv6 udp probe");
    assert_eq!(probe, ExtractedProbe::Udp { src_port: 60000 });
}

#[test]
fn test_parse_icmp_payload_too_short() {
    let short_data = [0u8; 7];
    let result = parse_icmp_payload(&short_data);
    assert!(result.is_err());
}

#[test]
fn test_parse_icmp_payload_unsupported_protocol() {
    // IPv4 packet with ICMP payload (not TCP or UDP)
    let builder =
        PacketBuilder::ipv4([192, 168, 1, 100], [8, 8, 8, 8], 64).icmpv4_echo_request(1, 1);
    let mut payload = Vec::new();
    builder.write(&mut payload, &[]).expect("write packet");

    let result = parse_icmp_payload(&payload);
    assert!(result.is_err());
}
