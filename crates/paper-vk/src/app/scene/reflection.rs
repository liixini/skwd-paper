use super::mesh::{mirror_vector, mirror_view, mirror_world};
use super::{Group, OrderedParticleDraw, ParticleBatches, vk};
use anyhow::Result;
use std::collections::HashMap;

pub(super) const PARTICLE_VECTORS: [&str; 6] = [
    "g_OrientationUp",
    "g_OrientationRight",
    "g_OrientationForward",
    super::FX_VIEW_UP,
    super::FX_VIEW_RIGHT,
    super::mesh::EYE_POSITION,
];
const CLIP_Y: usize = 1;
const CLIP_W: usize = 2;
const ORTHO_CLIP_ORIGIN: f32 = -1.0;

pub(super) fn mirror_overrides(
    overrides: &mut std::collections::BTreeMap<String, Vec<f32>>,
    names: &[&str],
) {
    for name in names {
        if let Some(value) = overrides.get_mut(*name)
            && let Ok(vector) = <[f32; 3]>::try_from(value.as_slice())
        {
            *value = mirror_vector(vector).to_vec();
        }
    }
}

pub(super) fn mirrored_projection(view_projection: &super::Mat4, model: &super::Mat4) -> Vec<f32> {
    let mut mirrored = *view_projection;
    mirror_view(&mut mirrored);
    super::mat4_mul(&mirrored, model).to_vec()
}

fn orthographic_rows(mut rows: [[f32; 4]; 3]) -> [[f32; 4]; 3] {
    let w = rows[CLIP_W];
    for (y, w) in rows[CLIP_Y].iter_mut().zip(w) {
        *y = -*y + 2.0 * ORTHO_CLIP_ORIGIN * w;
    }
    rows
}

fn reflected_particles<'a>(
    particles: &'a [super::ParticleGroup],
    staging: &Staging,
    draws: &[OrderedParticleDraw],
    grab: Option<&'a vk::SceneTarget>,
) -> Vec<vk::ParticleDraw<'a>> {
    super::particle_batches(
        particles,
        &staging.orders,
        &[],
        draws,
        ParticleBatches { from: 0, before: None, mirrored: true },
        grab,
    )
}

fn adopt_quad(own: &mut vk::SceneQuad, main: &vk::SceneQuad) {
    *own = vk::SceneQuad { texture: own.texture, ..main.clone() };
}

#[derive(Clone, Copy)]
pub(super) struct ReflectionFrame {
    pub canvas: [f32; 2],
    pub clock: paper_scene::effects::FrameClock,
    pub screen: (u32, u32),
    pub batched: bool,
    pub debug_fx: bool,
}

#[derive(Default)]
pub(super) struct Staging {
    sources: Vec<vk::SceneQuad>,
    keys: Vec<(usize, i8, usize)>,
    ordered: Vec<vk::SceneQuad>,
    orders: Vec<usize>,
    plain: Vec<usize>,
    drawn: Vec<vk::SceneQuad>,
    particled: bool,
    counts: (usize, usize, usize),
    mirrors: HashMap<usize, Option<usize>>,
    chains: HashMap<usize, usize>,
    live: HashMap<usize, bool>,
}

impl Staging {
    fn sort(&mut self) {
        self.keys.clear();
        self.keys.extend(
            self.sources
                .iter()
                .zip(&self.orders)
                .enumerate()
                .map(|(serial, (quad, order))| (*order, quad.order_bias, serial)),
        );
        self.keys.sort_unstable();
        self.ordered.clear();
        self.ordered.extend(self.keys.iter().map(|(_, _, serial)| self.sources[*serial].clone()));
        self.orders.clear();
        self.orders.extend(self.keys.iter().map(|(order, _, _)| *order));
    }

    fn starve_chain(&mut self, slot: usize, base: Option<usize>) {
        let mut index = 0;
        while index < self.ordered.len() {
            if self.ordered[index].texture != slot {
                index += 1;
            } else if let Some(base) = base {
                self.ordered[index].texture = base;
                index += 1;
            } else {
                self.ordered.remove(index);
                self.orders.remove(index);
            }
        }
    }

    fn stale(&self, ready: bool, animated: bool) -> bool {
        !ready || animated || self.particled || self.ordered != self.drawn
    }
}

