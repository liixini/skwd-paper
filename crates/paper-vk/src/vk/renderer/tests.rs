use super::{anisotropy_limit, sampler_anisotropy};
use ash::vk;

#[test]
fn anisotropy_respects_device_limit() {
    assert_eq!(anisotropy_limit(false, 16.0), None);
    assert_eq!(anisotropy_limit(true, 1.0), None);
    assert_eq!(anisotropy_limit(true, 4.0), Some(4.0));
    assert_eq!(anisotropy_limit(true, 16.0), Some(8.0));
    assert_eq!(anisotropy_limit(true, 32.0), Some(8.0));
}

#[test]
fn only_linear_mips_anisotropic() {
    let limit = anisotropy_limit(true, 16.0);
    assert_eq!(sampler_anisotropy(limit, vk::Filter::LINEAR, true, true), 8.0);
    assert_eq!(sampler_anisotropy(limit, vk::Filter::LINEAR, true, false), 1.0);
    assert_eq!(sampler_anisotropy(limit, vk::Filter::LINEAR, false, true), 1.0);
    assert_eq!(sampler_anisotropy(limit, vk::Filter::NEAREST, true, true), 1.0);
    assert_eq!(sampler_anisotropy(None, vk::Filter::LINEAR, true, true), 1.0);
}
