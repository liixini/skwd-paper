use std::ffi::c_void;

use anyhow::{Context, Result, anyhow};
use ash::vk;

pub struct Renderer {
    pub(super) owns_device: bool,
    pub(super) plane_views: std::collections::HashMap<u64, (vk::ImageView, vk::ImageView)>,
    pub(super) _entry: ash::Entry,
    pub(super) instance: ash::Instance,
    pub(super) surface_fns: ash::khr::surface::Instance,
    pub(super) swapchain_fns: ash::khr::swapchain::Device,
    pub(super) ext_mem_fd: ash::khr::external_memory_fd::Device,
    pub device: ash::Device,
    pub(super) phys: vk::PhysicalDevice,
    pub(super) queue: vk::Queue,
    pub(super) queue_family: u32,
    pub(super) foreign_queue: bool,
    pub(super) drm_modifier_ext: bool,
    pub(super) import_modifiers: [Vec<u64>; 2],
    pub(super) import_modifier_warned: std::sync::atomic::AtomicBool,
    pub(super) surface: vk::SurfaceKHR,
    pub(super) swapchain: vk::SwapchainKHR,
    pub(super) format: vk::Format,
    pub extent: vk::Extent2D,
    pub(super) views: Vec<vk::ImageView>,
    pub(super) render_pass: vk::RenderPass,
    pub(super) framebuffers: Vec<vk::Framebuffer>,
    pub(super) pipeline_layout: vk::PipelineLayout,
    pub(super) pipeline: vk::Pipeline,
    pub(super) pipeline_fade: vk::Pipeline,
    pub(super) pipeline_layout_sand: vk::PipelineLayout,
    pub(super) pipeline_sand: vk::Pipeline,
    pub(super) pipeline_base: vk::Pipeline,
    pub(super) effect_pipes: std::collections::HashMap<usize, vk::Pipeline>,
    pub(super) scene_pass: vk::RenderPass,
    pub(super) format_passes: Vec<(vk::Format, vk::RenderPass)>,
    pub(super) format_passes_load: Vec<(vk::Format, vk::RenderPass)>,
    pub(super) scene_pool: vk::DescriptorPool,
    pub(super) fx_pool: vk::DescriptorPool,
    pub(super) pipeline_layout_layer: vk::PipelineLayout,
    pub(super) pipeline_layer: vk::Pipeline,
    pub(super) pipeline_layer_add: vk::Pipeline,
    pub(super) pipeline_layer_copy: vk::Pipeline,
    pub(super) pipeline_layer_screen: vk::Pipeline,
    pub(super) pipeline_puppet: vk::Pipeline,
    pub(super) pipeline_rgba: vk::Pipeline,
    pub(super) pipeline_rgba_fade: vk::Pipeline,
    pub(super) direct_render: bool,
    pub(super) desc_layout: vk::DescriptorSetLayout,
    pub(super) desc_pool: vk::DescriptorPool,
    pub(super) desc_set: vk::DescriptorSet,
    pub(super) desc_set_b: vk::DescriptorSet,
    pub(super) sampler: vk::Sampler,
    pub(super) sampler_repeat: vk::Sampler,
    pub(super) sampler_nearest: vk::Sampler,
    pub(super) sampler_nearest_repeat: vk::Sampler,
    pub(super) sampler_mip: vk::Sampler,
    pub(super) sampler_mip_repeat: vk::Sampler,
    pub(super) sampler_mip_nearest: vk::Sampler,
    pub(super) sampler_mip_nearest_repeat: vk::Sampler,
    pub(super) bc_supported: bool,
    pub(super) tex_mips: bool,
    pub(super) cmd_pool: vk::CommandPool,
    pub(super) cmd: vk::CommandBuffer,
    pub(super) acquire_sem: vk::Semaphore,
    pub(super) render_sem: vk::Semaphore,
    pub(super) fence: vk::Fence,
    pub(super) scene_query_pool: Option<vk::QueryPool>,
    pub(super) scene_timestamp_period: f64,
    pub(super) scene_timestamp_valid_bits: u32,
    pub(super) scene_timestamp_active: bool,
    pub(super) scene_gpu_time_ns: Option<u64>,
}

