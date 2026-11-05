use super::{Group, Scripts};
use anyhow::Result;
use paper_scene::model::PreparedImage;
use std::collections::HashMap;

pub(super) fn reserve_order(
    order: &mut [(Option<usize>, usize)],
) -> (usize, HashMap<usize, usize>) {
    let rank = order
        .iter()
        .filter(|(slot, _)| slot.is_some())
        .map(|(_, rank)| rank + 1)
        .max()
        .unwrap_or(0);
    let remap = order
        .iter_mut()
        .map(|(_, old)| {
            let next = *old + usize::from(*old >= rank);
            (std::mem::replace(old, next), next)
        })
        .collect();
    (rank, remap)
}

pub(super) fn release_order(order: &mut [(Option<usize>, usize)], slot: usize) {
    if let Some(entry) = order.iter_mut().find(|(owner, _)| *owner == Some(slot)) {
        entry.0 = None;
    }
}

fn retire_order(
    order: &mut Vec<(Option<usize>, usize)>,
    slot: usize,
) -> Option<HashMap<usize, usize>> {
    let index = order.iter().position(|(owner, _)| *owner == Some(slot))?;
    let rank = order.remove(index).1;
    Some(
        order
            .iter_mut()
            .map(|(_, old)| {
                let next = *old - usize::from(*old > rank);
                (std::mem::replace(old, next), next)
            })
            .collect(),
    )
}

