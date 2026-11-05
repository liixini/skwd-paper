use super::super::{TextureInterner, mesh, vk};
use super::{Group, Scripts};
use anyhow::{Context, Result};
use paper_scene::model::{PreparedModel, ProjectionMode};
use std::collections::HashMap;

pub(super) struct Model {
    pub quads: Vec<usize>,
    textures: Vec<usize>,
    depth: bool,
    actors: Vec<mesh::MeshActor>,
    owner: Option<std::rc::Rc<paper_scene::model::mesh::Mesh>>,
}

impl Scripts {
    pub(in crate::app::scene) fn shadow_meshes_mut(
        &mut self,
    ) -> impl Iterator<Item = &mut mesh::MeshActor> {
        self.models.values_mut().flat_map(|model| &mut model.actors)
    }
    pub(in crate::app::scene) fn shadow_meshes(&self) -> impl Iterator<Item = &mesh::MeshActor> {
        self.models.values().flat_map(|model| &model.actors)
    }
}

const RUNTIME_REFLECTION_GAP: &str = "reflection read only after startup leaves layers out";

fn present_actor(
    actor: &mut mesh::MeshActor,
    renderer: &vk::Renderer,
    textures: &mut [vk::SceneTexture],
    [flat, deep]: [&mesh::View; 2],
    reflection: &mut Option<mesh::Reflection>,
) -> Result<()> {
    if actor.visible {
        let view = if actor.perspective { deep } else { flat };
        actor.camera(renderer, &mut textures[actor.texture], view)?;
    }
    if let Some(reflection) = reflection {
        reflection.ready &= !actor.mirror(renderer, textures, reflection)?;
    }
    Ok(())
}

impl Group {
    fn model_viewport(&self) -> Result<Option<vk::SceneViewport>> {
        Ok(if self.mesh_settings.projection == ProjectionMode::Orthographic {
            Some(
                self.target.viewport.unwrap_or(
                    vk::SceneViewport::cover(
                        [self.canvas.0, self.canvas.1],
                        [self.target.extent.width, self.target.extent.height],
                    )
                    .context("model viewport invalid")?,
                ),
            )
        } else {
            None
        })
    }

    fn enable_model_depth(&mut self) -> Result<()> {
        self.target.enable_depth(&mut self.renderer, vk::DepthOrder::Greater)?;
        if let Some(target) = &mut self.scene_snapshot {
            self.renderer.enable_scene_depth(target, vk::DepthOrder::Greater)?;
        }
        self.dynamic_depth = true;
        Ok(())
    }

    pub(super) fn retire_model_depth(&mut self, scripts: &Scripts) -> Result<()> {
        if self.mesh_settings.depth || scripts.models.values().any(|model| model.depth) {
            return Ok(());
        }
        self.target.disable_depth(&mut self.renderer)?;
        if let Some(target) = &mut self.scene_snapshot {
            self.renderer.disable_scene_depth(target)?;
        }
        self.dynamic_depth = false;
        Ok(())
    }

    pub(super) fn mesh_camera(&mut self, scripts: &mut Scripts) -> Result<()> {
        if self.meshes.is_empty() && scripts.models.is_empty() {
            return Ok(());
        }
        let [flat, deep] = self.mesh_views()?;
        let source = match &scripts.host {
            Some(host) => Some((&host.scene, &host.properties)),
            None => scripts.native.as_ref().map(|(scene, properties)| (scene, properties)),
        };
        let Some((scene, properties)) = source else {
            return Ok(());
        };
        let objects = super::effect_objects(scene);
        let attachments = super::puppet_attachments(&self.puppets);
        let mut lights =
            paper_scene::model::lighting::uniforms(scene, properties, &attachments, &self.sun_view);
        lights.extend(self.cookie_layout.uniforms(&self.lights));
        for actor in self
            .meshes
            .iter_mut()
            .chain(scripts.models.values_mut().flat_map(|model| &mut model.actors))
        {
            if let Some(object) = objects.get(actor.id.as_str()) {
                let posed =
                    paper_scene::model::mesh_pose(object, properties, &actor.id).and_then(|pose| {
                        let parents = paper_scene::model::mesh_parents(
                            object, &objects, properties, &actor.id,
                        )?;
                        actor.pose(pose, &parents.poses);
                        Ok(())
                    });
                let degenerate = match posed {
                    Ok(()) => false,
                    Err(paper_scene::model::PoseError::Degenerate(_)) => true,
                    Err(error) => return Err(anyhow::Error::msg(error)),
                };
                let visible =
                    !degenerate && paper_scene::model::object_visible(object, &objects, properties);
                actor.cast_shadow = paper_scene::model::lighting::casts_shadow(object, properties);
                actor.perspective = paper_scene::model::mesh_perspective(object, properties);
                actor.set_visible(visible, &mut self.quads);
            }
            if actor.visible {
                actor.lighting(&lights);
            }
            present_actor(
                actor,
                &self.renderer,
                &mut self.textures,
                [&flat, &deep],
                &mut self.reflection,
            )?;
        }
        Ok(())
    }

