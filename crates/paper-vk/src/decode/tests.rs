#![cfg(test)]

use super::{
    AnyDecoder, CONSECUTIVE_PACKET_ERROR_LIMIT, FOURCC_GR88, FOURCC_NV12, FOURCC_R8, VaapiDecoder,
    container_wrap_is_fatal, decoder_requires_software, descriptor_planes, open_decoder,
    open_vulkan_decoder, packet_error_is_fatal, pump_decode, resolved_thread_count, retain_frame,
    source_requires_software, vaapi_device,
};
use ffmpeg_the_third as ff;
use std::path::{Path, PathBuf};

static VAAPI_DEVICE_TESTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn vaapi_device_tests() -> std::sync::MutexGuard<'static, ()> {
    VAAPI_DEVICE_TESTS.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

const VP8_KEYFRAME_TAG: &[u8] = &[0x50, 0x00, 0x00, 0x9d, 0x01, 0x2a, 0x40, 0x00, 0x40, 0x00];

fn ivf_sample(directory: &Path, frames: &[&[u8]]) -> PathBuf {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"DKIF");
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&32u16.to_le_bytes());
    bytes.extend_from_slice(b"VP80");
    bytes.extend_from_slice(&64u16.to_le_bytes());
    bytes.extend_from_slice(&64u16.to_le_bytes());
    bytes.extend_from_slice(&30u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&(frames.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    for (index, frame) in frames.iter().enumerate() {
        bytes.extend_from_slice(&(frame.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&(index as u64).to_le_bytes());
        bytes.extend_from_slice(frame);
    }
    let path = directory.join("sample.ivf");
    std::fs::write(&path, bytes).expect("write IVF sample");
    path
}

fn still_named_as_video(directory: &Path) -> PathBuf {
    let mut encoded = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image::RgbaImage::new(4, 4))
        .write_to(&mut encoded, image::ImageFormat::Png)
        .expect("encode PNG sample");
    let path = directory.join("still.mp4");
    std::fs::write(&path, encoded.into_inner()).expect("write PNG sample");
    path
}

#[test]
fn cpu_floor_one_thread() {
    assert_eq!(resolved_thread_count(-1, 1366, 768), 1);
    assert_eq!(resolved_thread_count(-1, 1280, 720), 1);
    assert_eq!(resolved_thread_count(-1, 1920, 1080), 2);
    assert_eq!(resolved_thread_count(3, 1366, 768), 3);
}

#[test]
fn png_decoder_linked() {
    ff::init().unwrap();
    assert!(ff::codec::decoder::find(ff::codec::Id::PNG).is_some());
}

#[test]
fn retain_frame_keeps_hw_ctx() {
    let mut frame = ff::frame::Video::new(ff::format::Pixel::NV12, 4, 4);
    unsafe {
        (*frame.as_mut_ptr()).hw_frames_ctx = ff::ffi::av_buffer_alloc(16);
    }
    let retained = retain_frame(&frame).unwrap();
    unsafe {
        let source = (*frame.as_ptr()).hw_frames_ctx;
        let copy = (*retained.as_ptr()).hw_frames_ctx;
        assert!(!source.is_null());
        assert!(!copy.is_null());
        assert_eq!((*source).data, (*copy).data);
    }
}

#[test]
fn packet_error_budget() {
    let mut errors = 0;
    for _ in 1..CONSECUTIVE_PACKET_ERROR_LIMIT {
        assert!(!packet_error_is_fatal(&mut errors));
    }
    assert!(packet_error_is_fatal(&mut errors));

    errors = 0;
    assert_eq!(errors, 0);
    assert!(!packet_error_is_fatal(&mut errors));
}

#[test]
fn preview_paths_need_software() {
    for path in ["preview.gif", "preview.jpg", "preview.webp", "preview.png"] {
        assert!(source_requires_software(path));
    }
    for path in ["wall.mp4", "wall.mkv", "wall.webm"] {
        assert!(!source_requires_software(path));
    }
}

#[test]
fn software_device_no_vulkan() {
    for path in ["wall.mp4", "wall.mkv", "wall.webm"] {
        assert!(decoder_requires_software(path, true));
    }
}

#[test]
fn container_wrap_fatal() {
    let mut wraps = 0;
    assert!(!container_wrap_is_fatal(&mut wraps));
    assert!(container_wrap_is_fatal(&mut wraps));
}

#[test]
fn cascade_falls_back_software() {
    let _serialized = vaapi_device_tests();
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = ivf_sample(directory.path(), &[VP8_KEYFRAME_TAG]);
    let decoder = open_decoder(path.to_str().expect("sample path"), None, true, None, false)
        .expect("cascade must still produce a decoder for a codec Vulkan rejects");
    assert!(matches!(decoder, AnyDecoder::Sw(_)));
}

#[test]
fn vulkan_rung_declines_codec() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = ivf_sample(directory.path(), &[VP8_KEYFRAME_TAG]);
    let opened = open_vulkan_decoder(path.to_str().expect("sample path"), None, true);
    assert!(opened.is_none());
}

#[test]
fn vulkan_rung_needs_queue() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = ivf_sample(directory.path(), &[VP8_KEYFRAME_TAG]);
    assert!(open_vulkan_decoder(path.to_str().expect("sample path"), None, false).is_none());
}

