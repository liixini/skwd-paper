use super::{
    FxTargetClass, FxTargetId, FxTargetStorage, LayerFx, MeshBlend, PooledTarget, SceneDimensions,
    TextureInterner, TextureKey, active_effect_counts, base_scene_blend, chainless_blend,
    effect_uv_inset, fallback_tint, layer_effect_dimensions, layer_tint, layer_uniforms, lifetime,
    pass_blend, record_effect_skip, vk_format,
};
use super::{RGBA_CHANNELS, fx_pool, material};
use crate::vk;
use anyhow::Result;
use paper_scene::effects::EffectBind;
use std::collections::{BTreeMap, BTreeSet};

const REFLECTED_LAYER: &str = " (reflection)";

pub(super) struct FxInput<'mesh> {
    pub layer_slot: usize,
    pub text_prefix_dynamic: bool,
    pub text_prefix_clock: bool,
    pub source_keyframes: bool,
    pub mesh: Option<&'mesh vk::SceneMesh>,
    pub puppeted: bool,
}

pub(super) struct FxBuilder<'ctx, 'device> {
    pub renderer: &'ctx mut vk::Renderer,
    pub interner: &'ctx mut TextureInterner<'device>,
    pub textures: &'ctx mut Vec<vk::SceneTexture>,
    pub quads: &'ctx mut Vec<vk::SceneQuad>,
    pub materials: &'ctx mut Vec<material::Material>,
    pub skipped: &'ctx mut Vec<String>,
    pub dimensions: SceneDimensions,
    pub canvas: (f32, f32),
    pub scene: &'ctx serde_json::Value,
    pub properties: &'ctx paper_scene::model::Properties,
    pub scene_lights: ([f32; 3], [f32; 3]),
    pub pointer_scene: bool,
    pub strict: bool,
    pub mirror: bool,
    pub scripted: bool,
    pub world: bool,
    pub pass_cap: usize,
    pub fx_limit: usize,
    pub dynamic_chains: &'ctx mut usize,
}

impl FxBuilder<'_, '_> {
    fn chain_target(
        &mut self,
        class: FxTargetClass,
        clear: [f32; RGBA_CHANNELS],
        pooled: bool,
    ) -> Result<FxTargetStorage> {
        if pooled {
            return Ok(FxTargetStorage::Pooled(PooledTarget { class, lease: None }));
        }
        let target = fx_pool::cleared_target(self.renderer, class, clear)?;
        Ok(FxTargetStorage::Owned(target))
    }

    fn rollback_uploads(&mut self, first_texture: usize) {
        self.interner.slots.retain(|_, slot| *slot < first_texture);
        self.interner.media_slots.retain(|_, slot| *slot < first_texture);
        self.interner.video_slots.retain(|_, slot| *slot < first_texture);
        for texture in self.textures.drain(first_texture..) {
            self.renderer.destroy_scene_texture(texture);
        }
    }

    fn skip(&mut self, layer: &str, reason: &str) -> Result<()> {
        if self.mirror {
            let layer = format!("{layer}{REFLECTED_LAYER}");
            return record_effect_skip(self.strict, self.skipped, &layer, reason);
        }
        record_effect_skip(self.strict, self.skipped, layer, reason)
    }

