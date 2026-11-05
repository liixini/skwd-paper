use crate::read::Reader;
use anyhow::{Context, Result, anyhow};

mod psd;
pub mod stream;

pub const FLAG_NO_INTERPOLATION: u32 = 1;
pub const FLAG_CLAMP_UVS: u32 = 2;
pub const FLAG_IS_GIF: u32 = 4;
pub const FLAG_IS_VIDEO: u32 = 32;
pub const COMPONENT_COUNT: usize = 4;
const COMPONENT_SHIFT: u32 = 20;
const COMPONENT_MASK: u32 = (1 << COMPONENT_COUNT) - 1;

pub const MAX_TEXTURE_EDGE: u32 = 8192;
pub const MAX_MIP_BYTES: usize = 256 << 20;
pub const MAX_TEX_PAYLOAD_BYTES: usize = 512 << 20;
pub const MAX_RGBA_BYTES: usize = 256 << 20;
pub const MAX_STREAMED_BYTES: usize = 4 << 30;
const MAX_VARIANTS: usize = 64;
const MAX_CONDITION_BYTES: usize = 4096;
const VARIANT_MARKER: u32 = 1;
const VARIANT_FLAGS: u32 = 13;
const FORMAT_RGBA8888: i32 = 0;
const FORMAT_DXT5: i32 = 4;
const FORMAT_DXT3: i32 = 6;
const FORMAT_DXT1: i32 = 7;
const FORMAT_RG88: i32 = 8;
const FORMAT_R8: i32 = 9;
const FORMAT_RGBA16F: i32 = 10;
const FORMAT_RG16F: i32 = 11;
const FORMAT_BC7: i32 = 12;
const FORMAT_R16F: i32 = 13;
const FORMAT_BC6H: i32 = 14;
const FORMAT_RGB565: i32 = 15;
const FORMAT_RGBA1010102: i32 = 16;
const NAME_RGBA16F: &str = "rgba16f";
const NAME_RG16F: &str = "rg16f";
const NAME_BC7: &str = "bc7";
const NAME_R16F: &str = "r16f";
const NAME_BC6H: &str = "bc6h";
const NAME_RGB565: &str = "rgb565";
const NAME_RGBA1010102: &str = "rgba1010102";
const BLOCK_EDGE: usize = 4;
const BC1_BLOCK_BYTES: usize = 8;
const BC_BLOCK_BYTES: usize = 16;
pub(crate) const RGBA8_PIXEL_BYTES: usize = 4;
const RG8_PIXEL_BYTES: usize = 2;
const RGBA16_PIXEL_BYTES: usize = 8;
const RG16_PIXEL_BYTES: usize = 4;
const PACKED16_PIXEL_BYTES: usize = 2;
const PACKED32_PIXEL_BYTES: usize = 4;
const BC6H_CHANNELS: usize = 3;
const HALF_ONE_BITS: u16 = 0x3c00;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TexFormat {
    Rgba8888,
    Dxt5,
    Dxt3,
    Dxt1,
    Rg88,
    R8,
    Rgba16f,
    Rg16f,
    Bc7,
    R16f,
    Bc6h,
    Rgb565,
    Rgba1010102,
    Other(i32),
}

impl TexFormat {
    fn from_raw(raw: i32) -> Self {
        match raw {
            FORMAT_RGBA8888 => Self::Rgba8888,
            FORMAT_DXT5 => Self::Dxt5,
            FORMAT_DXT3 => Self::Dxt3,
            FORMAT_DXT1 => Self::Dxt1,
            FORMAT_RG88 => Self::Rg88,
            FORMAT_R8 => Self::R8,
            FORMAT_RGBA16F => Self::Rgba16f,
            FORMAT_RG16F => Self::Rg16f,
            FORMAT_BC7 => Self::Bc7,
            FORMAT_R16F => Self::R16f,
            FORMAT_BC6H => Self::Bc6h,
            FORMAT_RGB565 => Self::Rgb565,
            FORMAT_RGBA1010102 => Self::Rgba1010102,
            other => Self::Other(other),
        }
    }

    pub fn name(self) -> String {
        match self {
            Self::Rgba8888 => "rgba8888".into(),
            Self::Dxt5 => "dxt5".into(),
            Self::Dxt3 => "dxt3".into(),
            Self::Dxt1 => "dxt1".into(),
            Self::Rg88 => "rg88".into(),
            Self::R8 => "r8".into(),
            Self::Rgba16f => NAME_RGBA16F.into(),
            Self::Rg16f => NAME_RG16F.into(),
            Self::Bc7 => NAME_BC7.into(),
            Self::R16f => NAME_R16F.into(),
            Self::Bc6h => NAME_BC6H.into(),
            Self::Rgb565 => NAME_RGB565.into(),
            Self::Rgba1010102 => NAME_RGBA1010102.into(),
            Self::Other(raw) => format!("format{raw}"),
        }
    }

