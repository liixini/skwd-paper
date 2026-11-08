#[cfg(test)]
use super::resize;
use super::{
    FxBuilder, FxInput, Group, RGB_CHANNELS, RGBA_CHANNELS, SceneDimensions, TextureInterner,
    layer_quad, layer_sampling,
};
use crate::vk;
use anyhow::{Context, Result, anyhow};
use std::collections::HashMap;

const SCENE_FX_PASSES: &str = "SKWD_VK_FX_PASSES";
const GENERAL_FIELD: &str = "general";
const ALPHA_COMPONENT: usize = RGBA_CHANNELS - 1;

impl Group {
    pub(super) fn sync_bloom(
        &mut self,
        scene: &serde_json::Value,
        properties: &paper_scene::model::Properties,
        resources: &paper_scene::model::ImageResources,
        order: &mut Vec<(Option<usize>, usize)>,
    ) -> Result<()> {
        let enabled = paper_scene::model::bloom::enabled(scene.get(GENERAL_FIELD), properties);
        let active = self.fx.iter().any(|fx| fx.layer_id == paper_scene::model::bloom::ID);
        match (enabled, active) {
            (true, false) => self.enable_bloom(scene, properties, resources, order),
            (false, true) => self.disable_bloom(),
            _ => Ok(()),
        }
    }

    fn disable_bloom(&mut self) -> Result<()> {
        self.renderer.wait_render()?;
        let position = self.fx.iter().position(|fx| fx.layer_id == paper_scene::model::bloom::ID);
        let Some(position) = position else { return Ok(()) };
        let fx = self.fx.remove(position);
        let planned = self
            .scene_target_plan(true)
            .and_then(|plan| self.scene_target_plan(false).map(|animation| (plan, animation)));
        let (plan, animation) = match planned {
            Ok(planned) => planned,
            Err(error) => {
                self.fx.insert(position, fx);
                return Err(error.into());
            }
        };
        if let Err(error) = self.apply_scene_target_plan(&plan) {
            self.fx.push(fx);
            self.configure_scene_targets(true)?;
            return Err(error);
        }
        self.effects_animated = animation.dynamic.iter().any(|dynamic| *dynamic);
        self.quads[fx.quad].tint[ALPHA_COMPONENT] = 0.0;
        self.discard_layer_fx(fx);
        for slot in self.bloom.active_slots.drain(..) {
            let texture = std::mem::replace(&mut self.textures[slot], vk::SceneTexture::vacant());
            self.renderer.destroy_scene_texture(texture);
            if self.bloom.base_slot == Some(slot) {
                self.bloom.base_slot = None;
            }
            self.bloom.spare_slots.push(slot);
        }
        self.release_unused_scene_snapshot();
        self.release_empty_fx_quads();
        Ok(())
    }