impl Group {
    #[expect(
        clippy::too_many_lines,
        reason = "Dynamic scene resource staging and retirement transaction"
    )]
    pub(super) fn image_changes(&mut self, scripts: &mut Scripts) -> Result<bool> {
        let Some(host) = &mut scripts.host else {
            return Ok(false);
        };
        let created = host.take_created_images();
        let retired = host.take_destroyed_images();
        let models = host.take_created_models();
        let particles = host.take_created_particles();
        if created.is_empty() && retired.is_empty() && models.is_empty() && particles.is_empty() {
            host.acknowledge_destroyed(&[]);
            return Ok(false);
        }
        self.renderer.wait_render()?;
        let mut staged = Vec::new();
        let mut accepted = Vec::new();
        for (slot, image) in created {
            if retired.contains(&slot) {
                continue;
            }
            match self.stage_image(&image.layer) {
                Ok((texture, video)) => {
                    accepted.push(slot);
                    staged.push((slot, image, texture, video));
                }
                Err(error) => {
                    if let Err(lost) = scripts.discard_created(slot, error) {
                        for (_, _, texture, _) in staged {
                            self.renderer.destroy_scene_texture(texture);
                        }
                        return Err(lost);
                    }
                }
            }
        }
        let backdrop = if staged.iter().any(|(_, image, _, _)| image.layer.is_text()) {
            match scripts.stage_backdrop(&mut self.renderer) {
                Ok(backdrop) => backdrop,
                Err(error) => {
                    let lost = crate::vk::device_lost(&error);
                    let (text, images): (Vec<_>, Vec<_>) = staged
                        .into_iter()
                        .partition(|(_, image, _, _)| lost || image.layer.is_text());
                    staged = images;
                    for (slot, _, texture, _) in text {
                        self.renderer.destroy_scene_texture(texture);
                        accepted.retain(|accepted| *accepted != slot);
                        if !lost {
                            scripts.discard_created(slot, anyhow::anyhow!("{error:#}"))?;
                        }
                    }
                    if lost {
                        return Err(error);
                    }
                    None
                }
            }
        } else {
            None
        };
        self.retire_models(scripts, &retired);
        for slot in &retired {
            if let Some(id) = scripts.particles.remove(slot) {
                let scripts_media = scripts
                    .host
                    .as_ref()
                    .is_some_and(paper_scene::script::SceneScripts::needs_media);
                self.retire_dynamic_particle(&id, scripts_media);
                if let Some(remap) = retire_order(&mut scripts.order, *slot) {
                    self.remap_scene_order(&remap);
                }
            }
            if let Some((quad, texture)) = scripts.images.remove(slot) {
                release_order(&mut scripts.order, *slot);
                self.animations.retain(|animation| animation.quad != quad);
                scripts.text.retain(|(owner, _, _)| *owner != quad);
                scripts.backdrops.retain(|entry| {
                    let (owner, backdrop) = (entry.layer, entry.quad);
                    if owner != quad {
                        return true;
                    }
                    self.quads[backdrop].tint.fill(0.0);
                    scripts.spare_backdrops.insert(quad, backdrop);
                    false
                });
                self.quads[quad].tint.fill(0.0);
                let previous = std::mem::replace(
                    &mut self.textures[texture],
                    crate::vk::SceneTexture::vacant(),
                );
                self.renderer.destroy_scene_texture(previous);
                self.image_videos.remove(slot);
                scripts.spare_images.push((quad, texture));
            }
        }
        scripts.accept_backdrop(backdrop, &mut self.textures);
        for (slot, image, texture, video) in staged {
            self.insert_image(scripts, slot, image, texture);
            if let Some(video) = video {
                self.image_videos.insert(slot, video);
            }
        }
        for (slot, model) in models {
            if retired.contains(&slot) {
                continue;
            }
            match self.insert_model(scripts, slot, model) {
                Ok(()) => accepted.push(slot),
                Err(error) => scripts.discard_created(slot, error)?,
            }
        }
        for (slot, mut particle) in particles {
            if retired.contains(&slot) {
                continue;
            }
            let (rank, remap) = reserve_order(&mut scripts.order);
            self.remap_scene_order(&remap);
            particle.scene_order = rank;
            let id = particle.id.clone();
            if let Err(error) = self.add_dynamic_particle(particle) {
                let reverse = remap.into_iter().map(|(old, next)| (next, old)).collect();
                self.remap_scene_order(&reverse);
                for (_, current) in &mut scripts.order {
                    *current = reverse[current];
                }
                scripts.discard_created(slot, error)?;
                continue;
            }
            accepted.push(slot);
            scripts.order.push((Some(slot), rank));
            scripts.particles.insert(slot, id);
        }
        if let Err(error) = self.refresh_scene_targets(scripts) {
            if crate::vk::device_lost(&error) {
                return Err(error);
            }
            for slot in accepted {
                scripts.discard_created(slot, anyhow::anyhow!("{error:#}"))?;
            }
        }
        if let Some(host) = &mut scripts.host {
            host.acknowledge_destroyed(&retired);
        }
        scripts.pending = true;
        Ok(true)
    }

    fn refresh_scene_targets(&mut self, scripts: &Scripts) -> Result<()> {
        #[cfg(test)]
        super::tests::fault(super::tests::Fault::Targets)?;
        self.retire_model_depth(scripts)?;
        let plan = self.scene_target_plan(true)?;
        let animation = self.scene_target_plan(false)?;
        self.effects_animated = animation.dynamic.iter().any(|dynamic| *dynamic);
        self.apply_scene_target_plan(&plan)
    }

    fn stage_image(
        &mut self,
        layer: &paper_scene::model::Layer,
    ) -> Result<(crate::vk::SceneTexture, Option<super::super::video::VideoTexture>)> {
        #[cfg(test)]
        super::tests::fault(super::tests::Fault::Image)?;
        if let Some(payload) = &layer.texture.video {
            let mut video = super::super::video::VideoTexture::open(
                self.video_device.shared(),
                payload,
                layer.texture.clamp,
                layer.texture.nearest,
            )?;
            video.start_at(f64::from(self.composed_at));
            let slot = video.view_slot(&mut self.renderer)?;
            return Ok((slot, Some(video)));
        }
        let texture = if layer.is_text() {
            self.renderer.create_text_texture(
                &layer.texture.pixels,
                crate::vk::MipSampling::Disabled,
                layer.texture.text_batch.as_ref(),
            )?
        } else {
            self.renderer.create_scene_texture_pixels(
                &layer.texture.pixels,
                layer.texture.clamp,
                layer.texture.nearest,
                crate::vk::MipSampling::Anisotropic,
            )?
        };
        Ok((texture, None))
    }

    fn insert_image(
        &mut self,
        scripts: &mut Scripts,
        slot: usize,
        image: PreparedImage,
        texture: crate::vk::SceneTexture,
    ) {
        let PreparedImage { mut layer, layout } = image;
        let (rank, remap) = reserve_order(&mut scripts.order);
        self.remap_scene_order(&remap);
        let (quad, texture_slot) = if let Some((quad, texture_slot)) = scripts.spare_images.pop() {
            self.textures[texture_slot] = texture;
            let previous_rank = self.quad_scene_order[quad];
            let entry = scripts.order.iter_mut().find(|(_, rank)| *rank == previous_rank).unwrap();
            *entry = (Some(slot), rank);
            self.quads[quad] = super::super::layer_quad(&layer, texture_slot, self.canvas);
            self.quad_scene_order[quad] = rank;
            if let Some(&backdrop) = scripts.spare_backdrops.get(&quad) {
                self.quad_scene_order[backdrop] = rank;
            }
            scripts.layouts[quad] = layout;
            (quad, texture_slot)
        } else {
            let texture_slot = self.textures.len();
            let quad = scripts.layouts.len();
            self.textures.push(texture);
            for model in scripts.models.values_mut() {
                for index in &mut model.quads {
                    *index += usize::from(*index >= quad);
                }
            }
            for index in &mut scripts.spare_model_quads {
                *index += usize::from(*index >= quad);
            }
            for backdrop in &mut scripts.backdrops {
                backdrop.quad += usize::from(backdrop.quad >= quad);
            }
            for index in scripts.spare_backdrops.values_mut() {
                *index += usize::from(*index >= quad);
            }
            self.quads.insert(quad, super::super::layer_quad(&layer, texture_slot, self.canvas));
            self.quad_scene_order.insert(quad, rank);
            scripts.layouts.push(layout);
            scripts.order.push((Some(slot), rank));
            (quad, texture_slot)
        };
        if paper_scene::model::reflection_gap(&layer) {
            self.reflection_excluded.insert(quad);
        } else {
            self.reflection_excluded.remove(&quad);
        }
        if let Some(animation) =
            super::super::LayerAnimation::new(&layer, quad, Some(slot), texture_slot, None, None)
        {
            let frame = &animation.frames[0];
            self.quads[quad].uv = frame.uv;
            self.quads[quad].uv_axes = Some(frame.axes);
            self.quads[quad].uv_padding = layer.pad_uv;
            self.animations.push(animation);
        }
        if let Some(text) = layer.script_text.take() {
            scripts.text.push((quad, texture_slot, text));
            scripts.install_backdrop(quad, &mut self.quads, &mut self.quad_scene_order);
        }
        self.mouse.set_layer(quad, &layer);
        for fx in &mut self.fx {
            fx.source_keyframes |= fx.copy_background && fx.scene_order > rank;
        }
        scripts.images.insert(slot, (quad, texture_slot));
    }
}

#[cfg(test)]
mod tests {
    use super::{reserve_order, retire_order};

    #[test]
    fn particle_order_reuse() {
        let mut order = vec![(Some(0), 0), (None, 1)];
        for slot in 2..132 {
            let (rank, remap) = reserve_order(&mut order);
            assert_eq!(rank, 1);
            assert_eq!(remap[&1], 2);
            order.push((Some(slot), rank));
            assert_eq!(order, [(Some(0), 0), (None, 2), (Some(slot), 1)]);
            let remap = retire_order(&mut order, slot).unwrap();
            assert_eq!(remap[&2], 1);
            assert_eq!(order, [(Some(0), 0), (None, 1)]);
        }
    }
}
