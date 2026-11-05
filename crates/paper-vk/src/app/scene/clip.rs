use crate::vk;
use anyhow::Result;
use paper_scene::model::{Puppet, SceneModel};
use paper_scene::puppet::Mesh;
use paper_scene::puppet::draw_order::Sorter;
use paper_scene::puppet::masks::Plan;

pub(super) struct Clipping {
    extent: ash::vk::Extent2D,
    nested: bool,
    planes: Option<Option<vk::ClipPlanes>>,
}

impl Clipping {
    pub(super) fn new(model: &SceneModel, extent: ash::vk::Extent2D) -> Self {
        let nested = model
            .layers
            .iter()
            .filter_map(|layer| layer.puppet.as_ref())
            .any(|puppet| !puppet.mask_textures.is_empty() && puppet.mesh.masks.nested());
        Self { extent, nested, planes: None }
    }

    pub(super) fn clip(
        &mut self,
        renderer: &mut vk::Renderer,
        puppet: &Puppet,
        textures: &mut Vec<vk::SceneTexture>,
        order: Option<&Sorter>,
    ) -> Option<vk::PuppetClip> {
        if puppet.mask_textures.is_empty() {
            return None;
        }
        let (extent, nested) = (self.extent, self.nested);
        let planes = self
            .planes
            .get_or_insert_with(|| {
                renderer
                    .create_clip_planes(extent, nested)
                    .inspect_err(|err| {
                        tracing::warn!("skwd-wall-vk: puppet clipping masks unavailable: {err:#}");
                    })
                    .ok()
            })
            .as_ref()?;
        let sources = sources(renderer, puppet, textures)
            .inspect_err(|err| tracing::warn!("skwd-wall-vk: puppet mask textures failed: {err:#}"))
            .ok()?;
        Some(vk::PuppetClip::new(plan(&puppet.mesh, order), sources, planes))
    }

    pub(super) fn into_planes(self) -> Option<vk::ClipPlanes> {
        self.planes.flatten()
    }
}

pub(super) fn plan(mesh: &Mesh, order: Option<&Sorter>) -> Plan {
    let sections = order.map_or_else(|| (0..mesh.masks.spans.len()).collect(), Sorter::sections);
    mesh.masks.plan(&sections, &mesh.indices)
}

pub(super) fn padded(mesh: &Mesh, indices: &[u16]) -> Vec<u16> {
    let mut padded = indices.to_vec();
    padded.resize(mesh.masks.capacity(mesh.indices.len()), 0);
    padded
}

pub(super) fn bytes(indices: &[u16]) -> Vec<u8> {
    indices.iter().flat_map(|index| index.to_ne_bytes()).collect()
}

fn sources(
    renderer: &mut vk::Renderer,
    puppet: &Puppet,
    textures: &mut Vec<vk::SceneTexture>,
) -> Result<Vec<vk::ClipSource>> {
    puppet
        .mask_textures
        .iter()
        .zip(&puppet.mesh.masks.records)
        .map(|(pixels, mask)| {
            let texture = renderer.create_scene_texture_pixels(
                pixels,
                true,
                false,
                vk::MipSampling::Disabled,
            )?;
            let source = vk::ClipSource {
                set: texture.sample_set(),
                additive: mask.additive(),
                inverted: mask.inverted(),
            };
            textures.push(texture);
            Ok(source)
        })
        .collect()
}
