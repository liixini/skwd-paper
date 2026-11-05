use super::hidden::HiddenLayers;
use super::{
    Attachments, DEGENERATE_MESH_GAP, ID_FIELD, IMAGE_FIELD, ImageContext, Layer, MESH_FIELD,
    MESH_MORPH_GAP, MeshLayer, MeshObject, PARTICLE_FIELD, ParticleLayer, SOUND_FIELD, TEXT_FIELD,
    VISUAL_FIELDS, authored_visible, charge_particle_textures, id_of, mesh_layer, model_texture,
    script, shape, tex_candidates,
};
use anyhow::Result;
use serde_json::Value;
use std::collections::{BTreeSet, HashSet};

pub(super) struct SceneObjects<'a> {
    pub(super) images: &'a ImageContext<'a>,
    pub(super) particles: &'a crate::particles::LoadContext<'a>,
    pub(super) image_sources: &'a BTreeSet<String>,
    pub(super) parents: &'a HashSet<String>,
}

impl SceneObjects<'_> {
    pub(super) fn deferrable(
        &self,
        hidden: &HiddenLayers,
        object_index: usize,
        object: &Value,
    ) -> bool {
        let id = object_id(object_index, object);
        self.images.scripted
            && hidden.deferrable(object_index)
            && !authored_visible(object, self.images.by_id, &self.images.assets.properties)
            && !self.images.composites.contains(&id)
            && !self.image_sources.contains(&id)
    }

    pub(super) fn texture_floor<'v>(&self, objects: impl Iterator<Item = &'v Value>) -> usize {
        let ImageContext { pkg, assets, .. } = *self.images;
        let mut seen = BTreeSet::new();
        objects
            .filter(|object| object.get(IMAGE_FIELD).and_then(Value::as_str).is_some())
            .filter_map(|object| model_texture(pkg, assets, object))
            .filter(|source| source.user.is_none() && source.media.is_none())
            .filter_map(|source| {
                tex_candidates(&source.path?).iter().find_map(|name| pkg.entry_path(name))
            })
            .filter(|entry| seen.insert(*entry))
            .filter_map(|entry| pkg.find(entry))
            .map(crate::tex::decoded_floor)
            .sum()
    }

    pub(super) fn load(
        &self,
        loaded: &mut Loaded,
        object_index: usize,
        object: &Value,
    ) -> Result<()> {
        let start = loaded.skipped.len();
        let result = self.load_object(loaded, object_index, object);
        loaded.spans.push((object_index, start..loaded.skipped.len()));
        result
    }

    fn load_object(&self, loaded: &mut Loaded, object_index: usize, object: &Value) -> Result<()> {
        let ImageContext { scripted, pkg, assets, by_id, parallax, created, composites, .. } =
            *self.images;
        let props = &assets.properties;
        let id = object_id(object_index, object);
        let name = object.get("name").and_then(Value::as_str).unwrap_or("object").to_string();
        let visible = authored_visible(object, by_id, props);
        if !visible && !scripted && !composites.contains(&id) && !self.image_sources.contains(&id) {
            return Ok(());
        }
        let visual = VISUAL_FIELDS.iter().any(|key| object.get(key).is_some());
        loaded.background_only &=
            !visual && (object.get(SOUND_FIELD).is_some() || self.parents.contains(&id));
        if object.get(MESH_FIELD).and_then(Value::as_str).is_some() {
            if !visible && !scripted {
                loaded.skipped.push(format!("{name}: hidden mesh source unsupported"));
            } else {
                let source = MeshObject {
                    pkg,
                    assets,
                    object,
                    by_id,
                    id,
                    name: name.clone(),
                    scene_order: object_index,
                };
                match mesh_layer(source) {
                    Ok(mesh) if !mesh.visible && !scripted => {
                        loaded.skipped.push(format!("{name}: {DEGENERATE_MESH_GAP}"));
                    }
                    Ok(mut mesh) => {
                        if mesh.mesh.rig.as_ref().is_some_and(|rig| rig.skipped_morphs) {
                            loaded.skipped.push(format!("{name}: {MESH_MORPH_GAP}"));
                        }
                        mesh.visible &= visible;
                        loaded.meshes.push(mesh);
                    }
                    Err(reason) => loaded.skipped.push(format!("{name}: {reason}")),
                }
            }
            return Ok(());
        }
        if object.get(shape::FIELD).is_some() && object.get(IMAGE_FIELD).is_none() {
            if !shape::is_quad(object) {
                loaded.skipped.push(format!("{name}: unsupported shape"));
            } else if let Some(layer) = self.images.load(
                object,
                object_index,
                &mut loaded.skipped,
                &mut loaded.texture_bytes,
            )? {
                loaded.layers.push(layer);
            }
            return Ok(());
        }
        let Some(_) = object.get("image").and_then(Value::as_str) else {
            if let Some(path) = object.get(PARTICLE_FIELD).and_then(Value::as_str) {
                if created.binary_search(&object_index).is_ok() {
                    return Ok(());
                }
                let (system, particle_skips) = crate::particles::load(self.particles, object, path);
                let unexplained = particle_skips.is_empty();
                loaded
                    .skipped
                    .extend(particle_skips.into_iter().map(|skip| format!("{name}: {skip}")));
                match system {
                    Some(mut system) => {
                        let state = script::particle_frame(
                            object,
                            by_id,
                            props,
                            parallax,
                            &Attachments::new(),
                        );
                        state.apply(&mut system);
                        if system.validate_bounds(object) {
                            loaded.skipped.push(format!(
                                "{name}: particle {path}: collisionbounds transform unsupported"
                            ));
                        }
                        charge_particle_textures(assets, &system, &mut loaded.texture_bytes)?;
                        loaded.particles.push(ParticleLayer {
                            parallax: state.parallax,
                            id: id.clone(),
                            visible,
                            depth: system.origin.2,
                            scene_order: object_index,
                            system,
                        });
                    }
                    None if unexplained => {
                        loaded.skipped.push(format!("{name}: particle {path} unsupported"));
                    }
                    None => {}
                }
            } else if object.get(TEXT_FIELD).is_some()
                && let Some(layer) = self.images.text(
                    object,
                    object_index,
                    &mut loaded.skipped,
                    &mut loaded.texture_bytes,
                )?
            {
                loaded.layers.push(layer);
            }
            return Ok(());
        };
        if let Some(layer) = self.images.load(
            object,
            object_index,
            &mut loaded.skipped,
            &mut loaded.texture_bytes,
        )? {
            loaded.layers.push(layer);
        }
        Ok(())
    }
}