#[test]
fn still_detection_in_decoder() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = still_named_as_video(directory.path());
    let path = path.to_str().expect("sample path");
    assert!(!decoder_requires_software(path, false));
    let decoder =
        open_decoder(path, None, true, None, false).expect("a still container must still decode");
    assert!(matches!(decoder, AnyDecoder::Sw(_)));
    assert!(decoder.still());
}

#[test]
fn vaapi_rung_declines_still() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = still_named_as_video(directory.path());
    let Err(error) = VaapiDecoder::open(path.to_str().expect("sample path"), None) else {
        panic!("a still container has no VAAPI decode path");
    };
    assert!(format!("{error:#}").contains("still image container"), "{error:#}");
}

#[test]
fn vaapi_device_shared() {
    let _serialized = vaapi_device_tests();
    let Ok(first) = vaapi_device(None) else {
        return;
    };
    let second = vaapi_device(None).expect("a second handle on a device that already exists");
    assert!(std::sync::Arc::ptr_eq(&first, &second));
    assert_eq!(first.0, second.0);
}

#[test]
fn vaapi_device_released() {
    let _serialized = vaapi_device_tests();
    let Ok(device) = vaapi_device(None) else {
        return;
    };
    let released = std::sync::Arc::downgrade(&device);
    drop(device);
    assert!(released.upgrade().is_none());
    let rebuilt = vaapi_device(None).expect("a released device must be rebuilt on demand");
    assert!(!rebuilt.0.is_null());
}

#[test]
fn render_node_distinct_devices() {
    let _serialized = vaapi_device_tests();
    let Ok(default_device) = vaapi_device(None) else {
        return;
    };
    let Ok(named) = vaapi_device(Some(Path::new("/dev/dri/renderD128"))) else {
        return;
    };
    assert!(!std::sync::Arc::ptr_eq(&default_device, &named));
}

#[test]
fn pump_decode_starved_stream() {
    ff::init().expect("ffmpeg init");
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = ivf_sample(directory.path(), &[VP8_KEYFRAME_TAG; 3]);
    let mut ictx = ff::format::input(&path).expect("open IVF sample");
    let (stream_index, mut decoder) = {
        let stream = ictx.streams().best(ff::media::Type::Video).expect("video stream");
        let decoder = ff::codec::context::Context::from_parameters(stream.parameters())
            .expect("codec context")
            .decoder()
            .video()
            .expect("VP8 decoder");
        (stream.index(), decoder)
    };
    let starved_stream = stream_index + 1;
    let mut draining = false;
    let outcome =
        pump_decode(&mut ictx, &mut decoder, &mut draining, starved_stream, 1.0 / 30.0, None);
    let Err(error) = outcome else {
        panic!("a stream that yields no frame must terminate instead of looping");
    };
    assert!(format!("{error:#}").contains("full pass of the container"), "{error:#}");
}

#[test]
fn vaapi_odd_dimensions() {
    assert!(super::vaapi_dimensions_supported(1920, 1080));
    assert!(super::vaapi_dimensions_supported(456, 320));
    assert!(!super::vaapi_dimensions_supported(455, 320));
    assert!(!super::vaapi_dimensions_supported(1920, 1081));
}

fn nv12_descriptor() -> ff::ffi::AVDRMFrameDescriptor {
    let mut descriptor: ff::ffi::AVDRMFrameDescriptor = unsafe { std::mem::zeroed() };
    descriptor.nb_objects = 1;
    descriptor.objects[0].fd = 9;
    descriptor.objects[0].size = 16_384;
    descriptor.nb_layers = 1;
    descriptor.layers[0].format = FOURCC_NV12;
    descriptor.layers[0].nb_planes = 2;
    descriptor.layers[0].planes[0].object_index = 0;
    descriptor.layers[0].planes[0].pitch = 128;
    descriptor.layers[0].planes[1].object_index = 0;
    descriptor.layers[0].planes[1].offset = 8192;
    descriptor.layers[0].planes[1].pitch = 128;
    descriptor
}

#[test]
fn drm_single_nv12_layer() {
    let descriptor = nv12_descriptor();
    let (luma, chroma) = descriptor_planes(&descriptor).expect("NV12 descriptor");
    assert_eq!(luma.fd, 9);
    assert_eq!(luma.pitch, 128);
    assert_eq!(chroma.offset, 8192);
}

#[test]
fn drm_split_r8_gr88() {
    let mut descriptor = nv12_descriptor();
    descriptor.nb_layers = 2;
    descriptor.layers[0].format = FOURCC_R8;
    descriptor.layers[0].nb_planes = 1;
    descriptor.layers[1].format = FOURCC_GR88;
    descriptor.layers[1].nb_planes = 1;
    descriptor.layers[1].planes[0] = descriptor.layers[0].planes[1];
    let (_, chroma) = descriptor_planes(&descriptor).expect("split NV12 descriptor");
    assert_eq!(chroma.offset, 8192);
}

#[test]
fn drm_rejects_bad_layers() {
    let mut descriptor = nv12_descriptor();
    descriptor.layers[0].format = u32::from_le_bytes(*b"P010");
    assert!(descriptor_planes(&descriptor).is_err());
    descriptor.layers[0].format = FOURCC_NV12;
    descriptor.layers[0].planes[1].object_index = 1;
    assert!(descriptor_planes(&descriptor).is_err());
}
