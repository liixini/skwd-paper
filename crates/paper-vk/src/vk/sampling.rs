use super::{
    EffectDraw, EffectPipeline, EffectPipelineSpec, QuadBuffer, ReadbackBuf, Renderer, SceneBlend,
    SceneTarget,
};
use anyhow::{Result, ensure};
use ash::vk;
use paper_scene::particles::image::{SampleCell, SampleGrid};
use paper_scene::shader::{Stage, Uniform, pack_uniforms, translate, unify_uniforms};
use std::collections::BTreeMap;

const VERTEX: &str = include_str!("../../shaders/particle_sample.vert");
const FRAGMENT: &str = include_str!("../../shaders/particle_sample.frag");
const SOURCE_SIZE: &str = "g_SourceSize";
const HAS_MASK: &str = "g_HasMask";
const LABEL: &str = "particle image sampling";
const FORMAT: vk::Format = vk::Format::R8G8B8A8_UNORM;
const RANGE: vk::ImageSubresourceRange = vk::ImageSubresourceRange {
    aspect_mask: vk::ImageAspectFlags::COLOR,
    base_mip_level: 0,
    level_count: 1,
    base_array_layer: 0,
    layer_count: 1,
};
const LAYERS: vk::ImageSubresourceLayers = vk::ImageSubresourceLayers {
    aspect_mask: vk::ImageAspectFlags::COLOR,
    mip_level: 0,
    base_array_layer: 0,
    layer_count: 1,
};

pub struct Sampling {
    device: ash::Device,
    target: Option<SceneTarget>,
    pipeline: Option<EffectPipeline>,
    quad: Option<QuadBuffer>,
    readback: Option<ReadbackBuf>,
    uniforms: Vec<Uniform>,
    grid: SampleGrid,
    pending: bool,
    completed: bool,
}

impl Sampling {
    pub fn new(renderer: &mut Renderer, grid: SampleGrid) -> Result<Self> {
        let mut sampling = Self {
            device: renderer.device.clone(),
            target: None,
            pipeline: None,
            quad: None,
            readback: None,
            uniforms: Vec::new(),
            grid,
            pending: false,
            completed: false,
        };
        let mut vertex = translate(VERTEX, Stage::Vertex, &BTreeMap::new());
        let mut fragment = translate(FRAGMENT, Stage::Fragment, &BTreeMap::new());
        unify_uniforms(&mut vertex, &mut fragment);
        sampling.pipeline = Some(renderer.create_effect_pipeline_for(
            &vertex,
            &fragment,
            None,
            LABEL,
            EffectPipelineSpec::quad(FORMAT, SceneBlend::Copy, None),
        )?);
        sampling.uniforms = fragment.uniforms;
        sampling.quad = Some(renderer.create_quad_buffer_ndc()?);
        sampling.resize(renderer, grid)?;
        Ok(sampling)
    }

    pub fn resize(&mut self, renderer: &mut Renderer, grid: SampleGrid) -> Result<()> {
        ensure!(!self.pending, "sample readback is pending");
        let target =
            renderer.create_scene_target_fmt(grid.target()[0], grid.target()[1], false, FORMAT)?;
        if let Some(previous) = self.target.replace(target) {
            renderer.destroy_scene_target(previous);
        }
        self.readback = Some(renderer.create_readback_buf(grid.bytes() as u64)?);
        self.grid = grid;
        Ok(())
    }

    pub fn record(
        &mut self,
        renderer: &mut Renderer,
        source: (vk::ImageView, vk::Sampler),
        mask: Option<(vk::ImageView, vk::Sampler)>,
    ) -> Result<()> {
        ensure!(!self.pending, "sample readback is pending");
        let target = self.target.as_ref().expect("sampling target");
        let values = BTreeMap::from([
            (SOURCE_SIZE.to_owned(), self.grid.source().map(|value| value as f32).to_vec()),
            (HAS_MASK.to_owned(), vec![f32::from(u8::from(mask.is_some()))]),
        ]);
        renderer.record_effect_pass(
            self.pipeline.as_ref().expect("sampling pipeline"),
            self.quad.as_ref().expect("sampling quad"),
            EffectDraw { target, preserve: false, final_vertices: false },
            &[source, mask.unwrap_or(source)],
            &pack_uniforms(&self.uniforms, &values),
        )?;
        renderer.record_sample_copy(target, self.readback.as_ref().expect("sampling readback"));
        self.pending = true;
        self.completed = false;
        Ok(())
    }

    pub fn complete_batch(&mut self) {
        self.completed = self.pending;
    }

