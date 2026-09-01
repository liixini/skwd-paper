use anyhow::{Context, Result, anyhow};
use ash::vk;
use ffmpeg_the_third as ff;
use std::ffi::CString;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
enum DeviceSelector {
    Auto,
    Integrated,
    Discrete,
    Cpu,
    Index(usize),
    Name(String),
}

#[derive(Clone)]
struct DeviceChoice {
    enumeration_index: usize,
    name: String,
    device_type: vk::PhysicalDeviceType,
    vendor_id: u32,
    device_id: u32,
    compositor_match: bool,
}

#[derive(Clone, Copy, Debug)]
struct DrmNodes {
    primary: Option<(u32, u32)>,
    render: Option<(u32, u32)>,
}

struct DeviceCandidate {
    physical_device: vk::PhysicalDevice,
    choice: DeviceChoice,
    drm_nodes: Option<DrmNodes>,
}

struct DeviceSelection {
    candidate_index: usize,
    reason: String,
    explicit: bool,
}

fn wanted_device_extension(name: &str) -> bool {
    [
        "video",
        "external_memory",
        "external_semaphore",
        "external_fence",
        "drm",
        "swapchain",
        "push_descriptor",
        "atomic_float",
        "timeline_semaphore",
        "synchronization2",
        "maintenance",
        "sampler_ycbcr",
        "descriptor_buffer",
        "shader_object",
        "expect_assume",
        "subgroup_rotate",
        "cooperative_matrix",
        "queue_family_foreign",
    ]
    .iter()
    .any(|wanted| name.contains(wanted))
}

pub struct SharedDevice {
    pub entry: ash::Entry,
    pub instance: ash::Instance,
    pub phys: vk::PhysicalDevice,
    pub device: ash::Device,
    pub gfx_family: u32,
    pub queue: vk::Queue,
    pub hwdev: *mut ff::ffi::AVBufferRef,
    pub queue_sync: bool,
    pub software: bool,
    pub foreign_queue: bool,
    pub video_decode: bool,
    pub render_node: Option<PathBuf>,
    pub decode_fingerprint: String,
}

unsafe impl Send for SharedDevice {}
unsafe impl Sync for SharedDevice {}

static mut QUEUE_MUTEXES: [libc::pthread_mutex_t; 64] = [libc::PTHREAD_MUTEX_INITIALIZER; 64];

unsafe extern "C" fn lock_queue(_: *mut ff::ffi::AVHWDeviceContext, family: u32, _: u32) {
    unsafe {
        libc::pthread_mutex_lock(std::ptr::addr_of_mut!(QUEUE_MUTEXES[family as usize % 64]));
    }
}

unsafe extern "C" fn unlock_queue(_: *mut ff::ffi::AVHWDeviceContext, family: u32, _: u32) {
    unsafe {
        libc::pthread_mutex_unlock(std::ptr::addr_of_mut!(QUEUE_MUTEXES[family as usize % 64]));
    }
}

pub fn lock_queue_family(family: u32) {
    unsafe {
        libc::pthread_mutex_lock(std::ptr::addr_of_mut!(QUEUE_MUTEXES[family as usize % 64]));
    }
}

pub fn unlock_queue_family(family: u32) {
    unsafe {
        libc::pthread_mutex_unlock(std::ptr::addr_of_mut!(QUEUE_MUTEXES[family as usize % 64]));
    }
}

pub(crate) fn software_decode_required(software: bool, queue_sync: bool) -> bool {
    software || !queue_sync
}

pub fn create(display: *mut std::ffi::c_void) -> Result<SharedDevice> {
    create_with_policy(display, true)
}

pub(crate) fn unreliable_virtual_driver_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.contains("virtio-gpu venus") || name.contains("mesa venus")
}

fn parse_device_selector(value: Option<&str>) -> DeviceSelector {
    let value = value.unwrap_or("auto").trim();
    match value.to_ascii_lowercase().as_str() {
        "" | "auto" | "default" => DeviceSelector::Auto,
        "integrated" | "low" | "low-power" => DeviceSelector::Integrated,
        "discrete" | "high" | "high-performance" => DeviceSelector::Discrete,
        "cpu" | "software" => DeviceSelector::Cpu,
        value => {
            let index = value.strip_prefix("index:").unwrap_or(value);
            index
                .parse()
                .map_or_else(|_| DeviceSelector::Name(value.to_string()), DeviceSelector::Index)
        }
    }
}

