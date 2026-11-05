use super::{MipLevel, PixelFormat, Pixels, TexFormat, decoded_floor, parse, take_pixels};

use crate::tests::{FULL_VARIANT, build_tex, build_tex_levels, build_variant_tex, variant_fill};

fn psd_fixture(depth: u16, compressed: bool, channels: &[u16]) -> Vec<u8> {
    const RGB_MODE: u16 = 3;
    const SIXTEEN_BITS: u16 = 16;
    let mut bytes = b"8BPS".to_vec();
    bytes.extend(1_u16.to_be_bytes());
    bytes.extend([0; 6]);
    bytes.extend((channels.len() as u16).to_be_bytes());
    bytes.extend(1_u32.to_be_bytes());
    bytes.extend(1_u32.to_be_bytes());
    bytes.extend(depth.to_be_bytes());
    bytes.extend(RGB_MODE.to_be_bytes());
    for _ in 0..3 {
        bytes.extend(0_u32.to_be_bytes());
    }
    bytes.extend(u16::from(compressed).to_be_bytes());
    if compressed {
        let row_size = if depth == SIXTEEN_BITS { 3_u16 } else { 2_u16 };
        for _ in channels {
            bytes.extend(row_size.to_be_bytes());
        }
    }
    for &channel in channels {
        if compressed {
            bytes.push(u8::from(depth == SIXTEEN_BITS));
        }
        if depth == SIXTEEN_BITS {
            bytes.extend(channel.to_be_bytes());
        } else {
            bytes.push(channel as u8);
        }
    }
    bytes
}

#[test]
fn psd_composite_depths() {
    for compressed in [false, true] {
        let encoded = psd_fixture(8, compressed, &[191, 127, 127, 128]);
        let mut tex = parse(&build_tex("TEXB0003", 0, 0, &encoded, false, None), None).unwrap();
        tex.meta.free_image_format = Some(20);
        let pixels = take_pixels(&mut tex).unwrap();
        assert_eq!(pixels.format, PixelFormat::Rgba8);
        assert_eq!(&pixels.levels[0].data, &[127, 0, 0, 128]);

        let encoded = psd_fixture(16, compressed, &[0xc000, 0x7fff, 0x7fff, 0x8000]);
        let mut tex = parse(&build_tex("TEXB0003", 0, 0, &encoded, false, None), None).unwrap();
        tex.meta.free_image_format = Some(20);
        let pixels = take_pixels(&mut tex).unwrap();
        assert_eq!(pixels.format, PixelFormat::Rgba16);
        let sample = |channel| {
            u16::from_le_bytes(pixels.levels[0].data[channel..channel + 2].try_into().unwrap())
        };
        assert!((32769..=32771).contains(&sample(0)));
        assert_eq!([sample(2), sample(4), sample(6)], [0, 0, 0x8000]);
    }
}

#[test]
fn psd_rejects_bad_rows() {
    let valid = psd_fixture(16, true, &[0xffff, 0, 0, 0xffff]);
    for cut in 1..=3 {
        assert!(super::psd::decode(&valid[..valid.len() - cut]).is_none());
    }
    let mut bad = valid.clone();
    bad[4] = 0;
    bad[5] = 2;
    assert!(super::psd::decode(&bad).is_none());
    bad = valid.clone();
    bad[25] = 0;
    bad[26] = 4;
    assert!(super::psd::decode(&bad).is_none());
}

#[test]
fn psd_packbits_noop() {
    const ROW_TABLE: usize = 40;
    const ROW_BYTES: u16 = 3;
    let mut encoded = psd_fixture(8, true, &[255, 0, 0]);
    let payload = encoded.len() - 6;
    for length in encoded[ROW_TABLE..payload].chunks_exact_mut(2) {
        length.copy_from_slice(&ROW_BYTES.to_be_bytes());
    }
    encoded.truncate(payload);
    encoded.extend([0, 255, 0x80, 0, 0, 0x80, 0, 0, 0x80]);
    let (_, pixels) = super::psd::decode(&encoded).unwrap();
    assert_eq!(pixels.data, [255, 0, 0, 255]);
}

#[test]
fn psd_channel_rows() {
    const SIDE: u32 = 2;
    const ROW_BYTES: u16 = 3;
    const CHANNELS: u16 = 3;
    let planes = [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]];
    for compressed in [false, true] {
        let mut bytes = b"8BPS".to_vec();
        bytes.extend(1_u16.to_be_bytes());
        bytes.extend([0; 6]);
        bytes.extend(CHANNELS.to_be_bytes());
        bytes.extend(SIDE.to_be_bytes());
        bytes.extend(SIDE.to_be_bytes());
        bytes.extend(8_u16.to_be_bytes());
        bytes.extend(CHANNELS.to_be_bytes());
        for _ in 0..3 {
            bytes.extend(0_u32.to_be_bytes());
        }
        bytes.extend(u16::from(compressed).to_be_bytes());
        if compressed {
            for _ in 0..CHANNELS as usize * SIDE as usize {
                bytes.extend(ROW_BYTES.to_be_bytes());
            }
        }
        for plane in planes {
            for row in plane.chunks_exact(SIDE as usize) {
                if compressed {
                    bytes.push(1);
                }
                bytes.extend(row);
            }
        }
        let (format, pixels) = super::psd::decode(&bytes).unwrap();
        assert_eq!(format, PixelFormat::Rgba8);
        assert_eq!(
            pixels.data,
            [255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255]
        );
    }
}

