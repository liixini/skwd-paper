#[cfg(feature = "shared-device")]
use anyhow::{Context, Result, anyhow};
use ash::vk;

use super::Renderer;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExportOwner {
    None,
    External,
    Foreign,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct QueueTransfer {
    pub src: u32,
    pub dst: u32,
}

impl ExportOwner {
    fn queue_family(self) -> Option<u32> {
        match self {
            Self::None => None,
            Self::External => Some(vk::QUEUE_FAMILY_EXTERNAL),
            Self::Foreign => Some(vk::QUEUE_FAMILY_FOREIGN_EXT),
        }
    }

    pub(crate) fn acquire(self, ready: bool, queue_family: u32) -> QueueTransfer {
        self.queue_family().filter(|_| ready).map_or(
            QueueTransfer { src: vk::QUEUE_FAMILY_IGNORED, dst: vk::QUEUE_FAMILY_IGNORED },
            |src| QueueTransfer { src, dst: queue_family },
        )
    }

    pub(crate) fn release(self, queue_family: u32) -> QueueTransfer {
        self.queue_family().map_or(
            QueueTransfer { src: vk::QUEUE_FAMILY_IGNORED, dst: vk::QUEUE_FAMILY_IGNORED },
            |dst| QueueTransfer { src: queue_family, dst },
        )
    }
}

#[cfg(feature = "shared-device")]
/// The creating Vulkan device must outlive this value.
pub struct ReadbackBuf {
    pub buffer: vk::Buffer,
    mem: vk::DeviceMemory,
    pub ptr: *mut u8,
    pub size: u64,
    device: ash::Device,
    released: bool,
}

#[cfg(feature = "shared-device")]
unsafe impl Send for ReadbackBuf {}

#[cfg(feature = "shared-device")]
impl ReadbackBuf {
    fn empty(device: &ash::Device, size: u64) -> Self {
        Self {
            buffer: vk::Buffer::null(),
            mem: vk::DeviceMemory::null(),
            ptr: std::ptr::null_mut(),
            size,
            device: device.clone(),
            released: false,
        }
    }

    fn release(&mut self) {
        if self.released {
            return;
        }
        self.released = true;
        unsafe {
            if !self.ptr.is_null() && self.mem != vk::DeviceMemory::null() {
                self.device.unmap_memory(self.mem);
                self.ptr = std::ptr::null_mut();
            }
            if self.buffer != vk::Buffer::null() {
                self.device.destroy_buffer(self.buffer, None);
                self.buffer = vk::Buffer::null();
            }
            if self.mem != vk::DeviceMemory::null() {
                self.device.free_memory(self.mem, None);
                self.mem = vk::DeviceMemory::null();
            }
        }
    }
}

#[cfg(feature = "shared-device")]
impl Drop for ReadbackBuf {
    fn drop(&mut self) {
        self.release();
    }
}

#[cfg(feature = "shared-device")]
/// The creating Vulkan device must outlive this value.
pub struct Nv12Export {
    pub image: vk::Image,
    pub memory: vk::DeviceMemory,
    pub fd: std::os::fd::RawFd,
    pub plane0_offset: u32,
    pub plane0_stride: u32,
    pub plane1_offset: u32,
    pub plane1_stride: u32,
    pub modifier: u64,
    pub ready: bool,
    pub(crate) owner: ExportOwner,
    device: ash::Device,
    released: std::cell::Cell<bool>,
}

#[cfg(feature = "shared-device")]
impl Nv12Export {
    pub(crate) fn empty(device: &ash::Device) -> Self {
        Self {
            image: vk::Image::null(),
            memory: vk::DeviceMemory::null(),
            fd: -1,
            plane0_offset: 0,
            plane0_stride: 0,
            plane1_offset: 0,
            plane1_stride: 0,
            modifier: 0,
            ready: false,
            owner: ExportOwner::Foreign,
            device: device.clone(),
            released: std::cell::Cell::new(false),
        }
    }

    fn release(&self) {
        if self.released.replace(true) {
            return;
        }
        unsafe {
            if self.image != vk::Image::null() {
                self.device.destroy_image(self.image, None);
            }
            if self.memory != vk::DeviceMemory::null() {
                self.device.free_memory(self.memory, None);
            }
            if self.fd >= 0 {
                let _ = libc::close(self.fd);
            }
        }
    }
}

#[cfg(feature = "shared-device")]
impl Drop for Nv12Export {
    fn drop(&mut self) {
        self.release();
    }
}

/// Any [`RenderTarget`] built from this image drops first; the device outlives both.
pub struct ExportImage {
    pub image: vk::Image,
    pub memory: vk::DeviceMemory,
    pub fd: std::os::fd::RawFd,
    pub stride: u32,
    pub offset: u32,
    pub allocation_size: u64,
    pub modifier: Option<u64>,
    pub direct_render: bool,
    pub(crate) owner: ExportOwner,
    pub ready: bool,
    pub mapped: Option<*mut u8>,
    device: ash::Device,
    released: bool,
}

pub struct ExternalSemaphore {
    pub semaphore: vk::Semaphore,
    pub fd: std::os::fd::RawFd,
    device: ash::Device,
}

impl Drop for ExternalSemaphore {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_semaphore(self.semaphore, None);
            if self.fd >= 0 {
                let _ = libc::close(self.fd);
            }
        }
    }
}

