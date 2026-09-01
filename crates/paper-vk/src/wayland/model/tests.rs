use super::parse_drm_device;

#[test]
fn native_dev_t() {
    let expected = libc::makedev(226, 128);
    assert_eq!(parse_drm_device(&expected.to_ne_bytes()), Some(expected));
    assert_eq!(parse_drm_device(&[1, 2, 3]), None);
}