impl Renderer {
    pub fn direct_render(&self) -> bool {
        self.direct_render
    }

    pub fn supports_foreign_import(&self) -> bool {
        self.foreign_queue
    }

    pub fn new(
        display: *mut c_void,
        wl_surface: *mut c_void,
        width: u32,
        height: u32,
    ) -> Result<Self> {
        Self::build(None, display, wl_surface, width, height)
    }

    pub fn new_shared(
        parts: (ash::Entry, ash::Instance, vk::PhysicalDevice, ash::Device, u32, vk::Queue),
        display: *mut c_void,
        wl_surface: *mut c_void,
        width: u32,
        height: u32,
    ) -> Result<Self> {
        Self::build(Some(parts), display, wl_surface, width, height)
    }

    #[cfg(feature = "shared-device")]
    pub fn new_shared_headless(
        parts: (ash::Entry, ash::Instance, vk::PhysicalDevice, ash::Device, u32, vk::Queue),
        width: u32,
        height: u32,
    ) -> Result<Self> {
        let (entry, instance, phys, device, queue_family, queue) = parts;
        let surface_fns = ash::khr::surface::Instance::new(&entry, &instance);
        let swapchain_fns = ash::khr::swapchain::Device::new(&instance, &device);
        let ext_mem_fd = ash::khr::external_memory_fd::Device::new(&instance, &device);
        Self::finish(
            false,
            entry,
            instance,
            surface_fns,
            swapchain_fns,
            ext_mem_fd,
            device,
            phys,
            queue,
            queue_family,
            vk::SurfaceKHR::null(),
            width,
            height,
        )
    }

    fn build(
        shared: Option<(
            ash::Entry,
            ash::Instance,
            vk::PhysicalDevice,
            ash::Device,
            u32,
            vk::Queue,
        )>,
        display: *mut c_void,
        wl_surface: *mut c_void,
        width: u32,
        height: u32,
    ) -> Result<Self> {
        unsafe {
            let owns_device = shared.is_none();
            let (entry, instance) = if let Some((e, i, ..)) = &shared {
                (e.clone(), i.clone())
            } else {
                let entry = ash::Entry::load().context("load libvulkan")?;
                let app = vk::ApplicationInfo::default()
                    .application_name(c"skwd-wall-vk")
                    .api_version(vk::API_VERSION_1_2);
                let inst_exts =
                    [ash::khr::surface::NAME.as_ptr(), ash::khr::wayland_surface::NAME.as_ptr()];
                let instance = entry
                    .create_instance(
                        &vk::InstanceCreateInfo::default()
                            .application_info(&app)
                            .enabled_extension_names(&inst_exts),
                        None,
                    )
                    .context("create instance")?;
                (entry, instance)
            };

            let wl_fns = ash::khr::wayland_surface::Instance::new(&entry, &instance);
            let surface = wl_fns
                .create_wayland_surface(
                    &vk::WaylandSurfaceCreateInfoKHR::default()
                        .display(display)
                        .surface(wl_surface),
                    None,
                )
                .context("create wayland surface")?;
            let surface_fns = ash::khr::surface::Instance::new(&entry, &instance);

            let (phys, device, queue_family, queue) = if let Some((_, _, p, d, qf, q)) = shared {
                if !surface_fns.get_physical_device_surface_support(p, qf, surface).unwrap_or(false)
                {
                    return Err(anyhow!("shared device graphics family cannot present"));
                }
                (p, d, qf, q)
            } else {
                Self::standalone_device(&instance, &surface_fns, surface)?
            };
            let swapchain_fns = ash::khr::swapchain::Device::new(&instance, &device);
            let ext_mem_fd = ash::khr::external_memory_fd::Device::new(&instance, &device);

            Self::finish(
                owns_device,
                entry,
                instance,
                surface_fns,
                swapchain_fns,
                ext_mem_fd,
                device,
                phys,
                queue,
                queue_family,
                surface,
                width,
                height,
            )
        }
    }