impl ExportImage {
    fn empty(device: &ash::Device) -> Self {
        Self {
            image: vk::Image::null(),
            memory: vk::DeviceMemory::null(),
            fd: -1,
            stride: 0,
            offset: 0,
            allocation_size: 0,
            modifier: None,
            direct_render: false,
            owner: ExportOwner::None,
            ready: false,
            mapped: None,
            device: device.clone(),
            released: false,
        }
    }

    fn release(&mut self) {
        if self.released {
            return;
        }
        self.released = true;
        unsafe {
            if self.mapped.take().is_some() && self.memory != vk::DeviceMemory::null() {
                self.device.unmap_memory(self.memory);
            }
            if self.image != vk::Image::null() {
                self.device.destroy_image(self.image, None);
                self.image = vk::Image::null();
            }
            if self.memory != vk::DeviceMemory::null() {
                self.device.free_memory(self.memory, None);
                self.memory = vk::DeviceMemory::null();
            }
            if self.fd >= 0 {
                let _ = libc::close(self.fd);
                self.fd = -1;
            }
        }
    }

    pub(crate) fn mark_wayland(&mut self) {
        self.owner = ExportOwner::Foreign;
    }
}

impl Drop for ExportImage {
    fn drop(&mut self) {
        self.release();
    }
}

#[cfg(feature = "shared-device")]
/// A target from `create_export_rt` borrows its image, so it drops before that [`ExportImage`].
pub struct RenderTarget {
    pub image: vk::Image,
    pub memory: vk::DeviceMemory,
    pub view: vk::ImageView,
    pub framebuffer: vk::Framebuffer,
    device: ash::Device,
    owns_image: bool,
    released: bool,
}

#[cfg(feature = "shared-device")]
impl RenderTarget {
    fn empty(device: &ash::Device, owns_image: bool) -> Self {
        Self {
            image: vk::Image::null(),
            memory: vk::DeviceMemory::null(),
            view: vk::ImageView::null(),
            framebuffer: vk::Framebuffer::null(),
            device: device.clone(),
            owns_image,
            released: false,
        }
    }

    fn release(&mut self) {
        if self.released {
            return;
        }
        self.released = true;
        unsafe {
            if self.framebuffer != vk::Framebuffer::null() {
                self.device.destroy_framebuffer(self.framebuffer, None);
                self.framebuffer = vk::Framebuffer::null();
            }
            if self.view != vk::ImageView::null() {
                self.device.destroy_image_view(self.view, None);
                self.view = vk::ImageView::null();
            }
            if self.owns_image && self.image != vk::Image::null() {
                self.device.destroy_image(self.image, None);
                self.image = vk::Image::null();
            }
            if self.owns_image && self.memory != vk::DeviceMemory::null() {
                self.device.free_memory(self.memory, None);
                self.memory = vk::DeviceMemory::null();
            }
        }
    }
}

