use std::ptr::{self, NonNull};
use std::sync::Arc;

use freetype_sys as ft;

use super::{Glyph, MAX_EDGE};

const FT_26_6_SCALE: f64 = 64.0;
const FT_POINTS_PER_INCH: ft::FT_UInt = 72;
const MONO_BITS: usize = 1;
const GRAY2_BITS: usize = 2;
const GRAY4_BITS: usize = 4;
const GRAY_BITS: usize = u8::BITS as usize;

pub(super) struct RasterFace {
    _bytes: Arc<[u8]>,
    library: NonNull<ft::FT_LibraryRec>,
    face: NonNull<ft::FT_FaceRec>,
}

unsafe impl Send for RasterFace {}

impl RasterFace {
    pub(super) fn new(bytes: Arc<[u8]>, em: f32) -> Option<Self> {
        let size = (f64::from(em) * FT_26_6_SCALE).round();
        if !size.is_finite() || size <= 0.0 || size > ft::FT_F26Dot6::MAX as f64 {
            return None;
        }
        let file_size = ft::FT_Long::try_from(bytes.len()).ok()?;
        let mut library = ptr::null_mut();
        if unsafe { ft::FT_Init_FreeType(&raw mut library) } != 0 {
            return None;
        }
        let library = NonNull::new(library)?;
        let mut face = ptr::null_mut();
        if unsafe {
            ft::FT_New_Memory_Face(library.as_ptr(), bytes.as_ptr(), file_size, 0, &raw mut face)
        } != 0
        {
            unsafe { ft::FT_Done_FreeType(library.as_ptr()) };
            return None;
        }
        let Some(face) = NonNull::new(face) else {
            unsafe { ft::FT_Done_FreeType(library.as_ptr()) };
            return None;
        };
        let raster = Self { _bytes: bytes, library, face };
        if unsafe {
            ft::FT_Set_Char_Size(
                raster.face.as_ptr(),
                0,
                size as ft::FT_F26Dot6,
                FT_POINTS_PER_INCH,
                FT_POINTS_PER_INCH,
            )
        } != 0
        {
            return None;
        }
        Some(raster)
    }

    pub(super) fn height(&self) -> Option<f32> {
        let size = NonNull::new(unsafe { self.face.as_ref().size })?;
        Some(unsafe { size.as_ref().metrics.height } as f32 / FT_26_6_SCALE as f32)
    }

    pub(super) fn rasterize(&mut self, id: u16) -> Option<(Glyph, Vec<u8>)> {
        if unsafe { ft::FT_Load_Glyph(self.face.as_ptr(), u32::from(id), ft::FT_LOAD_DEFAULT) } != 0
        {
            return None;
        }
        let slot = NonNull::new(unsafe { self.face.as_ref().glyph })?;
        if unsafe { ft::FT_Render_Glyph(slot.as_ptr(), ft::FT_RENDER_MODE_NORMAL) } != 0 {
            return None;
        }
        let slot = unsafe { slot.as_ref() };
        let bitmap = &slot.bitmap;
        let width = usize::try_from(bitmap.width).ok()?;
        let height = usize::try_from(bitmap.rows).ok()?;
        if width == 0 || height == 0 {
            return Some((Glyph::default(), Vec::new()));
        }
        if width > MAX_EDGE as usize || height > MAX_EDGE as usize {
            return None;
        }
        let ymin = slot.bitmap_top.checked_sub(i32::try_from(height).ok()?)?;
        let glyph = Glyph { width, height, xmin: slot.bitmap_left, ymin };
        Some((glyph, unsafe { alpha_bitmap(bitmap, width, height)? }))
    }
}

unsafe fn alpha_bitmap(bitmap: &ft::FT_Bitmap, width: usize, height: usize) -> Option<Vec<u8>> {
    let mode = u32::from(bitmap.pixel_mode as u8);
    let bits = match mode {
        ft::FT_PIXEL_MODE_MONO => MONO_BITS,
        ft::FT_PIXEL_MODE_GRAY2 => GRAY2_BITS,
        ft::FT_PIXEL_MODE_GRAY4 => GRAY4_BITS,
        ft::FT_PIXEL_MODE_GRAY => GRAY_BITS,
        _ => return None,
    };
    let max_sample = match mode {
        ft::FT_PIXEL_MODE_GRAY => {
            let levels = u16::try_from(bitmap.num_grays).ok()?;
            if !(2..=u16::from(u8::MAX) + 1).contains(&levels) {
                return None;
            }
            levels - 1
        }
        _ => (1u16 << bits) - 1,
    };
    let row_bytes = width.checked_mul(bits)?.div_ceil(GRAY_BITS);
    let pitch = bitmap.pitch as isize;
    let stride = pitch.unsigned_abs();
    if stride < row_bytes || stride.checked_mul(height)? > isize::MAX as usize {
        return None;
    }
    let buffer = NonNull::new(bitmap.buffer)?;
    let mut data = Vec::with_capacity(width.checked_mul(height)?);
    for row in 0..height {
        let offset = isize::try_from(row).ok()?.checked_mul(pitch)?;
        let bytes =
            unsafe { std::slice::from_raw_parts(buffer.as_ptr().offset(offset), row_bytes) };
        if bits == GRAY_BITS && max_sample == u16::from(u8::MAX) {
            data.extend_from_slice(bytes);
            continue;
        }
        for col in 0..width {
            let bit = col.checked_mul(bits)?;
            let shift = GRAY_BITS - bits - bit % GRAY_BITS;
            let sample = (u16::from(bytes[bit / GRAY_BITS]) >> shift) & ((1 << bits) - 1);
            if sample > max_sample {
                return None;
            }
            data.push((sample * u16::from(u8::MAX) / max_sample) as u8);
        }
    }
    Some(data)
}

impl Drop for RasterFace {
    fn drop(&mut self) {
        unsafe {
            ft::FT_Done_Face(self.face.as_ptr());
            ft::FT_Done_FreeType(self.library.as_ptr());
        }
    }
}

#[cfg(test)]
mod tests;
