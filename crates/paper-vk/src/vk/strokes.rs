use super::images::{Spec, Storage};
use super::pipelines::{PipelineSpec, build_pipeline};
use super::{DynBuffer, Renderer};
use anyhow::Result;
use ash::vk;
use paper_control::{DrawingInk, DrawingStroke};
use std::cell::Cell;
use std::sync::Arc;

const FORMAT: vk::Format = vk::Format::R8G8B8A8_UNORM;
const CORNERS: u32 = 4;
const COVERAGE_PASS: i32 = 0;
const ATTRIBUTE_PASS: i32 = 1;
const LENGTH_FLOOR: f32 = 1e-6;
const VEC4_BYTES: u32 = size_of::<[f32; 4]>() as u32;
const SEGMENT_FLOATS: usize = 11;
const SEGMENT_BYTES: u32 = (SEGMENT_FLOATS * size_of::<f32>()) as u32;
const SEGMENT_ATTRIBUTES: [(vk::Format, u32); 3] = [
    (vk::Format::R32G32B32A32_SFLOAT, 0),
    (vk::Format::R32G32B32A32_SFLOAT, VEC4_BYTES),
    (vk::Format::R32G32B32_SFLOAT, VEC4_BYTES * 2),
];
const STROKE_VERTEX: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/strokes.vert.spv"));
const STROKE_FRAGMENT: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/strokes.frag.spv"));

#[repr(C)]
struct StrokePush {
    size: [f32; 2],
    t: f32,
    dry: f32,
    pass: i32,
}

pub struct StrokeSpec {
    pub extent: vk::Extent2D,
    pub ink: DrawingInk,
    pub seconds: f32,
}

pub struct DrawingStrokes {
    device: Option<Arc<ash::Device>>,
    image: vk::Image,
    memory: vk::DeviceMemory,
    pub view: vk::ImageView,
    render_pass: vk::RenderPass,
    framebuffer: vk::Framebuffer,
    layout: vk::PipelineLayout,
    coverage: vk::Pipeline,
    attributes: vk::Pipeline,
    instances: DynBuffer,
    count: u32,
    extent: vk::Extent2D,
    pub ink: DrawingInk,
    pub seconds: f32,
    pub(super) drawn: Cell<Option<f32>>,
}

#[cfg(test)]
thread_local! {
    static LIVE_STROKES: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
pub(super) fn live_strokes() -> usize {
    LIVE_STROKES.get()
}

fn segments(strokes: &[DrawingStroke]) -> Vec<f32> {
    let mut floats = Vec::new();
    for stroke in strokes {
        let mut lengths = vec![0.0];
        for pair in stroke.points.windows(2) {
            let run = (pair[1][0] - pair[0][0]).hypot(pair[1][1] - pair[0][1]);
            lengths.push(lengths[lengths.len() - 1] + run);
        }
        let total = lengths[lengths.len() - 1].max(LENGTH_FLOOR);
        let span = stroke.end - stroke.start;
        let hatch = f32::from(u8::from(stroke.hatch));
        for (pair, run) in stroke.points.windows(2).zip(lengths.windows(2)) {
            let [from, to] = [run[0] / total, run[1] / total];
            floats.extend_from_slice(&[
                pair[0][0],
                pair[0][1],
                pair[1][0],
                pair[1][1],
                stroke.start + span * from,
                stroke.start + span * to,
                from,
                to,
                stroke.width,
                hatch,
                stroke.start,
            ]);
        }
    }
    floats
}

fn stroke_pass(device: &ash::Device) -> Result<vk::RenderPass> {
    let attachment = [vk::AttachmentDescription::default()
        .format(FORMAT)
        .samples(vk::SampleCountFlags::TYPE_1)
        .load_op(vk::AttachmentLoadOp::CLEAR)
        .store_op(vk::AttachmentStoreOp::STORE)
        .initial_layout(vk::ImageLayout::UNDEFINED)
        .final_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
    let color = [vk::AttachmentReference::default()
        .attachment(0)
        .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];
    let subpass = [vk::SubpassDescription::default()
        .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
        .color_attachments(&color)];
    let dependencies = [
        vk::SubpassDependency::default()
            .src_subpass(vk::SUBPASS_EXTERNAL)
            .dst_subpass(0)
            .src_stage_mask(vk::PipelineStageFlags::FRAGMENT_SHADER)
            .dst_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
            .dst_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE),
        vk::SubpassDependency::default()
            .src_subpass(0)
            .dst_subpass(vk::SUBPASS_EXTERNAL)
            .src_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
            .dst_stage_mask(vk::PipelineStageFlags::FRAGMENT_SHADER)
            .src_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)
            .dst_access_mask(vk::AccessFlags::SHADER_READ),
    ];
    Ok(unsafe {
        device.create_render_pass(
            &vk::RenderPassCreateInfo::default()
                .attachments(&attachment)
                .subpasses(&subpass)
                .dependencies(&dependencies),
            None,
        )?
    })
}

