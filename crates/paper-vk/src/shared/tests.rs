use super::{
    DeviceChoice, DeviceSelector, QUEUE_LOCKS, QUEUE_MUTEXES, parse_device_selector, select_device,
    software_decode_required, unreliable_virtual_driver_name, wanted_device_extension,
};
use ash::vk;
use std::ffi::{CStr, c_void};
use std::sync::atomic::{AtomicU32, Ordering};

fn choice(
    enumeration_index: usize,
    name: &str,
    device_type: vk::PhysicalDeviceType,
    compositor_match: bool,
) -> DeviceChoice {
    DeviceChoice {
        uuid: [enumeration_index as u8; 16],
        enumeration_index,
        name: name.into(),
        device_type,
        vendor_id: enumeration_index as u32,
        device_id: enumeration_index as u32,
        compositor_match,
    }
}

#[test]
fn venus_driver_unreliable() {
    assert!(unreliable_virtual_driver_name("Virtio-GPU Venus (NVIDIA GeForce RTX 5080)"));
    assert!(!unreliable_virtual_driver_name("NVIDIA GeForce RTX 5080"));
    assert!(!unreliable_virtual_driver_name("AMD Radeon RX 7900 XTX"));
}

#[test]
fn decode_requires_queue_sync() {
    assert!(!software_decode_required(false, true));
    assert!(software_decode_required(false, false));
    assert!(software_decode_required(true, true));
    assert!(software_decode_required(true, false));
}

#[test]
fn compositor_device_wins() {
    let choices = [
        choice(0, "Integrated", vk::PhysicalDeviceType::INTEGRATED_GPU, false),
        choice(1, "Compositor GPU", vk::PhysicalDeviceType::DISCRETE_GPU, true),
    ];
    assert_eq!(select_device(&choices, &DeviceSelector::Auto).candidate_index, 1);
}

#[test]
fn integrated_low_power_fallback() {
    let choices = [
        choice(0, "Discrete", vk::PhysicalDeviceType::DISCRETE_GPU, false),
        choice(1, "Integrated", vk::PhysicalDeviceType::INTEGRATED_GPU, false),
        choice(2, "Software", vk::PhysicalDeviceType::CPU, false),
    ];
    assert_eq!(select_device(&choices, &DeviceSelector::Auto).candidate_index, 1);
}

#[test]
fn plasma_shared_images_require_the_consumers_device_and_driver() {
    let candidates = [
        super::DeviceCandidate {
            physical_device: vk::PhysicalDevice::null(),
            choice: choice(0, "Intel", vk::PhysicalDeviceType::INTEGRATED_GPU, false),
            drm_nodes: None,
            uuid: [1; 16],
            driver_uuid: [3; 16],
        },
        super::DeviceCandidate {
            physical_device: vk::PhysicalDevice::null(),
            choice: choice(1, "NVIDIA", vk::PhysicalDeviceType::DISCRETE_GPU, false),
            drm_nodes: None,
            uuid: [2; 16],
            driver_uuid: [4; 16],
        },
    ];
    assert_eq!(super::shared_image_device(&candidates, [2; 16], Some([4; 16])), Some(1));
    assert_eq!(super::shared_image_device(&candidates, [1; 16], Some([3; 16])), Some(0));
    assert_eq!(super::shared_image_device(&candidates, [2; 16], Some([3; 16])), None);
    assert_eq!(super::shared_image_device(&candidates, [9; 16], None), None);
}

#[test]
fn explicit_class_overrides() {
    let choices = [
        choice(0, "Integrated", vk::PhysicalDeviceType::INTEGRATED_GPU, true),
        choice(1, "Discrete", vk::PhysicalDeviceType::DISCRETE_GPU, false),
    ];
    let selection = select_device(&choices, &DeviceSelector::Discrete);
    assert_eq!(selection.candidate_index, 1);
    assert!(selection.explicit);
}

#[test]
fn selector_parse_forms() {
    assert_eq!(parse_device_selector(Some("low-power")), DeviceSelector::Integrated);
    assert_eq!(parse_device_selector(Some("high")), DeviceSelector::Discrete);
    assert_eq!(parse_device_selector(Some("index:2")), DeviceSelector::Index(2));
    assert_eq!(
        parse_device_selector(Some("RADV RENOIR")),
        DeviceSelector::Name("radv renoir".into())
    );
}

#[test]
fn foreign_queue_extension() {
    assert!(wanted_device_extension("VK_EXT_queue_family_foreign"));
}

#[test]
fn renderer_extensions() {
    for name in
        ["VK_KHR_swapchain", "VK_KHR_maintenance1", "VK_KHR_maintenance4", "VK_KHR_video_queue"]
    {
        assert!(wanted_device_extension(name), "{name}");
    }
}

#[test]
fn video_decode_extensions() {
    for name in [
        "VK_KHR_video_queue",
        "VK_KHR_video_decode_queue",
        "VK_KHR_video_decode_h264",
        "VK_KHR_video_decode_h265",
        "VK_KHR_video_decode_av1",
    ] {
        assert!(wanted_device_extension(name), "{name}");
    }
}