    #[expect(clippy::too_many_lines, reason = "one bloom activation and rollback transaction")]
    fn enable_bloom(
        &mut self,
        scene: &serde_json::Value,
        properties: &paper_scene::model::Properties,
        resources: &paper_scene::model::ImageResources,
        order: &mut Vec<(Option<usize>, usize)>,
    ) -> Result<()> {
        self.renderer.wait_render()?;
        let rank = self.bloom.quad.map_or_else(
            || order.iter().map(|(_, rank)| rank + 1).max().unwrap_or(0),
            |quad| self.quad_scene_order[quad],
        );
        let view = paper_scene::model::bloom::View {
            canvas: self.canvas,
            hdr: self.renderer.scene_format() == ash::vk::Format::R16G16B16A16_SFLOAT,
            order: rank,
        };
        let mut layer = resources
            .bloom_layer(scene, properties, view)?
            .context("bloom layer disabled during activation")?;
        let _compilation = paper_scene::shader::CompilationSession::new()?;
        let dimensions = SceneDimensions {
            logical: [self.canvas.0, self.canvas.1],
            raster: (self.target.extent.width, self.target.extent.height),
        };
        layer.fit_viewport(self.canvas, dimensions.raster);
        let first_texture = self.textures.len();
        let base_slot = if let Some(slot) = self.bloom.base_slot {
            slot
        } else {
            let texture = self.renderer.create_scene_texture_pixels(
                &layer.texture.pixels,
                layer.texture.clamp,
                layer.texture.nearest,
                layer_sampling(false),
            )?;
            let slot = self.textures.len();
            self.textures.push(texture);
            slot
        };
        let quad = self.bloom.quad.unwrap_or(self.quads.len());
        let old_quad = self.quads.get(quad).cloned();
        let candidate = layer_quad(&layer, base_slot, self.canvas);
        if quad == self.quads.len() {
            self.quads.push(candidate);
            self.quad_scene_order.push(rank);
        } else {
            self.quads[quad] = candidate;
        }
        let mut interner = TextureInterner::new(None);
        let mut skipped = Vec::new();
        let mut dynamic_chains = self.fx.iter().filter(|fx| fx.intrinsic_dynamic(true)).count();
        let pass_cap = std::env::var(SCENE_FX_PASSES)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(usize::MAX);
        let fx_limit = super::dynamic_chain_limit();
        let staged = FxBuilder {
            mirror: false,
            renderer: &mut self.renderer,
            interner: &mut interner,
            textures: &mut self.textures,
            quads: &mut self.quads,
            materials: &mut self.materials,
            skipped: &mut skipped,
            dimensions,
            canvas: self.canvas,
            scene,
            properties,
            scene_lights: ([0.0; RGB_CHANNELS], [0.0; RGB_CHANNELS]),
            pointer_scene: self.mouse.enabled,
            strict: true,
            scripted: true,
            world: false,
            pass_cap,
            fx_limit,
            dynamic_chains: &mut dynamic_chains,
        }
        .layer(
            &mut layer,
            quad,
            &FxInput {
                layer_slot: base_slot,
                text_prefix_dynamic: false,
                text_prefix_clock: false,
                source_keyframes: true,
                mesh: None,
                puppeted: false,
            },
        );
        let mut fx = match staged {
            Ok(Some(fx)) => fx,
            Ok(None) => {
                self.rollback_bloom(quad, old_quad, first_texture);
                return Err(anyhow!("bloom effect unavailable"));
            }
            Err(error) => {
                self.rollback_bloom(quad, old_quad, first_texture);
                return Err(error);
            }
        };
        #[cfg(test)]
        if let Err(error) = resize::tests::after_effect() {
            self.destroy_layer_fx(fx);
            self.rollback_bloom(quad, old_quad, first_texture);
            return Err(error);
        }
        fx.uniforms.extend(self.scene_uniforms.clone());
        self.quads[quad].uv = [0.0, 0.0, 1.0, 1.0];
        self.fx.push(fx);
        if let Err(error) = self.ensure_fx_quads() {
            self.rollback_bloom_fx(quad, old_quad, first_texture)?;
            return Err(error);
        }
        let planned = self
            .scene_target_plan(true)
            .and_then(|plan| self.scene_target_plan(false).map(|animation| (plan, animation)));
        let planned = match planned {
            Ok(planned) => planned,
            Err(error) => {
                self.rollback_bloom_fx(quad, old_quad, first_texture)?;
                return Err(error.into());
            }
        };
        self.effects_animated = planned.1.dynamic.iter().any(|dynamic| *dynamic);
        if let Err(error) = self.apply_scene_target_plan(&planned.0) {
            self.rollback_bloom_fx(quad, old_quad, first_texture)?;
            return Err(error);
        }
        let new_slots: Vec<usize> = (first_texture..self.textures.len()).collect();
        let (base_slot, active_slots) = self.reuse_bloom_slots(base_slot, &new_slots);
        self.bloom.quad = Some(quad);
        self.bloom.base_slot = Some(base_slot);
        self.bloom.active_slots = active_slots;
        if old_quad.is_none() {
            order.push((None, rank));
        }
        Ok(())
    }