    fn standalone_device(
        instance: &ash::Instance,
        surface_fns: &ash::khr::surface::Instance,
        surface: vk::SurfaceKHR,
    ) -> Result<(vk::PhysicalDevice, ash::Device, u32, vk::Queue)> {
        unsafe {
            let required_dev_exts = [
                ash::khr::swapchain::NAME,
                ash::khr::external_memory_fd::NAME,
                ash::ext::external_memory_dma_buf::NAME,
                ash::ext::image_drm_format_modifier::NAME,
            ];
            let phys_devs = instance.enumerate_physical_devices()?;
            let mut chosen: Option<(vk::PhysicalDevice, u32, bool)> = None;
            for pd in phys_devs {
                let exts = instance.enumerate_device_extension_properties(pd)?;
                let has = |name: &std::ffi::CStr| {
                    exts.iter().any(|ext| ext.extension_name_as_c_str() == Ok(name))
                };
                if !required_dev_exts.iter().all(|name| has(name)) {
                    continue;
                }
                let foreign = has(ash::ext::queue_family_foreign::NAME);
                let qfs = instance.get_physical_device_queue_family_properties(pd);
                for (i, qf) in qfs.iter().enumerate() {
                    let idx = i as u32;
                    if qf.queue_flags.contains(vk::QueueFlags::GRAPHICS)
                        && surface_fns
                            .get_physical_device_surface_support(pd, idx, surface)
                            .unwrap_or(false)
                    {
                        chosen = Some((pd, idx, foreign));
                        break;
                    }
                }
                if chosen.is_some() {
                    break;
                }
            }
            let (phys, queue_family, has_foreign) = chosen.ok_or_else(|| {
                anyhow!("no suitable Vulkan device (need dmabuf import + wayland present)")
            })?;

            let mut dev_exts: Vec<*const i8> =
                required_dev_exts.iter().map(|name| name.as_ptr()).collect();
            if has_foreign {
                dev_exts.push(ash::ext::queue_family_foreign::NAME.as_ptr());
            }
            let prio = [1.0f32];
            let qci = [vk::DeviceQueueCreateInfo::default()
                .queue_family_index(queue_family)
                .queue_priorities(&prio)];
            let supported = instance.get_physical_device_features(phys);
            let features = vk::PhysicalDeviceFeatures::default()
                .texture_compression_bc(supported.texture_compression_bc == vk::TRUE);
            let device = instance
                .create_device(
                    phys,
                    &vk::DeviceCreateInfo::default()
                        .queue_create_infos(&qci)
                        .enabled_extension_names(&dev_exts)
                        .enabled_features(&features),
                    None,
                )
                .context("create device")?;
            let queue = device.get_device_queue(queue_family, 0);
            Ok((phys, device, queue_family, queue))
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn finish(
        owns_device: bool,
        entry: ash::Entry,
        instance: ash::Instance,
        surface_fns: ash::khr::surface::Instance,
        swapchain_fns: ash::khr::swapchain::Device,
        ext_mem_fd: ash::khr::external_memory_fd::Device,
        device: ash::Device,
        phys: vk::PhysicalDevice,
        queue: vk::Queue,
        queue_family: u32,
        surface: vk::SurfaceKHR,
        width: u32,
        height: u32,
    ) -> Result<Self> {
        unsafe {
            let headless = surface == vk::SurfaceKHR::null();
            let device_exts = instance.enumerate_device_extension_properties(phys)?;
            let has_ext = |name: &std::ffi::CStr| {
                device_exts.iter().any(|ext| ext.extension_name_as_c_str() == Ok(name))
            };
            let foreign_queue = has_ext(ash::ext::queue_family_foreign::NAME);
            let drm_modifier_ext = has_ext(ash::ext::image_drm_format_modifier::NAME);
            let direct_render = instance
                .get_physical_device_format_properties(phys, vk::Format::B8G8R8A8_UNORM)
                .linear_tiling_features
                .contains(vk::FormatFeatureFlags::COLOR_ATTACHMENT);
            let (sf_format, extent, swapchain, views) = Self::create_swapchain(
                &surface_fns,
                &swapchain_fns,
                &device,
                phys,
                surface,
                width,
                height,
                headless,
            )?;
            let sf = vk::SurfaceFormatKHR {
                format: sf_format,
                color_space: vk::ColorSpaceKHR::SRGB_NONLINEAR,
            };

            let render_pass = Self::create_render_pass(&device, sf.format, headless)?;
            let framebuffers: Vec<vk::Framebuffer> = views
                .iter()
                .map(|&v| {
                    let att = [v];
                    device.create_framebuffer(
                        &vk::FramebufferCreateInfo::default()
                            .render_pass(render_pass)
                            .attachments(&att)
                            .width(extent.width)
                            .height(extent.height)
                            .layers(1),
                        None,
                    )
                })
                .collect::<std::result::Result<_, _>>()?;

            let make_sampler = |filter: vk::Filter, mode: vk::SamplerAddressMode, mips: bool| {
                let mipmap_mode = if filter == vk::Filter::NEAREST {
                    vk::SamplerMipmapMode::NEAREST
                } else {
                    vk::SamplerMipmapMode::LINEAR
                };
                device.create_sampler(
                    &vk::SamplerCreateInfo::default()
                        .mag_filter(filter)
                        .min_filter(filter)
                        .mipmap_mode(mipmap_mode)
                        .address_mode_u(mode)
                        .address_mode_v(mode)
                        .address_mode_w(mode)
                        .max_lod(if mips { vk::LOD_CLAMP_NONE } else { 0.0 }),
                    None,
                )
            };
            let clamp = vk::SamplerAddressMode::CLAMP_TO_EDGE;
            let repeat = vk::SamplerAddressMode::REPEAT;
            let sampler = make_sampler(vk::Filter::LINEAR, clamp, false)?;
            let sampler_repeat = make_sampler(vk::Filter::LINEAR, repeat, false)?;
            let sampler_nearest = make_sampler(vk::Filter::NEAREST, clamp, false)?;
            let sampler_nearest_repeat = make_sampler(vk::Filter::NEAREST, repeat, false)?;
            let sampler_mip = make_sampler(vk::Filter::LINEAR, clamp, true)?;
            let sampler_mip_repeat = make_sampler(vk::Filter::LINEAR, repeat, true)?;
            let sampler_mip_nearest = make_sampler(vk::Filter::NEAREST, clamp, true)?;
            let sampler_mip_nearest_repeat = make_sampler(vk::Filter::NEAREST, repeat, true)?;
            let env_on = |name: &str| std::env::var(name).as_deref() != Ok("0");
            let bc_supported = env_on("SKWD_PAPER_TEX_BC")
                && instance.get_physical_device_features(phys).texture_compression_bc == vk::TRUE;
            let tex_mips = env_on("SKWD_PAPER_TEX_MIPS");

            let bindings = [
                vk::DescriptorSetLayoutBinding::default()
                    .binding(0)
                    .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                    .descriptor_count(1)
                    .stage_flags(vk::ShaderStageFlags::FRAGMENT),
                vk::DescriptorSetLayoutBinding::default()
                    .binding(1)
                    .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                    .descriptor_count(1)
                    .stage_flags(vk::ShaderStageFlags::FRAGMENT),
            ];
            let desc_layout = device.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings),
                None,
            )?;
            let pool_sizes = [vk::DescriptorPoolSize {
                ty: vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
                descriptor_count: 4,
            }];
            let desc_pool = device.create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::default().max_sets(2).pool_sizes(&pool_sizes),
                None,
            )?;
            let both_layouts = [desc_layout, desc_layout];
            let sets = device.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(desc_pool)
                    .set_layouts(&both_layouts),
            )?;
            let desc_set = sets[0];
            let desc_set_b = sets[1];