#[test]
fn modern_raw_formats() {
    let cases = [
        (10, TexFormat::Rgba16f, PixelFormat::Rgba16f, 128),
        (11, TexFormat::Rg16f, PixelFormat::Rg16f, 64),
        (12, TexFormat::Bc7, PixelFormat::Bc7, 16),
        (13, TexFormat::R16f, PixelFormat::R16f, 32),
        (14, TexFormat::Bc6h, PixelFormat::Bc6h, 16),
        (15, TexFormat::Rgb565, PixelFormat::Rgb565, 32),
        (16, TexFormat::Rgba1010102, PixelFormat::Rgba1010102, 64),
    ];
    for (raw, format, pixel_format, bytes) in cases {
        let payload = vec![0x5a; bytes];
        let mut tex = parse(&build_tex("TEXB0002", raw, 0, &payload, false, None), None).unwrap();
        assert_eq!(tex.meta.format, format);
        assert_eq!(tex.meta.format.raw(), raw);
        let pixels = take_pixels(&mut tex).unwrap();
        assert_eq!(pixels.format, pixel_format);
        assert_eq!(pixels.levels[0].data, payload);
    }
}

#[test]
fn modern_size_checks() {
    for (raw, valid) in [(10, 127), (11, 63), (12, 15), (13, 31), (14, 15), (15, 31), (16, 63)] {
        let mut tex =
            parse(&build_tex("TEXB0002", raw, 0, &vec![0; valid], false, None), None).unwrap();
        assert!(take_pixels(&mut tex).is_none(), "format {raw}");
    }
    assert_eq!(PixelFormat::Bc7.level_bytes(5, 5), Some(64));
    assert_eq!(PixelFormat::Bc6h.level_bytes(1, 1), Some(16));
    assert!(PixelFormat::Bc7.compressed());
    assert!(PixelFormat::Bc6h.compressed());
}

#[test]
fn half_preview_channels() {
    let channels = [
        half::f16::from_f32(2.0).to_bits(),
        half::f16::from_f32(0.5).to_bits(),
        half::f16::from_f32(-1.0).to_bits(),
        half::f16::from_f32(1.0).to_bits(),
    ];
    let payload: Vec<u8> =
        channels.into_iter().flat_map(u16::to_le_bytes).cycle().take(128).collect();
    let mut tex = parse(&build_tex("TEXB0002", 10, 0, &payload, false, None), None).unwrap();
    let pixels = take_pixels(&mut tex).unwrap();
    assert_eq!(&pixels.base_rgba().unwrap()[..4], &[255, 128, 0, 255]);
    assert_eq!(&pixels.levels[0].data[..8], &payload[..8]);
}

#[test]
fn packed_preview_channels() {
    let rgb = [0x00, 0xf8].repeat(16);
    let mut tex = parse(&build_tex("TEXB0002", 15, 0, &rgb, false, None), None).unwrap();
    assert_eq!(&take_pixels(&mut tex).unwrap().base_rgba().unwrap()[..4], &[255, 0, 0, 255]);

    let rgba = 0xC000_03FFu32.to_le_bytes().repeat(16);
    let mut tex = parse(&build_tex("TEXB0002", 16, 0, &rgba, false, None), None).unwrap();
    assert_eq!(&take_pixels(&mut tex).unwrap().base_rgba().unwrap()[..4], &[255, 0, 0, 255]);
}

#[test]
fn encoded_sixteen_bit() {
    let image = image::ImageBuffer::<image::Rgba<u16>, Vec<u16>>::from_pixel(
        4,
        4,
        image::Rgba([u16::MAX, 32768, 0x1234, u16::MAX]),
    );
    let mut encoded = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba16(image)
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    let mut tex =
        parse(&build_tex("TEXB0003", 0, 0, &encoded.into_inner(), false, None), None).unwrap();
    tex.meta.free_image_format = Some(13);
    let pixels = take_pixels(&mut tex).unwrap();
    assert_eq!(pixels.format, PixelFormat::Rgba16);
    assert_eq!(pixels.bytes(), 128);
    assert_eq!(&pixels.levels[0].data[4..6], &0x1234u16.to_le_bytes());
    assert_eq!(&pixels.base_rgba().unwrap()[..4], &[255, 128, 18, 255]);
}

