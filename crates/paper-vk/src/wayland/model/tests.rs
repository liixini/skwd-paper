use super::{BufferGenerations, parse_drm_device};

#[test]
fn native_dev_t() {
    let expected = libc::makedev(226, 128);
    assert_eq!(parse_drm_device(&expected.to_ne_bytes()), Some(expected));
    assert_eq!(parse_drm_device(&[1, 2, 3]), None);
}

#[test]
fn retired_buffer_release_isolated() {
    let mut buffers = BufferGenerations::default();
    let first = buffers.rotate(2);
    assert!(!buffers.release(first));
    assert!(!buffers.retired(first));
    let second = buffers.rotate(1);
    assert!(!buffers.release(first));
    assert!(buffers.retired(first));
    assert!(!buffers.release(second));
    assert!(buffers.retired(second));
    assert!(buffers.release(buffers.current()));
}

#[test]
fn staged_generation_preserves_releases() {
    let mut buffers = BufferGenerations::default();
    let old = buffers.current();
    buffers.stage(true);
    let candidate = buffers.allocation();
    assert_ne!(candidate, old);
    assert!(buffers.release(old));
    assert!(!buffers.release(candidate));
    buffers.stage(false);
    assert_eq!(buffers.allocation(), old);
    assert!(buffers.release(old));
    assert_eq!(buffers.rotate(1), old);
    assert_eq!(buffers.current(), candidate);
    assert!(!buffers.retired(old));
    assert!(!buffers.release(old));
    assert!(buffers.retired(old));
    assert!(buffers.release(candidate));
}
