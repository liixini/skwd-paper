use super::mesh::{MATRIX_TRANSLATION_X, MATRIX_TRANSLATION_Y};
use super::{
    CompositeBuffer, EffectBind, FX_MODEL_MATRIX, FX_MODEL_PROJECTION, FxTargetId, Group,
    MATRIX_COLUMN_COMPONENTS, Mat4, PresentView, active_effect_counts, background_projection,
    camera_projection, flip_clip_y, last_effect_pass, layer_puppet, layer_view, mat4_mul,
    pixel_means, present_uniforms, scene_view_projection, size4, vk,
};
use anyhow::{Context, Result};

pub(super) struct FxChain<'a> {
    pub index: usize,
    pub mirror: bool,
    pub hidden_text: bool,
    pub snapshot: Option<(ash::vk::ImageView, ash::vk::Sampler, ash::vk::Extent2D)>,
    pub clock: paper_scene::effects::FrameClock,
    pub screen: (u32, u32),
    pub canvas: [f32; 2],
    pub debug_fx: bool,
    pub scene_targets: Option<&'a mut super::mesh::CompositeTargets>,
}

impl Group {
    #[expect(clippy::too_many_lines, reason = "one effect chain recording transaction")]
    pub(super) fn run_fx_chain(&mut self, chain: FxChain<'_>) -> Result<()> {
        let FxChain {
            index,
            mirror,
            hidden_text,
            snapshot,
            clock,
            screen,
            canvas: logical_canvas,
            debug_fx,
            scene_targets,
        } = chain;
        let ndc_quad = self.ndc_quad.as_ref().context("ndc quad missing")?;
        let ndc_quad_d3d = self.ndc_quad_d3d.as_ref().context("ndc quad missing")?;
        let fx_scratch = &self.fx_scratch;
        let cookie_sample = self.cookie_sample();
        let fx_quad = if mirror { self.mirror_fx[index].quad } else { self.fx[index].quad };
        let quad = if mirror && !self.screen_quads.contains(&fx_quad) {
            let quad = self.view_quad(fx_quad);
            vk::SceneQuad { projection: Some(self.mirrored_rows(fx_quad, &quad)), ..quad }
        } else {
            self.view_quad(fx_quad)
        };
        let projection = background_projection(&quad, self.canvas);
        let clip = self.clip;
        let present_view = |quad: usize, d3d_clip: bool| PresentView {
            canvas: self.canvas,
            camera: self.camera.filter(|_| !self.screen_quads.contains(&quad)),
            view: layer_view(quad, self.bloom.quad, &self.screen_quads, clip),
            d3d_clip,
        };
        let fx = if mirror { &mut self.mirror_fx[index] } else { &mut self.fx[index] };
        if fx.copy_background {
            fx.base_quad.projection = Some(projection);
        }
        if let Some(backdrop) = &mut fx.text_backdrop {
            backdrop.projection = Some(projection);
        }
        if let Some(crop_quad) = &mut fx.crop_quad {
            crop_quad.projection = Some(projection);
        }
        let active_counts = active_effect_counts(&fx.owner, &fx.swap_owners, &fx.effects);
        if fx.reset_targets || active_counts != fx.active_counts {
            for ((_, target), clear) in fx.fbos.iter().zip(&fx.fbo_clears) {
                if let Some(clear) = clear {
                    self.renderer.record_scene(target.get(fx_scratch), *clear, &[], &self.textures);
                }
            }
            fx.active_counts = active_counts;
            fx.reset_targets = false;
        }
        for &(index, clear) in &fx.pending_clears {
            self.renderer.record_scene(
                fx.target(FxTargetId::Fbo(index), fx_scratch),
                clear,
                &[],
                &self.textures,
            );
        }
        fx.pending_clears.clear();
        let ping = fx.ping.get(fx_scratch);
        let pong = fx.pong.get(fx_scratch);
        if let (Some(crop), Some(crop_quad)) = (&fx.crop, &fx.crop_quad) {
            let crop = crop.get(fx_scratch);
            let quad = std::slice::from_ref(crop_quad);
            if debug_fx {
                self.renderer.render_scene(crop, [0.0; 4], quad, &self.textures)?;
            } else {
                self.renderer.record_scene(crop, [0.0; 4], quad, &self.textures);
            }
        }
        let base_quads = [
            fx.text_backdrop.clone().unwrap_or_else(|| fx.base_quad.clone()),
            fx.base_quad.clone(),
        ];
        let base_quads = &base_quads[..1 + usize::from(fx.text_backdrop.is_some())];
        if debug_fx {
            self.renderer.render_scene(pong, [0.0, 0.0, 0.0, 0.0], base_quads, &self.textures)?;
        } else {
            self.renderer.record_scene(pong, [0.0, 0.0, 0.0, 0.0], base_quads, &self.textures);
        }
        if debug_fx && let Ok((w, h, px)) = self.renderer.read_scene_target(pong) {
            let (color, alpha) = pixel_means(&px);
            let mid = ((h / 2) * w + w / 2) as usize * 4;
            tracing::info!(
                "FXDBG primed {w}x{h} mean={} a={} mid={:?}",
                color,
                alpha,
                &px[mid..mid + 4]
            );
        }
        let mut previous = (pong.view, pong.sampler, pong.extent);
        let mut source = previous;
        let mut source_id = FxTargetId::Pong;
        let mut flip = true;
        let mut current_effect = fx.owner.first().copied().unwrap_or(0);
        let final_pass = last_effect_pass(&fx.owner, &fx.effects);
        fx.presentation = None;
        fx.presentation_inputs.clear();
        self.textures[fx.slot].effect = None;
        fx.apply_swaps(None);
        for slot in 0..fx.pipelines.len() {
            let pipe = &fx.pipelines[slot];
            if fx.owner.get(slot).copied().unwrap_or(0) != current_effect {
                current_effect = fx.owner.get(slot).copied().unwrap_or(0);
                previous = source;
            }
            if !fx.effects[fx.owner[slot]].visible {
                fx.apply_swaps(Some(slot));
                continue;
            }
            let named_id = fx.pass_targets[slot]
                .as_ref()
                .and_then(|name| fx.resolve_fbo(fx.owner[slot], name))
                .map(FxTargetId::Fbo);
            let final_owner = final_pass.is_some_and(|last| fx.owner[last] == fx.owner[slot]);
            if named_id.is_some()
                && final_owner
                && Some(slot) != final_pass
                && !fx.passthrough
                && !fx.copy_background
            {
                let mut model: Mat4 =
                    fx.uniforms[FX_MODEL_MATRIX].as_slice().try_into().expect("model matrix");
                let d3d_clip = pipe.hlsl && vk::d3d_clip_enabled();
                let camera = self.camera.filter(|_| !self.screen_quads.contains(&fx.quad));
                let projection = if let Some(camera) = camera {
                    let view = layer_view(fx.quad, self.bloom.quad, &self.screen_quads, clip);
                    if mirror {
                        flip_clip_y(&mut model);
                    }
                    let mut projection =
                        camera_projection(camera, self.canvas, &model, d3d_clip, view);
                    if mirror {
                        flip_clip_y(&mut projection);
                    }
                    projection
                } else if mirror {
                    model[MATRIX_TRANSLATION_X] = quad.rect[0];
                    model[MATRIX_TRANSLATION_Y] = self.canvas.1 - quad.rect[1];
                    flip_clip_y(&mut model);
                    let mut projection = mat4_mul(&scene_view_projection(self.canvas), &model);
                    if d3d_clip {
                        flip_clip_y(&mut projection);
                    }
                    projection
                } else {
                    let mut projection = mat4_mul(&scene_view_projection(self.canvas), &model);
                    if !d3d_clip {
                        flip_clip_y(&mut projection);
                    }
                    vk::EffectProjection::for_layer(projection, model, self.canvas, 0).at(
                        [quad.rect[0], quad.rect[1]],
                        logical_canvas,
                        d3d_clip,
                    )
                };
                fx.uniforms.insert(FX_MODEL_PROJECTION.into(), projection.to_vec());
            } else {
                fx.uniforms.remove(FX_MODEL_PROJECTION);
            }
            let named = named_id.map(|id| fx.target(id, fx_scratch));
            let mut views = vec![(previous.0, previous.1)];
            let mut sizes: Vec<Option<[f32; 4]>> = vec![Some(size4(previous.2))];
            for &texture in &fx.inputs[slot] {
                let texture = &self.textures[texture];
                views.push((texture.view, texture.sampler));
                sizes.push(None);
            }
            for (index, binding) in &fx.binds[slot] {
                let bound = match binding {
                    EffectBind::Previous => Some(previous),
                    EffectBind::Named(name) => fx
                        .resolve_fbo(fx.owner[slot], name)
                        .map(|index| fx.target(FxTargetId::Fbo(index), fx_scratch))
                        .map(|rt| (rt.view, rt.sampler, rt.extent)),
                    EffectBind::LayerComposite { layer, buffer } => scene_targets
                        .as_deref()
                        .and_then(|targets| targets.get(&(layer.clone(), *buffer)))
                        .copied(),
                    EffectBind::SceneSoFar => snapshot,
                    EffectBind::LightCookie => cookie_sample,
                    EffectBind::ShadowAtlas => self.shadows.as_ref().map(|shadows| {
                        let (view, sampler) =
                            self.renderer.effect_depth_sample(pipe, *index, shadows.sample());
                        (view, sampler, shadows.extent())
                    }),
                    EffectBind::Reflection => self.reflection.as_ref().map(|reflection| {
                        let target = reflection.target();
                        (target.view, target.sampler, target.extent)
                    }),
                    EffectBind::SceneHistory => {
                        self.scene_history.as_ref().map(vk::SceneHistory::sample)
                    }
                    EffectBind::SceneUnderLayer => fx.crop.as_ref().map(|storage| {
                        let crop = storage.get(fx_scratch);
                        (crop.view, crop.sampler, crop.extent)
                    }),
                };
                if let Some((view, sampler, extent)) = bound {
                    if *index >= views.len() {
                        views.resize(*index + 1, (view, sampler));
                        sizes.resize(*index + 1, None);
                    }
                    views[*index] = (view, sampler);
                    sizes[*index] = Some(size4(extent));
                }
            }
            if let Some(blend) = fx.mesh_blend.filter(|blend| blend.pass == slot) {
                let grab = self.scene_grab.as_ref().context("puppet blend grab missing")?;
                let backdrop = paper_scene::effects::COLOR_BLEND_BACKDROP_SLOT;
                let slots = views.len().max(backdrop + 1);
                views.resize(slots, (grab.view, grab.sampler));
                sizes.resize(slots, None);
                views[backdrop] = (grab.view, grab.sampler);
                sizes[backdrop] = Some(size4(grab.extent));
                let model = fx.model_matrix();
                let present = present_view(fx.quad, false);
                let mut projection = present.projection(&model);
                for (column, scale) in projection
                    .chunks_exact_mut(MATRIX_COLUMN_COMPONENTS)
                    .zip([blend.image[0], -blend.image[1]])
                {
                    for value in column {
                        *value *= scale;
                    }
                }
                let uniforms = present_uniforms(
                    &mut fx.uniforms,
                    &fx.passes[slot],
                    projection,
                    (clock, screen),
                    &sizes,
                    false,
                );
                self.renderer.write_effect_inputs(pipe, &views, &uniforms);
                let meta = &fx.passes[slot];
                let puppet =
                    layer_puppet(&self.puppets, fx.quad).context("puppet blend mesh missing")?;
                self.textures[fx.slot].effect = Some(vk::SceneEffect::puppet(
                    pipe,
                    &puppet.mesh,
                    meta.matrix_offset(FX_MODEL_PROJECTION)
                        .map(|offset| present.binding(projection, model, offset)),
                    grab.grab(),
                )?);
                fx.uniforms.remove(FX_MODEL_PROJECTION);
                fx.apply_swaps(Some(slot));
                continue;
            }
            let target_id = if let Some(id) = named_id {
                id
            } else {
                let candidate = if flip { FxTargetId::Ping } else { FxTargetId::Pong };
                let candidate_target = fx.target(candidate, fx_scratch);
                if candidate_target.view == previous.0
                    || views.iter().any(|(view, _)| *view == candidate_target.view)
                {
                    flip = !flip;
                    if flip { FxTargetId::Ping } else { FxTargetId::Pong }
                } else {
                    candidate
                }
            };
            let target = fx.target(target_id, fx_scratch);
            let meta = &fx.passes[slot];
            let quad_dims = named.is_none().then_some((ping.extent.width, ping.extent.height));
            let uniforms = meta.uniform_bytes_with(
                clock,
                quad_dims,
                target.extent.width,
                target.extent.height,
                screen,
                &sizes,
                &fx.uniforms,
                pipe.hlsl && vk::d3d_clip_enabled(),
            );
            if debug_fx {
                let ids: Vec<String> = views
                    .iter()
                    .map(|(view, _)| {
                        if *view == ping.view {
                            "PING".into()
                        } else if *view == pong.view {
                            "PONG".into()
                        } else if let Some((name, _)) = fx
                            .fbos
                            .iter()
                            .find(|(_, storage)| storage.get(fx_scratch).view == *view)
                        {
                            name.clone()
                        } else {
                            "tex".into()
                        }
                    })
                    .collect();
                let tgt = if target.view == ping.view {
                    "PING"
                } else if target.view == pong.view {
                    "PONG"
                } else {
                    "FBO"
                };
                tracing::info!(
                    "FXDBG pass{slot} {} hlsl={} views={ids:?} target={tgt}",
                    meta.name,
                    pipe.hlsl
                );
                if let Ok((_, _, px)) = self.renderer.read_scene_target(ping) {
                    let (_, alpha) = pixel_means(&px);
                    tracing::info!("FXDBG   ping now a={alpha}");
                }
            }
            let buffer = if named.is_none() && !meta.ndc() {
                fx.verts.as_ref().expect("effect vertices")
            } else if pipe.hlsl && vk::d3d_clip_enabled() {
                ndc_quad_d3d
            } else {
                ndc_quad
            };
            let draw_target = vk::EffectDraw {
                target,
                preserve: named.is_some(),
                final_vertices: Some(slot) == final_pass && named.is_none() && !meta.ndc(),
            };
            if debug_fx {
                self.renderer.run_effect_pass(pipe, buffer, draw_target, &views, &uniforms)?;
            } else {
                self.renderer.record_effect_pass(pipe, buffer, draw_target, &views, &uniforms)?;
            }
            fx.apply_swaps(Some(slot));
            if debug_fx && let Ok((w, h, px)) = self.renderer.read_scene_target(target) {
                let (color, alpha) = pixel_means(&px);
                let mid = ((h / 2) * w + w / 2) as usize * 4;
                tracing::info!(
                    "FXDBG pass{slot} {w}x{h} mean={} a={} mid={:?}",
                    color,
                    alpha,
                    &px[mid..mid + 4]
                );
            }
            source = (target.view, target.sampler, target.extent);
            source_id = target_id;
            if named.is_none() {
                flip = !flip;
            }
            if Some(slot) == final_pass
                && let Some(scene_pipe) = &pipe.scene
            {
                let model = fx.model_matrix();
                let d3d_clip = scene_pipe.hlsl && vk::d3d_clip_enabled();
                let present = present_view(fx.quad, d3d_clip);
                let projection = present.projection(&model);
                let uniforms = present_uniforms(
                    &mut fx.uniforms,
                    &fx.passes[slot],
                    projection,
                    (clock, screen),
                    &sizes,
                    d3d_clip,
                );
                self.renderer.write_effect_inputs(scene_pipe, &views, &uniforms);
                let meta = &fx.passes[slot];
                fx.presentation_inputs = fx
                    .target_ids()
                    .filter(|id| {
                        views.iter().any(|(view, _)| *view == fx.target(*id, fx_scratch).view)
                    })
                    .collect();
                fx.presentation = Some(slot);
                self.textures[fx.slot].effect = Some(vk::SceneEffect::new(
                    scene_pipe,
                    fx.verts.as_ref().expect("effect vertices"),
                    meta.matrix_offset(FX_MODEL_PROJECTION)
                        .map(|offset| present.binding(projection, model, offset)),
                )?);
                fx.uniforms.remove(FX_MODEL_PROJECTION);
            }
        }
        self.renderer.point_slot_at(&self.textures[fx.slot], source.0);
        if let Some(material) = self.materials.iter().find(|material| {
            !mirror && material.quad == fx.quad && material.skin_source().is_some()
        }) {
            material.finish_effect(
                &self.renderer,
                &mut self.textures,
                fx.slot,
                (source.0, source.1),
            );
        }
        if !hidden_text && !mirror {
            self.quads[fx.quad].texture = fx.slot;
        }
        fx.output = Some(source_id);
        if let Some(scene_targets) = scene_targets {
            for (buffer, target_id) in
                [(CompositeBuffer::A, source_id), (CompositeBuffer::B, FxTargetId::Pong)]
            {
                let target = fx.target(target_id, fx_scratch);
                scene_targets.insert(
                    (fx.layer_id.clone(), buffer),
                    (target.view, target.sampler, target.extent),
                );
            }
        }
        Ok(())
    }
}
