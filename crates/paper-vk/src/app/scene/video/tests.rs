use super::Timeline;
use crate::decode::AnyDecoder;
use paper_scene::model::{Video, VideoRange};
use std::sync::Arc;

#[test]
fn video_pts_advance_at_source_rate_and_continue_across_loops() {
    let mut clock = Timeline::new(3.0);
    assert!((clock.next(3.04) - 0.04).abs() < 0.000001);
    assert!((clock.next(3.08) - 0.08).abs() < 0.000001);
    assert!((clock.next(3.0) - 0.12).abs() < 0.000001);
    assert!((clock.next(3.04) - 0.16).abs() < 0.000001);
}

#[test]
fn repeated_pts_do_not_spin_at_the_same_deadline() {
    let mut clock = Timeline::new(0.0);
    assert!(clock.next(0.0) > 0.0);
    let previous = clock.due;
    assert!(clock.next(0.0) > previous);
}

#[test]
#[ignore = "requires Vulkan and SKWD_WE_VIDEO_PACKAGE"]
fn embedded_video_decodes_moves_and_loops_on_gpu() {
    let path = std::env::var("SKWD_WE_VIDEO_PACKAGE").expect("scene.pkg path");
    let pkg = paper_scene::pkg::Package::open(std::path::Path::new(&path)).unwrap();
    let entry = pkg
        .entries()
        .iter()
        .find(|entry| {
            paper_scene::tex::video_span(pkg.read(entry)).is_ok_and(|span| span.is_some())
        })
        .expect("embedded video");
    let span = paper_scene::tex::video_span(pkg.read(entry)).unwrap().unwrap();
    let (file, base) = pkg.origin(&entry.path).unwrap();
    let mut raw = paper_scene::tex::parse(pkg.read(entry), None).unwrap();
    let sources = [
        Video::Bytes(std::mem::take(&mut raw.images[0][0].data).into()),
        Video::Range(VideoRange { file, offset: base + span.offset, length: span.length }),
    ];
    let sd = crate::shared::test_device();
    let allocations = frame_allocations();
    let frames: Vec<_> = sources.iter().map(|source| playback_frames(&sd, source)).collect();
    assert_eq!(frames[0], frames[1]);
    assert_eq!(frame_allocations(), allocations);
}

fn frame_allocations() -> [usize; 2] {
    [crate::vk::UploadPath::live_allocations(), crate::vk::FrameImages::live_allocations()]
}

fn playback_frames(sd: &crate::shared::SharedDevice, source: &Video) -> [Vec<u8>; 3] {
    let mut video = super::VideoTexture::open(sd, source, true, false).unwrap();
    let first = video.renderer.read_scene_target(video.target.as_ref().unwrap()).unwrap().2;
    video.advance(1.0).unwrap();
    let second = video.renderer.read_scene_target(video.target.as_ref().unwrap()).unwrap().2;
    assert_ne!(first, second, "video must advance");
    let mut seen_loop = false;
    for frame in 31..=1260 {
        let previous_pts = video.timeline.pts;
        video.advance(f64::from(frame) / 30.0).unwrap();
        seen_loop |= video.timeline.pts < previous_pts;
    }
    assert!(seen_loop, "video PTS rewind");
    let looped = video.renderer.read_scene_target(video.target.as_ref().unwrap()).unwrap().2;
    assert_ne!(second, looped, "video continues after loop");
    video.command(paper_scene::script::VideoOp::Pause).unwrap();
    video.advance(50.0).unwrap();
    assert_eq!(video.renderer.read_scene_target(video.target.as_ref().unwrap()).unwrap().2, looped);
    video.command(paper_scene::script::VideoOp::Seek(1.0)).unwrap();
    assert_eq!(video.renderer.read_scene_target(video.target.as_ref().unwrap()).unwrap().2, second);
    video.command(paper_scene::script::VideoOp::Stop).unwrap();
    assert_eq!(video.renderer.read_scene_target(video.target.as_ref().unwrap()).unwrap().2, first);
    let decoder = video.ahead.stop().unwrap();
    assert!(matches!(decoder, AnyDecoder::Vk(_) | AnyDecoder::Vaapi(_)));
    if crate::decode::vaapi_decode_forced() {
        assert!(matches!(decoder, AnyDecoder::Vaapi(_)));
        assert!(video.slot.as_ref().is_some_and(crate::app::shared_support::FrameSlot::imported));
    }
    [first, second, looped]
}