fn device_selector_name(selector: &DeviceSelector) -> String {
    match selector {
        DeviceSelector::Auto => "auto".into(),
        DeviceSelector::Integrated => "integrated".into(),
        DeviceSelector::Discrete => "discrete".into(),
        DeviceSelector::Cpu => "cpu".into(),
        DeviceSelector::Index(index) => format!("index:{index}"),
        DeviceSelector::Name(name) => name.clone(),
    }
}

fn device_type_name(device_type: vk::PhysicalDeviceType) -> &'static str {
    match device_type {
        vk::PhysicalDeviceType::INTEGRATED_GPU => "integrated",
        vk::PhysicalDeviceType::DISCRETE_GPU => "discrete",
        vk::PhysicalDeviceType::VIRTUAL_GPU => "virtual",
        vk::PhysicalDeviceType::CPU => "cpu",
        _ => "other",
    }
}

fn low_power_rank(device_type: vk::PhysicalDeviceType) -> u8 {
    match device_type {
        vk::PhysicalDeviceType::INTEGRATED_GPU => 0,
        vk::PhysicalDeviceType::VIRTUAL_GPU => 1,
        vk::PhysicalDeviceType::DISCRETE_GPU => 2,
        vk::PhysicalDeviceType::OTHER => 3,
        vk::PhysicalDeviceType::CPU => 4,
        _ => 5,
    }
}

fn stable_identity(choice: &DeviceChoice) -> (String, u32, u32, usize) {
    (choice.name.to_ascii_lowercase(), choice.vendor_id, choice.device_id, choice.enumeration_index)
}

fn automatic_selection(choices: &[DeviceChoice]) -> DeviceSelection {
    let compositor_matches: Vec<usize> = choices
        .iter()
        .enumerate()
        .filter_map(|(index, choice)| choice.compositor_match.then_some(index))
        .collect();
    if let Some(candidate_index) =
        compositor_matches.into_iter().min_by_key(|&index| stable_identity(&choices[index]))
    {
        return DeviceSelection {
            candidate_index,
            reason: "compositor DRM device".into(),
            explicit: false,
        };
    }

    let candidate_index = (0..choices.len())
        .min_by_key(|&index| {
            let choice = &choices[index];
            (low_power_rank(choice.device_type), stable_identity(choice))
        })
        .expect("automatic_selection requires at least one candidate");
    DeviceSelection {
        candidate_index,
        reason: "automatic low-power preference".into(),
        explicit: false,
    }
}

fn select_device(choices: &[DeviceChoice], selector: &DeviceSelector) -> DeviceSelection {
    if *selector == DeviceSelector::Auto {
        return automatic_selection(choices);
    }

    let matching: Vec<usize> = choices
        .iter()
        .enumerate()
        .filter_map(|(index, choice)| {
            let matched = match selector {
                DeviceSelector::Auto => false,
                DeviceSelector::Integrated => {
                    choice.device_type == vk::PhysicalDeviceType::INTEGRATED_GPU
                }
                DeviceSelector::Discrete => {
                    choice.device_type == vk::PhysicalDeviceType::DISCRETE_GPU
                }
                DeviceSelector::Cpu => choice.device_type == vk::PhysicalDeviceType::CPU,
                DeviceSelector::Index(wanted) => choice.enumeration_index == *wanted,
                DeviceSelector::Name(wanted) => {
                    choice.name.to_ascii_lowercase().contains(wanted.as_str())
                }
            };
            matched.then_some(index)
        })
        .collect();

    if let Some(candidate_index) = matching.into_iter().min_by_key(|&index| {
        let choice = &choices[index];
        let exact_name_rank = match selector {
            DeviceSelector::Name(wanted) if choice.name.eq_ignore_ascii_case(wanted) => 0,
            DeviceSelector::Name(_) => 1,
            _ => 0,
        };
        (exact_name_rank, stable_identity(choice))
    }) {
        return DeviceSelection {
            candidate_index,
            reason: format!("SKWD_VK_DEVICE={}", device_selector_name(selector)),
            explicit: true,
        };
    }

    let mut selection = automatic_selection(choices);
    selection.reason = format!("{} (unmatched SKWD_VK_DEVICE override)", selection.reason);
    selection
}

fn drm_node_matches(nodes: DrmNodes, device: libc::dev_t) -> bool {
    let wanted = (libc::major(device), libc::minor(device));
    nodes.primary == Some(wanted) || nodes.render == Some(wanted)
}

