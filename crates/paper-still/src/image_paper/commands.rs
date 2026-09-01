use super::buffer_set::BufferSet;
use super::decode::decode_image;
use super::model::{App, ReadyBuffer, SlideAnim};
use super::shm_pixels::{choose_shm_format, pack_pixels};
use super::slide::{compose_tall, ease_out_cubic, slide_source_y};
use crate::fill_mode::{FillMode, apply_fill_mode};
use anyhow::{Result, anyhow};
use std::collections::HashSet;
use std::time::{Duration, Instant};

impl App {
    pub(super) fn current_dims(&self) -> (u32, u32) {
        self.distinct_targets()
            .first()
            .copied()
            .or_else(|| self.buffers.keys().copied().next())
            .unwrap_or((0, 0))
    }

    pub(super) fn make_ready(&mut self, path: &str) -> Result<ReadyBuffer> {
        let (sw, sh) = self.current_dims();
        if sw == 0 || sh == 0 {
            return Err(anyhow!("no surface dimensions yet"));
        }
        let (rw, rh, raw) = decode_image(path, self.blur, self.dim)?;
        let (bw, bh, pixels) = apply_fill_mode(rw, rh, &raw, sw, sh, self.fill_mode);
        let buffer = BufferSet::new(&self.shm, bw, bh, self.surfaces.len(), |canvas, format| {
            pack_pixels(canvas, &pixels, format);
        })?;
        Ok(ReadyBuffer { buffer, for_w: sw, for_h: sh })
    }

    pub(super) fn do_preload(&mut self, paths: &[String]) {
        if self.fill_mode == FillMode::Span {
            return;
        }
        let keep: HashSet<&String> = paths.iter().collect();
        let cur = self.path.clone();
        self.preloaded.retain(|key, _| keep.contains(key) || *key == cur);
        let (sw, sh) = self.current_dims();
        if sw == 0 || sh == 0 {
            tracing::info!("preload: no surface dimensions yet, deferring");
            self.pending_preload = paths.to_vec();
            return;
        }
        self.pending_preload.clear();
        for path in paths.iter().take(8) {
            if self.preloaded.get(path).is_some_and(|ready| ready.for_w == sw && ready.for_h == sh)
            {
                continue;
            }
            let t0 = Instant::now();
            match self.make_ready(path) {
                Ok(ready) => {
                    tracing::info!(path = %path, ms = t0.elapsed().as_millis() as u64, "preloaded");
                    self.preloaded.insert(path.clone(), ready);
                }
                Err(err) => tracing::warn!(error = %err, path = %path, "preload failed"),
            }
        }
        #[cfg(target_env = "gnu")]
        unsafe {
            libc::malloc_trim(0)
        };
    }

    pub(super) fn ensure_cached(&mut self, path: &str) -> Result<()> {
        let (sw, sh) = self.current_dims();
        if self.preloaded.get(path).is_some_and(|ready| ready.for_w == sw && ready.for_h == sh) {
            return Ok(());
        }
        let ready = self.make_ready(path)?;
        self.preloaded.insert(path.to_string(), ready);
        Ok(())
    }

    pub(super) fn cached_bytes(&mut self, path: &str) -> Option<(u32, u32, Vec<u8>)> {
        let (sw, sh) = self.current_dims();
        let ready = self.preloaded.get_mut(path)?;
        if ready.for_w != sw || ready.for_h != sh {
            return None;
        }
        let (w, h) = ready.buffer.dimensions();
        ready.buffer.canvas().map(|canvas| (w, h, canvas.to_vec()))
    }

    pub(super) fn decode_packed(&self, path: &str) -> Result<(u32, u32, Vec<u8>)> {
        let (sw, sh) = self.current_dims();
        if sw == 0 || sh == 0 {
            return Err(anyhow!("no surface dimensions yet"));
        }
        let (rw, rh, raw) = decode_image(path, self.blur, self.dim)?;
        let (w, h, pixels) = apply_fill_mode(rw, rh, &raw, sw, sh, self.fill_mode);
        let mut packed = vec![0; pixels.len()];
        pack_pixels(&mut packed, &pixels, choose_shm_format(self.shm.formats()));
        Ok((w, h, packed))
    }

