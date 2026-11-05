use super::{Build, Gpu, PassMeta, vk};
use anyhow::{Context, Result};
use paper_scene::effects::FrameClock;
use paper_scene::puppet::channels;
use std::collections::BTreeMap;

const FORMAT: ash::vk::Format = ash::vk::Format::R8G8B8A8_UNORM;
const COLOR: &str = paper_scene::effects::SOURCE_COLOR_UNIFORM;

pub(super) struct Channels {
    gpu: Gpu,
    pub output: usize,
    meta: PassMeta,
    inputs: Vec<usize>,
    sizes: Vec<Option<[f32; 4]>>,
    quads: [vk::SceneQuad; 2],
    sampled: Option<Vec<f32>>,
    uniform: String,
    dynamic: bool,
}

impl Channels {
    #[expect(clippy::too_many_lines, reason = "one texture channel GPU resource transaction")]
    pub fn new(
        build: &mut Build<'_, '_>,
        mut authored: paper_scene::effects::material::Channels,
        source: usize,
        texture: &paper_scene::model::Texture,
    ) -> Result<Self> {
        let meta = PassMeta::of(&authored.pass);
        let dynamic = authored
            .pass
            .textures
            .iter()
            .flatten()
            .chain(std::iter::once(texture))
            .any(|texture| texture.video.is_some() || texture.system_texture.is_some());
        let uniform = authored
            .pass
            .vertex
            .uniforms
            .iter()
            .find(|uniform| uniform.name.split('[').next() == Some(channels::UNIFORM))
            .context("puppet channel blend uniform missing")?
            .name
            .clone();
        let mut gpu = Gpu {
            device: build.renderer.device.clone(),
            pipelines: Vec::new(),
            mirror: None,
            vertices: None,
            mesh: None,
            target: None,
        };
        gpu.target = Some(build.renderer.create_scene_target_fmt(
            texture.img_width,
            texture.img_height,
            false,
            FORMAT,
        )?);
        gpu.pipelines.push(build.renderer.create_effect_pipeline_for(
            &authored.pass.vertex,
            &authored.pass.fragment,
            authored.pass.hlsl.as_ref(),
            &authored.pass.name,
            vk::EffectPipelineSpec {
                depth_attachment: true,
                format: FORMAT,
                samples: ash::vk::SampleCountFlags::TYPE_1,
                blend: super::pass_blend(authored.pass.blend),
                scene: None,
                geometry: vk::EffectGeometry::Channels { alpha: authored.alpha_write },
                coordinates: vk::EffectCoordinates::Viewport,
                depth: vk::EffectDepth {
                    cull: authored.cull == paper_scene::effects::material::CullMode::Normal,
                    ..vk::EffectDepth::default()
                },
            },
        )?);
        upload(build.renderer, &authored.geometry, &mut gpu)?;
        let mut inputs = Vec::new();
        let mut sizes = Vec::new();
        for texture in &mut authored.pass.textures {
            let texture = texture.as_mut().context("puppet channel texture missing")?;
            sizes.push(Some([
                texture.width as f32,
                texture.height as f32,
                texture.img_width as f32,
                texture.img_height as f32,
            ]));
            inputs.push(build.interner.intern_texture(
                build.renderer,
                build.textures,
                texture,
                vk::MipSampling::Anisotropic,
            )?);
        }
        let (vertices, indices, count) = gpu.mesh.as_ref().expect("channel mesh");
        let effect = vk::SceneEffect::mesh(
            &gpu.pipelines[0],
            vk::MeshGeometry {
                vertices: vertices.buffer,
                indices: indices.buffer,
                index_type: ash::vk::IndexType::UINT16,
                count: *count,
            },
            None,
        )?;
        let target = gpu.target.as_ref().expect("channel target");
        let draw = view(build, target, Some(effect))?;
        let output = view(build, target, None)?;
        let size = [texture.img_width as f32, texture.img_height as f32];
        let (u, v) = texture.uv_scale();
        let base = vk::SceneQuad {
            rect: [size[0] * super::HALF, size[1] * super::HALF, size[0], size[1]],
            uv: [0.0, 0.0, u, v],
            uv_axes: None,
            uv_padding: false,
            tint: [1.0; super::CHANNELS],
            angle: 0.0,
            projection: None,
            order_bias: 0,
            texture: source,
            blend: vk::SceneBlend::Copy,
        };
        let overlay = vk::SceneQuad { texture: draw, ..base.clone() };
        Ok(Self {
            gpu,
            output,
            meta,
            inputs,
            sizes,
            quads: [base, overlay],
            sampled: None,
            uniform,
            dynamic,
        })
    }