            let (pipeline_layout, pipeline, pipeline_layout_sand) =
                Self::create_pipelines(&device, extent, render_pass, desc_layout)?;

            let cmd_pool = device.create_command_pool(
                &vk::CommandPoolCreateInfo::default()
                    .queue_family_index(queue_family)
                    .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER),
                None,
            )?;
            let cmd = device.allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(cmd_pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(1),
            )?[0];
            let acquire_sem = device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None)?;
            let render_sem = device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None)?;
            let fence = device.create_fence(
                &vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED),
                None,
            )?;
            let queue_properties = instance.get_physical_device_queue_family_properties(phys);
            let scene_timestamp_valid_bits = queue_properties
                .get(queue_family as usize)
                .map_or(0, |properties| properties.timestamp_valid_bits);
            let timestamp_requested = std::env::var_os("SKWD_VK_SCENE_GPU_TIMESTAMPS").is_some();
            let scene_query_pool = (scene_timestamp_valid_bits > 0 && timestamp_requested)
                .then(|| {
                    device.create_query_pool(
                        &vk::QueryPoolCreateInfo::default()
                            .query_type(vk::QueryType::TIMESTAMP)
                            .query_count(2),
                        None,
                    )
                })
                .transpose()?;
            let scene_timestamp_active = scene_query_pool.is_some();
            let scene_timestamp_period =
                f64::from(instance.get_physical_device_properties(phys).limits.timestamp_period);

            let mut renderer = Self {
                owns_device,
                plane_views: std::collections::HashMap::new(),
                _entry: entry,
                instance,
                surface_fns,
                swapchain_fns,
                ext_mem_fd,
                device,
                phys,
                queue,
                queue_family,
                foreign_queue,
                drm_modifier_ext,
                import_modifiers: [Vec::new(), Vec::new()],
                import_modifier_warned: std::sync::atomic::AtomicBool::new(false),
                surface,
                swapchain,
                format: sf.format,
                extent,
                views,
                render_pass,
                framebuffers,
                pipeline_layout,
                pipeline,
                pipeline_fade: vk::Pipeline::null(),
                pipeline_layout_sand,
                pipeline_sand: vk::Pipeline::null(),
                pipeline_base: vk::Pipeline::null(),
                effect_pipes: std::collections::HashMap::new(),
                scene_pass: vk::RenderPass::null(),
                format_passes: Vec::new(),
                format_passes_load: Vec::new(),
                scene_pool: vk::DescriptorPool::null(),
                fx_pool: vk::DescriptorPool::null(),
                pipeline_layout_layer: vk::PipelineLayout::null(),
                pipeline_layer: vk::Pipeline::null(),
                pipeline_layer_add: vk::Pipeline::null(),
                pipeline_layer_copy: vk::Pipeline::null(),
                pipeline_layer_screen: vk::Pipeline::null(),
                pipeline_puppet: vk::Pipeline::null(),
                pipeline_rgba: vk::Pipeline::null(),
                pipeline_rgba_fade: vk::Pipeline::null(),
                direct_render,
                desc_layout,
                desc_pool,
                desc_set,
                desc_set_b,
                sampler,
                sampler_repeat,
                sampler_nearest,
                sampler_nearest_repeat,
                sampler_mip,
                sampler_mip_repeat,
                sampler_mip_nearest,
                sampler_mip_nearest_repeat,
                bc_supported,
                tex_mips,
                cmd_pool,
                cmd,
                acquire_sem,
                render_sem,
                fence,
                scene_query_pool,
                scene_timestamp_period,
                scene_timestamp_valid_bits,
                scene_timestamp_active,
                scene_gpu_time_ns: None,
            };
            if drm_modifier_ext {
                renderer.import_modifiers = [
                    renderer.single_plane_import_modifiers(vk::Format::R8_UNORM),
                    renderer.single_plane_import_modifiers(vk::Format::R8G8_UNORM),
                ];
            }
            Ok(renderer)
        }
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        unsafe {
            let _ = self.device.device_wait_idle();
            for (_, (lv, cv)) in self.plane_views.drain() {
                self.device.destroy_image_view(lv, None);
                self.device.destroy_image_view(cv, None);
            }
            if let Some(pool) = self.scene_query_pool.take() {
                self.device.destroy_query_pool(pool, None);
            }
            self.device.destroy_fence(self.fence, None);
            self.device.destroy_semaphore(self.acquire_sem, None);
            self.device.destroy_semaphore(self.render_sem, None);
            self.device.destroy_command_pool(self.cmd_pool, None);
            self.device.destroy_pipeline(self.pipeline, None);
            self.device.destroy_pipeline(self.pipeline_fade, None);
            self.device.destroy_pipeline(self.pipeline_layer, None);
            self.device.destroy_pipeline(self.pipeline_layer_add, None);
            self.device.destroy_pipeline(self.pipeline_layer_copy, None);
            self.device.destroy_pipeline(self.pipeline_layer_screen, None);
            self.device.destroy_pipeline(self.pipeline_puppet, None);
            self.device.destroy_pipeline(self.pipeline_rgba, None);
            self.device.destroy_pipeline(self.pipeline_rgba_fade, None);
            if self.pipeline_layout_layer != vk::PipelineLayout::null() {
                self.device.destroy_pipeline_layout(self.pipeline_layout_layer, None);
            }
            if self.scene_pool != vk::DescriptorPool::null() {
                self.device.destroy_descriptor_pool(self.scene_pool, None);
            }
            if self.fx_pool != vk::DescriptorPool::null() {
                self.device.destroy_descriptor_pool(self.fx_pool, None);
            }
            if self.scene_pass != vk::RenderPass::null() {
                self.device.destroy_render_pass(self.scene_pass, None);
                for (_, pass) in
                    self.format_passes.drain(..).chain(self.format_passes_load.drain(..))
                {
                    self.device.destroy_render_pass(pass, None);
                }
            }
            self.device.destroy_pipeline(self.pipeline_sand, None);
            self.device.destroy_pipeline(self.pipeline_base, None);
            for (_, pipe) in self.effect_pipes.drain() {
                self.device.destroy_pipeline(pipe, None);
            }
            self.device.destroy_pipeline_layout(self.pipeline_layout, None);
            self.device.destroy_pipeline_layout(self.pipeline_layout_sand, None);
            self.device.destroy_descriptor_pool(self.desc_pool, None);
            self.device.destroy_descriptor_set_layout(self.desc_layout, None);
            self.device.destroy_sampler(self.sampler, None);
            self.device.destroy_sampler(self.sampler_repeat, None);
            self.device.destroy_sampler(self.sampler_nearest, None);
            self.device.destroy_sampler(self.sampler_nearest_repeat, None);
            self.device.destroy_sampler(self.sampler_mip, None);
            self.device.destroy_sampler(self.sampler_mip_repeat, None);
            self.device.destroy_sampler(self.sampler_mip_nearest, None);
            self.device.destroy_sampler(self.sampler_mip_nearest_repeat, None);
            for fb in &self.framebuffers {
                self.device.destroy_framebuffer(*fb, None);
            }
            self.device.destroy_render_pass(self.render_pass, None);
            for view in &self.views {
                self.device.destroy_image_view(*view, None);
            }
            if self.swapchain != vk::SwapchainKHR::null() {
                self.swapchain_fns.destroy_swapchain(self.swapchain, None);
            }
            if self.surface != vk::SurfaceKHR::null() {
                self.surface_fns.destroy_surface(self.surface, None);
            }
            if self.owns_device {
                self.device.destroy_device(None);
                self.instance.destroy_instance(None);
            }
        }
    }
}
