use super::{RESOLUTION, VERTEX, vk};
use anyhow::Result;
use ash::vk as raw;
use paper_scene::shader::{self, Stage, Uniform};
use std::collections::BTreeMap;

const DIVISOR: u32 = 8;
const BLUR: &str = "BLUR";
const VERTICAL: &str = "VERTICAL";
const LABEL: &str = "volumetric blur";
const COMBINE_LABEL: &str = "volumetric combine";
const FRAGMENT: &str = include_str!("blur.frag");

struct Pass {
    pipeline: vk::EffectPipeline,
    uniforms: Vec<Uniform>,
}

impl Pass {
    fn new(
        renderer: &mut vk::Renderer,
        target: &vk::SceneTarget,
        direction: Option<bool>,
    ) -> Result<Self> {
        let combos = BTreeMap::from([
            (BLUR.into(), i64::from(direction.is_some())),
            (VERTICAL.into(), i64::from(direction.unwrap_or(false))),
        ]);
        let mut vertex = shader::translate(VERTEX, Stage::Vertex, &combos);
        let mut fragment = shader::translate(FRAGMENT, Stage::Fragment, &combos);
        shader::unify_uniforms(&mut vertex, &mut fragment);
        let blend = if direction.is_some() { vk::SceneBlend::Copy } else { vk::SceneBlend::Add };
        let mut spec = vk::EffectPipelineSpec::quad(target.format, blend, None);
        spec.depth_attachment = direction.is_none();
        spec.samples = target.samples;
        let label = if direction.is_some() { LABEL } else { COMBINE_LABEL };
        let pipeline =
            renderer.create_effect_pipeline_for(&vertex, &fragment, None, label, spec)?;
        Ok(Self { pipeline, uniforms: fragment.uniforms })
    }

    fn record(
        &self,
        renderer: &mut vk::Renderer,
        quad: &vk::QuadBuffer,
        target: &vk::SceneTarget,
        source: &vk::SceneTarget,
        preserve: bool,
    ) -> Result<()> {
        let values = BTreeMap::from([(
            RESOLUTION.into(),
            vec![target.extent.width as f32, target.extent.height as f32],
        )]);
        let bytes = shader::pack_uniforms(&self.uniforms, &values);
        renderer.record_effect_pass(
            &self.pipeline,
            quad,
            vk::EffectDraw { target, preserve, final_vertices: false },
            &[(source.view, source.sampler)],
            &bytes,
        )
    }
}

pub(super) struct Blur {
    pub accumulation: vk::SceneTarget,
    temporary: vk::SceneTarget,
    horizontal: Pass,
    vertical: Pass,
    combine: Pass,
}

impl Blur {
    pub fn new(renderer: &mut vk::Renderer, target: &vk::SceneTarget) -> Result<Self> {
        let spec = vk::SceneTargetSpec {
            depth_attachment: false,
            extent: raw::Extent2D {
                width: (target.extent.width / DIVISOR).max(1),
                height: (target.extent.height / DIVISOR).max(1),
            },
            format: target.format,
            repeat: false,
            samples: raw::SampleCountFlags::TYPE_1,
        };
        let accumulation = renderer.create_scene_target_opts(spec)?;
        let temporary = renderer.create_scene_target_opts(spec)?;
        let horizontal = Pass::new(renderer, &temporary, Some(false))?;
        let vertical = Pass::new(renderer, &accumulation, Some(true))?;
        let combine = Pass::new(renderer, target, None)?;
        Ok(Self { accumulation, temporary, horizontal, vertical, combine })
    }

    pub fn allocation_bytes(&self) -> u64 {
        self.accumulation.allocation_bytes + self.temporary.allocation_bytes
    }

    pub fn record(
        &self,
        renderer: &mut vk::Renderer,
        quad: &vk::QuadBuffer,
        target: &vk::SceneTarget,
    ) -> Result<()> {
        self.horizontal.record(renderer, quad, &self.temporary, &self.accumulation, false)?;
        self.vertical.record(renderer, quad, &self.accumulation, &self.temporary, false)?;
        self.combine.record(renderer, quad, target, &self.accumulation, true)
    }
}