#[cfg(feature = "shared-device")]
impl Drop for RenderTarget {
    fn drop(&mut self) {
        self.release();
    }
}

pub struct FrameViews {
    pub luma_view: vk::ImageView,
    pub chroma_view: vk::ImageView,
    pub pre_barrier: bool,
}

pub struct FrameImages {
    pub(crate) luma_img: vk::Image,
    pub(crate) luma_mem: vk::DeviceMemory,
    pub(crate) luma_view: vk::ImageView,
    pub(crate) chroma_img: vk::Image,
    pub(crate) chroma_mem: vk::DeviceMemory,
    pub(crate) chroma_view: vk::ImageView,
    pub(crate) ready: std::cell::Cell<bool>,
}

impl Renderer {
    fn drm_format_modifier_properties(
        &self,
        format: vk::Format,
    ) -> Vec<vk::DrmFormatModifierPropertiesEXT> {
        unsafe {
            let count = {
                let mut modifiers = vk::DrmFormatModifierPropertiesListEXT::default();
                let mut properties = vk::FormatProperties2::default().push_next(&mut modifiers);
                self.instance.get_physical_device_format_properties2(
                    self.phys,
                    format,
                    &mut properties,
                );
                modifiers.drm_format_modifier_count as usize
            };
            let mut available = vec![vk::DrmFormatModifierPropertiesEXT::default(); count];
            let returned = {
                let mut modifiers = vk::DrmFormatModifierPropertiesListEXT::default()
                    .drm_format_modifier_properties(&mut available);
                let mut properties = vk::FormatProperties2::default().push_next(&mut modifiers);
                self.instance.get_physical_device_format_properties2(
                    self.phys,
                    format,
                    &mut properties,
                );
                modifiers.drm_format_modifier_count as usize
            };
            available.truncate(returned.min(available.len()));
            available
        }
    }

    pub(super) fn single_plane_import_modifiers(&self, format: vk::Format) -> Vec<u64> {
        let available = self.drm_format_modifier_properties(format);
        available
            .iter()
            .filter(|properties| properties.drm_format_modifier_plane_count == 1)
            .map(|properties| properties.drm_format_modifier)
            .collect()
    }

    fn single_plane_xr24_modifiers(&self, modifiers: &[u64]) -> Vec<u64> {
        let available = self.drm_format_modifier_properties(vk::Format::B8G8R8A8_UNORM);
        single_plane_modifiers(modifiers, &available)
    }

    #[cfg(feature = "shared-device")]
    pub fn create_external_semaphore(&self) -> Result<ExternalSemaphore> {
        unsafe {
            let mut export_info = vk::ExportSemaphoreCreateInfo::default()
                .handle_types(vk::ExternalSemaphoreHandleTypeFlags::OPAQUE_FD);
            let semaphore = self
                .device
                .create_semaphore(
                    &vk::SemaphoreCreateInfo::default().push_next(&mut export_info),
                    None,
                )
                .context("create external semaphore")?;
            let loader = ash::khr::external_semaphore_fd::Device::new(&self.instance, &self.device);
            let fd = match loader.get_semaphore_fd(
                &vk::SemaphoreGetFdInfoKHR::default()
                    .semaphore(semaphore)
                    .handle_type(vk::ExternalSemaphoreHandleTypeFlags::OPAQUE_FD),
            ) {
                Ok(fd) => fd,
                Err(error) => {
                    self.device.destroy_semaphore(semaphore, None);
                    return Err(error).context("export semaphore fd");
                }
            };
            Ok(ExternalSemaphore { semaphore, fd, device: self.device.clone() })
        }
    }

    #[cfg(feature = "shared-device")]
    pub fn signal_external_semaphore(&self, semaphore: &ExternalSemaphore) -> Result<()> {
        unsafe {
            let semaphores = [semaphore.semaphore];
            let _guard = self.queue_guard();
            self.device.queue_submit(
                self.queue,
                &[vk::SubmitInfo::default().signal_semaphores(&semaphores)],
                vk::Fence::null(),
            )?;
        }
        Ok(())
    }

