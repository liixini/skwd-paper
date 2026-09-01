use anyhow::{Context, Result, anyhow};
use ash::vk;

use super::Renderer;

impl Renderer {
    pub(crate) fn create_swapchain(
        surface_fns: &ash::khr::surface::Instance,
        swapchain_fns: &ash::khr::swapchain::Device,
        device: &ash::Device,
        phys: vk::PhysicalDevice,
        surface: vk::SurfaceKHR,
        width: u32,
        height: u32,
        headless: bool,
    ) -> Result<(vk::Format, vk::Extent2D, vk::SwapchainKHR, Vec<vk::ImageView>)> {
        unsafe {
            if headless {
                return Ok((
                    vk::Format::B8G8R8A8_UNORM,
                    vk::Extent2D { width, height },
                    vk::SwapchainKHR::null(),
                    Vec::new(),
                ));
            }
            let formats = surface_fns.get_physical_device_surface_formats(phys, surface)?;
            let sf = formats
                .iter()
                .find(|fmt| fmt.format == vk::Format::B8G8R8A8_UNORM)
                .or_else(|| formats.first())
                .copied()
                .ok_or_else(|| anyhow!("no surface formats"))?;
            let caps = surface_fns.get_physical_device_surface_capabilities(phys, surface)?;
            let extent = if caps.current_extent.width != u32::MAX {
                caps.current_extent
            } else {
                vk::Extent2D { width, height }
            };
            let min_images = (caps.min_image_count + 1).min(if caps.max_image_count > 0 {
                caps.max_image_count
            } else {
                u32::MAX
            });
            let swapchain = swapchain_fns
                .create_swapchain(
                    &vk::SwapchainCreateInfoKHR::default()
                        .surface(surface)
                        .min_image_count(min_images)
                        .image_format(sf.format)
                        .image_color_space(sf.color_space)
                        .image_extent(extent)
                        .image_array_layers(1)
                        .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
                        .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
                        .pre_transform(caps.current_transform)
                        .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
                        .present_mode(Self::pick_present_mode(surface_fns, phys, surface))
                        .clipped(true),
                    None,
                )
                .context("create swapchain")?;
            let images = swapchain_fns.get_swapchain_images(swapchain)?;
            let views: Vec<vk::ImageView> = images
                .iter()
                .map(|&img| {
                    device.create_image_view(
                        &vk::ImageViewCreateInfo::default()
                            .image(img)
                            .view_type(vk::ImageViewType::TYPE_2D)
                            .format(sf.format)
                            .subresource_range(vk::ImageSubresourceRange {
                                aspect_mask: vk::ImageAspectFlags::COLOR,
                                base_mip_level: 0,
                                level_count: 1,
                                base_array_layer: 0,
                                layer_count: 1,
                            }),
                        None,
                    )
                })
                .collect::<std::result::Result<_, _>>()?;
            Ok((sf.format, extent, swapchain, views))
        }
    }

    pub(crate) fn recreate_swapchain(&mut self) -> Result<()> {
        unsafe {
            self.device.device_wait_idle()?;
            for fb in self.framebuffers.drain(..) {
                self.device.destroy_framebuffer(fb, None);
            }
            for view in self.views.drain(..) {
                self.device.destroy_image_view(view, None);
            }
            let caps = self
                .surface_fns
                .get_physical_device_surface_capabilities(self.phys, self.surface)?;
            let extent = if caps.current_extent.width != u32::MAX {
                caps.current_extent
            } else {
                self.extent
            };
            let min_images = (caps.min_image_count + 1).min(if caps.max_image_count > 0 {
                caps.max_image_count
            } else {
                u32::MAX
            });
            let new_chain = self.swapchain_fns.create_swapchain(
                &vk::SwapchainCreateInfoKHR::default()
                    .surface(self.surface)
                    .min_image_count(min_images)
                    .image_format(self.format)
                    .image_color_space(vk::ColorSpaceKHR::SRGB_NONLINEAR)
                    .image_extent(extent)
                    .image_array_layers(1)
                    .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
                    .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
                    .pre_transform(caps.current_transform)
                    .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
                    .present_mode(Self::pick_present_mode(
                        &self.surface_fns,
                        self.phys,
                        self.surface,
                    ))
                    .clipped(true)
                    .old_swapchain(self.swapchain),
                None,
            )?;
            self.swapchain_fns.destroy_swapchain(self.swapchain, None);
            self.swapchain = new_chain;
            self.extent = extent;
            let images = self.swapchain_fns.get_swapchain_images(self.swapchain)?;
            for img in images {
                let view = self.device.create_image_view(
                    &vk::ImageViewCreateInfo::default()
                        .image(img)
                        .view_type(vk::ImageViewType::TYPE_2D)
                        .format(self.format)
                        .subresource_range(vk::ImageSubresourceRange {
                            aspect_mask: vk::ImageAspectFlags::COLOR,
                            base_mip_level: 0,
                            level_count: 1,
                            base_array_layer: 0,
                            layer_count: 1,
                        }),
                    None,
                )?;
                let att = [view];
                let fb = self.device.create_framebuffer(
                    &vk::FramebufferCreateInfo::default()
                        .render_pass(self.render_pass)
                        .attachments(&att)
                        .width(extent.width)
                        .height(extent.height)
                        .layers(1),
                    None,
                )?;
                self.views.push(view);
                self.framebuffers.push(fb);
            }
            Ok(())
        }
    }

    fn pick_present_mode(
        surface_fns: &ash::khr::surface::Instance,
        phys: vk::PhysicalDevice,
        surface: vk::SurfaceKHR,
    ) -> vk::PresentModeKHR {
        let want = match std::env::var("SKWD_VK_PRESENT").as_deref() {
            Ok("fifo") => vk::PresentModeKHR::FIFO,
            Ok("immediate") => vk::PresentModeKHR::IMMEDIATE,
            Ok("mailbox") => vk::PresentModeKHR::MAILBOX,
            _ => {
                if std::env::var("SKWD_VK_VSYNC").as_deref() == Ok("0") {
                    vk::PresentModeKHR::MAILBOX
                } else {
                    vk::PresentModeKHR::FIFO
                }
            }
        };
        let supported = unsafe {
            surface_fns.get_physical_device_surface_present_modes(phys, surface).unwrap_or_default()
        };
        let mode = if supported.contains(&want) { want } else { vk::PresentModeKHR::FIFO };
        let label = if mode == vk::PresentModeKHR::MAILBOX {
            "mailbox"
        } else if mode == vk::PresentModeKHR::IMMEDIATE {
            "immediate"
        } else {
            "fifo"
        };
        tracing::info!("skwd-wall-vk: present mode {label}");
        mode
    }
}
