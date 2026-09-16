//! QUIC Initial packet generation routines.

use rand::Rng;

const QUIC_PREFIX: [u8; 5] = [0xCA, 0xFF, 0xFF, 0xFF, 0xFF];
const TARGET_LENGTH: usize = 1200;

/// Generates a randomised QUIC Initial packet padded to 1,200 bytes.
pub fn generate_quic_initial_packet() -> Vec<u8> {
    let mut packet = Vec::with_capacity(TARGET_LENGTH);
    packet.extend_from_slice(&QUIC_PREFIX);

    let mut rng = rand::rng();

    // 10-byte Destination Connection ID
    packet.push(10);
    let mut dest_conn_id = [0u8; 10];
    rng.fill_bytes(&mut dest_conn_id);
    packet.extend_from_slice(&dest_conn_id);

    // 4-byte Source Connection ID
    packet.push(4);
    let mut src_conn_id = [0u8; 4];
    rng.fill_bytes(&mut src_conn_id);
    packet.extend_from_slice(&src_conn_id);

    // Pad remaining space with zeroes
    packet.resize(TARGET_LENGTH, 0);
    packet
}