fn render_node_for(device: Option<(u32, u32)>) -> Option<PathBuf> {
    let wanted = device?;
    std::fs::read_dir("/dev/dri").ok()?.filter_map(Result::ok).map(|entry| entry.path()).find(
        |path| {
            path.file_name().is_some_and(|name| name.as_encoded_bytes().starts_with(b"renderD"))
                && std::fs::metadata(path).is_ok_and(|metadata| {
                    metadata.file_type().is_char_device()
                        && (libc::major(metadata.rdev()), libc::minor(metadata.rdev())) == wanted
                })
        },
    )
}

fn drm_nodes_for_device(
    instance: &ash::Instance,
    physical_device: vk::PhysicalDevice,
) -> Result<Option<DrmNodes>> {
    let extensions = unsafe { instance.enumerate_device_extension_properties(physical_device)? };
    let supports_drm = extensions.iter().any(|extension| {
        extension.extension_name_as_c_str().ok() == Some(ash::ext::physical_device_drm::NAME)
    });
    if !supports_drm {
        return Ok(None);
    }

    let mut drm = vk::PhysicalDeviceDrmPropertiesEXT::default();
    let mut properties = vk::PhysicalDeviceProperties2::default().push_next(&mut drm);
    unsafe {
        instance.get_physical_device_properties2(physical_device, &mut properties);
    }
    let primary = (drm.has_primary != vk::FALSE)
        .then_some((drm.primary_major as u32, drm.primary_minor as u32));
    let render =
        (drm.has_render != vk::FALSE).then_some((drm.render_major as u32, drm.render_minor as u32));
    Ok(Some(DrmNodes { primary, render }))
}

