use super::{DynBuffer, Renderer, SceneTexture};
use anyhow::{Context, Result};
use ash::vk;

const UNPREPARED_EFFECT: &str = "transition effect is not prepared";
const PALETTE_BYTES: usize = std::mem::size_of::<[[f32; 4]; 4]>();
pub(super) const ENVIRONMENT_SET: u32 = 2;
const PALETTE_BINDING: u32 = 0;
const ENVIRONMENT_BINDING: u32 = 1;

pub(super) struct Transition {
    device: ash::Device,
    layout: vk::PipelineLayout,
    descriptor_layout: vk::DescriptorSetLayout,
    pool: vk::DescriptorPool,
    set: vk::DescriptorSet,
    palette: DynBuffer,
    environment: Option<SceneTexture>,
    effect: Option<usize>,
}

impl Transition {
    fn new(renderer: &Renderer) -> Result<Self> {
        let bindings = [
            (PALETTE_BINDING, vk::DescriptorType::UNIFORM_BUFFER),
            (ENVIRONMENT_BINDING, vk::DescriptorType::COMBINED_IMAGE_SAMPLER),
        ]
        .map(|(binding, kind)| {
            vk::DescriptorSetLayoutBinding::default()
                .binding(binding)
                .descriptor_type(kind)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT)
        });
        let palette =
            renderer.create_dyn_buffer(PALETTE_BYTES, vk::BufferUsageFlags::UNIFORM_BUFFER)?;
        let mut state = Self {
            device: renderer.device.clone(),
            layout: vk::PipelineLayout::null(),
            descriptor_layout: vk::DescriptorSetLayout::null(),
            pool: vk::DescriptorPool::null(),
            set: vk::DescriptorSet::null(),
            palette,
            environment: None,
            effect: None,
        };
        unsafe {
            state.descriptor_layout = renderer.device.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings),
                None,
            )?;
            let layouts = [renderer.desc_layout, renderer.desc_layout, state.descriptor_layout];
            let pushes = [vk::PushConstantRange::default()
                .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT)
                .size(super::draw::EFFECT_PUSH_BYTES)];
            state.layout = renderer.device.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default()
                    .set_layouts(&layouts)
                    .push_constant_ranges(&pushes),
                None,
            )?;
            let sizes = [
                vk::DescriptorPoolSize {
                    ty: vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
                    descriptor_count: 1,
                },
                vk::DescriptorPoolSize {
                    ty: vk::DescriptorType::UNIFORM_BUFFER,
                    descriptor_count: 1,
                },
            ];
            state.pool = renderer.device.create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::default().max_sets(1).pool_sizes(&sizes),
                None,
            )?;
            state.set = renderer.device.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(state.pool)
                    .set_layouts(&[state.descriptor_layout]),
            )?[0];
        }
        Ok(state)
    }

    fn bind(&self, renderer: &Renderer, environment: &SceneTexture) {
        let image = [vk::DescriptorImageInfo::default()
            .image_view(environment.view)
            .sampler(environment.sampler)
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
        let palette = [vk::DescriptorBufferInfo::default()
            .buffer(self.palette.buffer)
            .range(PALETTE_BYTES as u64)];
        let writes = [
            vk::WriteDescriptorSet::default()
                .dst_set(self.set)
                .dst_binding(PALETTE_BINDING)
                .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                .buffer_info(&palette),
            vk::WriteDescriptorSet::default()
                .dst_set(self.set)
                .dst_binding(ENVIRONMENT_BINDING)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .image_info(&image),
        ];
        unsafe {
            renderer.device.update_descriptor_sets(&writes, &[]);
        }
    }
}

impl Drop for Transition {
    fn drop(&mut self) {
        let _ = crate::shared::wait_idle(&self.device);
        drop(self.environment.take());
        unsafe {
            self.device.destroy_descriptor_pool(self.pool, None);
            self.device.destroy_pipeline_layout(self.layout, None);
            self.device.destroy_descriptor_set_layout(self.descriptor_layout, None);
        }
    }
}

impl Renderer {
    pub fn clear_transition(&mut self) -> Result<()> {
        let Some(environment) = self.transition.as_mut().and_then(|state| {
            state.effect = None;
            state.environment.take()
        }) else {
            return Ok(());
        };
        self.wait_render()?;
        self.destroy_scene_texture(environment);
        Ok(())
    }

    pub(super) fn transition_layout(&mut self) -> Result<vk::PipelineLayout> {
        if let Some(state) = &self.transition {
            return Ok(state.layout);
        }
        let state = Transition::new(self)?;
        let layout = state.layout;
        self.transition = Some(state);
        Ok(layout)
    }

    pub fn prepare_transition(&mut self, effect: usize) -> Result<()> {
        if self.transition.as_ref().is_some_and(|state| state.effect == Some(effect)) {
            return Ok(());
        }
        self.clear_transition()?;
        if !paper_shaders::effect_requires_environment(effect) {
            return Ok(());
        }
        self.transition_layout()?;
        let palette = crate::transition_theme::palette();
        let [width, height] = paper_shaders::STUDIO_ENVIRONMENT_SIZE;
        let environment = self.create_scene_texture_opts(
            width,
            height,
            paper_shaders::STUDIO_ENVIRONMENT_RGBA,
            true,
            true,
        )?;
        let mut state = self.transition.take().expect("transition state");
        let bytes: Vec<u8> =
            palette.as_flattened().iter().flat_map(|channel| channel.to_ne_bytes()).collect();
        self.write_dyn_buffer(&state.palette, &bytes);
        state.bind(self, &environment);
        state.environment = Some(environment);
        state.effect = Some(effect);
        self.transition = Some(state);
        Ok(())
    }

    pub(super) fn transition_bindings(
        &self,
        effect: usize,
    ) -> Result<(vk::PipelineLayout, Option<vk::DescriptorSet>)> {
        if !paper_shaders::effect_requires_environment(effect) {
            return Ok((self.pipeline_layout_sand, None));
        }
        let state = self
            .transition
            .as_ref()
            .filter(|state| state.effect == Some(effect))
            .context(UNPREPARED_EFFECT)?;
        Ok((state.layout, Some(state.set)))
    }
}

#[cfg(test)]
mod tests;
