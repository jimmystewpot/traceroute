//! TCP packet generation routines with pseudo-header checksum calculation.

use etherparse::TcpHeader;
use std::net::Ipv4Addr;
use thiserror::Error;

/// Errors encountered during packet generation or parsing.
#[derive(Error, Debug, PartialEq, Eq, Clone)]
pub enum PacketError {
    #[error("packet serialisation error: {0}")]
    Serialisation(String),
    #[error("invalid header format: {0}")]
    InvalidHeader(String),
}

/// Serialises an IPv4 TCP SYN packet with accurate internet checksums.
pub fn build_tcp_syn_ipv4(
    src_ip: Ipv4Addr,
    dst_ip: Ipv4Addr,
    src_port: u16,
    dst_port: u16,
    seq: u32,
    _ttl: u8,
) -> Result<Vec<u8>, PacketError> {
    let mut tcp_header = TcpHeader::new(src_port, dst_port, seq, 14600);
    tcp_header.syn = true;

    // Calculate TCP checksum using the IPv4 pseudo-header
    tcp_header.checksum = tcp_header
        .calc_checksum_ipv4_raw(src_ip.octets(), dst_ip.octets(), &[])
        .map_err(|e| PacketError::Serialisation(e.to_string()))?;

    let mut buf = Vec::with_capacity(tcp_header.header_len());
    tcp_header
        .write(&mut buf)
        .map_err(|e| PacketError::Serialisation(e.to_string()))?;

    Ok(buf)
}

/// Alias for [`build_tcp_syn_ipv4`].
pub fn build_tcp_syn_packet(
    src_ip: Ipv4Addr,
    dst_ip: Ipv4Addr,
    src_port: u16,
    dst_port: u16,
    seq: u32,
    ttl: u8,
) -> Result<Vec<u8>, PacketError> {
    build_tcp_syn_ipv4(src_ip, dst_ip, src_port, dst_port, seq, ttl)
}