impl Group {
    pub(super) fn mirrored_rows(&self, index: usize, quad: &vk::SceneQuad) -> [[f32; 4]; 3] {
        let pose = self.layer_poses.get(&index);
        let view = match (pose, self.camera, self.mesh_settings.orientation) {
            (Some(_), Some(camera), _) => {
                Some(super::authored_perspective(camera, self.canvas, false).0)
            }
            (Some(_), None, Some(orientation))
                if self.mesh_settings.projection == paper_scene::model::ProjectionMode::Other =>
            {
                Some(super::mat4_mul(&super::scene_ortho(self.canvas, false), &orientation))
            }
            _ => None,
        };
        let mut rows = match (pose, view) {
            (Some(pose), Some(mut view)) => {
                mirror_world(&mut view);
                super::camera_quad(*pose, self.canvas, &view)
            }
            _ => orthographic_rows(super::quad_projection(quad, self.canvas)),
        };
        super::layer_view(index, self.bloom.quad, &self.screen_quads, self.clip)
            .apply_rows(&mut rows, true);
        for value in &mut rows[CLIP_Y] {
            *value = -*value;
        }
        rows
    }

    fn index_sources(&self, staging: &mut Staging) {
        let counts = (self.materials.len(), self.fx.len(), self.mirror_fx.len());
        if staging.counts == counts {
            return;
        }
        staging.counts = counts;
        staging.mirrors.clear();
        staging.chains.clear();
        staging.live.clear();
        for (index, fx) in self.mirror_fx.iter().enumerate() {
            staging.chains.insert(fx.quad, index);
        }
        for material in &self.materials {
            staging.mirrors.insert(material.quad, material.mirror_slot());
            staging
                .live
                .insert(material.quad, material.frame_dependent() || material.pointer_dependent());
        }
        for fx in &self.fx {
            staging.live.insert(fx.quad, true);
        }
    }

    fn stage_layers(&self, staging: &mut Staging) -> bool {
        let mut animated = self.media_animated() || !self.live_text.is_empty();
        staging.sources.clear();
        staging.orders.clear();
        staging.plain.clear();
        for (index, quad) in self.quads.iter().enumerate() {
            if self.reflection_excluded.contains(&index)
                || self.bloom.quad == Some(index)
                || quad.tint[super::ALPHA_CHANNEL] <= 0.0
            {
                continue;
            }
            let Some(texture) = self.textures.get(quad.texture) else { continue };
            if texture.draws_mesh() {
                continue;
            }
            let mut mirrored = quad.clone();
            let mut live = staging.live.get(&index).copied().unwrap_or(false);
            if let Some(&chain) = staging.chains.get(&index) {
                mirrored.texture = self.mirror_fx[chain].slot;
                live = self.mirror_fx[chain].frame_state_dependent();
            } else {
                match staging.mirrors.get(&index) {
                    Some(Some(slot)) if self.textures[*slot].effect.is_some() => {
                        mirrored.texture = *slot;
                    }
                    Some(_) if texture.effect.is_some() => continue,
                    _ if texture.effect.is_some() => staging.plain.push(quad.texture),
                    _ => {}
                }
            }
            if !self.screen_quads.contains(&index) {
                mirrored.projection = Some(self.mirrored_rows(index, quad));
            }
            animated |= live;
            staging.sources.push(mirrored);
            staging.orders.push(self.quad_scene_order[index]);
        }
        animated
    }

    pub(super) fn drop_twin(&mut self, quad: usize) {
        let Some(index) = self.mirror_fx.iter().position(|fx| fx.quad == quad) else { return };
        let fx = self.mirror_fx.remove(index);
        self.discard_layer_fx(fx);
        self.reflection_excluded.insert(quad);
        if let Some(reflection) = &mut self.reflection {
            reflection.staging = Staging::default();
        }
        self.release_empty_fx_quads();
    }

    fn sync_chains(&mut self) {
        for twin in &mut self.mirror_fx {
            let Some(fx) = self.fx.iter().find(|fx| fx.quad == twin.quad) else { continue };
            twin.scene_order = fx.scene_order;
            twin.effects.clone_from(&fx.effects);
            twin.uniforms.clone_from(&fx.uniforms);
            twin.material_values.clone_from(&fx.material_values);
            twin.fallback_tint = fx.fallback_tint;
            adopt_quad(&mut twin.base_quad, &fx.base_quad);
            if let (Some(own), Some(main)) = (&mut twin.text_backdrop, &fx.text_backdrop) {
                adopt_quad(own, main);
            }
            if let (Some(own), Some(main)) = (&mut twin.crop_quad, &fx.crop_quad) {
                adopt_quad(own, main);
            }
        }
    }