fn create_with_policy(
    display: *mut std::ffi::c_void,
    avoid_unreliable_virtual_driver: bool,
) -> Result<SharedDevice> {
    unsafe {
        let entry = ash::Entry::load().context("load libvulkan")?;
        let app = vk::ApplicationInfo::default()
            .application_name(c"skwd-wall-vk")
            .api_version(vk::API_VERSION_1_3);
        let inst_exts = [
            ash::khr::surface::NAME.as_ptr(),
            ash::khr::wayland_surface::NAME.as_ptr(),
            ash::khr::get_physical_device_properties2::NAME.as_ptr(),
        ];
        let instance = entry
            .create_instance(
                &vk::InstanceCreateInfo::default()
                    .application_info(&app)
                    .enabled_extension_names(&inst_exts),
                None,
            )
            .context("create instance")?;

        let compositor_device = crate::wayland::compositor_drm_device();
        let mut candidates = Vec::new();
        for (enumeration_index, physical_device) in
            instance.enumerate_physical_devices()?.into_iter().enumerate()
        {
            let props = instance.get_physical_device_properties(physical_device);
            if props.api_version < vk::API_VERSION_1_3 {
                continue;
            }
            let has_graphics = instance
                .get_physical_device_queue_family_properties(physical_device)
                .iter()
                .any(|family| family.queue_flags.contains(vk::QueueFlags::GRAPHICS));
            if !has_graphics {
                continue;
            }
            let name = props
                .device_name_as_c_str()
                .map_or_else(|_| String::new(), |name| name.to_string_lossy().into_owned());
            let drm_nodes = drm_nodes_for_device(&instance, physical_device)?;
            let compositor_match = compositor_device.is_some_and(|device| {
                drm_nodes.is_some_and(|nodes| drm_node_matches(nodes, device))
            });
            candidates.push(DeviceCandidate {
                physical_device,
                choice: DeviceChoice {
                    enumeration_index,
                    name,
                    device_type: props.device_type,
                    vendor_id: props.vendor_id,
                    device_id: props.device_id,
                    compositor_match,
                },
                drm_nodes,
            });
        }
        if candidates.is_empty() {
            return Err(anyhow!("no Vulkan 1.3 device with a graphics queue"));
        }
        for candidate in &candidates {
            tracing::info!(
                index = candidate.choice.enumeration_index,
                device = candidate.choice.name,
                kind = device_type_name(candidate.choice.device_type),
                vendor = format_args!("{:#06x}", candidate.choice.vendor_id),
                device_id = format_args!("{:#06x}", candidate.choice.device_id),
                drm_primary = ?candidate.drm_nodes.and_then(|nodes| nodes.primary),
                drm_render = ?candidate.drm_nodes.and_then(|nodes| nodes.render),
                compositor_match = candidate.choice.compositor_match,
                "skwd-wall-vk: Vulkan device candidate"
            );
        }

        let selector_value = std::env::var("SKWD_VK_DEVICE").ok();
        let selector = parse_device_selector(selector_value.as_deref());
        let choices: Vec<DeviceChoice> =
            candidates.iter().map(|candidate| candidate.choice.clone()).collect();
        let mut selection = select_device(&choices, &selector);
        if selector != DeviceSelector::Auto && !selection.explicit {
            tracing::warn!(
                value = selector_value.unwrap_or_default(),
                "skwd-wall-vk: Vulkan device override did not match; using automatic selection"
            );
        }
        if avoid_unreliable_virtual_driver
            && !selection.explicit
            && unreliable_virtual_driver_name(&choices[selection.candidate_index].name)
            && let Some(cpu_index) = choices
                .iter()
                .enumerate()
                .filter(|(_, choice)| choice.device_type == vk::PhysicalDeviceType::CPU)
                .min_by_key(|(_, choice)| stable_identity(choice))
                .map(|(index, _)| index)
        {
            selection.candidate_index = cpu_index;
            selection.reason = "software fallback for unreliable virtual driver".into();
        }
        let selected = &candidates[selection.candidate_index];
        let phys = selected.physical_device;
        let selected_props = instance.get_physical_device_properties(phys);
        tracing::info!(
            index = selected.choice.enumeration_index,
            device = selected.choice.name,
            kind = device_type_name(selected.choice.device_type),
            reason = selection.reason,
            "skwd-wall-vk: selected Vulkan device"
        );

        let ext_props = instance.enumerate_device_extension_properties(phys)?;
        let ext_names: Vec<CString> = ext_props
            .iter()
            .filter_map(|ext| ext.extension_name_as_c_str().ok().map(|name| name.to_owned()))
            .filter(|cname| {
                let name = cname.to_str().unwrap_or("");
                wanted_device_extension(name)
            })
            .collect();
        let ext_ptrs: Vec<*const i8> = ext_names.iter().map(|cname| cname.as_ptr()).collect();
        let foreign_queue =
            ext_names.iter().any(|name| name.as_c_str() == ash::ext::queue_family_foreign::NAME);

        let f11: &'static mut vk::PhysicalDeviceVulkan11Features<'static> =
            Box::leak(Box::new(vk::PhysicalDeviceVulkan11Features::default()));
        let f12: &'static mut vk::PhysicalDeviceVulkan12Features<'static> =
            Box::leak(Box::new(vk::PhysicalDeviceVulkan12Features::default()));
        let f13: &'static mut vk::PhysicalDeviceVulkan13Features<'static> =
            Box::leak(Box::new(vk::PhysicalDeviceVulkan13Features::default()));
        let feats: &'static mut vk::PhysicalDeviceFeatures2<'static> =
            Box::leak(Box::new(vk::PhysicalDeviceFeatures2::default()));
        f12.p_next = (f13 as *mut vk::PhysicalDeviceVulkan13Features).cast();
        f11.p_next = (f12 as *mut vk::PhysicalDeviceVulkan12Features).cast();
        feats.p_next = (f11 as *mut vk::PhysicalDeviceVulkan11Features).cast();
        instance.get_physical_device_features2(phys, feats);

        let qf_props_len = instance.get_physical_device_queue_family_properties(phys).len();
        let mut video_props: Vec<vk::QueueFamilyVideoPropertiesKHR> =
            vec![vk::QueueFamilyVideoPropertiesKHR::default(); qf_props_len];
        let mut qf2: Vec<vk::QueueFamilyProperties2> = video_props
            .iter_mut()
            .map(|vp| vk::QueueFamilyProperties2::default().push_next(vp))
            .collect();
        instance.get_physical_device_queue_family_properties2(phys, &mut qf2);

        let mut gfx_family: Option<u32> = None;
        let mut queue_infos: Vec<vk::DeviceQueueCreateInfo> = Vec::new();
        let prio = [1.0f32; 1];
        let mut families: Vec<(u32, vk::QueueFlags)> = Vec::new();
        for (idx, qf) in qf2.iter().enumerate() {
            let flags = qf.queue_family_properties.queue_flags;
            let interesting = flags.intersects(
                vk::QueueFlags::GRAPHICS
                    | vk::QueueFlags::COMPUTE
                    | vk::QueueFlags::TRANSFER
                    | vk::QueueFlags::VIDEO_DECODE_KHR,
            );
            if !interesting {
                continue;
            }
            if gfx_family.is_none() && flags.contains(vk::QueueFlags::GRAPHICS) {
                gfx_family = Some(idx as u32);
            }
            families.push((idx as u32, flags));
            queue_infos.push(
                vk::DeviceQueueCreateInfo::default()
                    .queue_family_index(idx as u32)
                    .queue_priorities(&prio),
            );
        }
        let gfx_family = gfx_family.ok_or_else(|| anyhow!("no graphics queue family"))?;

        let mut device_info = vk::DeviceCreateInfo::default()
            .queue_create_infos(&queue_infos)
            .enabled_extension_names(&ext_ptrs);
        device_info.p_next = (feats as *mut vk::PhysicalDeviceFeatures2).cast();
        let device = instance
            .create_device(phys, &device_info, None)
            .context("create device (all extensions)")?;
        let queue = device.get_device_queue(gfx_family, 0);

        let hwdev = ff::ffi::av_hwdevice_ctx_alloc(ff::ffi::AVHWDeviceType::VULKAN);
        if hwdev.is_null() {
            return Err(anyhow!("av_hwdevice_ctx_alloc failed"));
        }
        let dev_ctx = (*hwdev).data as *mut ff::ffi::AVHWDeviceContext;
        let vkctx = (*dev_ctx).hwctx.cast::<std::ffi::c_void>();

        let inst_ext_names: &'static [CString] = Vec::leak(vec![
            CString::new("VK_KHR_surface").unwrap(),
            CString::new("VK_KHR_wayland_surface").unwrap(),
            CString::new("VK_KHR_get_physical_device_properties2").unwrap(),
        ]);
        let inst_ext_ptrs: &'static [*const i8] =
            Vec::leak(inst_ext_names.iter().map(|cname| cname.as_ptr()).collect());
        let dev_ext_names: &'static [CString] = Vec::leak(ext_names);
        let dev_ext_ptrs: &'static [*const i8] =
            Vec::leak(dev_ext_names.iter().map(|cname| cname.as_ptr()).collect());

        let queue_for = |required: vk::QueueFlags| {
            families
                .iter()
                .find_map(|(idx, flags)| flags.contains(required).then_some(*idx))
                .unwrap_or(gfx_family)
        };
        let video_queue = |mask: u32| {
            video_props.iter().enumerate().find_map(|(idx, props)| {
                (props.video_codec_operations.as_raw() & mask != 0).then_some(idx as i32)
            })
        };
        let queues = families
            .iter()
            .map(|(idx, flags)| crate::ffmpeg_vulkan::DeviceQueue {
                index: *idx,
                flags: *flags,
                video_caps: video_props
                    .get(*idx as usize)
                    .map(|props| props.video_codec_operations)
                    .unwrap_or_default(),
            })
            .collect::<Vec<_>>();
        let queue_sync = crate::ffmpeg_vulkan::configure_device(
            vkctx,
            entry.static_fn().get_instance_proc_addr as usize,
            instance.handle(),
            phys,
            device.handle(),
            feats,
            inst_ext_ptrs,
            dev_ext_ptrs,
            &queues,
            crate::ffmpeg_vulkan::LegacyQueues {
                graphics: gfx_family as i32,
                transfer: queue_for(vk::QueueFlags::TRANSFER) as i32,
                compute: queue_for(vk::QueueFlags::COMPUTE) as i32,
                encode: video_queue(0xffff_0000).unwrap_or(-1),
                decode: video_queue(0x0000_ffff).unwrap_or(-1),
            },
            lock_queue as *const () as usize,
            unlock_queue as *const () as usize,
        )
        .ok_or_else(|| anyhow!("configure FFmpeg Vulkan device context failed"))?;

        let rc = ff::ffi::av_hwdevice_ctx_init(hwdev);
        if rc < 0 {
            return Err(anyhow!("av_hwdevice_ctx_init failed ({rc})"));
        }
        let _ = display;
        let software = selected_props.device_type == vk::PhysicalDeviceType::CPU;
        let video_decode = video_queue(0x0000_ffff).is_some();
        let render_node = render_node_for(selected.drm_nodes.and_then(|nodes| nodes.render));
        let kernel = std::fs::read_to_string("/proc/sys/kernel/osrelease")
            .unwrap_or_default()
            .trim()
            .to_string();
        let ffmpeg = std::ffi::CStr::from_ptr(ff::ffi::av_version_info()).to_string_lossy();
        let decode_fingerprint = format!(
            "{}:{:04x}:{:04x}:{}:{}:{}:{}",
            selected.choice.name,
            selected.choice.vendor_id,
            selected.choice.device_id,
            selected_props.driver_version,
            kernel,
            ffmpeg,
            env!("CARGO_PKG_VERSION")
        );
        Ok(SharedDevice {
            entry,
            instance,
            phys,
            device,
            gfx_family,
            queue,
            hwdev,
            queue_sync,
            software,
            foreign_queue,
            video_decode,
            render_node,
            decode_fingerprint,
        })
    }
}

#[cfg(test)]
mod tests;