fn pass_blend(pass: i32) -> vk::PipelineColorBlendAttachmentState {
    if pass == COVERAGE_PASS {
        return vk::PipelineColorBlendAttachmentState::default()
            .color_write_mask(vk::ColorComponentFlags::R | vk::ColorComponentFlags::A)
            .blend_enable(true)
            .src_color_blend_factor(vk::BlendFactor::ONE)
            .dst_color_blend_factor(vk::BlendFactor::ONE)
            .color_blend_op(vk::BlendOp::MAX)
            .src_alpha_blend_factor(vk::BlendFactor::ONE)
            .dst_alpha_blend_factor(vk::BlendFactor::ONE)
            .alpha_blend_op(vk::BlendOp::MAX);
    }
    vk::PipelineColorBlendAttachmentState::default()
        .color_write_mask(vk::ColorComponentFlags::G | vk::ColorComponentFlags::B)
}

impl DrawingStrokes {
    fn passes(&self) -> impl Iterator<Item = (vk::Pipeline, i32)> {
        [(self.coverage, COVERAGE_PASS), (self.attributes, ATTRIBUTE_PASS)]
            .into_iter()
            .filter(|(pipeline, _)| *pipeline != vk::Pipeline::null())
    }

    fn pipeline(&self, device: &ash::Device, pass: i32) -> Result<vk::Pipeline> {
        let binding = [vk::VertexInputBindingDescription::default()
            .binding(0)
            .stride(SEGMENT_BYTES)
            .input_rate(vk::VertexInputRate::INSTANCE)];
        let attributes = SEGMENT_ATTRIBUTES.iter().zip(0..).map(|(&(format, offset), location)| {
            vk::VertexInputAttributeDescription::default()
                .location(location)
                .binding(0)
                .format(format)
                .offset(offset)
        });
        let attributes: Vec<_> = attributes.collect();
        let input = vk::PipelineVertexInputStateCreateInfo::default()
            .vertex_binding_descriptions(&binding)
            .vertex_attribute_descriptions(&attributes);
        unsafe {
            build_pipeline(
                device,
                &PipelineSpec {
                    vert: STROKE_VERTEX,
                    frag: STROKE_FRAGMENT,
                    topology: vk::PrimitiveTopology::TRIANGLE_STRIP,
                    blend: pass_blend(pass),
                    dynamic: &[],
                    layout: self.layout,
                    render_pass: self.render_pass,
                    extent: self.extent,
                    vertex: Some(&input),
                },
            )
        }
    }
}

