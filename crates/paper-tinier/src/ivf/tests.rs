#![cfg(test)]

use super::*;

fn fixture(packet_sizes: &[usize]) -> Vec<u8> {
    let mut bytes = Vec::from(&b"DKIF\0\0\x20\0AV01"[..]);
    bytes.extend_from_slice(&1920u16.to_le_bytes());
    bytes.extend_from_slice(&1080u16.to_le_bytes());
    bytes.extend_from_slice(&30_000u32.to_le_bytes());
    bytes.extend_from_slice(&1001u32.to_le_bytes());
    bytes.extend_from_slice(&u32::try_from(packet_sizes.len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    for (index, size) in packet_sizes.iter().copied().enumerate() {
        bytes.extend_from_slice(&u32::try_from(size).unwrap().to_le_bytes());
        bytes.extend_from_slice(&(index as u64).to_le_bytes());
        bytes.extend(std::iter::repeat_n(u8::try_from(index + 1).unwrap(), size));
    }
    bytes
}

fn load(bytes: &[u8]) -> Result<ResidentVideo, String> {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("loop.ivf");
    std::fs::write(&path, bytes).unwrap();
    ResidentVideo::load(&path)
}

#[test]
fn indexes_av1_loop() {
    let bytes = fixture(&[3, 2]);
    let video = load(&bytes).unwrap();
    assert_eq!(video.packet_count(), 2);
    assert_eq!(video.packet(0).unwrap().as_ref(), &[1, 1, 1]);
    assert_eq!(video.packet(1).unwrap().as_ref(), &[2, 2]);
    assert_eq!(video.frame_rate(), FrameRate { numerator: 30_000, denominator: 1001 });
    assert_eq!(video.source_bytes(), bytes.len());
    assert_eq!(video.resident_bytes(), 2 * std::mem::size_of::<PacketIndex>());
}

#[test]
fn rejects_malformed_inputs() {
    let mut wrong_codec = fixture(&[3]);
    wrong_codec[8..12].copy_from_slice(b"H264");
    assert!(load(&wrong_codec).is_err());

    let mut truncated = fixture(&[3]);
    truncated.pop();
    assert!(load(&truncated).is_err());

    let mut oversized = fixture(&[3]);
    oversized[32..36].copy_from_slice(&(MAX_PACKET_BYTES as u32 + 1).to_le_bytes());
    assert!(load(&oversized).is_err());

    let mut wrong_count = fixture(&[3]);
    wrong_count[24..28].copy_from_slice(&2u32.to_le_bytes());
    assert!(load(&wrong_count).is_err());
}

#[test]
fn rejects_bad_timestamps() {
    let mut timestamp = fixture(&[3, 3]);
    let second_header = 32 + 12 + 3;
    timestamp[second_header + 4..second_header + 12].copy_from_slice(&9u64.to_le_bytes());
    assert!(load(&timestamp).is_err());

    let mut zero_rate = fixture(&[3]);
    zero_rate[16..20].copy_from_slice(&0u32.to_le_bytes());
    assert!(load(&zero_rate).is_err());
}