    pub fn read(&mut self, cells: &mut Vec<SampleCell>) -> Result<bool> {
        if !self.completed {
            return Ok(false);
        }
        let readback = self.readback.as_ref().expect("sampling readback");
        let pixels = unsafe { std::slice::from_raw_parts(readback.ptr, self.grid.bytes()) };
        self.grid.read(pixels, cells)?;
        self.pending = false;
        self.completed = false;
        Ok(true)
    }
}

impl Drop for Sampling {
    fn drop(&mut self) {
        let _ = crate::shared::wait_idle(&self.device);
        if let Some(target) = self.target.take() {
            target.destroy(&self.device);
        }
        if let Some(pipeline) = self.pipeline.take() {
            pipeline.destroy(&self.device);
        }
        if let Some(quad) = self.quad.take() {
            quad.destroy(&self.device);
        }
    }
}

impl Renderer {
    fn record_sample_copy(&mut self, target: &SceneTarget, readback: &ReadbackBuf) {
        unsafe {
            self.device.cmd_pipeline_barrier(
                self.cmd,
                vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[vk::ImageMemoryBarrier::default()
                    .image(target.image)
                    .old_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                    .new_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                    .src_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)
                    .dst_access_mask(vk::AccessFlags::TRANSFER_READ)
                    .subresource_range(RANGE)],
            );
            self.device.cmd_copy_image_to_buffer(
                self.cmd,
                target.image,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                readback.buffer,
                &[vk::BufferImageCopy::default().image_subresource(LAYERS).image_extent(
                    vk::Extent3D {
                        width: target.extent.width,
                        height: target.extent.height,
                        depth: 1,
                    },
                )],
            );
            self.device.cmd_pipeline_barrier(
                self.cmd,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::FRAGMENT_SHADER | vk::PipelineStageFlags::HOST,
                vk::DependencyFlags::empty(),
                &[],
                &[vk::BufferMemoryBarrier::default()
                    .buffer(readback.buffer)
                    .size(readback.size)
                    .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                    .dst_access_mask(vk::AccessFlags::HOST_READ)],
                &[vk::ImageMemoryBarrier::default()
                    .image(target.image)
                    .old_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                    .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                    .src_access_mask(vk::AccessFlags::TRANSFER_READ)
                    .dst_access_mask(vk::AccessFlags::SHADER_READ)
                    .subresource_range(RANGE)],
            );
        }
    }
}

#[cfg(all(test, feature = "shared-device"))]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires a Vulkan device"]
    fn samples_current_views() -> Result<()> {
        let shared = crate::shared::test_device();
        let mut renderer = Renderer::new_shared_headless(
            (
                shared.entry.clone(),
                shared.instance.clone(),
                shared.phys,
                shared.device.clone(),
                shared.gfx_family,
                shared.queue,
            ),
            16,
            8,
        )?;
        renderer.configure_scene(crate::vk::SceneConfig {
            format: vk::Format::R16G16B16A16_SFLOAT,
            depth: None,
        })?;
        renderer.ensure_scene_pipelines()?;
        let red = renderer.create_scene_texture(8, 8, &[[255, 0, 0, 255]; 64].concat())?;
        let green = renderer.create_scene_texture(8, 8, &[[0, 255, 0, 255]; 64].concat())?;
        let wide = renderer.create_scene_texture(16, 8, &[[0, 0, 255, 255]; 128].concat())?;
        let white = renderer.create_scene_texture(1, 1, &[255; 4])?;
        let black = renderer.create_scene_texture(1, 1, &[0, 0, 0, 255])?;
        let mut sampling = Sampling::new(&mut renderer, SampleGrid::new([8, 8]).unwrap())?;
        let mut cells = Vec::new();
        let result = (|| {
            for (source, mask, size, color) in [
                (&red, &white, [8, 8], Some([255, 0, 0])),
                (&green, &white, [8, 8], Some([0, 255, 0])),
                (&green, &black, [8, 8], None),
                (&wide, &white, [16, 8], Some([0, 0, 255])),
            ] {
                if size != sampling.grid.source() {
                    sampling.resize(&mut renderer, SampleGrid::new(size).unwrap())?;
                }
                assert!(!sampling.read(&mut cells)?);
                renderer.begin_scene_batch()?;
                sampling.record(
                    &mut renderer,
                    (source.view, source.sampler),
                    Some((mask.view, mask.sampler)),
                )?;
                renderer.submit_scene_batch()?;
                sampling.complete_batch();
                assert!(sampling.read(&mut cells)?);
                assert_eq!(
                    cells.len(),
                    if color.is_some() {
                        sampling.grid.target().iter().product::<u32>() as usize
                    } else {
                        0
                    }
                );
                if let Some(color) = color {
                    assert!(cells.iter().all(|cell| cell.color == color));
                }
            }
            Ok(())
        })();
        drop(sampling);
        for texture in [red, green, wide, white, black] {
            renderer.destroy_scene_texture(texture);
        }
        result
    }
}