pub(super) struct Loaded {
    pub(super) layers: Vec<Layer>,
    pub(super) meshes: Vec<MeshLayer>,
    pub(super) particles: Vec<ParticleLayer>,
    base_skips: usize,
    skipped: Vec<String>,
    spans: Vec<(usize, std::ops::Range<usize>)>,
    pub(super) texture_bytes: usize,
    pub(super) background_only: bool,
}

#[derive(Clone, Copy)]
pub(super) struct LoadedMark {
    layers: usize,
    meshes: usize,
    particles: usize,
    skipped: usize,
    spans: usize,
    texture_bytes: usize,
    background_only: bool,
}

impl Loaded {
    pub(super) fn new(skipped: Vec<String>, background_only: bool) -> Self {
        Self {
            layers: Vec::new(),
            meshes: Vec::new(),
            particles: Vec::new(),
            base_skips: skipped.len(),
            skipped,
            spans: Vec::new(),
            texture_bytes: 0,
            background_only,
        }
    }

    pub(super) fn mark(&self) -> LoadedMark {
        LoadedMark {
            layers: self.layers.len(),
            meshes: self.meshes.len(),
            particles: self.particles.len(),
            skipped: self.skipped.len(),
            spans: self.spans.len(),
            texture_bytes: self.texture_bytes,
            background_only: self.background_only,
        }
    }

    pub(super) fn rollback(&mut self, mark: LoadedMark) {
        self.layers.truncate(mark.layers);
        self.meshes.truncate(mark.meshes);
        self.particles.truncate(mark.particles);
        self.skipped.truncate(mark.skipped);
        self.spans.truncate(mark.spans);
        self.texture_bytes = mark.texture_bytes;
        self.background_only = mark.background_only;
    }

    pub(super) fn ordered_skips(&mut self) -> Vec<String> {
        self.spans.sort_by_key(|(object_index, _)| *object_index);
        let mut skipped = self.skipped[..self.base_skips].to_vec();
        for (_, span) in &self.spans {
            skipped.extend_from_slice(&self.skipped[span.clone()]);
        }
        skipped
    }
}

pub(super) fn object_id(index: usize, object: &Value) -> String {
    object.get(ID_FIELD).and_then(id_of).unwrap_or_else(|| format!("@object-{index}"))
}