impl Renderer {
    pub fn create_drawing_strokes(
        &self,
        strokes: &[DrawingStroke],
        spec: StrokeSpec,
    ) -> Result<DrawingStrokes> {
        let floats = segments(strokes);
        let bytes: Vec<u8> = floats.iter().flat_map(|value| value.to_ne_bytes()).collect();
        let instances = self.create_dyn_buffer(bytes.len(), vk::BufferUsageFlags::VERTEX_BUFFER)?;
        self.write_dyn_buffer(&instances, &bytes);
        let storage = Storage::new(
            self,
            &Spec {
                extent: spec.extent,
                format: FORMAT,
                levels: 1,
                samples: vk::SampleCountFlags::TYPE_1,
                usage: vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::SAMPLED,
                aspect: vk::ImageAspectFlags::COLOR,
            },
        )?;
        #[cfg(test)]
        LIVE_STROKES.set(LIVE_STROKES.get() + 1);
        let mut output = DrawingStrokes {
            device: Some(self.resource_device()),
            image: storage.image,
            memory: storage.memory,
            view: storage.view,
            render_pass: vk::RenderPass::null(),
            framebuffer: vk::Framebuffer::null(),
            layout: vk::PipelineLayout::null(),
            coverage: vk::Pipeline::null(),
            attributes: vk::Pipeline::null(),
            instances,
            count: (floats.len() / SEGMENT_FLOATS) as u32,
            extent: spec.extent,
            ink: spec.ink,
            seconds: spec.seconds,
            drawn: Cell::new(None),
        };
        output.render_pass = stroke_pass(&self.device)?;
        unsafe {
            output.framebuffer = self.device.create_framebuffer(
                &vk::FramebufferCreateInfo::default()
                    .render_pass(output.render_pass)
                    .attachments(&[output.view])
                    .width(spec.extent.width)
                    .height(spec.extent.height)
                    .layers(1),
                None,
            )?;
            let push = [vk::PushConstantRange::default()
                .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT)
                .size(size_of::<StrokePush>() as u32)];
            output.layout = self.device.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default().push_constant_ranges(&push),
                None,
            )?;
        }
        output.coverage = output.pipeline(&self.device, COVERAGE_PASS)?;
        if spec.ink == DrawingInk::Rainbow {
            output.attributes = output.pipeline(&self.device, ATTRIBUTE_PASS)?;
        }
        Ok(output)
    }

    pub(super) fn record_strokes(&self, strokes: &DrawingStrokes, progress: f32) -> Option<f32> {
        let start = paper_shaders::DRAWING_PAPER_START;
        let span = paper_shaders::DRAWING_BUILD_END - start;
        let t = ((progress - start) / span).clamp(0.0, 1.0);
        if strokes.drawn.get() == Some(t) {
            return None;
        }
        let clear = [vk::ClearValue::default()];
        unsafe {
            self.device.cmd_begin_render_pass(
                self.cmd,
                &vk::RenderPassBeginInfo::default()
                    .render_pass(strokes.render_pass)
                    .framebuffer(strokes.framebuffer)
                    .render_area(vk::Rect2D {
                        offset: vk::Offset2D::default(),
                        extent: strokes.extent,
                    })
                    .clear_values(&clear),
                vk::SubpassContents::INLINE,
            );
            self.device.cmd_bind_vertex_buffers(self.cmd, 0, &[strokes.instances.buffer], &[0]);
            let mut push = StrokePush {
                size: [strokes.extent.width as f32, strokes.extent.height as f32],
                t,
                dry: paper_shaders::DRAWING_DRY_SECONDS / (strokes.seconds * span),
                pass: COVERAGE_PASS,
            };
            for (pipeline, pass) in strokes.passes() {
                push.pass = pass;
                self.device.cmd_bind_pipeline(self.cmd, vk::PipelineBindPoint::GRAPHICS, pipeline);
                self.device.cmd_push_constants(
                    self.cmd,
                    strokes.layout,
                    vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                    0,
                    std::slice::from_raw_parts((&raw const push).cast(), size_of::<StrokePush>()),
                );
                self.device.cmd_draw(self.cmd, CORNERS, strokes.count, 0, 0);
            }
            self.device.cmd_end_render_pass(self.cmd);
        }
        Some(t)
    }
}

impl Drop for DrawingStrokes {
    fn drop(&mut self) {
        let Some(device) = self.device.take() else {
            return;
        };
        let _ = crate::shared::wait_idle(&device);
        unsafe {
            device.destroy_pipeline(self.attributes, None);
            device.destroy_pipeline(self.coverage, None);
            device.destroy_pipeline_layout(self.layout, None);
            device.destroy_framebuffer(self.framebuffer, None);
            device.destroy_render_pass(self.render_pass, None);
            device.destroy_image_view(self.view, None);
            device.destroy_image(self.image, None);
            device.free_memory(self.memory, None);
        }
        #[cfg(test)]
        LIVE_STROKES.set(LIVE_STROKES.get() - 1);
    }
}

#[cfg(test)]
#[path = "strokes_tests.rs"]
mod tests;