    #[expect(clippy::too_many_lines, reason = "one effect chain GPU resource transaction")]
    pub fn layer(
        &mut self,
        layer: &mut paper_scene::model::Layer,
        index: usize,
        input: &FxInput<'_>,
    ) -> Result<Option<LayerFx>> {
        let first_texture = self.textures.len();
        let mirror = self.mirror;
        let skin = self
            .materials
            .iter()
            .find(|material| material.quad == index)
            .and_then(super::material::Material::skin_source);
        if skin.is_some()
            && let Some(material) =
                self.materials.iter_mut().find(|material| !mirror && material.quad == index)
        {
            material.effect_target(self.renderer, self.textures, None, self.canvas);
        }
        let (fx_w, fx_h) = layer_effect_dimensions(layer, self.dimensions);
        let mut pipelines = Vec::new();
        let mut inputs = Vec::new();
        let mut metas = Vec::new();
        let mut pass_targets = Vec::new();
        let mut pass_binds = Vec::new();
        let mut pass_owner: Vec<usize> = Vec::new();
        let mut pass_overwrites = Vec::new();
        let mut fbos: Vec<(String, FxTargetClass)> = Vec::new();
        let mut fbo_owners: Vec<Option<usize>> = Vec::new();
        let mut fbo_clears = Vec::new();
        let mut fbo_specs = Vec::new();
        let mut swaps: Vec<(Option<usize>, usize, usize)> = Vec::new();
        let mut failed = None;
        let mut swap_owners = Vec::new();
        let mut blend_pass = None;
        let mut visible_passes: usize = layer
            .effects
            .iter()
            .filter(|effect| effect.state.visible)
            .map(|effect| effect.passes.len())
            .sum();
        let is_text = layer.is_text();
        let base_blend = base_scene_blend(layer);
        let bare_blend = layer.additive_text().then_some(vk::SceneBlend::Add);
        let composite_depth_test = layer.composite_depth_test();
        let presents: Vec<Vec<bool>> = layer
            .effects
            .iter()
            .map(|effect| {
                effect.passes.iter().map(|pass| layer.presents_pass(pass, input.puppeted)).collect()
            })
            .collect();
        for (effect_index, effect) in layer.effects.iter_mut().enumerate() {
            if effect.state.visible {
                visible_passes = visible_passes.saturating_sub(effect.passes.len());
            } else if effect.passes.len()
                > self.pass_cap.saturating_sub(pipelines.len()).saturating_sub(visible_passes)
            {
                effect.state.available = false;
                let reason = format!(
                    "hidden effect {effect_index} unavailable under pass budget {}",
                    self.pass_cap
                );
                self.skip(&layer.name, &reason)?;
                continue;
            }
            if effect.state.visible
                && effect.passes.len() > self.pass_cap.saturating_sub(pipelines.len())
            {
                let reason = format!("effect pass budget {} exceeded", self.pass_cap);
                self.skip(&layer.name, &reason)?;
            }
            if pipelines.len() >= self.pass_cap {
                continue;
            }
            let mesh_blend = effect.name == paper_scene::effects::COLOR_BLEND_MESH_EFFECT;
            let pass_offset = pipelines.len();
            let fbo_offset = fbos.len();
            let swap_offset = swaps.len();
            for fbo in &effect.fbos {
                let owner = fbo.unique.then_some(effect_index);
                if fbos
                    .iter()
                    .zip(&fbo_owners)
                    .any(|((existing, _), known)| existing == &fbo.name && *known == owner)
                {
                    continue;
                }
                let base = if layer.composition_size.is_some()
                    || is_text
                    || layer.passthrough
                    || layer.copy_background
                {
                    (fx_w, fx_h)
                } else {
                    (layer.texture.width, layer.texture.height)
                };
                let (sw, sh) = fbo.extent(base);
                let format = self.renderer.color_target_format(
                    vk_format(fbo.format),
                    ash::vk::Format::R16G16B16A16_SFLOAT,
                );
                let class = FxTargetClass {
                    width: sw,
                    height: sh,
                    repeat: fbo.repeat.unwrap_or(false),
                    format: format.as_raw(),
                };
                fbos.push((fbo.name.clone(), class));
                fbo_owners.push(owner);
                fbo_specs.push(fbo.clone());
                fbo_clears.push(fbo.clear_on_setup.then_some(fbo.clear));
            }
            for swap in &effect.swaps {
                let resolve = |name: &str| {
                    lifetime::resolve_fbo_scoped(
                        fbos.iter().map(|(fbo, _)| fbo.as_str()),
                        &fbo_owners,
                        effect_index,
                        name,
                    )
                };
                if let (Some(a), Some(b)) = (resolve(&swap.source), resolve(&swap.target)) {
                    let after = match swap.after_pass {
                        Some(local) => Some(pass_offset + local),
                        None => pass_offset.checked_sub(1),
                    };
                    swaps.push((after, a, b));
                    swap_owners.push(effect_index);
                }
            }
            for (pass, presented) in effect.passes.iter_mut().zip(&presents[effect_index]) {
                if pipelines.len() >= self.pass_cap {
                    break;
                }
                let mut meta = paper_scene::effects::PassMeta::of(pass);
                if layer.base_scene_input && effect_index == 0 && pipelines.is_empty() {
                    meta.set_source_tint([
                        layer.color[0],
                        layer.color[1],
                        layer.color[2],
                        layer.alpha,
                    ]);
                }
                let write_format = pass
                    .target
                    .as_deref()
                    .and_then(|name| {
                        lifetime::resolve_fbo_scoped(
                            fbos.iter().map(|(fbo, _)| fbo.as_str()),
                            &fbo_owners,
                            effect_index,
                            name,
                        )
                    })
                    .map_or(self.renderer.scene_format(), |index| {
                        ash::vk::Format::from_raw(fbos[index].1.format)
                    });
                let mesh = input.mesh.filter(|_| mesh_blend);
                let write_blend =
                    if mesh.is_some() || base_blend.is_some() || is_text && pass.target.is_none() {
                        vk::SceneBlend::Copy
                    } else {
                        pass_blend(pass.blend)
                    };
                let created = if let Some(mesh) = mesh {
                    blend_pass = Some(pipelines.len());
                    self.renderer.create_effect_pipeline_for(
                        &pass.vertex,
                        &pass.fragment,
                        None,
                        &pass.name,
                        vk::EffectPipelineSpec {
                            depth_attachment: true,
                            format: write_format,
                            samples: self.renderer.scene_sample_count(),
                            blend: vk::SceneBlend::Copy,
                            scene: None,
                            geometry: vk::EffectGeometry::SceneMesh,
                            depth: vk::EffectDepth { cull: mesh.cull, ..Default::default() },
                            coordinates: vk::EffectCoordinates::Canvas,
                        },
                    )
                } else {
                    self.renderer.create_effect_pipeline_for(
                        &pass.vertex,
                        &pass.fragment,
                        pass.hlsl.as_ref(),
                        &pass.name,
                        vk::EffectPipelineSpec {
                            depth_attachment: true,
                            format: write_format,
                            samples: ash::vk::SampleCountFlags::TYPE_1,
                            blend: write_blend,
                            geometry: vk::EffectGeometry::Quad,
                            depth: vk::EffectDepth::default(),
                            coordinates: vk::EffectCoordinates::Canvas,
                            scene: (*presented && !self.mirror).then_some(vk::ScenePresent {
                                blend: if bare_blend.is_some() {
                                    vk::SceneBlend::Alpha
                                } else {
                                    self.quads[index].blend
                                },
                                depth_test: composite_depth_test,
                            }),
                        },
                    )
                };
                match created {
                    Ok(pipe) => {
                        metas.push(meta);
                        inputs.push(Vec::new());
                        pass_targets.push(pass.target.clone());
                        pass_binds.push(pass.binds.clone());
                        pass_owner.push(effect_index);
                        pass_overwrites.push(fx_pool::overwrites(pass, write_blend));
                        pipelines.push(pipe);
                    }
                    Err(err) => {
                        failed = Some(format!("{}: {err:#}", pass.name));
                        break;
                    }
                }
            }
            if failed.is_none() {
                for ((pass, pipe), slots) in effect
                    .passes
                    .iter_mut()
                    .zip(&pipelines[pass_offset..])
                    .zip(&mut inputs[pass_offset..])
                {
                    let mut slot_textures = Vec::new();
                    let wanted = pipe.sampler_count.saturating_sub(1) as usize;
                    for slot in 1..pass.textures.len().max(wanted + 1) {
                        let uploaded = match pass.textures.get_mut(slot).and_then(Option::as_mut) {
                            Some(texture) => self
                                .interner
                                .intern_texture(
                                    self.renderer,
                                    self.textures,
                                    texture,
                                    vk::MipSampling::Anisotropic,
                                )
                                .map_err(|err| {
                                    tracing::warn!(
                                        "skwd-wall-vk: effect texture upload failed: {err:#}"
                                    );
                                })
                                .ok(),
                            None => None,
                        };
                        let uploaded = match uploaded {
                            Some(texture) => Some(texture),
                            None if slot <= wanted => self
                                .interner
                                .intern_rgba(self.renderer, self.textures, TextureKey::clear())
                                .ok(),
                            None => None,
                        };
                        let Some(uploaded) = uploaded else {
                            break;
                        };
                        slot_textures.push(uploaded);
                    }
                    *slots = slot_textures;
                }
            }
            if !effect.state.visible
                && let Some(reason) = failed.take()
            {
                effect.state.available = false;
                tracing::warn!(layer = %layer.name, effect = effect_index, %reason, "skwd-wall-vk: hidden effect unavailable");
                for pipe in pipelines.drain(pass_offset..) {
                    self.renderer.destroy_effect_pipeline(pipe);
                }
                fbos.truncate(fbo_offset);
                fbo_owners.truncate(fbo_offset);
                fbo_clears.truncate(fbo_offset);
                fbo_specs.truncate(fbo_offset);
                inputs.truncate(pass_offset);
                metas.truncate(pass_offset);
                pass_targets.truncate(pass_offset);
                pass_binds.truncate(pass_offset);
                pass_owner.truncate(pass_offset);
                pass_overwrites.truncate(pass_offset);
                swaps.truncate(swap_offset);
                swap_owners.truncate(swap_offset);
                self.skip(&layer.name, &reason)?;
            }
            if failed.is_some() {
                break;
            }
        }
        let planned = blend_pass.unwrap_or(pass_owner.len());
        let scratch = lifetime::plan_targets_with(
            &fbos.iter().map(|(name, _)| name.clone()).collect::<Vec<_>>(),
            &fbo_owners,
            &swaps,
            &pass_targets[..planned],
            &pass_binds[..planned],
            &pass_owner[..planned],
        )
        .ok()
        .filter(|_| !is_text)
        .map(|plan| fx_pool::scratch_targets(&plan, &pass_owner, &pass_overwrites));
        let pooled = |id: FxTargetId| {
            scratch.as_ref().is_some_and(|scratch| scratch[id.index()])
                && (!mirror || !matches!(id, FxTargetId::Fbo(_)))
        };
        if let Some(reason) = failed {
            tracing::info!("skwd-wall-vk: effect skipped on {} ({reason})", layer.name);
            for pipe in pipelines {
                self.renderer.destroy_effect_pipeline(pipe);
            }
            self.rollback_uploads(first_texture);
            self.quads[index].tint = fallback_tint(layer);
            if let Some(blend) = chainless_blend(base_scene_blend(layer), bare_blend) {
                self.quads[index].blend = blend;
            }
            self.skip(&layer.name, &reason)?;
            return Ok(None);
        }
        if pipelines.is_empty() {
            self.rollback_uploads(first_texture);
            self.quads[index].tint =
                if !layer.passthrough && layer.effects.iter().all(|effect| !effect.state.visible) {
                    layer_tint(layer)
                } else {
                    fallback_tint(layer)
                };
            if let Some(blend) = chainless_blend(base_scene_blend(layer), bare_blend) {
                self.quads[index].blend = blend;
            }
            return Ok(None);
        }
        let audio_dependent = metas.iter().any(paper_scene::effects::PassMeta::audio_dependent);
        let material_dynamic = self
            .materials
            .iter()
            .any(|material| material.quad == index && material.effect_frame_dependent());
        let intrinsic_dynamic = material_dynamic
            || layer.material.as_ref().is_some_and(|material| {
                let meta = paper_scene::effects::PassMeta::of(&material.pass);
                meta.time_dependent() || meta.audio_dependent() || meta.pointer_dependent()
            })
            || audio_dependent
            || metas.iter().any(paper_scene::effects::PassMeta::time_dependent)
            || lifetime::plan_targets(
                &fbos.iter().map(|(name, _)| name.clone()).collect::<Vec<_>>(),
                &pass_targets,
                &pass_binds,
                &pass_owner,
            )
            .map(|plan| plan.lifetimes.iter().any(|lifetime| lifetime.loop_carried))
            .unwrap_or(true);
        let over_dynamic_cap =
            intrinsic_dynamic && layer.visible && (*self.dynamic_chains) >= self.fx_limit;
        if over_dynamic_cap {
            if let Some(material) =
                self.materials.iter_mut().find(|material| !mirror && material.quad == index)
            {
                material.effect_target(self.renderer, self.textures, None, self.canvas);
            }
            tracing::info!(
                "skwd-wall-vk: effect chain dropped on {} (chains={})",
                layer.name,
                (*self.dynamic_chains)
            );
            for pipe in pipelines {
                self.renderer.destroy_effect_pipeline(pipe);
            }
            self.rollback_uploads(first_texture);
            self.quads[index].tint = fallback_tint(layer);
            if let Some(blend) = chainless_blend(base_scene_blend(layer), bare_blend) {
                self.quads[index].blend = blend;
            }
            let reason = format!(
                "[native-scene-gap:{}] dynamic effect chains {}/{}",
                paper_scene::capability::EFFECT_CHAIN_LIMIT_GAP,
                self.dynamic_chains,
                self.fx_limit
            );
            self.skip(&layer.name, &reason)?;
            return Ok(None);
        }
        if intrinsic_dynamic && layer.visible {
            (*self.dynamic_chains) += 1;
        }
        let mut staged_vertices = None;
        let mut staged_fbos = Vec::new();
        let mut staged_chain = Vec::new();
        let built = (|| -> Result<LayerFx> {
            if skin.is_none()
                && let Some(material) =
                    self.materials.iter_mut().find(|material| !mirror && material.quad == index)
            {
                let size = super::image_effect_dimensions(layer).unwrap_or(material.size);
                let target = self.renderer.create_scene_target(size.0, size.1)?;
                material.effect_target(self.renderer, self.textures, Some(target), self.canvas);
            }
            let [u, v] = effect_uv_inset(layer);
            let uv = [u, v, 1.0 - u, 1.0 - v];
            let (scene, properties) = (self.scene, self.properties);
            let canvas = if matches!(layer.kind, paper_scene::model::LayerKind::Viewport { .. }) {
                (self.dimensions.logical[0], self.dimensions.logical[1])
            } else {
                self.canvas
            };
            let image =
                paper_scene::model::script::Layout::new(layer, scene, canvas, properties).size;
            staged_vertices =
                Some(self.renderer.create_quad_buffer((fx_w, fx_h), uv, (image[0], image[1]))?);
            for (index, ((_, class), spec)) in fbos.iter().zip(&fbo_specs).enumerate() {
                let storage =
                    self.chain_target(*class, spec.clear, pooled(FxTargetId::Fbo(index)))?;
                staged_fbos.push(storage);
            }
            let class = FxTargetClass {
                width: fx_w,
                height: fx_h,
                repeat: !layer.texture.clamp,
                format: self.renderer.scene_format().as_raw(),
            };
            for id in [FxTargetId::Ping, FxTargetId::Pong] {
                staged_chain.push(self.chain_target(class, [0.0; RGBA_CHANNELS], pooled(id))?);
            }
            let (uvw, uvh) = skin.map_or_else(
                || if layer.material.is_some() { (1.0, 1.0) } else { layer.texture.uv_scale() },
                |(_, [u, v])| (u, v),
            );
            let needs_crop = pass_binds
                .iter()
                .flatten()
                .any(|(_, bind)| matches!(bind, EffectBind::SceneUnderLayer));
            if needs_crop {
                staged_chain.push(self.chain_target(
                    class,
                    [0.0; RGBA_CHANNELS],
                    scratch.is_some(),
                )?);
            }
            let snapshot_slot = if layer.copy_background || needs_crop || is_text {
                let slot_texture = self.renderer.create_view_slot()?;
                self.textures.push(slot_texture);
                Some(self.textures.len() - 1)
            } else {
                None
            };
            let atlas = layer
                .material
                .is_none()
                .then(|| layer.atlas_frames())
                .flatten()
                .map(|frames| frames[0]);
            let (base_texture, base_uv, base_axes) = match snapshot_slot {
                Some(slot) if layer.copy_background => (slot, [0.0, 0.0, 1.0, 1.0], None),
                _ => (
                    skin.map_or(input.layer_slot, |(source, _)| source),
                    atlas.map_or([0.0, 0.0, uvw, uvh], |frame| frame.uv),
                    atlas.map(|frame| frame.axes),
                ),
            };
            let full_rect = [fx_w as f32 / 2.0, fx_h as f32 / 2.0, fx_w as f32, fx_h as f32];
            let base_quad = vk::SceneQuad {
                uv_axes: base_axes,
                uv_padding: layer.pad_uv && base_axes.is_some(),
                projection: None,
                order_bias: 0,
                rect: full_rect,
                uv: base_uv,
                tint: if layer.passthrough {
                    [f32::from(layer.copy_background); RGBA_CHANNELS]
                } else if skin.is_none() && (layer.material.is_some() || layer.base_scene_input) {
                    [1.0; RGBA_CHANNELS]
                } else {
                    [layer.color[0], layer.color[1], layer.color[2], layer.alpha]
                },
                angle: 0.0,
                texture: base_texture,
                blend: vk::SceneBlend::Copy,
            };
            let snapshot_quad = snapshot_slot.map(|slot| vk::SceneQuad {
                uv_axes: None,
                uv_padding: false,
                projection: None,
                order_bias: 0,
                rect: full_rect,
                uv: [0.0, 0.0, 1.0, 1.0],
                tint: [1.0, 1.0, 1.0, 1.0],
                angle: 0.0,
                texture: slot,
                blend: vk::SceneBlend::Copy,
            });
            let crop_quad = snapshot_quad.clone().filter(|_| needs_crop);
            let text_backdrop = snapshot_quad.filter(|_| is_text).map(|mut quad| {
                quad.blend = vk::SceneBlend::CopyRgb;
                quad
            });
            let mut slot_texture = self.renderer.create_view_slot()?;
            if let Some(mesh) = input.mesh {
                slot_texture.use_scene_mesh(mesh);
            }
            let slot = self.textures.len();
            self.textures.push(slot_texture);
            let source_keyframes = input.source_keyframes;
            let source_dynamic = material_dynamic
                || layer.material.as_ref().is_some_and(|material| {
                    let meta = paper_scene::effects::PassMeta::of(&material.pass);
                    meta.time_dependent() || meta.audio_dependent() || meta.pointer_dependent()
                })
                || layer.texture.system_texture.is_some()
                || input.text_prefix_dynamic
                || self.scripted
                || layer.texture.video.is_some()
                || inputs.iter().flatten().any(|slot| {
                    self.interner.video_slots.values().any(|video_slot| video_slot == slot)
                        || self.interner.media_slots.values().any(|media_slot| media_slot == slot)
                })
                || atlas.is_some();
            let effects: Vec<_> = layer.effects.iter().map(|effect| effect.state).collect();
            let active_counts = active_effect_counts(&pass_owner, &swap_owners, &effects);
            let mut chain = std::mem::take(&mut staged_chain).into_iter();
            Ok(LayerFx {
                layer_id: layer.id.clone(),
                quad: index,
                scene_order: layer.scene_order,
                slot,
                pipelines: std::mem::take(&mut pipelines),
                ping: chain.next().expect("ping target"),
                pong: chain.next().expect("pong target"),
                base_quad,
                text_backdrop,
                verts: staged_vertices.take(),
                presentation: None,
                presentation_inputs: Vec::new(),
                inputs,
                remap: (0..fbos.len()).map(std::cell::Cell::new).collect(),
                fbos: fbos
                    .iter()
                    .map(|(name, _)| name.clone())
                    .zip(std::mem::take(&mut staged_fbos))
                    .collect(),
                fbo_owners,
                swaps,
                pass_targets,
                binds: pass_binds,
                owner: pass_owner,
                effects,
                active_counts,
                reset_targets: false,
                fbo_clears,
                fbo_specs,
                clear_functions: layer
                    .effects
                    .iter()
                    .map(|effect| effect.clear_functions.clone())
                    .collect(),
                pending_clears: Vec::new(),
                swap_owners,
                material_values: vec![BTreeMap::new(); metas.len()],
                passes: metas,
                output: None,
                source_dynamic,
                audio_dependent,
                source_clock: layer.live_text.is_some() || input.text_prefix_clock,
                source_keyframes,
                source_pointer: layer
                    .effects
                    .iter()
                    .flat_map(|effect| &effect.passes)
                    .any(|pass| paper_scene::effects::PassMeta::of(pass).pointer_dependent())
                    || (needs_crop || layer.passthrough || is_text) && self.pointer_scene,
                dependency_dynamic: false,
                retain_targets: false,
                starved: None,
                parked: None,
                snapshot: false,
                passthrough: layer.passthrough,
                copy_background: layer.copy_background,
                base_scene_input: layer.base_scene_input,
                base_blend: base_scene_blend(layer),
                bare_blend,
                mesh_blend: blend_pass.map(|pass| MeshBlend { pass, image: [image[0], image[1]] }),
                snapshot_slot,
                crop: chain.next(),
                crop_quad,
                fallback_tint: fallback_tint(layer),
                sampled_targets: BTreeSet::new(),
                uniforms: layer_uniforms(layer, canvas, self.scene_lights, self.world),
            })
        })();
        match built {
            Ok(fx) => {
                if skin.is_some()
                    && let Some(material) =
                        self.materials.iter_mut().find(|material| !mirror && material.quad == index)
                {
                    material.prepare_effect((fx_w, fx_h));
                }
                Ok(Some(fx))
            }
            Err(error) => {
                if intrinsic_dynamic && layer.visible {
                    (*self.dynamic_chains) = (*self.dynamic_chains).saturating_sub(1);
                }
                if let Some(material) =
                    self.materials.iter_mut().find(|material| !mirror && material.quad == index)
                {
                    material.effect_target(self.renderer, self.textures, None, self.canvas);
                }
                for pipeline in pipelines {
                    self.renderer.destroy_effect_pipeline(pipeline);
                }
                for target in staged_fbos
                    .iter_mut()
                    .chain(&mut staged_chain)
                    .filter_map(FxTargetStorage::take_owned)
                {
                    self.renderer.destroy_scene_target(target);
                }
                if let Some(vertices) = staged_vertices {
                    self.renderer.destroy_quad_buffer(vertices);
                }
                self.rollback_uploads(first_texture);
                let reason = format!("{error:#}");
                tracing::info!("skwd-wall-vk: effect skipped on {} ({reason})", layer.name);
                self.quads[index].tint = fallback_tint(layer);
                if let Some(blend) = chainless_blend(base_scene_blend(layer), bare_blend) {
                    self.quads[index].blend = blend;
                }
                self.skip(&layer.name, &reason)?;
                Ok(None)
            }
        }
    }
}