    fn record_chains(
        &mut self,
        staging: &mut Staging,
        draws: &[OrderedParticleDraw],
        frame: ReflectionFrame,
    ) -> Result<()> {
        self.sync_chains();
        let mut order: Vec<usize> = (0..self.mirror_fx.len()).collect();
        order.sort_by_key(|&index| self.mirror_fx[index].scene_order);
        for index in order {
            let (quad, scene_order) =
                (self.mirror_fx[index].quad, self.mirror_fx[index].scene_order);
            if self.quads[quad].tint[super::ALPHA_CHANNEL] <= 0.0 {
                continue;
            }
            let cut = staging.orders.partition_point(|order| *order < scene_order);
            let batches = super::particle_batches(
                &self.particles,
                &staging.orders[..cut],
                &[],
                draws,
                ParticleBatches { from: 0, before: Some(scene_order), mirrored: true },
                self.scene_grab.as_ref(),
            );
            let reflection = self.reflection.as_ref().expect("reflection sampled");
            let target = reflection.snapshot.as_ref().expect("reflection snapshot");
            let layers = vk::SceneLayers {
                quads: &staging.ordered[..cut],
                textures: &self.textures,
                particles: &batches,
                plain: &staging.plain,
            };
            self.renderer.record_scene_layers(
                target,
                frame.canvas,
                Some(self.clear_color()),
                layers,
            )?;
            let snapshot = Some((target.view, target.sampler, target.extent));
            drop(batches);
            self.run_fx_chain(super::fx_chain::FxChain {
                index,
                mirror: true,
                hidden_text: false,
                snapshot,
                clock: frame.clock,
                screen: frame.screen,
                canvas: frame.canvas,
                debug_fx: frame.debug_fx,
                scene_targets: None,
            })?;
            let fx = &self.mirror_fx[index];
            if fx.output.is_none() {
                let base = fx.base_quad.texture;
                staging.starve_chain(fx.slot, (fx.snapshot_slot != Some(base)).then_some(base));
            }
        }
        Ok(())
    }

    pub(super) fn record_reflection(
        &mut self,
        draws: &[OrderedParticleDraw],
        frame: ReflectionFrame,
    ) -> Result<()> {
        let ReflectionFrame { canvas, batched, .. } = frame;
        let Some(reflection) = self.reflection.as_mut().filter(|reflection| reflection.sampled)
        else {
            return Ok(());
        };
        let mut staging = std::mem::take(&mut reflection.staging);
        self.index_sources(&mut staging);
        let animated = self.stage_layers(&mut staging);
        let reflection = self.reflection.as_ref().expect("reflection sampled");
        for twin in reflection.twins.values() {
            if twin.quad.tint[super::ALPHA_CHANNEL] > 0.0 {
                staging.sources.push(twin.quad.clone());
                staging.orders.push(twin.order);
            }
        }
        staging.sort();
        let particled =
            !reflected_particles(&self.particles, &staging, draws, self.scene_grab.as_ref())
                .is_empty();
        let ready = self.reflection.as_ref().is_some_and(|reflection| reflection.ready);
        if !staging.stale(ready, animated || particled) {
            self.reflection.as_mut().expect("reflection sampled").staging = staging;
            return Ok(());
        }
        if !batched {
            self.renderer.begin_scene_batch()?;
        }
        if !self.mirror_fx.is_empty() {
            self.record_chains(&mut staging, draws, frame)?;
        }
        let reflection = self.reflection.as_ref().expect("reflection sampled");
        let batches =
            reflected_particles(&self.particles, &staging, draws, self.scene_grab.as_ref());
        let layers = vk::SceneLayers {
            quads: &staging.ordered,
            textures: &self.textures,
            particles: &batches,
            plain: &staging.plain,
        };
        let clear = self.clear_enabled.then(|| self.clear_color());
        let target = reflection.target();
        self.renderer.record_scene_layers(target, canvas, clear, layers)?;
        if !batched {
            self.renderer.submit_scene_batch()?;
        }
        staging.particled = !batches.is_empty();
        drop(batches);
        std::mem::swap(&mut staging.ordered, &mut staging.drawn);
        let reflection = self.reflection.as_mut().expect("reflection sampled");
        reflection.staging = staging;
        reflection.ready = true;
        Ok(())
    }
}

#[cfg(test)]
#[path = "reflection_tests.rs"]
mod tests;
