use super::decode::decode_image;
use super::model::App;
use super::shm_pixels::{choose_shm_format, pack_pixels};
use smithay_client_toolkit::shm::slot::SlotPool;

impl App {
    pub(super) fn ensure_raw_pixels(&mut self) -> bool {
        if !self.raw_pixels.is_empty() {
            return true;
        }
        match decode_image(&self.path, self.blur, self.dim) {
            Ok((w, h, bytes)) => {
                self.raw_w = w;
                self.raw_h = h;
                self.raw_pixels = bytes;
                true
            }
            Err(err) => {
                tracing::error!(error = %err, path = %self.path, "span: decode failed");
                false
            }
        }
    }

    pub(super) fn span_bounds(&self) -> Option<(i32, i32, u32, u32)> {
        let rects: Vec<(i32, i32, u32, u32)> = self
            .surfaces
            .iter()
            .filter(|surf| surf.width > 0 && surf.height > 0)
            .map(|surf| (surf.pos.0, surf.pos.1, surf.width, surf.height))
            .collect();
        paper_geom::desktop_bounds(&rects)
    }

    pub(super) fn attach_span(&mut self, idx: usize) {
        if !self.ensure_raw_pixels() {
            return;
        }
        let Some(bounds) = self.span_bounds() else {
            return;
        };
        let surf = &self.surfaces[idx];
        let out_rect = (surf.pos.0, surf.pos.1, surf.width, surf.height);
        let sc = surf.scale.max(1) as u32;
        let (dst_w, dst_h) = (surf.width * sc, surf.height * sc);
        let (bw, bh, pixels) = crate::fill_mode::span_pixels(
            self.raw_w,
            self.raw_h,
            &self.raw_pixels,
            bounds,
            out_rect,
            dst_w,
            dst_h,
        );
        let stride = (bw as i32) * 4;
        let pool_size = (stride as usize) * (bh as usize);
        let format = choose_shm_format(self.shm.formats());
        let surf = &mut self.surfaces[idx];
        let needs_new_pool = match &surf.span_pool {
            None => true,
            Some(pool) => pool.len() < pool_size,
        };
        if needs_new_pool {
            match SlotPool::new(pool_size, &self.shm) {
                Ok(pool) => surf.span_pool = Some(pool),
                Err(err) => {
                    tracing::error!(error = %err, "span: SlotPool::new failed");
                    return;
                }
            }
        }
        let pool = surf.span_pool.as_mut().unwrap();
        let (buffer, canvas) = match pool.create_buffer(bw as i32, bh as i32, stride, format) {
            Ok(pair) => pair,
            Err(err) => {
                tracing::error!(error = %err, "span: create_buffer failed");
                return;
            }
        };
        pack_pixels(canvas, &pixels, format);
        surf.viewport.set_source(0.0, 0.0, bw as f64, bh as f64);
        surf.viewport.set_destination(surf.width as i32, surf.height as i32);
        if let Err(err) = buffer.attach_to(&surf.surface) {
            tracing::error!(error = %err, "span: attach buffer failed");
            return;
        }
        surf.surface.damage_buffer(0, 0, bw as i32, bh as i32);
        surf.span_keepalive = Some(buffer);
        surf.attached = true;
        surf.surface.commit();
        if !self.ready_signaled {
            crate::ipc::signal_ready();
            self.ready_signaled = true;
        }
    }

    pub(super) fn attach_all_span(&mut self) {
        for idx in 0..self.surfaces.len() {
            if self.surfaces[idx].width > 0 && self.surfaces[idx].height > 0 {
                self.attach_span(idx);
            }
        }
    }

    pub(super) fn refresh_span_positions(&mut self) -> bool {
        let mut changed = false;
        for idx in 0..self.surfaces.len() {
            let Some(info) = self.output_state.info(&self.surfaces[idx].output) else {
                continue;
            };
            let pos = info.logical_position.unwrap_or(info.location);
            if self.surfaces[idx].pos != pos {
                self.surfaces[idx].pos = pos;
                changed = true;
            }
        }
        changed
    }
}
