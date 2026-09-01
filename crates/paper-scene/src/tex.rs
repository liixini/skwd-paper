use crate::read::Reader;
use anyhow::{Context, Result, anyhow};

pub const FLAG_NO_INTERPOLATION: u32 = 1;
pub const FLAG_CLAMP_UVS: u32 = 2;
pub const FLAG_IS_GIF: u32 = 4;
pub const FLAG_IS_VIDEO: u32 = 32;

pub const MAX_TEXTURE_EDGE: u32 = 8192;
pub const MAX_MIP_BYTES: usize = 256 << 20;
pub const MAX_TEX_PAYLOAD_BYTES: usize = 512 << 20;
pub const MAX_RGBA_BYTES: usize = 256 << 20;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TexFormat {
    Rgba8888,
    Dxt5,
    Dxt3,
    Dxt1,
    Rg88,
    R8,
    Other(i32),
}

impl TexFormat {
    fn from_raw(raw: i32) -> Self {
        match raw {
            0 => Self::Rgba8888,
            4 => Self::Dxt5,
            6 => Self::Dxt3,
            7 => Self::Dxt1,
            8 => Self::Rg88,
            9 => Self::R8,
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
            Self::Other(raw) => format!("format{raw}"),
        }
    }
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

struct Walker<'a> {
    reader: Reader<'a>,
    keep_payload: bool,
}

impl Walker<'_> {
    fn parse(mut self) -> Result<Tex> {
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
        if rank >= 4 {
            self.reader.i32()?;
        }

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
                if byte_count > MAX_MIP_BYTES {
                    return Err(anyhow!("mip encoded size {byte_count} over cap"));
                }
                if retained > MAX_MIP_BYTES {
                    return Err(anyhow!("mip decompressed size {retained} over cap"));
                }
                total_payload = total_payload
                    .checked_add(retained)
                    .ok_or_else(|| anyhow!("texture payload size overflow"))?;
                if total_payload > MAX_TEX_PAYLOAD_BYTES {
                    return Err(anyhow!(
                        "texture payload is {total_payload} bytes; limit is {MAX_TEX_PAYLOAD_BYTES}"
                    ));
                }
                if self.keep_payload {
                    let payload = self.reader.take(byte_count)?;
                    let data = if compressed {
                        lz4_flex::block::decompress(payload, decompressed_len)
                            .map_err(|err| anyhow!("lz4: {err}"))?
                    } else {
                        payload.to_vec()
                    };
                    image.push(TexMip { width, height, data });
                } else {
                    self.reader.skip(byte_count)?;
                }
            }
            images.push(image);
        }

        let mut frames = Vec::new();
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
                let _gif_width = self.reader.i32()?;
                let _gif_height = self.reader.i32()?;
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
            },
            images,
            frames,
        })
    }
}

pub fn parse(data: &[u8]) -> Result<Tex> {
    Walker { reader: Reader::new(data), keep_payload: true }.parse()
}

pub fn parse_meta(data: &[u8]) -> Result<TexMeta> {
    Walker { reader: Reader::new(data), keep_payload: false }.parse().map(|tex| tex.meta)
}

fn decode_free_image(mip: &TexMip) -> Option<(u32, u32, Vec<u8>)> {
    let reader =
        image::ImageReader::new(std::io::Cursor::new(&mip.data)).with_guessed_format().ok()?;
    let mut reader = reader;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_TEXTURE_EDGE);
    limits.max_image_height = Some(MAX_TEXTURE_EDGE);
    limits.max_alloc = Some(MAX_RGBA_BYTES as u64);
    reader.limits(limits);
    let decoded = reader.decode().ok()?;
    let rgba = decoded.to_rgba8();
    let (width, height) = (rgba.width(), rgba.height());
    rgba_len(width, height)?;
    Some((width, height, rgba.into_raw()))
}

fn rgba_len(width: u32, height: u32) -> Option<usize> {
    if width == 0 || height == 0 || width > MAX_TEXTURE_EDGE || height > MAX_TEXTURE_EDGE {
        return None;
    }
    let bytes = (width as usize).checked_mul(height as usize)?.checked_mul(4)?;
    (bytes <= MAX_RGBA_BYTES).then_some(bytes)
}

pub fn decode_rgba(tex: &Tex) -> Option<(u32, u32, Vec<u8>)> {
    let mip = tex.images.first()?.first()?;
    if tex.meta.free_image_format.is_some_and(|format| format >= 0) {
        return decode_free_image(mip);
    }
    let width = u32::try_from(mip.width).ok()?;
    let height = u32::try_from(mip.height).ok()?;
    let rgba_bytes = rgba_len(width, height)?;
    let pixels = rgba_bytes / 4;
    match tex.meta.format {
        TexFormat::Rgba8888 => {
            if mip.data.len() < rgba_bytes {
                return None;
            }
            Some((width, height, mip.data[..rgba_bytes].to_vec()))
        }
        TexFormat::Dxt1 | TexFormat::Dxt3 | TexFormat::Dxt5 => {
            let fmt = match tex.meta.format {
                TexFormat::Dxt1 => texpresso::Format::Bc1,
                TexFormat::Dxt3 => texpresso::Format::Bc2,
                _ => texpresso::Format::Bc3,
            };
            if mip.data.len() < fmt.compressed_size(width as usize, height as usize) {
                return None;
            }
            let mut out = vec![0u8; rgba_bytes];
            fmt.decompress(&mip.data, width as usize, height as usize, &mut out);
            Some((width, height, out))
        }
        TexFormat::R8 => {
            if mip.data.len() < pixels {
                return None;
            }
            let mut out = Vec::with_capacity(rgba_bytes);
            for &value in &mip.data[..pixels] {
                out.extend_from_slice(&[value, value, value, 255]);
            }
            Some((width, height, out))
        }
        TexFormat::Rg88 => {
            let encoded_bytes = pixels.checked_mul(2)?;
            if mip.data.len() < encoded_bytes {
                return None;
            }
            let mut out = Vec::with_capacity(rgba_bytes);
            for pair in mip.data[..encoded_bytes].chunks_exact(2) {
                out.extend_from_slice(&[pair[0], pair[1], 0, 255]);
            }
            Some((width, height, out))
        }
        TexFormat::Other(_) => None,
    }
}