#[test]
fn playback_clock_controls() {
    let mut clock = super::Playback::new(2.0);
    clock.advance(1.0);
    assert_eq!(clock.position(), 1.0);
    clock.playing = false;
    clock.advance(10.0);
    assert_eq!(clock.position(), 1.0);
    clock.playing = true;
    clock.rate = 2.0;
    clock.advance(10.5);
    assert_eq!(clock.position(), 0.0);
    assert_eq!(clock.ended, 1);
    clock.looping = false;
    clock.advance(11.0);
    assert_eq!(clock.position(), 2.0);
    assert!(!clock.playing);
    assert_eq!(clock.ended, 2);
}

fn video_payload() -> Vec<u8> {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("controls.mp4");
    let encoded = std::process::Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=red:s=16x16:r=10:d=1",
            "-f",
            "lavfi",
            "-i",
            "color=blue:s=16x16:r=10:d=1",
            "-filter_complex",
            "[0:v][1:v]concat=n=2:v=1:a=0",
            "-c:v",
            "mpeg4",
            "-q:v",
            "2",
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(&path)
        .output()
        .unwrap();
    assert!(encoded.status.success(), "{}", String::from_utf8_lossy(&encoded.stderr));
    std::fs::read(path).unwrap()
}

#[test]
#[ignore = "requires Vulkan and ffmpeg"]
fn video_controls_pixels() {
    use paper_scene::script::VideoOp;
    let shared = crate::shared::test_device();
    let allocations = crate::vk::Renderer::scene_target_live_allocations();
    let frames = frame_allocations();
    let mut video =
        super::VideoTexture::open(&shared, &Video::Bytes(video_payload().into()), true, false)
            .unwrap();
    let pixel = |video: &mut super::VideoTexture| {
        let (_, _, pixels) =
            video.renderer.read_scene_target(video.target.as_ref().unwrap()).unwrap();
        if video.target().format == ash::vk::Format::B8G8R8A8_UNORM {
            [pixels[2], pixels[1], pixels[0]]
        } else {
            [pixels[0], pixels[1], pixels[2]]
        }
    };
    let red = pixel(&mut video);
    assert!(red[0] > 200 && red[2] < 20, "{red:?}");
    assert!((video.state(0).duration - 2.0).abs() < 0.01);
    video.advance(1.2).unwrap();
    let blue = pixel(&mut video);
    assert!(blue[2] > 200 && blue[0] < 20, "{blue:?}");
    video.command(VideoOp::Pause).unwrap();
    video.advance(5.0).unwrap();
    assert_eq!(pixel(&mut video), blue);
    assert!(!video.playing());
    video.command(VideoOp::Seek(0.2)).unwrap();
    assert_eq!(pixel(&mut video), red);
    video.advance(5.5).unwrap();
    assert_eq!(pixel(&mut video), red);
    video.command(VideoOp::Rate(2.0)).unwrap();
    video.command(VideoOp::Play).unwrap();
    video.advance(5.95).unwrap();
    assert_eq!(pixel(&mut video), blue);
    video.command(VideoOp::Stop).unwrap();
    assert_eq!(video.state(0).position, 0.0);
    assert_eq!(pixel(&mut video), red);
    video.command(VideoOp::Loop(false)).unwrap();
    video.command(VideoOp::Play).unwrap();
    video.advance(8.0).unwrap();
    assert_eq!(pixel(&mut video), blue);
    assert!(!video.playing());
    assert_eq!(video.state(0).ended, 1);
    video.command(VideoOp::Loop(true)).unwrap();
    video.command(VideoOp::Seek(1.8)).unwrap();
    video.command(VideoOp::Rate(1.0)).unwrap();
    video.command(VideoOp::Play).unwrap();
    video.advance(8.3).unwrap();
    assert_eq!(pixel(&mut video), red);
    video.command(VideoOp::Seek(1.2)).unwrap();
    video.command(VideoOp::Rate(-1.0)).unwrap();
    video.advance(8.8).unwrap();
    assert_eq!(pixel(&mut video), red);
    drop(video);
    assert_eq!(crate::vk::Renderer::scene_target_live_allocations(), allocations);
    assert_eq!(frame_allocations(), frames);
}