    pub(super) fn attach_from_cache(&mut self, path: &str) -> bool {
        if self.distinct_targets().len() > 1 {
            return false;
        }
        let (sw, sh) = self.current_dims();
        let Some(ready) = self.preloaded.get_mut(path) else {
            return false;
        };
        if ready.for_w != sw || ready.for_h != sh {
            return false;
        }
        let (w, h) = ready.buffer.dimensions();
        for (idx, surf) in self.surfaces.iter_mut().enumerate() {
            if surf.width == 0 || surf.height == 0 {
                continue;
            }
            surf.viewport.set_source(0.0, 0.0, f64::from(w), f64::from(h));
            surf.viewport.set_destination(surf.width as i32, surf.height as i32);
            if let Err(err) = ready.buffer.attach_to(idx, &surf.surface) {
                tracing::error!(error = %err, "attach cached buffer failed");
                return false;
            }
            surf.surface.damage_buffer(0, 0, w as i32, h as i32);
            surf.surface.commit();
        }
        self.retire_current();
        self.path = path.to_string();
        self.raw_pixels = Vec::new();
        true
    }

    pub(super) fn start_slide(&mut self, path: &str, dir: &str, duration_ms: u64) -> bool {
        if self.fill_mode == FillMode::Span {
            tracing::info!("slide: span mode, falling back to instant swap");
            return false;
        }
        if self.slide.is_some() {
            self.finish_slide();
        }
        let t0 = Instant::now();
        let cur = self.path.clone();
        let dims = self.current_dims();
        let old = self.cached_bytes(&cur).or_else(|| {
            let buffer = self.buffers.get_mut(&dims)?;
            let (w, h) = buffer.dimensions();
            buffer.canvas().map(|canvas| (w, h, canvas.to_vec()))
        });
        let old = old.or_else(|| match self.decode_packed(&cur) {
            Ok(bytes) => Some(bytes),
            Err(err) => {
                tracing::warn!(error = %err, path = %cur, "slide: rebuilding current image failed");
                None
            }
        });
        let Some((ow, oh, old_bytes)) = old else {
            tracing::info!("slide: current image unavailable, instant swap");
            return false;
        };
        if let Err(err) = self.ensure_cached(path) {
            tracing::warn!(error = %err, path = %path, "slide: building target failed");
            return false;
        }
        let Some((w, h, new_bytes)) = self.cached_bytes(path) else {
            tracing::warn!("slide: target buffer canvas unavailable, instant swap");
            return false;
        };
        if w != ow || h != oh {
            tracing::info!(
                new_w = w,
                new_h = h,
                cur_w = ow,
                cur_h = oh,
                "slide: buffer dims differ (fill mode keeps aspect), instant swap"
            );
            return false;
        }
        let dir_up = dir != "down";
        let buffer = match BufferSet::new(&self.shm, w, h * 2, self.surfaces.len(), |canvas, _| {
            compose_tall(canvas, &old_bytes, &new_bytes, dir_up);
        }) {
            Ok(buffer) => buffer,
            Err(err) => {
                tracing::warn!(error = %err, "slide: tall pool failed");
                return false;
            }
        };
        let y0 = slide_source_y(dir_up, h, 0.0);
        let qh = self.qh.clone();
        let mut buffer = buffer;
        for (idx, surf) in self.surfaces.iter_mut().enumerate() {
            if surf.width == 0 || surf.height == 0 {
                continue;
            }
            surf.viewport.set_source(0.0, y0, f64::from(w), f64::from(h));
            surf.viewport.set_destination(surf.width as i32, surf.height as i32);
            if let Err(err) = buffer.attach_to(idx, &surf.surface) {
                tracing::warn!(error = %err, "slide: attach buffer failed");
                return false;
            }
            surf.surface.damage_buffer(0, 0, w as i32, (h * 2) as i32);
            if idx == 0 {
                surf.surface.frame(&qh, surf.surface.clone());
            }
            surf.surface.commit();
        }
        tracing::info!(
            path = %path, dir = %dir, duration_ms,
            setup_ms = t0.elapsed().as_millis() as u64,
            "slide: started"
        );
        self.slide = Some(SlideAnim {
            buffer,
            w,
            h,
            dir_up,
            started: Instant::now(),
            duration: Duration::from_millis(duration_ms.clamp(50, 2000)),
            target: path.to_string(),
        });
        true
    }