#[test]
fn disabled_video_features() {
    for name in [
        "VK_KHR_video_maintenance1",
        "VK_KHR_video_maintenance2",
        "VK_KHR_video_decode_vp9",
        "VK_KHR_video_encode_av1",
    ] {
        assert!(!wanted_device_extension(name), "{name}");
    }
}

#[test]
fn unrelated_maintenance_extensions() {
    for name in [
        "VK_KHR_ray_tracing_maintenance1",
        "VK_KHR_swapchain_maintenance1",
        "VK_EXT_swapchain_maintenance1",
    ] {
        assert!(!wanted_device_extension(name), "{name}");
    }
}

#[test]
fn plasma_device_uuid_is_exact_and_validated() {
    assert_eq!(
        super::parse_device_uuid("00112233445566778899aAbBcCdDeEfF").unwrap(),
        [0, 17, 34, 51, 68, 85, 102, 119, 136, 153, 170, 187, 204, 221, 238, 255]
    );
    assert!(super::parse_device_uuid("0").is_err());
    assert!(super::parse_device_uuid("zz112233445566778899aabbccddeeff00").is_err());
}

#[test]
fn explicit_uuid_survives_reordering_and_overrides_integrated_preference() {
    let integrated = choice(1, "Integrated", vk::PhysicalDeviceType::INTEGRATED_GPU, true);
    let discrete = choice(2, "Discrete", vk::PhysicalDeviceType::DISCRETE_GPU, false);
    let selector = parse_device_selector(Some("uuid:02020202020202020202020202020202"));
    let choices = [integrated.clone(), discrete.clone()];
    let selected = select_device(&choices, &selector);
    assert_eq!(choices[selected.candidate_index].uuid, discrete.uuid);
    assert!(selected.explicit);
    let choices = [discrete, integrated];
    assert_eq!(select_device(&choices, &selector).candidate_index, 0);
    let unavailable = select_device(&choices, &DeviceSelector::Uuid([9; 16]));
    assert_eq!(unavailable.candidate_index, 1);
    assert!(!unavailable.explicit);
}

#[test]
fn exact_probe_device() {
    let candidates = [super::DeviceCandidate {
        physical_device: vk::PhysicalDevice::null(),
        choice: choice(0, "Probe", vk::PhysicalDeviceType::DISCRETE_GPU, false),
        drm_nodes: None,
        uuid: [1; 16],
        driver_uuid: [2; 16],
    }];
    let expected = super::DecodeDevice {
        uuid: [1; 16],
        driver_uuid: [2; 16],
        render_node: None,
        primary_node: None,
        fingerprint: String::new(),
    };
    let malformed = || {
        super::parse_device_uuid("malformed inherited Plasma UUID")?;
        unreachable!()
    };
    assert_eq!(
        super::probe_selection(&candidates, Some(&expected), malformed).unwrap().candidate_index,
        0
    );
    assert!(super::probe_selection(&candidates, None, malformed).is_err());
    let conflicting = || {
        Ok(super::DeviceSelection {
            candidate_index: 1,
            reason: "conflicting inherited device".into(),
            explicit: true,
        })
    };
    assert_eq!(
        super::probe_selection(&candidates, Some(&expected), conflicting).unwrap().candidate_index,
        0
    );
    assert_eq!(super::probe_selection(&candidates, None, conflicting).unwrap().candidate_index, 1);
}

static BUSY_DURING_IDLE: AtomicU32 = AtomicU32::new(0);

fn busy_queues() -> u32 {
    (0..QUEUE_LOCKS)
        .filter(|&family| unsafe {
            let lock = std::ptr::addr_of_mut!(QUEUE_MUTEXES[family as usize]);
            let free = libc::pthread_mutex_trylock(lock) == 0;
            if free {
                libc::pthread_mutex_unlock(lock);
            }
            !free
        })
        .count() as u32
}

unsafe extern "system" fn record_idle(_: vk::Device) -> vk::Result {
    BUSY_DURING_IDLE.store(busy_queues(), Ordering::SeqCst);
    vk::Result::SUCCESS
}

fn idle_only(name: &CStr) -> *const c_void {
    if name == c"vkDeviceWaitIdle" { record_idle as *const c_void } else { std::ptr::null() }
}

#[test]
fn idle_excludes_submits() {
    let device = unsafe { ash::Device::load_with(idle_only, vk::Device::null()) };
    crate::shared::wait_idle(&device).expect("stub idle");
    assert_eq!(BUSY_DURING_IDLE.load(Ordering::SeqCst), QUEUE_LOCKS);
}

#[test]
fn ledger_skips_quiet_device() {
    use ash::vk::Handle;

    let (first, second) = (vk::Device::from_raw(1), vk::Device::from_raw(2));
    let mut ledger = super::IdleLedger(Vec::new());
    assert!(!ledger.quiet(first, 5));
    ledger.record(first, 5);
    ledger.record(second, 5);
    assert!(ledger.quiet(first, 5));
    assert!(ledger.quiet(second, 5));
    assert!(!ledger.quiet(first, 6));
    ledger.record(first, 6);
    assert!(ledger.quiet(first, 6));
    assert!(!ledger.quiet(second, 6));
}
