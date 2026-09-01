use super::{
    DeviceChoice, DeviceSelector, parse_device_selector, select_device, software_decode_required,
    unreliable_virtual_driver_name, wanted_device_extension,
};
use ash::vk;

fn choice(
    enumeration_index: usize,
    name: &str,
    device_type: vk::PhysicalDeviceType,
    compositor_match: bool,
) -> DeviceChoice {
    DeviceChoice {
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
