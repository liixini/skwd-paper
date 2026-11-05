use super::{MipLevel, PixelFormat};

pub(super) const SIGNATURE: &[u8; 4] = b"8BPS";
const VERSION: u16 = 1;
const RGB_MODE: u16 = 3;
const RAW: u16 = 0;
const RLE: u16 = 1;
const EIGHT_BITS: u16 = 8;
const SIXTEEN_BITS: u16 = 16;
const RGB_CHANNELS: usize = 3;
const ALPHA_CHANNEL: usize = 3;
const MAX_CHANNELS: usize = 16;
const SECTION_COUNT: usize = 3;
const RESERVED_BYTES: usize = 6;
const SAMPLE16_BYTES: usize = 2;
const RGBA_CHANNELS: usize = 4;
const MAX_CHANNEL8: u32 = u8::MAX as u32;
const MAX_CHANNEL16: u32 = u16::MAX as u32;

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, length: usize) -> Option<&'a [u8]> {
        let end = self.offset.checked_add(length)?;
        let value = self.bytes.get(self.offset..end)?;
        self.offset = end;
        Some(value)
    }

    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_be_bytes(self.take(SAMPLE16_BYTES)?.try_into().ok()?))
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_be_bytes(self.take(std::mem::size_of::<u32>())?.try_into().ok()?))
    }
}

fn packbits(encoded: &[u8], expected: usize) -> Option<Vec<u8>> {
    let mut output = Vec::with_capacity(expected);
    let mut input = Cursor { bytes: encoded, offset: 0 };
    while input.offset < encoded.len() {
        let count = *input.take(1)?.first()? as i8;
        if count >= 0 {
            let length = count as usize + 1;
            if output.len().checked_add(length)? > expected {
                return None;
            }
            output.extend_from_slice(input.take(length)?);
        } else if count != i8::MIN {
            let length = (1_i16 - i16::from(count)) as usize;
            if output.len().checked_add(length)? > expected {
                return None;
            }
            output.resize(output.len() + length, *input.take(1)?.first()?);
        }
    }
    (output.len() == expected).then_some(output)
}

fn unmatte(composite: u32, alpha: u32, maximum: u32) -> u32 {
    if alpha == 0 || alpha == maximum {
        return composite;
    }
    let white = maximum - alpha;
    composite.saturating_sub(white).saturating_mul(maximum).saturating_div(alpha).min(maximum)
}

fn write_row(
    output: &mut [u8],
    row: &[u8],
    channel: usize,
    row_index: usize,
    width: usize,
    sample_bytes: usize,
) -> Option<()> {
    if channel >= RGBA_CHANNELS {
        return Some(());
    }
    for (column, sample) in row.chunks_exact(sample_bytes).enumerate() {
        let destination = ((row_index * width + column) * RGBA_CHANNELS + channel) * sample_bytes;
        let target = output.get_mut(destination..destination + sample_bytes)?;
        if sample_bytes == SAMPLE16_BYTES {
            target.copy_from_slice(&u16::from_be_bytes(sample.try_into().ok()?).to_le_bytes());
        } else {
            target.copy_from_slice(sample);
        }
    }
    Some(())
}

pub(super) fn decode(bytes: &[u8]) -> Option<(PixelFormat, MipLevel)> {
    let mut input = Cursor { bytes, offset: 0 };
    if input.take(SIGNATURE.len())? != SIGNATURE || input.u16()? != VERSION {
        return None;
    }
    input.take(RESERVED_BYTES)?;
    let channels = input.u16()? as usize;
    if !(RGB_CHANNELS..=MAX_CHANNELS).contains(&channels) {
        return None;
    }
    let height = input.u32()?;
    let width = input.u32()?;
    let sample_bytes = match input.u16()? {
        EIGHT_BITS => 1,
        SIXTEEN_BITS => SAMPLE16_BYTES,
        _ => return None,
    };
    if input.u16()? != RGB_MODE {
        return None;
    }
    for _ in 0..SECTION_COUNT {
        let length = input.u32()? as usize;
        input.take(length)?;
    }
    let compression = input.u16()?;
    if !matches!(compression, RAW | RLE) {
        return None;
    }
    let format =
        if sample_bytes == SAMPLE16_BYTES { PixelFormat::Rgba16 } else { PixelFormat::Rgba8 };
    let size = format.level_bytes(width, height)?;
    let (width_usize, height_usize) = (width as usize, height as usize);
    let row_bytes = width_usize.checked_mul(sample_bytes)?;
    let row_count = channels.checked_mul(height_usize)?;
    let lengths = if compression == RLE {
        (0..row_count).map(|_| input.u16()).collect::<Option<Vec<_>>>()?
    } else {
        Vec::new()
    };
    let mut output = vec![0; size];
    for channel in 0..channels {
        for row_index in 0..height_usize {
            let row = if compression == RAW {
                input.take(row_bytes)?
            } else {
                let length = usize::from(lengths[channel * height_usize + row_index]);
                let encoded = input.take(length)?;
                let decoded = packbits(encoded, row_bytes)?;
                write_row(&mut output, &decoded, channel, row_index, width_usize, sample_bytes)?;
                continue;
            };
            write_row(&mut output, row, channel, row_index, width_usize, sample_bytes)?;
        }
    }
    if channels == RGB_CHANNELS {
        for pixel in output.chunks_exact_mut(RGBA_CHANNELS * sample_bytes) {
            pixel[ALPHA_CHANNEL * sample_bytes..].fill(u8::MAX);
        }
    } else if sample_bytes == SAMPLE16_BYTES {
        for pixel in output.chunks_exact_mut(RGBA_CHANNELS * sample_bytes) {
            let alpha = u32::from(u16::from_le_bytes(
                pixel[ALPHA_CHANNEL * sample_bytes..].try_into().ok()?,
            ));
            for channel in pixel[..ALPHA_CHANNEL * sample_bytes].chunks_exact_mut(sample_bytes) {
                let value = u32::from(u16::from_le_bytes(channel.try_into().ok()?));
                channel
                    .copy_from_slice(&(unmatte(value, alpha, MAX_CHANNEL16) as u16).to_le_bytes());
            }
        }
    } else {
        for pixel in output.chunks_exact_mut(RGBA_CHANNELS) {
            let alpha = u32::from(pixel[ALPHA_CHANNEL]);
            for channel in &mut pixel[..ALPHA_CHANNEL] {
                *channel = unmatte(u32::from(*channel), alpha, MAX_CHANNEL8) as u8;
            }
        }
    }
    Some((format, MipLevel { width, height, data: output }))
}
