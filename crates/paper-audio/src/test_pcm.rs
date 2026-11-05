pub(crate) fn pcm_wav(channels: u16, channel_mask: Option<u32>) -> Vec<u8> {
    let frames = 4_800;
    let format_size = if channel_mask.is_some() { 40_u32 } else { 16 };
    let data_size = frames * u32::from(channels) * 2;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(20 + format_size + data_size).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&format_size.to_le_bytes());
    bytes.extend_from_slice(&if channel_mask.is_some() { 0xfffe_u16 } else { 1 }.to_le_bytes());
    bytes.extend_from_slice(&channels.to_le_bytes());
    bytes.extend_from_slice(&48_000_u32.to_le_bytes());
    bytes.extend_from_slice(&(48_000 * u32::from(channels) * 2).to_le_bytes());
    bytes.extend_from_slice(&(channels * 2).to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    if let Some(mask) = channel_mask {
        bytes.extend_from_slice(&22_u16.to_le_bytes());
        bytes.extend_from_slice(&16_u16.to_le_bytes());
        bytes.extend_from_slice(&mask.to_le_bytes());
        bytes.extend_from_slice(&[1, 0, 0, 0, 0, 0, 16, 0, 128, 0, 0, 170, 0, 56, 155, 113]);
    }
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_size.to_le_bytes());
    for index in 0..frames {
        let sample = ((index as f32 * std::f32::consts::TAU / 100.0).sin() * 8_192.0) as i16;
        for channel in 0..channels {
            bytes.extend_from_slice(&if channel % 2 == 0 { sample } else { -sample }.to_le_bytes());
        }
    }
    bytes
}
