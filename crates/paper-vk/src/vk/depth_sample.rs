use super::{EffectDraw, EffectPipeline, EffectPipelineSpec, QuadBuffer, Renderer, SceneTarget};
use anyhow::{Context, Result, ensure};
use ash::vk;
use paper_scene::shader::{self, Stage};
use std::collections::BTreeMap;

const EXTENT_ERROR: &str = "depth sample extent changed";
const SAMPLE_ERROR: &str = "depth sample count changed";

const LABEL: &str = "scene depth sample";
const SAMPLE_COUNT: &str = "DEPTH_SAMPLES";
const SAMPLER: &str = "sampler2D g_Texture0";
const MULTISAMPLE_SAMPLER: &str = "sampler2DMS g_Texture0";
const VERTEX: &str = "attribute vec3 a_Position;\nvoid main(){gl_Position=vec4(a_Position,1.0);}";
const FRAGMENT: &str = include_str!("depth_sample.frag");
const FORMAT: vk::Format = vk::Format::R32_SFLOAT;

pub struct DepthSample {
    target: SceneTarget,
    pipeline: EffectPipeline,
    quad: QuadBuffer,
    samples: vk::SampleCountFlags,
}

impl DepthSample {
    pub fn new(renderer: &mut Renderer, source: &SceneTarget) -> Result<Option<Self>> {
        if source.depth_image().is_none() {
            return Ok(None);
        }
        let combos = BTreeMap::from([(SAMPLE_COUNT.into(), i64::from(source.samples.as_raw()))]);
        let vertex = shader::translate(VERTEX, Stage::Vertex, &combos);
        let mut fragment = shader::translate(FRAGMENT, Stage::Fragment, &combos);
        if source.samples != vk::SampleCountFlags::TYPE_1 {
            fragment.source = fragment.source.replace(SAMPLER, MULTISAMPLE_SAMPLER);
        }
        let target = renderer.create_scene_target_opts(super::SceneTargetSpec {
            depth_attachment: false,
            extent: source.extent,
            format: FORMAT,
            repeat: false,
            samples: vk::SampleCountFlags::TYPE_1,
        })?;
        let mut spec = EffectPipelineSpec::quad(FORMAT, super::SceneBlend::Copy, None);
        spec.depth_attachment = false;
        let pipeline =
            renderer.create_effect_pipeline_for(&vertex, &fragment, None, LABEL, spec)?;
        let quad = renderer.create_quad_buffer_ndc()?;
        Ok(Some(Self { target, pipeline, quad, samples: source.samples }))
    }

    pub fn sample(&self) -> (vk::ImageView, vk::Sampler) {
        (self.target.view, self.target.sampler)
    }

    pub fn allocation_bytes(&self) -> u64 {
        self.target.allocation_bytes
    }

    pub fn record(&self, renderer: &mut Renderer, source: &SceneTarget) -> Result<()> {
        ensure!(source.extent == self.target.extent, EXTENT_ERROR);
        ensure!(source.samples == self.samples, SAMPLE_ERROR);
        let (image, view) = source.depth_image().context("scene depth missing")?;
        renderer.depth_sample_barrier(image, true);
        let result = renderer.record_effect_pass(
            &self.pipeline,
            &self.quad,
            EffectDraw { target: &self.target, preserve: false, final_vertices: false },
            &[(view, renderer.sampler_nearest)],
            &[],
        );
        renderer.depth_sample_barrier(image, false);
        result
    }
}

impl Renderer {
    fn depth_sample_barrier(&self, image: vk::Image, sampled: bool) {
        let write = vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE;
        let read = vk::AccessFlags::SHADER_READ;
        let attachment = vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL;
        let shader = vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL;
        let depth = vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS
            | vk::PipelineStageFlags::LATE_FRAGMENT_TESTS;
        let fragment = vk::PipelineStageFlags::FRAGMENT_SHADER;
        let (old, new, from_access, to_access, from_stage, to_stage) = if sampled {
            (attachment, shader, write, read, depth, fragment)
        } else {
            (shader, attachment, read, write, fragment, depth)
        };
        unsafe {
            self.device.cmd_pipeline_barrier(
                self.cmd,
                from_stage,
                to_stage,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[vk::ImageMemoryBarrier::default()
                    .image(image)
                    .old_layout(old)
                    .new_layout(new)
                    .src_access_mask(from_access)
                    .dst_access_mask(to_access)
                    .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .subresource_range(vk::ImageSubresourceRange {
                        aspect_mask: vk::ImageAspectFlags::DEPTH,
                        base_mip_level: 0,
                        level_count: 1,
                        base_array_layer: 0,
                        layer_count: 1,
                    })],
            );
        }
    }
}
