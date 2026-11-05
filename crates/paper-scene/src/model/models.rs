use super::{ImageResources, MESH_FIELD, MeshLayer, mesh};
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::rc::Rc;

const SKELETAL_MODEL_GAP: &str = "dynamic skeletal model unsupported";
const MOUNTED_MODEL_GAP: &str = "dynamic mounted model unsupported";

pub struct PreparedModel {
    pub layer: MeshLayer,
    pub owner: Option<Rc<mesh::Mesh>>,
}

impl ImageResources {
    pub fn has_models(&self) -> bool {
        !self.models.borrow().is_empty()
            || self.model_bytes.borrow().iter().any(|(owner, _)| owner.strong_count() != 0)
    }

    pub(super) fn check_model_bytes(&self, mut total: usize, extra: usize) -> Result<()> {
        for (_, bytes) in
            self.model_bytes.borrow().iter().filter(|(owner, _)| owner.strong_count() != 0)
        {
            super::add_texture_bytes(&mut total, *bytes)?;
        }
        for bytes in self.model_layers.borrow().values() {
            super::add_texture_bytes(&mut total, *bytes)?;
        }
        super::add_texture_bytes(&mut total, extra)
    }

    pub fn create_model(&self, id: usize, data: mesh::ModelData) -> Result<()> {
        let mesh = data.mesh()?;
        let bytes = mesh
            .parts
            .iter()
            .map(|part| part.vertices.len() * size_of::<mesh::Vertex>() + part.indices.byte_len())
            .sum();
        self.check_model_bytes(self.bytes.get(), bytes)?;
        let mut live = self.model_bytes.borrow_mut();
        live.retain(|(owner, _)| owner.strong_count() != 0);
        let mesh = Rc::new(mesh);
        let mut models = self.models.borrow_mut();
        ensure!(!models.contains_key(&id), "model handle already exists");
        live.push((Rc::downgrade(&mesh), bytes));
        models.insert(id, mesh);
        self.changed();
        Ok(())
    }

    pub fn destroy_model(&self, id: usize) {
        self.models.borrow_mut().remove(&id);
        self.changed();
    }

    pub fn load_model(&self, scene: &Value, slot: usize) -> Result<PreparedModel> {
        ensure!(
            super::ProjectionMode::of(scene) != super::ProjectionMode::Automatic,
            "procedural model automatic projection unsupported"
        );
        let objects = scene
            .get(super::OBJECTS_FIELD)
            .and_then(Value::as_array)
            .context("model objects missing")?;
        ensure!(objects.len() <= super::MAX_SCENE_OBJECTS, "model object budget exceeded");
        let object = objects.get(slot).context("model object missing")?;
        let by_id: std::collections::HashMap<_, _> = objects
            .iter()
            .filter_map(|object| {
                Some((object.get(super::ID_FIELD).and_then(super::id_of)?, object))
            })
            .collect();
        let source = super::MeshObject {
            pkg: &self.package,
            assets: &self.assets,
            object,
            by_id: &by_id,
            id: object.get(super::ID_FIELD).and_then(super::id_of).context("model id missing")?,
            name: object
                .get(super::NAME_FIELD)
                .and_then(Value::as_str)
                .unwrap_or(MESH_FIELD)
                .into(),
            scene_order: slot,
        };
        let (layer, owner) = if object.get(MESH_FIELD).is_some_and(Value::is_string) {
            (super::mesh_layer(source).map_err(anyhow::Error::msg)?, None)
        } else {
            let handle =
                object.get(MESH_FIELD).and_then(Value::as_u64).context("model handle missing")?;
            let owner = self
                .models
                .borrow()
                .get(&usize::try_from(handle)?)
                .cloned()
                .context("model handle unavailable")?;
            let layer = super::build_mesh_layer(source, (*owner).clone(), MESH_FIELD)
                .map_err(anyhow::Error::msg)?;
            (layer, Some(owner))
        };
        ensure!(layer.mesh.rig.is_none(), SKELETAL_MODEL_GAP);
        ensure!(layer.mount.is_none(), MOUNTED_MODEL_GAP);
        let materials = &layer.materials;
        ensure!(
            materials.iter().all(|material| material.pass.binds.iter().all(|(_, bind)| matches!(
                bind,
                crate::effects::EffectBind::Reflection | crate::effects::EffectBind::SceneSoFar
            )) && material.pass.textures.iter().flatten().all(
                |texture| texture.video.is_none()
                    && texture.system_texture.is_none()
                    && texture.frames.is_empty()
                    && texture.pages.is_empty()
            )),
            "dynamic model material unsupported"
        );
        let mut bytes = 0;
        for part in &layer.mesh.parts {
            super::add_texture_bytes(
                &mut bytes,
                part.vertices.len() * size_of::<mesh::Vertex>() + part.indices.byte_len(),
            )?;
        }
        for texture in materials.iter().flat_map(|material| material.pass.textures.iter().flatten())
        {
            super::add_texture_bytes(&mut bytes, texture.payload_bytes())?;
        }
        self.check_model_bytes(self.bytes.get(), bytes)?;
        ensure!(!self.model_layers.borrow().contains_key(&slot), "model layer already reserved");
        self.model_layers.borrow_mut().insert(slot, bytes);
        self.changed();
        Ok(PreparedModel { layer, owner })
    }
}