fn video_model(objects: &serde_json::Value) -> paper_scene::model::SceneModel {
    let payload = video_payload();
    let mut texture = b"TEXV0005\0TEXI0001\0".to_vec();
    for value in [0_u32, paper_scene::tex::FLAG_IS_VIDEO, 16, 16, 16, 16, 0] {
        texture.extend(value.to_le_bytes());
    }
    texture.extend(b"TEXB0003\0");
    for value in [1_i32, -1, 1, 16, 16, 0, payload.len() as i32, payload.len() as i32] {
        texture.extend(value.to_le_bytes());
    }
    texture.extend(payload);
    let source = serde_json::json!({"general":{"orthogonalprojection":{"width":64,"height":64}},"objects":objects}).to_string();
    super::super::effect_lifetime_tests::model_files(&[
        ("scene.json", source.as_bytes()),
        ("models/video.json", br#"{"material":"materials/video.json"}"#),
        (
            "materials/video.json",
            br#"{"passes":[{"shader":"genericimage2","textures":["video"]}]}"#,
        ),
        ("materials/video.tex", &texture),
    ])
}

#[test]
#[ignore = "requires Vulkan and ffmpeg"]
fn script_video_pixels() {
    use super::super::{FillMode, build_group};
    let script = r"
let stage=-1;
export function update(value) {
    const next=Math.floor(engine.runtime);
    if(next===stage)return value;
    stage=next;
    const video=thisLayer.getVideoTexture();
    if(stage===0){video.pause();video.setCurrentTime(0.2);}
    if(stage===1)video.setCurrentTime(1.2);
    if(stage===2)video.stop();
    if(stage===3){video.rate=2;video.play();}
    return value;
}
";
    let mut model = video_model(&serde_json::json!([{
        "id":1,"image":"models/video.json","size":"64 64","origin":"32 32 0",
        "alpha":{"value":1,"script":script}
    }]));
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    let shared = crate::shared::test_device();
    let allocations = crate::vk::Renderer::scene_target_live_allocations();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    assert_eq!(group.video_objects.len(), 1);
    let pixel = |group: &mut super::super::Group| {
        let (_, _, pixels) = group.read_canvas().unwrap();
        let offset = (32 * 64 + 32) * 4;
        [pixels[offset], pixels[offset + 1], pixels[offset + 2]]
    };
    group.compose(0.0, 0.0).unwrap();
    let red = pixel(&mut group);
    assert!(red[0] > 200 && red[2] < 20, "{red:?}");
    group.compose(1.0, 1.0).unwrap();
    let blue = pixel(&mut group);
    assert!(blue[2] > 200 && blue[0] < 20, "{blue:?}");
    group.compose(2.0, 1.0).unwrap();
    assert_eq!(pixel(&mut group), red);
    group.compose(3.0, 1.0).unwrap();
    group.compose(3.6, 0.6).unwrap();
    assert_eq!(pixel(&mut group), blue);
    assert!(group.scripts.as_ref().unwrap().host.as_ref().unwrap().diagnostics.is_empty());
    group.destroy();
    assert_eq!(crate::vk::Renderer::scene_target_live_allocations(), allocations);
}

#[test]
#[ignore = "requires Vulkan and ffmpeg"]
fn dynamic_video_lifetime() {
    use super::super::{FillMode, build_group};
    let mut model = video_model(&serde_json::json!([{"id":1,"text":{"value":"","script":r"
let stage=-1, layer;
export function update(value) {
    const next=Math.floor(engine.runtime);
    if(next===stage)return layer ? layer.getVideoTexture().getCurrentTime().toFixed(1) : value;
    stage=next;
    if(stage===1 || stage===4)layer=thisScene.createLayer({image:'models/video.json',size:'64 64',origin:'32 32 0'});
    if(stage===4){const video=layer.getVideoTexture();video.setCurrentTime(0.2);video.play();}
    if(stage===2){const video=layer.getVideoTexture();video.pause();video.setCurrentTime(1.2);}
    if(stage===3 || stage===5){thisScene.destroyLayer(layer);layer=undefined;}
    return value;
}
"}}]));
    let shared = crate::shared::test_device();
    let allocations = crate::vk::Renderer::scene_target_live_allocations();
    let mut group = build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap();
    group.compose(0.0, 0.0).unwrap();
    let baseline = crate::vk::Renderer::scene_target_live_allocations();
    let pixel = |group: &mut super::super::Group| {
        let (_, _, pixels) = group.read_canvas().unwrap();
        let offset = (32 * 64 + 32) * 4;
        [pixels[offset], pixels[offset + 1], pixels[offset + 2]]
    };
    group.compose(1.0, 1.0).unwrap();
    assert_eq!(group.image_videos.len(), 1);
    assert_eq!(group.image_videos.values().next().unwrap().state(0).position, 0.0);
    let red = pixel(&mut group);
    assert!(red[0] > 200 && red[2] < 20, "{red:?}");
    group.compose(2.0, 1.0).unwrap();
    let blue = pixel(&mut group);
    assert!(blue[2] > 200 && blue[0] < 20, "{blue:?}");
    group.compose(3.0, 1.0).unwrap();
    assert!(group.image_videos.is_empty());
    assert_eq!(crate::vk::Renderer::scene_target_live_allocations(), baseline);
    let slots = group.textures.len();
    group.compose(4.0, 1.0).unwrap();
    assert_eq!(pixel(&mut group), red);
    assert_eq!(group.textures.len(), slots);
    group.compose(4.5, 0.5).unwrap();
    assert_eq!(
        group.scripts.as_ref().unwrap().host.as_ref().unwrap().scene["objects"][0]["text"],
        "0.7"
    );
    group.compose(5.0, 0.5).unwrap();
    assert!(group.image_videos.is_empty());
    assert_eq!(crate::vk::Renderer::scene_target_live_allocations(), baseline);
    assert!(group.scripts.as_ref().unwrap().host.as_ref().unwrap().diagnostics.is_empty());
    group.destroy();
    assert_eq!(crate::vk::Renderer::scene_target_live_allocations(), allocations);
}

#[test]
fn package_keys_dedupe() {
    let file = Arc::new(tempfile::tempfile().unwrap());
    let other = Arc::new(tempfile::tempfile().unwrap());
    let key = |file: &Arc<std::fs::File>, offset| {
        super::VideoKey::of(&Video::Range(VideoRange { file: file.clone(), offset, length: 8 }))
    };
    assert!(key(&file, 4) == key(&file, 4));
    assert!(key(&file, 4) != key(&file, 5));
    assert!(key(&file, 4) != key(&other, 4));
}

#[test]
fn range_buffers_bytes() {
    use std::io::Write;
    let mut file = tempfile::tempfile().unwrap();
    file.write_all(b"headpayloadtail").unwrap();
    let range = VideoRange { file: Arc::new(file), offset: 4, length: 7 };
    assert_eq!(super::buffer(&range).unwrap(), b"payload");
    let oversized = VideoRange { length: paper_scene::tex::MAX_MIP_BYTES as u64 + 1, ..range };
    assert!(super::buffer(&oversized).is_err());
}
