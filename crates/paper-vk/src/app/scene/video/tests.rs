use super::Timeline;

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
    let raw = pkg
        .entries()
        .iter()
        .find_map(|entry| {
            let mut parsed = paper_scene::tex::parse(pkg.read(entry)).ok()?;
            (parsed.meta.flags & paper_scene::tex::FLAG_IS_VIDEO != 0)
                .then(|| std::mem::take(&mut parsed.images[0][0].data))
        })
        .expect("embedded video");
    let sd = crate::shared::create(std::ptr::null_mut()).unwrap();
    let mut video = super::VideoTexture::open(&sd, &raw, true, false).unwrap();
    let first = video.renderer.read_scene_target(video.target.as_ref().unwrap()).unwrap().2;
    video.advance(1.0).unwrap();
    let second = video.renderer.read_scene_target(video.target.as_ref().unwrap()).unwrap().2;
    assert_ne!(first, second, "video must advance");
    for frame in 31..=1260 {
        video.advance(f64::from(frame) / 30.0).unwrap();
    }
    let looped = video.renderer.read_scene_target(video.target.as_ref().unwrap()).unwrap().2;
    assert_ne!(second, looped, "video must continue after the 40.6 second loop");
}