    fn mesh_views(&mut self) -> Result<[mesh::View; 2]> {
        let viewport = self.model_viewport()?;
        self.renderer.wait_render()?;
        let clip = self.clip;
        Ok([
            self.mesh_settings.view(self.canvas, viewport, clip, false)?,
            self.mesh_settings.view(self.canvas, viewport, clip, true)?,
        ])
    }

    pub(in crate::app::scene) fn refresh_mesh_views(&mut self) -> Result<()> {
        if self.meshes.is_empty()
            && self.scripts.as_ref().is_none_or(|scripts| scripts.models.is_empty())
        {
            return Ok(());
        }
        let [flat, deep] = self.mesh_views()?;
        for actor in self
            .meshes
            .iter_mut()
            .chain(self.scripts.iter_mut().flat_map(Scripts::shadow_meshes_mut))
        {
            present_actor(
                actor,
                &self.renderer,
                &mut self.textures,
                [&flat, &deep],
                &mut self.reflection,
            )?;
        }
        Ok(())
    }

    pub(super) fn retire_models(&mut self, scripts: &mut Scripts, retired: &[usize]) {
        for slot in retired {
            let Some(model) = scripts.models.remove(slot) else {
                continue;
            };
            super::images::release_order(&mut scripts.order, *slot);
            if let Some(shadows) = &mut self.shadows {
                shadows.invalidate();
            }
            for quad in model.quads {
                self.quads[quad].tint.fill(0.0);
                scripts.spare_model_quads.push(quad);
            }
            for texture in model.textures {
                let previous =
                    std::mem::replace(&mut self.textures[texture], vk::SceneTexture::vacant());
                self.renderer.destroy_scene_texture(previous);
                scripts.spare_model_textures.push(texture);
            }
            if let Some(reflection) = &mut self.reflection {
                reflection.retire(&model.actors);
            }
            drop(model.actors);
            drop(model.owner);
        }
    }

    fn model_reflection(&mut self, layer: &paper_scene::model::MeshLayer) -> Result<()> {
        let passes = || layer.materials.iter().map(|material| &material.pass);
        let sampled = passes().any(paper_scene::effects::EffectPass::samples_reflection);
        let was_sampled = self.reflection.as_ref().is_some_and(|reflection| reflection.sampled);
        if let Some(reflection) = &mut self.reflection {
            if sampled {
                reflection.activate(&mut self.renderer, &self.target)?;
            }
        } else if passes().any(paper_scene::model::pass_reflects) {
            self.reflection =
                Some(mesh::Reflection::new(&mut self.renderer, &self.target, sampled)?);
        }
        if sampled && !was_sampled {
            let missing = self.unmirrored_layers();
            if missing > 0 {
                tracing::warn!(
                    "skwd-wall-vk: [native-scene-gap:{RUNTIME_REFLECTION_GAP}] {missing} layers"
                );
            }
        }
        Ok(())
    }