    #[cfg(feature = "shared-device")]
    pub fn create_render_target(&self, width: u32, height: u32) -> Result<RenderTarget> {
        unsafe {
            let mut target = RenderTarget::empty(&self.device, true);
            target.image = self.device.create_image(
                &vk::ImageCreateInfo::default()
                    .image_type(vk::ImageType::TYPE_2D)
                    .format(vk::Format::B8G8R8A8_UNORM)
                    .extent(vk::Extent3D { width, height, depth: 1 })
                    .mip_levels(1)
                    .array_layers(1)
                    .samples(vk::SampleCountFlags::TYPE_1)
                    .tiling(vk::ImageTiling::OPTIMAL)
                    .usage(
                        vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_SRC,
                    )
                    .sharing_mode(vk::SharingMode::EXCLUSIVE)
                    .initial_layout(vk::ImageLayout::UNDEFINED),
                None,
            )?;
            let reqs = self.device.get_image_memory_requirements(target.image);
            let mem_props = self.instance.get_physical_device_memory_properties(self.phys);
            let type_index = (0..mem_props.memory_type_count)
                .find(|&i| {
                    reqs.memory_type_bits & (1 << i) != 0
                        && mem_props.memory_types[i as usize]
                            .property_flags
                            .contains(vk::MemoryPropertyFlags::DEVICE_LOCAL)
                })
                .or_else(|| {
                    (0..mem_props.memory_type_count)
                        .find(|&i| reqs.memory_type_bits & (1 << i) != 0)
                })
                .ok_or_else(|| anyhow!("no memory type for render target"))?;
            target.memory = self.device.allocate_memory(
                &vk::MemoryAllocateInfo::default()
                    .allocation_size(reqs.size)
                    .memory_type_index(type_index),
                None,
            )?;
            self.device.bind_image_memory(target.image, target.memory, 0)?;
            target.view = self.device.create_image_view(
                &vk::ImageViewCreateInfo::default()
                    .image(target.image)
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(vk::Format::B8G8R8A8_UNORM)
                    .subresource_range(vk::ImageSubresourceRange {
                        aspect_mask: vk::ImageAspectFlags::COLOR,
                        base_mip_level: 0,
                        level_count: 1,
                        base_array_layer: 0,
                        layer_count: 1,
                    }),
                None,
            )?;
            let atts = [target.view];
            target.framebuffer = self.device.create_framebuffer(
                &vk::FramebufferCreateInfo::default()
                    .render_pass(self.render_pass)
                    .attachments(&atts)
                    .width(width)
                    .height(height)
                    .layers(1),
                None,
            )?;
            Ok(target)
        }
    }

    #[cfg(feature = "shared-device")]
    pub fn create_export_rt(&self, export: &ExportImage) -> Result<RenderTarget> {
        unsafe {
            let mut target = RenderTarget::empty(&self.device, false);
            target.image = export.image;
            target.view = self.device.create_image_view(
                &vk::ImageViewCreateInfo::default()
                    .image(export.image)
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(vk::Format::B8G8R8A8_UNORM)
                    .subresource_range(vk::ImageSubresourceRange {
                        aspect_mask: vk::ImageAspectFlags::COLOR,
                        base_mip_level: 0,
                        level_count: 1,
                        base_array_layer: 0,
                        layer_count: 1,
                    }),
                None,
            )?;
            let atts = [target.view];
            target.framebuffer = self.device.create_framebuffer(
                &vk::FramebufferCreateInfo::default()
                    .render_pass(self.render_pass)
                    .attachments(&atts)
                    .width(self.extent.width)
                    .height(self.extent.height)
                    .layers(1),
                None,
            )?;
            Ok(target)
        }
    }

