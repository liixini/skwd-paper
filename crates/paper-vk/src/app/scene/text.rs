use super::{FxTargetId, FxTargetStorage, Group, RGBA_CHANNELS, vk};
use anyhow::{Result, ensure};

type TextTarget = (Option<FxTargetId>, (u32, u32), bool, ash::vk::Format, [f32; RGBA_CHANNELS]);

impl Group {
    fn text_effect_plan(
        &self,
        layer: usize,
        size: (u32, u32),
        mirror: bool,
    ) -> Result<Option<(usize, Vec<TextTarget>)>> {
        let chains = if mirror { &self.mirror_fx } else { &self.fx };
        let Some(index) =
            chains.iter().position(|fx| fx.quad == layer && fx.text_backdrop.is_some())
        else {
            return Ok(None);
        };
        let fx = &chains[index];
        if fx.sampled_targets.is_empty() && fx.owner.iter().all(|owner| !fx.effects[*owner].visible)
        {
            return Ok(None);
        }
        let current = fx.ping.get(&self.fx_pool).extent;
        if (current.width, current.height) == size {
            return Ok(None);
        }
        ensure!(fx.fbo_specs.len() == fx.fbos.len(), "text effect target specifications missing");
        let mut specs = Vec::new();
        for id in fx.target_ids() {
            let target = fx.target(id, &self.fx_pool);
            let (extent, clear) = match id {
                FxTargetId::Fbo(slot) => {
                    (fx.fbo_specs[slot].extent(size), fx.fbo_specs[slot].clear)
                }
                FxTargetId::Ping | FxTargetId::Pong => (size, [0.0; RGBA_CHANNELS]),
            };
            specs.push((Some(id), extent, target.repeat, target.format, clear));
        }
        if let Some(crop) = &fx.crop {
            let target = crop.get(&self.fx_pool);
            specs.push((None, size, target.repeat, target.format, [0.0; RGBA_CHANNELS]));
        }
        Ok(Some((index, specs)))
    }

    pub(super) fn resize_text_effect(&mut self, layer: usize, size: (u32, u32)) -> Result<()> {
        self.resize_text_chain(layer, size, false)?;
        if let Err(error) = self.resize_text_chain(layer, size, true) {
            tracing::warn!("skwd-wall-vk: reflected text effect dropped: {error:#}");
            self.drop_twin(layer);
        }
        Ok(())
    }

    fn resize_text_chain(&mut self, layer: usize, size: (u32, u32), mirror: bool) -> Result<()> {
        let Some((index, specs)) = self.text_effect_plan(layer, size, mirror)? else {
            return Ok(());
        };
        let mut targets = Vec::new();
        let built = (|| -> Result<vk::QuadBuffer> {
            for (id, extent, repeat, format, clear) in specs {
                let target =
                    self.renderer.create_scene_target_fmt(extent.0, extent.1, repeat, format)?;
                targets.push((id, target));
                self.renderer.render_scene(
                    &targets.last().expect("new target").1,
                    clear,
                    &[],
                    &[],
                )?;
            }
            let inset = paper_scene::model::IMAGE_UV_INSET;
            let (u, v) = (inset / size.0 as f32, inset / size.1 as f32);
            self.renderer.create_quad_buffer(
                size,
                [u, v, 1.0 - u, 1.0 - v],
                (size.0 as f32, size.1 as f32),
            )
        })();
        let vertices = match built {
            Ok(vertices) => vertices,
            Err(error) => {
                for (_, target) in targets {
                    self.renderer.destroy_scene_target(target);
                }
                return Err(error);
            }
        };
        let fx = if mirror { &mut self.mirror_fx[index] } else { &mut self.fx[index] };
        self.textures[fx.slot].effect = None;
        let mut old_targets = Vec::new();
        for (id, target) in targets {
            let storage = match id {
                Some(id) => fx.target_storage_mut(id),
                None => fx.crop.as_mut().expect("existing text effect crop"),
            };
            if let FxTargetStorage::Owned(old) =
                std::mem::replace(storage, FxTargetStorage::Owned(target))
            {
                old_targets.push(old);
            }
        }
        let old_vertices = fx.verts.replace(vertices);
        let rect = [size.0 as f32 * 0.5, size.1 as f32 * 0.5, size.0 as f32, size.1 as f32];
        fx.base_quad.rect = rect;
        fx.base_quad.uv = [0.0, 0.0, 1.0, 1.0];
        if let Some(backdrop) = &mut fx.text_backdrop {
            backdrop.rect = rect;
        }
        if let Some(crop) = &mut fx.crop_quad {
            crop.rect = rect;
        }
        for (slot, remap) in fx.remap.iter().enumerate() {
            remap.set(slot);
        }
        fx.presentation = None;
        fx.presentation_inputs.clear();
        fx.output = None;
        fx.reset_targets = true;
        self.renderer.point_slot_at(&self.textures[fx.slot], fx.pong.get(&self.fx_pool).view);
        for target in old_targets {
            self.renderer.destroy_scene_target(target);
        }
        if let Some(vertices) = old_vertices {
            self.renderer.destroy_quad_buffer(vertices);
        }
        Ok(())
    }
}