    pub(in crate::app::scene) fn unmirrored_layers(&self) -> usize {
        let materials = self.materials.iter().filter(|material| {
            material.mirror_slot().is_none()
                && !self.reflection_excluded.contains(&material.quad)
                && self.fx.iter().all(|fx| fx.quad != material.quad)
        });
        let particles = self.particles.iter().filter(|group| {
            !self.unreflected.contains(&group.id)
                && group.engines.iter().all(|engine| engine.mirror.is_none())
        });
        materials.count() + particles.count()
    }

    pub(super) fn insert_model(
        &mut self,
        scripts: &mut Scripts,
        slot: usize,
        prepared: PreparedModel,
    ) -> Result<()> {
        let depth = prepared.layer.materials.iter().any(|material| {
            material.test == paper_scene::effects::material::DepthMode::Enabled
                || material.write == paper_scene::effects::material::DepthMode::Enabled
        });
        if depth {
            self.enable_model_depth()?;
        }
        self.model_reflection(&prepared.layer)?;
        let mut interner = TextureInterner {
            device: None,
            media_slots: HashMap::new(),
            videos: Vec::new(),
            video_slots: HashMap::new(),
            slots: HashMap::new(),
            reused: 0,
            reused_bytes: 0,
        };
        let mut textures = Vec::new();
        let mut quads = Vec::new();
        let mut order = Vec::new();
        let viewport = self.model_viewport()?;
        let cookie = self.cookie_sample();
        let actors = mesh::Build {
            cookie,
            shadows: self.shadows.as_ref(),
            history: None,
            renderer: &mut self.renderer,
            interner: &mut interner,
            scene: self.mesh_settings,
            textures: &mut textures,
            quads: &mut quads,
            order: &mut order,
            canvas: self.canvas,
            viewport,
            reflection: self.reflection.as_mut(),
            clip: self.clip,
        }
        .load(vec![prepared.layer]);
        #[cfg(test)]
        let actors = actors.and_then(|actors| {
            super::tests::fault(super::tests::Fault::Model)?;
            Ok(actors)
        });
        let mut actors = match actors {
            Ok(actors) => actors,
            Err(error) => {
                for texture in textures {
                    self.renderer.destroy_scene_texture(texture);
                }
                return Err(error);
            }
        };
        let textures = textures
            .into_iter()
            .map(|texture| {
                if let Some(slot) = scripts.spare_model_textures.pop() {
                    self.textures[slot] = texture;
                    slot
                } else {
                    let slot = self.textures.len();
                    self.textures.push(texture);
                    slot
                }
            })
            .collect::<Vec<_>>();
        for actor in &mut actors {
            actor.texture = textures[actor.texture];
        }
        let fresh: Vec<_> = actors
            .iter()
            .filter_map(mesh::MeshActor::twin)
            .filter_map(|key| {
                let twin = self.reflection.as_mut()?.twins.remove(&key)?;
                Some((key, twin))
            })
            .collect();
        let (rank, remap) = super::images::reserve_order(&mut scripts.order);
        self.remap_scene_order(&remap);
        if let Some(reflection) = &mut self.reflection {
            for (key, mut twin) in fresh {
                twin.quad.texture = textures[twin.quad.texture];
                twin.order = rank;
                reflection.twins.insert(key, twin);
            }
            reflection.ready = false;
        }
        let quads = quads
            .into_iter()
            .map(|mut quad| {
                quad.texture = textures[quad.texture];
                if let Some(slot) = scripts.spare_model_quads.pop() {
                    self.quads[slot] = quad;
                    let old = std::mem::replace(&mut self.quad_scene_order[slot], rank);
                    if !self.quad_scene_order.contains(&old) {
                        scripts.order.retain(|(_, prior)| *prior != old);
                    }
                    slot
                } else {
                    let slot = self.quads.len();
                    self.quads.push(quad);
                    self.quad_scene_order.push(rank);
                    slot
                }
            })
            .collect();
        scripts.order.push((Some(slot), rank));
        scripts
            .models
            .insert(slot, Model { quads, textures, depth, actors, owner: prepared.owner });
        Ok(())
    }
}

#[cfg(test)]
#[path = "models_tests.rs"]
mod tests;
