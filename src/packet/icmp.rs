//! ICMP payload demultiplexing routines for correlating traceroute probes.

use crate::packet::tcp::PacketError;
use etherparse::SlicedPacket;

/// Extracted identification data from an inner probe packet inside an ICMP error payload.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum ExtractedProbe {
    Udp { src_port: u16 },
    Tcp { seq: u32 },
}

/// Type alias for [`ExtractedProbe`].
pub type IcmpExtractedProbe = ExtractedProbe;

/// Slices an incoming ICMP datagram and recovers inner probe identification data.
pub fn parse_icmp_payload(data: &[u8]) -> Result<ExtractedProbe, PacketError> {
    if data.len() < 8 {
        return Err(PacketError::InvalidHeader("ICMP payload too short".into()));
    }

    // Attempt high-level sliced packet analysis first
    if let Ok(sliced) = SlicedPacket::from_ip(data) {
        if let Some(transport) = sliced.transport {
            return match transport {
                etherparse::TransportSlice::Udp(udp) => Ok(ExtractedProbe::Udp {
                    src_port: udp.source_port(),
                }),
                etherparse::TransportSlice::Tcp(tcp) => Ok(ExtractedProbe::Tcp {
                    seq: tcp.sequence_number(),
                }),
                _ => Err(PacketError::InvalidHeader(
                    "unsupported inner transport".into(),
                )),
            };
        }
    }

    // Handle truncated transport layer (e.g. RFC 792 ICMP payload containing only the first 8 bytes of TCP)
    if let Ok((ip_slice, _stop_err)) = etherparse::LaxIpSlice::from_slice(data) {
        let payload = ip_slice.payload().payload;
        match ip_slice.payload_ip_number() {
            etherparse::ip_number::UDP if payload.len() >= 8 => {
                let src_port = u16::from_be_bytes([payload[0], payload[1]]);
                return Ok(ExtractedProbe::Udp { src_port });
            }
            etherparse::ip_number::TCP if payload.len() >= 8 => {
                let seq = u32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]);
                return Ok(ExtractedProbe::Tcp { seq });
            }
            _ => {}
        }
    }

    Err(PacketError::InvalidHeader(
        "missing or invalid inner transport layer".into(),
    ))
}

/// Alias for [`parse_icmp_payload`].
pub fn parse_icmp_time_exceeded_payload(data: &[u8]) -> Result<IcmpExtractedProbe, PacketError> {
    parse_icmp_payload(data)
}