    fn rollback_bloom(
        &mut self,
        quad: usize,
        old_quad: Option<vk::SceneQuad>,
        first_texture: usize,
    ) {
        if let Some(old_quad) = old_quad {
            self.quads[quad] = old_quad;
        } else {
            self.quads.pop();
            self.quad_scene_order.pop();
        }
        for texture in self.textures.drain(first_texture..) {
            self.renderer.destroy_scene_texture(texture);
        }
    }

    fn rollback_bloom_fx(
        &mut self,
        quad: usize,
        old_quad: Option<vk::SceneQuad>,
        first_texture: usize,
    ) -> Result<()> {
        let position = self.fx.iter().position(|fx| fx.layer_id == paper_scene::model::bloom::ID);
        if let Some(position) = position {
            let fx = self.fx.remove(position);
            self.discard_layer_fx(fx);
        }
        self.rollback_bloom(quad, old_quad, first_texture);
        let plan = self.scene_target_plan(true)?;
        self.apply_scene_target_plan(&plan)?;
        self.release_unused_scene_snapshot();
        self.release_empty_fx_quads();
        Ok(())
    }

    fn ensure_fx_quads(&mut self) -> Result<()> {
        if self.ndc_quad.is_some() && self.ndc_quad_d3d.is_some() {
            return Ok(());
        }
        let ndc = self.renderer.create_quad_buffer_ndc()?;
        let ndc_d3d = match self.renderer.create_quad_buffer_ndc_d3d() {
            Ok(buffer) => buffer,
            Err(error) => {
                self.renderer.destroy_quad_buffer(ndc);
                return Err(error);
            }
        };
        self.ndc_quad = Some(ndc);
        self.ndc_quad_d3d = Some(ndc_d3d);
        Ok(())
    }

    pub(super) fn release_empty_fx_quads(&mut self) {
        if !self.fx.is_empty() || !self.mirror_fx.is_empty() {
            return;
        }
        if let Some(buffer) = self.ndc_quad.take() {
            self.renderer.destroy_quad_buffer(buffer);
        }
        if let Some(buffer) = self.ndc_quad_d3d.take() {
            self.renderer.destroy_quad_buffer(buffer);
        }
    }

    fn reuse_bloom_slots(&mut self, base_slot: usize, new_slots: &[usize]) -> (usize, Vec<usize>) {
        let count = new_slots.len().min(self.bloom.spare_slots.len());
        let spares: Vec<usize> = self.bloom.spare_slots.drain(..count).collect();
        let remap: HashMap<usize, usize> =
            new_slots.iter().rev().take(count).copied().zip(spares).collect();
        for (&from, &to) in &remap {
            let texture = std::mem::replace(&mut self.textures[from], vk::SceneTexture::vacant());
            let previous = std::mem::replace(&mut self.textures[to], texture);
            self.renderer.destroy_scene_texture(previous);
        }
        for _ in 0..count {
            self.textures.pop();
        }
        let slot = |old: usize| remap.get(&old).copied().unwrap_or(old);
        if let Some(fx) = self.fx.iter_mut().find(|fx| fx.layer_id == paper_scene::model::bloom::ID)
        {
            fx.slot = slot(fx.slot);
            fx.snapshot_slot = fx.snapshot_slot.map(&slot);
            fx.base_quad.texture = slot(fx.base_quad.texture);
            if let Some(quad) = &mut fx.crop_quad {
                quad.texture = slot(quad.texture);
            }
            if let Some(quad) = &mut fx.text_backdrop {
                quad.texture = slot(quad.texture);
            }
            for input in fx.inputs.iter_mut().flatten() {
                *input = slot(*input);
            }
            self.quads[fx.quad].texture = slot(self.quads[fx.quad].texture);
        }
        let active = new_slots.iter().copied().map(slot).collect();
        (slot(base_slot), active)
    }
}
