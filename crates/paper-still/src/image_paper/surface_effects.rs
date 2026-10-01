use super::decode::decode_image;
use super::model::{App, EffectFrame};
use anyhow::Result;

impl App {
    pub(super) fn set_surface_effects(
        &mut self,
        surface: &paper_control::SurfacePolicy,
    ) -> Result<()> {
        surface.validate()?;
        if self.blur == surface.blur as f32 && self.dim == surface.dim {
            return Ok(());
        }
        if self.slide.is_some() {
            self.finish_slide();
        }
        let cached = self
            .effect_cache
            .as_ref()
            .is_some_and(|frame| frame.blur == surface.blur as f32 && frame.dim == surface.dim);
        let next = if cached {
            self.effect_cache.take().unwrap()
        } else {
            let (width, height, raw) = decode_image(&self.path, surface.blur as f32, surface.dim)?;
            EffectFrame {
                blur: surface.blur as f32,
                dim: surface.dim,
                width,
                height,
                raw,
                buffers: std::collections::HashMap::new(),
            }
        };
        let previous = EffectFrame {
            blur: std::mem::replace(&mut self.blur, next.blur),
            dim: std::mem::replace(&mut self.dim, next.dim),
            width: std::mem::replace(&mut self.raw_w, next.width),
            height: std::mem::replace(&mut self.raw_h, next.height),
            raw: std::mem::replace(&mut self.raw_pixels, next.raw),
            buffers: std::mem::replace(&mut self.buffers, next.buffers),
        };
        if let Some(expired) = self.effect_cache.replace(previous) {
            self.retired.extend(
                expired
                    .buffers
                    .into_values()
                    .filter(super::buffer_set::BufferSet::has_active_buffers),
            );
        }
        self.preloaded.clear();
        for idx in 0..self.surfaces.len() {
            self.attach_to(idx);
        }
        Ok(())
    }
}