    pub fn raw(self) -> i32 {
        match self {
            Self::Rgba8888 => FORMAT_RGBA8888,
            Self::Dxt5 => FORMAT_DXT5,
            Self::Dxt3 => FORMAT_DXT3,
            Self::Dxt1 => FORMAT_DXT1,
            Self::Rg88 => FORMAT_RG88,
            Self::R8 => FORMAT_R8,
            Self::Rgba16f => FORMAT_RGBA16F,
            Self::Rg16f => FORMAT_RG16F,
            Self::Bc7 => FORMAT_BC7,
            Self::R16f => FORMAT_R16F,
            Self::Bc6h => FORMAT_BC6H,
            Self::Rgb565 => FORMAT_RGB565,
            Self::Rgba1010102 => FORMAT_RGBA1010102,
            Self::Other(raw) => raw,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum PixelFormat {
    #[default]
    Rgba8,
    Bc1,
    Bc2,
    Bc3,
    R8,
    Rg8,
    Rgba16f,
    Rgba16,
    Rgba16Snorm,
    Rg16f,
    Bc7,
    R16f,
    Bc6h,
    Rgb565,
    Rgba1010102,
}

impl PixelFormat {
    fn of(format: TexFormat) -> Option<Self> {
        match format {
            TexFormat::Rgba8888 => Some(Self::Rgba8),
            TexFormat::Dxt1 => Some(Self::Bc1),
            TexFormat::Dxt3 => Some(Self::Bc2),
            TexFormat::Dxt5 => Some(Self::Bc3),
            TexFormat::R8 => Some(Self::R8),
            TexFormat::Rg88 => Some(Self::Rg8),
            TexFormat::Rgba16f => Some(Self::Rgba16f),
            TexFormat::Rg16f => Some(Self::Rg16f),
            TexFormat::Bc7 => Some(Self::Bc7),
            TexFormat::R16f => Some(Self::R16f),
            TexFormat::Bc6h => Some(Self::Bc6h),
            TexFormat::Rgb565 => Some(Self::Rgb565),
            TexFormat::Rgba1010102 => Some(Self::Rgba1010102),
            TexFormat::Other(_) => None,
        }
    }

    #[must_use]
    pub fn compressed(self) -> bool {
        matches!(self, Self::Bc1 | Self::Bc2 | Self::Bc3 | Self::Bc7 | Self::Bc6h)
    }

    #[must_use]
    pub fn level_bytes(self, width: u32, height: u32) -> Option<usize> {
        if width == 0 || height == 0 || width > MAX_TEXTURE_EDGE || height > MAX_TEXTURE_EDGE {
            return None;
        }
        let (w, h) = (width as usize, height as usize);
        let bytes = match self {
            Self::Rgba8 => w.checked_mul(h)?.checked_mul(RGBA8_PIXEL_BYTES)?,
            Self::R8 => w.checked_mul(h)?,
            Self::Rg8 => w.checked_mul(h)?.checked_mul(RG8_PIXEL_BYTES)?,
            Self::Rgba16f | Self::Rgba16 | Self::Rgba16Snorm => {
                w.checked_mul(h)?.checked_mul(RGBA16_PIXEL_BYTES)?
            }
            Self::Rg16f => w.checked_mul(h)?.checked_mul(RG16_PIXEL_BYTES)?,
            Self::Rgba1010102 => w.checked_mul(h)?.checked_mul(PACKED32_PIXEL_BYTES)?,
            Self::R16f | Self::Rgb565 => w.checked_mul(h)?.checked_mul(PACKED16_PIXEL_BYTES)?,
            Self::Bc1 => w
                .div_ceil(BLOCK_EDGE)
                .checked_mul(h.div_ceil(BLOCK_EDGE))?
                .checked_mul(BC1_BLOCK_BYTES)?,
            Self::Bc2 | Self::Bc3 | Self::Bc7 | Self::Bc6h => w
                .div_ceil(BLOCK_EDGE)
                .checked_mul(h.div_ceil(BLOCK_EDGE))?
                .checked_mul(BC_BLOCK_BYTES)?,
        };
        (bytes <= MAX_RGBA_BYTES).then_some(bytes)
    }
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct MipLevel {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct Pixels {
    pub format: PixelFormat,
    pub levels: Vec<MipLevel>,
}

impl Pixels {
    #[must_use]
    pub fn rgba(width: u32, height: u32, data: Vec<u8>) -> Self {
        Self { format: PixelFormat::Rgba8, levels: vec![MipLevel { width, height, data }] }
    }

    #[must_use]
    pub fn width(&self) -> u32 {
        self.levels.first().map_or(0, |level| level.width)
    }

    #[must_use]
    pub fn height(&self) -> u32 {
        self.levels.first().map_or(0, |level| level.height)
    }

    #[must_use]
    pub fn bytes(&self) -> usize {
        self.levels.iter().map(|level| level.data.len()).sum()
    }

    #[must_use]
    pub fn base_rgba(&self) -> Option<Vec<u8>> {
        let level = self.levels.first()?;
        level_rgba(self.format, level)
    }

    #[must_use]
    pub fn decompressed(&self) -> Option<Self> {
        if !self.format.compressed() {
            return Some(self.clone());
        }
        let format = if self.format == PixelFormat::Bc6h {
            PixelFormat::Rgba16f
        } else {
            PixelFormat::Rgba8
        };
        let levels = self
            .levels
            .iter()
            .map(|level| {
                let data = if self.format == PixelFormat::Bc6h {
                    decode_bc6h(level)
                } else {
                    level_rgba(self.format, level)
                };
                data.map(|data| MipLevel { width: level.width, height: level.height, data })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Self { format, levels })
    }
}

fn level_rgba(format: PixelFormat, level: &MipLevel) -> Option<Vec<u8>> {
    let (width, height) = (level.width as usize, level.height as usize);
    let rgba_bytes = rgba_len(level.width, level.height)?;
    let pixels = rgba_bytes / 4;
    match format {
        PixelFormat::Rgba8 => {
            (level.data.len() >= rgba_bytes).then(|| level.data[..rgba_bytes].to_vec())
        }
        PixelFormat::Bc1 | PixelFormat::Bc2 | PixelFormat::Bc3 => {
            let fmt = match format {
                PixelFormat::Bc1 => texpresso::Format::Bc1,
                PixelFormat::Bc2 => texpresso::Format::Bc2,
                _ => texpresso::Format::Bc3,
            };
            if level.data.len() < fmt.compressed_size(width, height) {
                return None;
            }
            let mut out = vec![0u8; rgba_bytes];
            fmt.decompress(&level.data, width, height, &mut out);
            Some(out)
        }
        PixelFormat::R8 => {
            if level.data.len() < pixels {
                return None;
            }
            let mut out = Vec::with_capacity(rgba_bytes);
            for &value in &level.data[..pixels] {
                out.extend_from_slice(&[value, 0, 0, 255]);
            }
            Some(out)
        }
        PixelFormat::Rg8 => {
            let encoded = pixels.checked_mul(2)?;
            if level.data.len() < encoded {
                return None;
            }
            let mut out = Vec::with_capacity(rgba_bytes);
            for pair in level.data[..encoded].chunks_exact(2) {
                out.extend_from_slice(&[pair[0], pair[1], 0, 255]);
            }
            Some(out)
        }
        PixelFormat::Rgba16f | PixelFormat::Rg16f | PixelFormat::R16f => {
            half_rgba(format, &level.data, pixels, rgba_bytes)
        }
        PixelFormat::Rgba16 | PixelFormat::Rgba16Snorm => {
            let bytes = pixels.checked_mul(RGBA16_PIXEL_BYTES)?;
            if level.data.len() < bytes {
                return None;
            }
            Some(
                level.data[..bytes]
                    .chunks_exact(2)
                    .map(|pair| {
                        let word = u16::from_le_bytes([pair[0], pair[1]]);
                        if format == PixelFormat::Rgba16Snorm {
                            unorm((word as i16).max(0) as u32, i16::MAX as u32)
                        } else {
                            unorm(u32::from(word), u32::from(u16::MAX))
                        }
                    })
                    .collect(),
            )
        }
        PixelFormat::Rgb565 => packed_rgba(&level.data, pixels, rgba_bytes, Packed::Rgb565),
        PixelFormat::Rgba1010102 => {
            packed_rgba(&level.data, pixels, rgba_bytes, Packed::Rgba1010102)
        }
        PixelFormat::Bc7 => decode_bc7(level),
        PixelFormat::Bc6h => {
            let data = decode_bc6h(level)?;
            level_rgba(
                PixelFormat::Rgba16f,
                &MipLevel { width: level.width, height: level.height, data },
            )
        }
    }
}

fn decode_bc7(level: &MipLevel) -> Option<Vec<u8>> {
    let (width, height) = (level.width as usize, level.height as usize);
    let len = PixelFormat::Rgba8.level_bytes(level.width, level.height)?;
    if level.data.len() < PixelFormat::Bc7.level_bytes(level.width, level.height)? {
        return None;
    }
    let mut out = vec![0; len];
    for block_y in 0..height.div_ceil(BLOCK_EDGE) {
        for block_x in 0..width.div_ceil(BLOCK_EDGE) {
            let index = (block_y * width.div_ceil(BLOCK_EDGE) + block_x) * BC_BLOCK_BYTES;
            let mut block = [0; BLOCK_EDGE * BLOCK_EDGE * RGBA8_PIXEL_BYTES];
            bcdec_rs::bc7(
                &level.data[index..index + BC_BLOCK_BYTES],
                &mut block,
                BLOCK_EDGE * RGBA8_PIXEL_BYTES,
            );
            for row in 0..(height - block_y * BLOCK_EDGE).min(BLOCK_EDGE) {
                let columns = (width - block_x * BLOCK_EDGE).min(BLOCK_EDGE);
                let from = row * BLOCK_EDGE * RGBA8_PIXEL_BYTES;
                let to = ((block_y * BLOCK_EDGE + row) * width + block_x * BLOCK_EDGE)
                    * RGBA8_PIXEL_BYTES;
                out[to..to + columns * RGBA8_PIXEL_BYTES]
                    .copy_from_slice(&block[from..from + columns * RGBA8_PIXEL_BYTES]);
            }
        }
    }
    Some(out)
}

fn decode_bc6h(level: &MipLevel) -> Option<Vec<u8>> {
    let (width, height) = (level.width as usize, level.height as usize);
    let len = PixelFormat::Rgba16f.level_bytes(level.width, level.height)?;
    if level.data.len() < PixelFormat::Bc6h.level_bytes(level.width, level.height)? {
        return None;
    }
    let mut out = vec![0; len];
    for block_y in 0..height.div_ceil(BLOCK_EDGE) {
        for block_x in 0..width.div_ceil(BLOCK_EDGE) {
            let index = (block_y * width.div_ceil(BLOCK_EDGE) + block_x) * BC_BLOCK_BYTES;
            let mut block = [0; BLOCK_EDGE * BLOCK_EDGE * BC6H_CHANNELS];
            bcdec_rs::bc6h_half(
                &level.data[index..index + BC_BLOCK_BYTES],
                &mut block,
                BLOCK_EDGE * BC6H_CHANNELS,
                false,
            );
            for row in 0..(height - block_y * BLOCK_EDGE).min(BLOCK_EDGE) {
                for column in 0..(width - block_x * BLOCK_EDGE).min(BLOCK_EDGE) {
                    let source = (row * BLOCK_EDGE + column) * BC6H_CHANNELS;
                    let target =
                        ((block_y * BLOCK_EDGE + row) * width + block_x * BLOCK_EDGE + column)
                            * RGBA16_PIXEL_BYTES;
                    for channel in 0..BC6H_CHANNELS {
                        let pair = block[source + channel].to_le_bytes();
                        out[target + channel * 2..target + channel * 2 + 2].copy_from_slice(&pair);
                    }
                    out[target + BC6H_CHANNELS * 2..target + RGBA16_PIXEL_BYTES]
                        .copy_from_slice(&HALF_ONE_BITS.to_le_bytes());
                }
            }
        }
    }
    Some(out)
}

fn half_unorm(bits: u16) -> u8 {
    let value = half::f16::from_bits(bits).to_f32();
    (value.clamp(0.0, 1.0) * f32::from(u8::MAX)).round() as u8
}

fn half_rgba(
    format: PixelFormat,
    data: &[u8],
    pixels: usize,
    rgba_bytes: usize,
) -> Option<Vec<u8>> {
    let channels = match format {
        PixelFormat::R16f => 1,
        PixelFormat::Rg16f => 2,
        PixelFormat::Rgba16f => 4,
        _ => return None,
    };
    let bytes = pixels.checked_mul(channels)?.checked_mul(2)?;
    if data.len() < bytes {
        return None;
    }
    let mut out = Vec::with_capacity(rgba_bytes);
    for pixel in data[..bytes].chunks_exact(channels * 2) {
        let mut rgba = [0, 0, 0, u8::MAX];
        for (channel, pair) in pixel.chunks_exact(2).enumerate() {
            rgba[channel] = half_unorm(u16::from_le_bytes([pair[0], pair[1]]));
        }
        out.extend_from_slice(&rgba);
    }
    Some(out)
}

#[derive(Clone, Copy)]
enum Packed {
    Rgb565,
    Rgba1010102,
}

fn packed_rgba(data: &[u8], pixels: usize, rgba_bytes: usize, format: Packed) -> Option<Vec<u8>> {
    const FIVE_BITS: u32 = 0x1f;
    const SIX_BITS: u32 = 0x3f;
    const TEN_BITS: u32 = 0x3ff;
    const TWO_BITS: u32 = 0x3;
    const RED_565_SHIFT: u32 = 11;
    const GREEN_565_SHIFT: u32 = 5;
    const GREEN_1010102_SHIFT: u32 = 10;
    const BLUE_1010102_SHIFT: u32 = 20;
    const ALPHA_1010102_SHIFT: u32 = 30;
    let bytes_per_pixel = match format {
        Packed::Rgb565 => PACKED16_PIXEL_BYTES,
        Packed::Rgba1010102 => PACKED32_PIXEL_BYTES,
    };
    let bytes = pixels.checked_mul(bytes_per_pixel)?;
    if data.len() < bytes {
        return None;
    }
    let mut out = Vec::with_capacity(rgba_bytes);
    for pixel in data[..bytes].chunks_exact(bytes_per_pixel) {
        let rgba = match format {
            Packed::Rgb565 => {
                let bits = u32::from(u16::from_le_bytes([pixel[0], pixel[1]]));
                [
                    unorm(bits >> RED_565_SHIFT & FIVE_BITS, FIVE_BITS),
                    unorm(bits >> GREEN_565_SHIFT & SIX_BITS, SIX_BITS),
                    unorm(bits & FIVE_BITS, FIVE_BITS),
                    u8::MAX,
                ]
            }
            Packed::Rgba1010102 => {
                let packed: [u8; PACKED32_PIXEL_BYTES] = pixel.try_into().ok()?;
                let bits = u32::from_le_bytes(packed);
                [
                    unorm(bits & TEN_BITS, TEN_BITS),
                    unorm(bits >> GREEN_1010102_SHIFT & TEN_BITS, TEN_BITS),
                    unorm(bits >> BLUE_1010102_SHIFT & TEN_BITS, TEN_BITS),
                    unorm(bits >> ALPHA_1010102_SHIFT & TWO_BITS, TWO_BITS),
                ]
            }
        };
        out.extend_from_slice(&rgba);
    }
    Some(out)
}

fn unorm(value: u32, max: u32) -> u8 {
    ((value * u32::from(u8::MAX) + max / 2) / max) as u8
}

#[derive(Debug)]
pub struct TexMeta {
    pub format: TexFormat,
    pub flags: u32,
    pub tex_width: i32,
    pub tex_height: i32,
    pub img_width: i32,
    pub img_height: i32,
    pub container: String,
    pub free_image_format: Option<i32>,
    pub image_count: usize,
    pub mip_counts: Vec<usize>,
    pub frame_count: usize,
    pub sprite_size: Option<(i32, i32)>,
    pub variants: Vec<TexVariant>,
}

#[derive(Debug)]
pub struct TexVariant {
    pub index: u32,
    pub condition: String,
    pub complete: bool,
}

impl TexMeta {
    pub fn component_mask(&self) -> u8 {
        ((self.flags >> COMPONENT_SHIFT) & COMPONENT_MASK) as u8
    }
}

#[derive(Debug)]
pub struct TexMip {
    pub width: i32,
    pub height: i32,
    pub data: Vec<u8>,
}

#[derive(Debug)]
pub struct Tex {
    pub meta: TexMeta,
    pub images: Vec<Vec<TexMip>>,
    pub frames: Vec<TexFrame>,
}

#[derive(Clone, Copy, Debug)]
struct Span {
    offset: usize,
    length: usize,
    unpacked: Option<usize>,
    width: u32,
    height: u32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Payload {
    Keep,
    Skip,
    Locate,
}

#[derive(Debug)]
pub struct TexFrame {
    pub image_id: i32,
    pub frame_time: f32,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub width_y: f32,
    pub height_x: f32,
    pub height: f32,
}

impl TexFrame {
    pub fn size(&self) -> (f32, f32) {
        let width = if self.width == 0.0 { self.height_x } else { self.width };
        let height = if self.height == 0.0 { self.width_y } else { self.height };
        (width.abs(), height.abs())
    }

    pub fn rotated(&self) -> bool {
        self.width == 0.0 || self.height == 0.0
    }
}

fn container_rank(container: &str) -> Result<u32> {
    match container {
        "TEXB0001" => Ok(1),
        "TEXB0002" => Ok(2),
        "TEXB0003" => Ok(3),
        "TEXB0004" => Ok(4),
        other => Err(anyhow!("unknown tex container {other:?}")),
    }
}

struct Header {
    format: TexFormat,
    flags: u32,
    tex_width: i32,
    tex_height: i32,
    img_width: i32,
    img_height: i32,
    container: String,
    rank: u32,
    image_count: usize,
    free_image_format: Option<i32>,
    variants: Vec<TexVariant>,
}

struct Walker<'a> {
    reader: Reader<'a>,
    payload: Payload,
    variant: Option<u32>,
    spans: Vec<Vec<Span>>,
}

impl Walker<'_> {
    fn header(&mut self) -> Result<Header> {
        let magic = self.reader.nul_string(16)?;
        if magic != "TEXV0005" {
            return Err(anyhow!("bad tex magic {magic:?}"));
        }
        let magic2 = self.reader.nul_string(16)?;
        if magic2 != "TEXI0001" {
            return Err(anyhow!("bad tex magic2 {magic2:?}"));
        }
        let format = TexFormat::from_raw(self.reader.i32()?);
        let flags = self.reader.i32()? as u32;
        let tex_width = self.reader.i32()?;
        let tex_height = self.reader.i32()?;
        let img_width = self.reader.i32()?;
        let img_height = self.reader.i32()?;
        let _dominant = self.reader.u32()?;
        let container = self.reader.nul_string(16)?;
        let rank = container_rank(&container)?;
        let image_count = self.reader.i32()?.max(0) as usize;
        if image_count > 64 {
            return Err(anyhow!("implausible image count {image_count}"));
        }
        let free_image_format = if rank >= 3 { Some(self.reader.i32()?) } else { None };
        let variant_count = if rank >= 4 { self.reader.i32()?.max(0) as usize } else { 0 };
        if variant_count > MAX_VARIANTS {
            return Err(anyhow!("implausible tex variant count {variant_count}"));
        }
        let still = flags & (FLAG_IS_GIF | FLAG_IS_VIDEO) == 0;
        let variants =
            (0..variant_count).map(|_| self.variant(still)).collect::<Result<Vec<_>>>()?;
        Ok(Header {
            format,
            flags,
            tex_width,
            tex_height,
            img_width,
            img_height,
            container,
            rank,
            image_count,
            free_image_format,
            variants,
        })
    }

    fn parse(&mut self) -> Result<Tex> {
        let Header {
            format,
            flags,
            tex_width,
            tex_height,
            img_width,
            img_height,
            container,
            rank,
            image_count,
            free_image_format,
            mut variants,
        } = self.header()?;

        let mut images = Vec::new();
        let mut mip_counts = Vec::new();
        let mut total_payload = 0_usize;
        for _ in 0..image_count {
            let mips = self.reader.i32()?.max(0) as usize;
            if mips > 32 {
                return Err(anyhow!("implausible mip count {mips}"));
            }
            mip_counts.push(mips);
            let mut image = Vec::new();
            let mut image_spans = Vec::new();
            for _ in 0..mips {
                let width = self.reader.i32()?;
                let height = self.reader.i32()?;
                let valid_dimensions = u32::try_from(width)
                    .ok()
                    .zip(u32::try_from(height).ok())
                    .is_some_and(|(width, height)| {
                        width > 0
                            && height > 0
                            && width <= MAX_TEXTURE_EDGE
                            && height <= MAX_TEXTURE_EDGE
                    });
                if !valid_dimensions {
                    return Err(anyhow!("invalid mip dimensions {width}x{height}"));
                }
                let (compressed, decompressed_len) = if rank >= 2 {
                    (self.reader.i32()? != 0, self.reader.i32()?.max(0) as usize)
                } else {
                    (false, 0)
                };
                let byte_count = self.reader.i32()?.max(0) as usize;
                let retained = if compressed { decompressed_len } else { byte_count };
                let in_place = self.payload != Payload::Keep && plain_video(flags) && !compressed;
                if byte_count > MAX_MIP_BYTES && !in_place {
                    return Err(anyhow!("mip encoded size {byte_count} over cap"));
                }
                if retained > MAX_MIP_BYTES && !in_place {
                    return Err(anyhow!("mip decompressed size {retained} over cap"));
                }
                total_payload = total_payload
                    .checked_add(retained)
                    .ok_or_else(|| anyhow!("texture payload size overflow"))?;
                if total_payload > MAX_TEX_PAYLOAD_BYTES
                    && self.payload != Payload::Locate
                    && !in_place
                {
                    return Err(anyhow!(
                        "texture payload is {total_payload} bytes; limit is {MAX_TEX_PAYLOAD_BYTES}"
                    ));
                }
                let base = (self.reader.pos(), byte_count);
                self.reader.skip(byte_count)?;
                let (offset, length) =
                    self.variant_payload(&mut variants, (width, height), base, compressed)?;
                match self.payload {
                    Payload::Keep => {
                        let payload = self.reader.at(offset, length)?;
                        let data = if compressed {
                            lz4_flex::block::decompress(payload, decompressed_len)
                                .map_err(|err| anyhow!("lz4: {err}"))?
                        } else {
                            payload.to_vec()
                        };
                        image.push(TexMip { width, height, data });
                    }
                    Payload::Skip => {}
                    Payload::Locate => {
                        image_spans.push(Span {
                            offset,
                            length,
                            unpacked: compressed.then_some(decompressed_len),
                            width: width.unsigned_abs(),
                            height: height.unsigned_abs(),
                        });
                    }
                }
            }
            images.push(image);
            self.spans.push(image_spans);
        }

        let mut frames = Vec::new();
        let mut sprite_size = None;
        if flags & FLAG_IS_GIF != 0 {
            let anim_magic =
                self.reader.nul_string(16).context("gif tex missing frame container")?;
            let version: u32 = match anim_magic.as_str() {
                "TEXS0001" => 1,
                "TEXS0002" => 2,
                "TEXS0003" => 3,
                other => return Err(anyhow!("unknown anim container {other:?}")),
            };
            let frame_count = self.reader.i32()?.max(0) as usize;
            if frame_count > 100_000 {
                return Err(anyhow!("implausible frame count {frame_count}"));
            }
            if version >= 3 {
                sprite_size = Some((self.reader.i32()?, self.reader.i32()?));
            }
            for _ in 0..frame_count {
                let image_id = self.reader.i32()?;
                let frame_time = self.reader.f32()?;
                let (x, y, width, width_y, height_x, height);
                if version == 1 {
                    x = self.reader.i32()? as f32;
                    y = self.reader.i32()? as f32;
                    width = self.reader.i32()? as f32;
                    width_y = self.reader.i32()? as f32;
                    height_x = self.reader.i32()? as f32;
                    height = self.reader.i32()? as f32;
                } else {
                    x = self.reader.f32()?;
                    y = self.reader.f32()?;
                    width = self.reader.f32()?;
                    width_y = self.reader.f32()?;
                    height_x = self.reader.f32()?;
                    height = self.reader.f32()?;
                }
                frames.push(TexFrame {
                    image_id,
                    frame_time,
                    x,
                    y,
                    width,
                    width_y,
                    height_x,
                    height,
                });
            }
        }

        if let Some(index) = self.variant
            && !variants.iter().any(|variant| variant.index == index && variant.complete)
        {
            return Err(anyhow!("tex variant {index} incomplete"));
        }
        let frame_count = frames.len();
        Ok(Tex {
            meta: TexMeta {
                format,
                flags,
                tex_width,
                tex_height,
                img_width,
                img_height,
                container,
                free_image_format,
                image_count,
                mip_counts,
                frame_count,
                sprite_size,
                variants,
            },
            images,
            frames,
        })
    }

    fn variant(&mut self, complete: bool) -> Result<TexVariant> {
        let marker = self.reader.u32()?;
        let index = self.reader.u32()?;
        let reserved = self.reader.u32()?;
        if marker != VARIANT_MARKER || reserved != 0 {
            return Err(anyhow!("unsupported tex variant header"));
        }
        let condition = self.reader.nul_string(MAX_CONDITION_BYTES)?;
        Ok(TexVariant { index, condition, complete })
    }

    fn variant_payload(
        &mut self,
        variants: &mut [TexVariant],
        (width, height): (i32, i32),
        base: (usize, usize),
        compressed: bool,
    ) -> Result<(usize, usize)> {
        if variants.is_empty() {
            return Ok(base);
        }
        let marker = self.reader.u32()?;
        let records = self.reader.u32()? as usize;
        if marker != VARIANT_MARKER || records > variants.len() {
            return Err(anyhow!("unsupported tex variant block"));
        }
        let mut chosen = base;
        let mut covered = Vec::with_capacity(records);
        for _ in 0..records {
            let marker = self.reader.u32()?;
            let index = self.reader.u32()?;
            let rect =
                [self.reader.i32()?, self.reader.i32()?, self.reader.i32()?, self.reader.i32()?];
            let flags = self.reader.u32()?;
            let length = self.reader.u32()? as usize;
            if marker != VARIANT_MARKER || flags != VARIANT_FLAGS || length > MAX_MIP_BYTES {
                return Err(anyhow!("unsupported tex variant record"));
            }
            let offset = self.reader.pos();
            self.reader.skip(length)?;
            if rect == [0, 0, width, height] && (compressed || length == base.1) {
                covered.push(index);
                if self.variant == Some(index) {
                    chosen = (offset, length);
                }
            }
        }
        for variant in variants.iter_mut() {
            variant.complete &= covered.contains(&variant.index);
        }
        Ok(chosen)
    }
}

fn walker(data: &[u8], payload: Payload) -> Walker<'_> {
    Walker { reader: Reader::new(data), payload, variant: None, spans: Vec::new() }
}

pub fn parse(data: &[u8], variant: Option<u32>) -> Result<Tex> {
    Walker { variant, ..walker(data, Payload::Keep) }.parse()
}

pub fn variants(data: &[u8]) -> Result<Vec<TexVariant>> {
    if walker(data, Payload::Skip).header()?.variants.is_empty() {
        return Ok(Vec::new());
    }
    parse_meta(data).map(|meta| meta.variants)
}

pub fn parse_meta(data: &[u8]) -> Result<TexMeta> {
    walker(data, Payload::Skip).parse().map(|tex| tex.meta)
}

pub struct VideoSpan {
    pub meta: TexMeta,
    pub offset: u64,
    pub length: u64,
}

fn plain_video(flags: u32) -> bool {
    flags & (FLAG_IS_VIDEO | FLAG_IS_GIF) == FLAG_IS_VIDEO
}

pub fn video_span(data: &[u8]) -> Result<Option<VideoSpan>> {
    let (tex, spans) = locate(data)?;
    if !plain_video(tex.meta.flags) {
        return Ok(None);
    }
    let Some(span) = spans.first().and_then(|image| image.first()) else { return Ok(None) };
    if span.unpacked.is_some() || span.length == 0 {
        return Ok(None);
    }
    Ok(Some(VideoSpan { meta: tex.meta, offset: span.offset as u64, length: span.length as u64 }))
}

fn locate(data: &[u8]) -> Result<(Tex, Vec<Vec<Span>>)> {
    let mut walker = walker(data, Payload::Locate);
    let tex = walker.parse()?;
    Ok((tex, walker.spans))
}

pub(crate) fn decoded_floor(data: &[u8]) -> usize {
    let Ok((tex, spans)) = locate(data) else { return 0 };
    if tex.meta.flags & FLAG_IS_VIDEO != 0 || !tex.meta.variants.is_empty() {
        return 0;
    }
    if stream::streamable(&tex, &spans) {
        return 0;
    }
    let free = tex.meta.free_image_format.is_some_and(|format| format >= 0);
    let Some(format) =
        (if free { Some(PixelFormat::Rgba8) } else { PixelFormat::of(tex.meta.format) })
    else {
        return 0;
    };
    let Some(base) = spans.first().and_then(|levels| levels.first()).copied() else { return 0 };
    spans[0]
        .iter()
        .enumerate()
        .take_while(|(index, span)| {
            (span.width, span.height) == mip_dimensions(base.width, base.height, *index)
        })
        .map_while(|(_, span)| {
            let bytes = format.level_bytes(span.width, span.height)?;
            (free || span.unpacked.unwrap_or(span.length) >= bytes).then_some(bytes)
        })
        .sum()
}

pub fn animation_times(data: &[u8]) -> Result<Vec<f32>> {
    let tex = walker(data, Payload::Skip).parse()?;
    tex.frames
        .iter()
        .map(|frame| {
            if !frame.frame_time.is_finite() {
                return Err(anyhow!("sprite frame duration is not finite"));
            }
            Ok(frame.frame_time.max(0.0))
        })
        .collect()
}

fn decode_free_image(mip: &TexMip) -> Option<(PixelFormat, MipLevel)> {
    if mip.data.starts_with(psd::SIGNATURE) {
        return psd::decode(&mip.data);
    }
    let reader =
        image::ImageReader::new(std::io::Cursor::new(&mip.data)).with_guessed_format().ok()?;
    let mut reader = reader;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_TEXTURE_EDGE);
    limits.max_image_height = Some(MAX_TEXTURE_EDGE);
    limits.max_alloc = Some(MAX_RGBA_BYTES as u64);
    reader.limits(limits);
    let decoded = reader.decode().ok()?;
    let (width, height) = (decoded.width(), decoded.height());
    let format = match decoded.color() {
        image::ColorType::L16
        | image::ColorType::La16
        | image::ColorType::Rgb16
        | image::ColorType::Rgba16 => PixelFormat::Rgba16,
        image::ColorType::Rgb32F | image::ColorType::Rgba32F => PixelFormat::Rgba16f,
        _ => PixelFormat::Rgba8,
    };
    format.level_bytes(width, height)?;
    let data = match format {
        PixelFormat::Rgba16 => {
            decoded.to_rgba16().into_raw().into_iter().flat_map(u16::to_le_bytes).collect()
        }
        PixelFormat::Rgba16f => decoded
            .to_rgba32f()
            .into_raw()
            .into_iter()
            .flat_map(|channel| half::f16::from_f32(channel).to_bits().to_le_bytes())
            .collect(),
        _ => decoded.to_rgba8().into_raw(),
    };
    Some((format, MipLevel { width, height, data }))
}

fn rgba_len(width: u32, height: u32) -> Option<usize> {
    if width == 0 || height == 0 || width > MAX_TEXTURE_EDGE || height > MAX_TEXTURE_EDGE {
        return None;
    }
    let bytes = (width as usize).checked_mul(height as usize)?.checked_mul(4)?;
    (bytes <= MAX_RGBA_BYTES).then_some(bytes)
}

fn mip_dimensions(width: u32, height: u32, index: usize) -> (u32, u32) {
    let shift = index.min((u32::BITS - 1) as usize) as u32;
    ((width >> shift).max(1), (height >> shift).max(1))
}

pub fn take_pixels(tex: &mut Tex) -> Option<Pixels> {
    take_image_pixels(tex, 0)
}

pub fn take_image_pixels(tex: &mut Tex, index: usize) -> Option<Pixels> {
    if tex.meta.flags & FLAG_IS_VIDEO != 0 {
        return None;
    }
    let image = std::mem::take(tex.images.get_mut(index)?);
    if tex.meta.free_image_format.is_some_and(|format| format >= 0) {
        let mut levels = Vec::new();
        let (mut base_width, mut base_height) = (0u32, 0u32);
        let mut format = None;
        for (index, mip) in image.into_iter().enumerate() {
            let Some((decoded_format, level)) = decode_free_image(&mip) else {
                break;
            };
            let (width, height) = (level.width, level.height);
            if index == 0 {
                (base_width, base_height) = (width, height);
            } else if (width, height) != mip_dimensions(base_width, base_height, index)
                || mip.width != width as i32
                || mip.height != height as i32
                || format != Some(decoded_format)
            {
                break;
            }
            format = Some(decoded_format);
            levels.push(level);
        }
        return format.map(|format| Pixels { format, levels });
    }
    let format = PixelFormat::of(tex.meta.format)?;
    let (mut base_width, mut base_height) = (0u32, 0u32);
    let mut levels = Vec::new();
    for (index, mut mip) in image.into_iter().enumerate() {
        let Some((width, height)) =
            u32::try_from(mip.width).ok().zip(u32::try_from(mip.height).ok())
        else {
            break;
        };
        if index == 0 {
            (base_width, base_height) = (width, height);
        } else if (width, height) != mip_dimensions(base_width, base_height, index) {
            break;
        }
        let Some(bytes) = format.level_bytes(width, height) else {
            break;
        };
        if mip.data.len() < bytes {
            break;
        }
        mip.data.truncate(bytes);
        levels.push(MipLevel { width, height, data: mip.data });
    }
    (!levels.is_empty()).then_some(Pixels { format, levels })
}

pub fn decode_rgba(tex: &mut Tex) -> Option<(u32, u32, Vec<u8>)> {
    let pixels = take_pixels(tex)?;
    Some((pixels.width(), pixels.height(), pixels.base_rgba()?))
}

#[cfg(test)]
mod tests;