    #[cfg(feature = "shared-device")]
    pub fn create_readback_buf(&self, bytes: u64) -> Result<ReadbackBuf> {
        unsafe {
            let mut readback = ReadbackBuf::empty(&self.device, bytes);
            readback.buffer = self.device.create_buffer(
                &vk::BufferCreateInfo::default()
                    .size(bytes)
                    .usage(vk::BufferUsageFlags::TRANSFER_DST)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE),
                None,
            )?;
            let reqs = self.device.get_buffer_memory_requirements(readback.buffer);
            let mem_props = self.instance.get_physical_device_memory_properties(self.phys);
            let pick = |extra: vk::MemoryPropertyFlags| {
                (0..mem_props.memory_type_count).find(|&i| {
                    reqs.memory_type_bits & (1 << i) != 0
                        && mem_props.memory_types[i as usize].property_flags.contains(
                            vk::MemoryPropertyFlags::HOST_VISIBLE
                                | vk::MemoryPropertyFlags::HOST_COHERENT
                                | extra,
                        )
                })
            };
            let idx = pick(vk::MemoryPropertyFlags::HOST_CACHED)
                .or_else(|| pick(vk::MemoryPropertyFlags::empty()))
                .ok_or_else(|| anyhow!("no host-visible memory type for readback"))?;
            readback.mem = self.device.allocate_memory(
                &vk::MemoryAllocateInfo::default()
                    .allocation_size(reqs.size)
                    .memory_type_index(idx),
                None,
            )?;
            self.device.bind_buffer_memory(readback.buffer, readback.mem, 0)?;
            readback.ptr = self
                .device
                .map_memory(readback.mem, 0, vk::WHOLE_SIZE, vk::MemoryMapFlags::empty())?
                .cast::<u8>();
            Ok(readback)
        }
    }

    #[cfg(feature = "shared-device")]
    pub fn destroy_nv12_export(&self, export: &Nv12Export) {
        export.release();
    }

    #[cfg(feature = "shared-device")]
    pub fn create_nv12_export(
        &self,
        width: u32,
        height: u32,
        modifiers: &[u64],
        allow_linear: bool,
    ) -> Result<Nv12Export> {
        if !modifiers.is_empty() {
            match self.create_nv12_export_tiling(
                width,
                height,
                vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT,
                modifiers,
            ) {
                Ok(export) => return Ok(export),
                Err(err) => {
                    tracing::info!(
                        "skwd-wall-vk: tiled NV12 presentation unavailable ({err:#}), using linear"
                    );
                }
            }
        }
        if !allow_linear {
            return Err(anyhow!("no importable NV12 presentation modifier"));
        }
        self.create_nv12_export_tiling(width, height, vk::ImageTiling::LINEAR, &[])
    }

    fn create_nv12_export_tiling(
        &self,
        width: u32,
        height: u32,
        tiling: vk::ImageTiling,
        modifiers: &[u64],
    ) -> Result<Nv12Export> {
        unsafe {
            let mut export = Nv12Export::empty(&self.device);
            let mut ext_info = vk::ExternalMemoryImageCreateInfo::default()
                .handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
            let mut modifier_info = vk::ImageDrmFormatModifierListCreateInfoEXT::default()
                .drm_format_modifiers(modifiers);
            let mut image_info = vk::ImageCreateInfo::default().push_next(&mut ext_info);
            if tiling == vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT {
                image_info = image_info.push_next(&mut modifier_info);
            }
            export.image = self
                .device
                .create_image(
                    &image_info
                        .image_type(vk::ImageType::TYPE_2D)
                        .format(vk::Format::G8_B8R8_2PLANE_420_UNORM)
                        .extent(vk::Extent3D { width, height, depth: 1 })
                        .mip_levels(1)
                        .array_layers(1)
                        .samples(vk::SampleCountFlags::TYPE_1)
                        .tiling(tiling)
                        .usage(vk::ImageUsageFlags::TRANSFER_DST)
                        .sharing_mode(vk::SharingMode::EXCLUSIVE)
                        .initial_layout(vk::ImageLayout::UNDEFINED),
                    None,
                )
                .context("create nv12 export image")?;
            if tiling == vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT {
                let modifier_fns =
                    ash::ext::image_drm_format_modifier::Device::new(&self.instance, &self.device);
                let mut properties = vk::ImageDrmFormatModifierPropertiesEXT::default();
                modifier_fns
                    .get_image_drm_format_modifier_properties(export.image, &mut properties)
                    .context("query nv12 export modifier")?;
                export.modifier = properties.drm_format_modifier;
            }
            let reqs = self.device.get_image_memory_requirements(export.image);
            let mem_props = self.instance.get_physical_device_memory_properties(self.phys);
            let type_index = (0..mem_props.memory_type_count)
                .find(|&i| {
                    reqs.memory_type_bits & (1 << i) != 0
                        && mem_props.memory_types[i as usize]
                            .property_flags
                            .contains(vk::MemoryPropertyFlags::DEVICE_LOCAL)
                })
                .or_else(|| {
                    (0..mem_props.memory_type_count)
                        .find(|&i| reqs.memory_type_bits & (1 << i) != 0)
                })
                .ok_or_else(|| anyhow!("no memory type for nv12 export"))?;
            let mut export_info = vk::ExportMemoryAllocateInfo::default()
                .handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
            let mut dedicated = vk::MemoryDedicatedAllocateInfo::default().image(export.image);
            export.memory = self
                .device
                .allocate_memory(
                    &vk::MemoryAllocateInfo::default()
                        .push_next(&mut export_info)
                        .push_next(&mut dedicated)
                        .allocation_size(reqs.size)
                        .memory_type_index(type_index),
                    None,
                )
                .context("allocate nv12 export memory")?;
            self.device.bind_image_memory(export.image, export.memory, 0)?;
            let plane0 = self.device.get_image_subresource_layout(
                export.image,
                vk::ImageSubresource {
                    aspect_mask: vk::ImageAspectFlags::PLANE_0,
                    mip_level: 0,
                    array_layer: 0,
                },
            );
            let plane1 = self.device.get_image_subresource_layout(
                export.image,
                vk::ImageSubresource {
                    aspect_mask: vk::ImageAspectFlags::PLANE_1,
                    mip_level: 0,
                    array_layer: 0,
                },
            );
            export.fd = self
                .ext_mem_fd
                .get_memory_fd(
                    &vk::MemoryGetFdInfoKHR::default()
                        .memory(export.memory)
                        .handle_type(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT),
                )
                .context("export nv12 memory fd")?;
            export.plane0_offset = plane0.offset as u32;
            export.plane0_stride = plane0.row_pitch as u32;
            export.plane1_offset = plane1.offset as u32;
            export.plane1_stride = plane1.row_pitch as u32;
            Ok(export)
        }
    }

    #[cfg(feature = "shared-device")]
    pub fn create_export_image(&self, width: u32, height: u32) -> Result<ExportImage> {
        self.create_export_image_opts(width, height, true)
    }

    #[cfg(feature = "shared-device")]
    pub fn create_export_image_opts(
        &self,
        width: u32,
        height: u32,
        export: bool,
    ) -> Result<ExportImage> {
        self.create_xr24_export_tiling(
            width,
            height,
            vk::ImageTiling::LINEAR,
            &[],
            export,
            self.direct_render(),
            vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT,
        )
    }

    #[cfg(feature = "shared-device")]
    pub fn create_stream_export(&self, width: u32, height: u32) -> Result<ExportImage> {
        let mut output = self.create_xr24_export_tiling(
            width,
            height,
            vk::ImageTiling::OPTIMAL,
            &[],
            true,
            true,
            vk::ExternalMemoryHandleTypeFlags::OPAQUE_FD,
        )?;
        output.owner = ExportOwner::External;
        Ok(output)
    }

    #[cfg(feature = "shared-device")]
    pub fn create_xr24_export(
        &self,
        width: u32,
        height: u32,
        modifiers: &[u64],
        linear_modifier: Option<u64>,
    ) -> Result<ExportImage> {
        let single_plane_modifiers = self.single_plane_xr24_modifiers(modifiers);
        if single_plane_modifiers.len() != modifiers.len() {
            tracing::info!(
                skipped = modifiers.len() - single_plane_modifiers.len(),
                "skwd-wall-vk: skipping multi-plane XR24 modifiers"
            );
        }
        if !single_plane_modifiers.is_empty() {
            match self.create_xr24_export_tiling(
                width,
                height,
                vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT,
                &single_plane_modifiers,
                true,
                true,
                vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT,
            ) {
                Ok(mut export) => {
                    export.mark_wayland();
                    return Ok(export);
                }
                Err(err) => {
                    tracing::info!(
                        "skwd-wall-vk: tiled XR24 direct render unavailable ({err:#}), using linear"
                    );
                }
            }
        }
        let modifier =
            linear_modifier.ok_or_else(|| anyhow!("no importable XR24 presentation modifier"))?;
        let mut output = self.create_export_image_opts(width, height, true)?;
        output.modifier = Some(modifier);
        output.mark_wayland();
        Ok(output)
    }

    fn create_xr24_export_tiling(
        &self,
        width: u32,
        height: u32,
        tiling: vk::ImageTiling,
        modifiers: &[u64],
        export: bool,
        direct_render: bool,
        handle_type: vk::ExternalMemoryHandleTypeFlags,
    ) -> Result<ExportImage> {
        unsafe {
            let mut output = ExportImage::empty(&self.device);
            let mut ext_info =
                vk::ExternalMemoryImageCreateInfo::default().handle_types(handle_type);
            let mut modifier_info = vk::ImageDrmFormatModifierListCreateInfoEXT::default()
                .drm_format_modifiers(modifiers);
            let mut image_info = vk::ImageCreateInfo::default();
            let gl_interop = handle_type == vk::ExternalMemoryHandleTypeFlags::OPAQUE_FD;
            let gl_usage = if gl_interop {
                let properties = self
                    .instance
                    .get_physical_device_format_properties(self.phys, vk::Format::B8G8R8A8_UNORM);
                gl_interop_image_usage(properties.optimal_tiling_features)
            } else {
                vk::ImageUsageFlags::empty()
            };
            if export {
                image_info = image_info.push_next(&mut ext_info);
            }
            if tiling == vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT {
                image_info = image_info.push_next(&mut modifier_info);
            }
            output.image = self
                .device
                .create_image(
                    &image_info
                        .flags(if gl_interop {
                            vk::ImageCreateFlags::MUTABLE_FORMAT
                        } else {
                            vk::ImageCreateFlags::empty()
                        })
                        .image_type(vk::ImageType::TYPE_2D)
                        .format(vk::Format::B8G8R8A8_UNORM)
                        .extent(vk::Extent3D { width, height, depth: 1 })
                        .mip_levels(1)
                        .array_layers(1)
                        .samples(vk::SampleCountFlags::TYPE_1)
                        .tiling(tiling)
                        .usage(
                            gl_usage
                                | vk::ImageUsageFlags::TRANSFER_DST
                                | if direct_render {
                                    vk::ImageUsageFlags::COLOR_ATTACHMENT
                                } else {
                                    vk::ImageUsageFlags::empty()
                                }
                                | if export {
                                    vk::ImageUsageFlags::empty()
                                } else {
                                    vk::ImageUsageFlags::TRANSFER_SRC
                                },
                        )
                        .sharing_mode(vk::SharingMode::EXCLUSIVE)
                        .initial_layout(vk::ImageLayout::UNDEFINED),
                    None,
                )
                .context("create export image")?;
            if tiling == vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT {
                let modifier_fns =
                    ash::ext::image_drm_format_modifier::Device::new(&self.instance, &self.device);
                let mut properties = vk::ImageDrmFormatModifierPropertiesEXT::default();
                modifier_fns
                    .get_image_drm_format_modifier_properties(output.image, &mut properties)
                    .context("query XR24 export modifier")?;
                output.modifier = Some(properties.drm_format_modifier);
            }
            let reqs = self.device.get_image_memory_requirements(output.image);
            let mem_props = self.instance.get_physical_device_memory_properties(self.phys);
            let want_host = tiling == vk::ImageTiling::LINEAR
                && std::env::var("SKWD_VK_PATTERN").as_deref() == Ok("cpu");
            let host_type = if want_host {
                (0..mem_props.memory_type_count).find(|&i| {
                    reqs.memory_type_bits & (1 << i) != 0
                        && mem_props.memory_types[i as usize].property_flags.contains(
                            vk::MemoryPropertyFlags::HOST_VISIBLE
                                | vk::MemoryPropertyFlags::HOST_COHERENT,
                        )
                })
            } else {
                None
            };
            let device_local = (0..mem_props.memory_type_count).find(|&i| {
                reqs.memory_type_bits & (1 << i) != 0
                    && mem_props.memory_types[i as usize]
                        .property_flags
                        .contains(vk::MemoryPropertyFlags::DEVICE_LOCAL)
            });
            let any_type = (0..mem_props.memory_type_count)
                .find(|&i| reqs.memory_type_bits & (1 << i) != 0)
                .ok_or_else(|| anyhow!("no memory type for export image"))?;
            let type_index = host_type.or(device_local).unwrap_or(any_type);
            let mut export_info = vk::ExportMemoryAllocateInfo::default().handle_types(handle_type);
            let mut dedicated = vk::MemoryDedicatedAllocateInfo::default().image(output.image);
            let mut alloc_info = vk::MemoryAllocateInfo::default();
            if export {
                alloc_info = alloc_info.push_next(&mut export_info);
            }
            output.memory = self
                .device
                .allocate_memory(
                    &alloc_info
                        .push_next(&mut dedicated)
                        .allocation_size(reqs.size)
                        .memory_type_index(type_index),
                    None,
                )
                .context("allocate export memory")?;
            output.allocation_size = reqs.size;
            self.device.bind_image_memory(output.image, output.memory, 0)?;
            output.mapped = if host_type.is_some() {
                self.device
                    .map_memory(output.memory, 0, vk::WHOLE_SIZE, vk::MemoryMapFlags::empty())
                    .ok()
                    .map(|ptr| ptr as *mut u8)
            } else {
                None
            };
            let layout = if tiling == vk::ImageTiling::OPTIMAL {
                vk::SubresourceLayout::default()
            } else {
                self.device.get_image_subresource_layout(
                    output.image,
                    vk::ImageSubresource {
                        aspect_mask: xr24_layout_aspect(tiling),
                        mip_level: 0,
                        array_layer: 0,
                    },
                )
            };
            output.fd = if export {
                self.ext_mem_fd
                    .get_memory_fd(
                        &vk::MemoryGetFdInfoKHR::default()
                            .memory(output.memory)
                            .handle_type(handle_type),
                    )
                    .context("export dmabuf fd")?
            } else {
                -1
            };
            output.stride = layout.row_pitch as u32;
            output.offset = layout.offset as u32;
            output.direct_render = direct_render;
            Ok(output)
        }
    }
}