    pub fn bytes(&self) -> u64 {
        self.gpu.target.as_ref().map_or(0, |target| target.allocation_bytes)
    }

    pub fn audio_dependent(&self) -> bool {
        self.meta.audio_dependent()
    }

    pub fn frame_dependent(&self) -> bool {
        self.dynamic || self.meta.time_dependent() || self.meta.audio_dependent()
    }

    pub fn render(
        &mut self,
        renderer: &mut vk::Renderer,
        textures: &[vk::SceneTexture],
        clock: FrameClock,
        weights: &[f32],
        batched: bool,
    ) -> Result<()> {
        if !self.frame_dependent() && self.sampled.as_deref() == Some(weights) {
            return Ok(());
        }
        let target = self.gpu.target.as_ref().expect("channel target");
        let pipe = &self.gpu.pipelines[0];
        let extent = target.extent;
        let canvas = (extent.width as f32, extent.height as f32);
        let values = BTreeMap::from([
            (self.uniform.clone(), weights.to_vec()),
            (super::FX_MODEL_PROJECTION.into(), super::scene_view_projection(canvas).to_vec()),
            (COLOR.into(), vec![1.0; super::CHANNELS]),
        ]);
        let bytes = self.meta.uniform_bytes_with(
            clock,
            None,
            extent.width,
            extent.height,
            (extent.width, extent.height),
            &self.sizes,
            &values,
            pipe.hlsl && vk::d3d_clip_enabled(),
        );
        let inputs = self
            .inputs
            .iter()
            .map(|index| {
                let texture = &textures[*index];
                (texture.view, texture.sampler)
            })
            .collect::<Vec<_>>();
        renderer.write_effect_inputs(pipe, &inputs, &bytes);
        if batched {
            renderer.record_scene_with_canvas(
                target,
                [canvas.0, canvas.1],
                [0.0; super::CHANNELS],
                &self.quads,
                textures,
                &[],
            );
        } else {
            renderer.render_scene_with_canvas(
                target,
                [canvas.0, canvas.1],
                [0.0; super::CHANNELS],
                &self.quads,
                textures,
                &[],
            )?;
        }
        self.sampled = Some(weights.to_vec());
        Ok(())
    }
}

fn upload(renderer: &vk::Renderer, geometry: &channels::Geometry, gpu: &mut Gpu) -> Result<()> {
    let vertices = renderer
        .create_dyn_buffer(geometry.vertices.len(), ash::vk::BufferUsageFlags::VERTEX_BUFFER)?;
    renderer.write_dyn_buffer(&vertices, &geometry.vertices);
    let bytes = geometry.indices.iter().flat_map(|index| index.to_ne_bytes()).collect::<Vec<_>>();
    let indices =
        renderer.create_dyn_buffer(bytes.len(), ash::vk::BufferUsageFlags::INDEX_BUFFER)?;
    renderer.write_dyn_buffer(&indices, &bytes);
    gpu.mesh = Some((vertices, indices, geometry.indices.len() as u32));
    Ok(())
}

pub(super) fn view(
    build: &mut Build<'_, '_>,
    target: &vk::SceneTarget,
    effect: Option<vk::SceneEffect>,
) -> Result<usize> {
    let mut texture = build.renderer.create_view_slot()?;
    texture.view = target.view;
    texture.sampler = target.sampler;
    texture.extent = target.extent;
    texture.effect = effect;
    build.renderer.point_slot_at(&texture, target.view);
    let slot = build.textures.len();
    build.textures.push(texture);
    Ok(slot)
}

#[cfg(test)]
mod tests;