#[test]
fn encoded_hdr_precision() {
    let mut encoded = Vec::new();
    let pixels = vec![image::Rgb([2.0, 0.5, 0.25]); 16];
    image::codecs::hdr::HdrEncoder::new(&mut encoded).encode(&pixels, 4, 4).unwrap();
    let mut tex = parse(&build_tex("TEXB0003", 0, 0, &encoded, false, None), None).unwrap();
    tex.meta.free_image_format = Some(26);
    let pixels = take_pixels(&mut tex).unwrap();
    assert_eq!(pixels.format, PixelFormat::Rgba16f);
    let bits = u16::from_le_bytes(pixels.levels[0].data[..2].try_into().unwrap());
    assert!(half::f16::from_bits(bits).to_f32() > 1.0);
}

#[test]
fn bc_fallback_edges() {
    let bc7 = Pixels {
        format: PixelFormat::Bc7,
        levels: vec![MipLevel { width: 5, height: 5, data: vec![0; 64] }],
    };
    let decoded = bc7.decompressed().unwrap();
    assert_eq!(decoded.format, PixelFormat::Rgba8);
    assert_eq!(decoded.levels[0].data.len(), 100);
    assert_eq!(bc7.base_rgba().unwrap().len(), 100);

    let bc6h = Pixels {
        format: PixelFormat::Bc6h,
        levels: vec![MipLevel { width: 5, height: 5, data: vec![0; 64] }],
    };
    let decoded = bc6h.decompressed().unwrap();
    assert_eq!(decoded.format, PixelFormat::Rgba16f);
    assert_eq!(decoded.levels[0].data.len(), 200);
    assert_eq!(&decoded.levels[0].data[6..8], &0x3c00u16.to_le_bytes());
    assert_eq!(bc6h.base_rgba().unwrap().len(), 100);
}

#[test]
fn variant_selects_payload() {
    let bytes = build_variant_tex(
        &[
            (1, r#"{"condition":{"condition":"2","name":"tunic"}}"#, FULL_VARIANT),
            (2, r#"{"condition":{"condition":"1","name":"tunic"}}"#, FULL_VARIANT),
        ],
        false,
    );
    let meta = super::parse_meta(&bytes).unwrap();
    assert_eq!(meta.variants.iter().map(|variant| variant.index).collect::<Vec<_>>(), [1, 2]);
    assert!(meta.variants[1].condition.contains(r#""condition":"1""#));
    for (variant, fill) in [(None, 0), (Some(1), 1), (Some(2), 2)] {
        assert_eq!(
            parse(&bytes, variant).unwrap().images[0][0].data,
            variant_fill(fill),
            "{variant:?}"
        );
    }
    assert!(parse(&bytes, Some(7)).is_err());
}

#[test]
fn compressed_variant_selects() {
    let bytes = build_variant_tex(&[(3, "{}", FULL_VARIANT)], true);
    assert!(super::variants(&bytes).unwrap()[0].complete);
    assert_eq!(parse(&bytes, Some(3)).unwrap().images[0][0].data, variant_fill(3));
}

#[test]
fn patch_variant_incomplete() {
    let bytes = build_variant_tex(&[(1, "{}", [1, 1, 2, 2])], false);
    assert!(!super::variants(&bytes).unwrap()[0].complete);
    assert_eq!(parse(&bytes, None).unwrap().images[0][0].data, [0; 64]);
    assert_eq!(parse(&bytes, Some(1)).unwrap_err().to_string(), "tex variant 1 incomplete");
}

#[test]
fn variant_free_unchanged() {
    let bytes = build_variant_tex(&[], false);
    assert!(super::variants(&bytes).unwrap().is_empty());
    assert_eq!(parse(&bytes, None).unwrap().images[0][0].data, [0; 64]);
}

#[test]
fn variant_block_rejected() {
    let mut bytes = build_variant_tex(&[(1, "{}", FULL_VARIANT)], false);
    let block = bytes.len() - 64 - size_of::<[u32; 9]>();
    bytes[block..block + size_of::<u32>()].copy_from_slice(&2_u32.to_le_bytes());
    assert_eq!(parse(&bytes, None).unwrap_err().to_string(), "unsupported tex variant block");
}

#[test]
fn floor_sums_mip_chain() {
    let bytes = build_tex_levels(7, &[(8, 4, &[1u8; 16]), (4, 2, &[2u8; 8]), (2, 1, &[3u8; 8])]);
    let chain: usize = [(8, 4), (4, 2), (2, 1)]
        .iter()
        .map(|&(width, height)| PixelFormat::Bc1.level_bytes(width, height).unwrap())
        .sum();
    assert_eq!(decoded_floor(&bytes), chain);
    assert!(chain > 0);
}

#[test]
fn floor_skips_streamable_sprite() {
    let bytes = super::stream::tests::sprite(&super::stream::tests::FRAMES, true);
    assert_eq!(decoded_floor(&bytes), 0);
}