fn gl_interop_image_usage(features: vk::FormatFeatureFlags) -> vk::ImageUsageFlags {
    let mut usage = vk::ImageUsageFlags::TRANSFER_SRC | vk::ImageUsageFlags::TRANSFER_DST;
    if features.contains(vk::FormatFeatureFlags::SAMPLED_IMAGE) {
        usage |= vk::ImageUsageFlags::SAMPLED;
    }
    if features.contains(vk::FormatFeatureFlags::STORAGE_IMAGE) {
        usage |= vk::ImageUsageFlags::STORAGE;
    }
    if features.contains(vk::FormatFeatureFlags::COLOR_ATTACHMENT) {
        usage |= vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::INPUT_ATTACHMENT;
    }
    usage
}

fn single_plane_modifiers(
    candidates: &[u64],
    available: &[vk::DrmFormatModifierPropertiesEXT],
) -> Vec<u64> {
    candidates
        .iter()
        .copied()
        .filter(|candidate| {
            available.iter().any(|properties| {
                properties.drm_format_modifier == *candidate
                    && properties.drm_format_modifier_plane_count == 1
            })
        })
        .collect()
}

fn xr24_layout_aspect(tiling: vk::ImageTiling) -> vk::ImageAspectFlags {
    if tiling == vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT {
        vk::ImageAspectFlags::MEMORY_PLANE_0_EXT
    } else {
        vk::ImageAspectFlags::COLOR
    }
}

#[cfg(test)]
mod tests;