    pub(super) fn slide_tick(&mut self) {
        let Some(anim) = &self.slide else {
            return;
        };
        let t = anim.started.elapsed().as_secs_f32() / anim.duration.as_secs_f32();
        let (dir_up, w, h) = (anim.dir_up, anim.w, anim.h);
        if t >= 1.0 {
            self.finish_slide();
            return;
        }
        let y = slide_source_y(dir_up, h, ease_out_cubic(t));
        let qh = self.qh.clone();
        for (idx, surf) in self.surfaces.iter_mut().enumerate() {
            if surf.width == 0 || surf.height == 0 {
                continue;
            }
            surf.viewport.set_source(0.0, y, f64::from(w), f64::from(h));
            surf.surface.damage_buffer(0, 0, w as i32, (h * 2) as i32);
            if idx == 0 {
                surf.surface.frame(&qh, surf.surface.clone());
            }
            surf.surface.commit();
        }
    }

    pub(super) fn finish_slide(&mut self) {
        let Some(anim) = self.slide.take() else {
            return;
        };
        let target = anim.target;
        if !self.attach_from_cache(&target) {
            tracing::warn!(path = %target, "slide: target vanished from cache, decoding");
            self.path.clone_from(&target);
            self.raw_pixels = Vec::new();
            self.retire_current();
            for idx in 0..self.surfaces.len() {
                self.attach_to(idx);
            }
        }
        if anim.buffer.has_active_buffers() {
            self.retired.push(anim.buffer);
        }
        #[cfg(target_env = "gnu")]
        unsafe {
            libc::malloc_trim(0)
        };
        tracing::info!(path = %self.path, "slide: finished");
    }

    pub(super) fn try_consume_pending_cmd(&mut self) {
        if !self.persist {
            return;
        }
        let Some(cmd) = self.pending_cmd.lock().unwrap().take() else {
            return;
        };
        let refill = cmd
            .fill
            .as_deref()
            .and_then(|name| name.parse::<FillMode>().ok())
            .is_some_and(|mode| std::mem::replace(&mut self.fill_mode, mode) != mode);
        if refill {
            self.preloaded.clear();
            tracing::info!(fill_mode = ?self.fill_mode, "image persist: fill mode changed");
        }
        if cmd.path.is_empty() {
            if !cmd.preload.is_empty() {
                self.do_preload(&cmd.preload);
            }
            return;
        }
        if let Some(dir) = cmd.slide.as_deref()
            && self.start_slide(&cmd.path, dir, cmd.duration_ms.unwrap_or(300))
        {
            return;
        }
        if self.slide.is_some() {
            self.finish_slide();
        }
        if !refill && self.attach_from_cache(&cmd.path) {
            #[cfg(target_env = "gnu")]
            unsafe {
                libc::malloc_trim(0)
            };
            tracing::info!(path = %cmd.path, "image persist: swapped (preloaded)");
            return;
        }
        let (new_w, new_h, new_bytes) = match decode_image(&cmd.path, self.blur, self.dim) {
            Ok(img) => img,
            Err(err) => {
                tracing::error!(error = %err, path = %cmd.path, "image persist: decode failed");
                return;
            }
        };
        self.raw_w = new_w;
        self.raw_h = new_h;
        self.raw_pixels = new_bytes;
        self.path.clone_from(&cmd.path);
        if self.fill_mode == FillMode::Span {
            self.attach_all_span();
            #[cfg(target_env = "gnu")]
            unsafe {
                libc::malloc_trim(0)
            };
            tracing::info!(path = %cmd.path, w = new_w, h = new_h, "image persist: swapped (span)");
            return;
        }
        self.retire_current();
        for idx in 0..self.surfaces.len() {
            self.attach_to(idx);
        }
        #[cfg(target_env = "gnu")]
        unsafe {
            libc::malloc_trim(0)
        };
        tracing::info!(path = %cmd.path, w = new_w, h = new_h, "image persist: swapped");
    }
}
