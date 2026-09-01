use super::{FadeState, promote_transition_target, shared_render_dims, xr_render_dims};

const OUTPUTS: [(u32, u32); 3] = [(1920, 1080), (2560, 1440), (3840, 2160)];

#[test]
fn shared_source_extent_replaces_every_render_extent() {
    assert_eq!(
        shared_render_dims(&OUTPUTS, Some((2560, 1440)), Some("source"), true, false),
        vec![(2560, 1440); 3]
    );
}

#[test]
fn shared_max_extent_uses_the_largest_output() {
    assert_eq!(
        shared_render_dims(&OUTPUTS, Some((2560, 1440)), Some("max"), true, false),
        vec![(3840, 2160); 3]
    );
}

#[test]
fn shared_extent_requires_viewporter_and_dmabuf() {
    assert_eq!(
        shared_render_dims(&OUTPUTS, Some((2560, 1440)), Some("source"), false, true),
        OUTPUTS
    );
    assert_eq!(shared_render_dims(&OUTPUTS, None, Some("source"), true, true), OUTPUTS);
}

#[test]
fn automatic_reuse_chooses_the_smaller_compatible_extent() {
    assert_eq!(
        shared_render_dims(&OUTPUTS, Some((2560, 1440)), None, true, true),
        vec![(2560, 1440); 3]
    );
    assert_eq!(
        shared_render_dims(&OUTPUTS, Some((7680, 4320)), None, true, true),
        vec![(3840, 2160); 3]
    );
}

#[test]
fn automatic_reuse_preserves_incompatible_output_groups() {
    let mixed = [(1920, 1080), (1080, 1920)];
    assert_eq!(shared_render_dims(&mixed, Some((1920, 1080)), None, true, true), mixed);
    assert_eq!(shared_render_dims(&OUTPUTS, Some((2560, 1080)), None, true, true), OUTPUTS);
    assert_eq!(shared_render_dims(&OUTPUTS, Some((2560, 1440)), None, true, false), OUTPUTS);
}

#[test]
fn native_override_disables_automatic_reuse() {
    assert_eq!(
        shared_render_dims(&OUTPUTS, Some((2560, 1440)), Some("native"), true, true),
        OUTPUTS
    );
}

#[test]
fn xr_transitions_keep_native_extents_for_mixed_output_sizes() {
    assert_eq!(xr_render_dims(&OUTPUTS, Some((2560, 1440)), None, true), OUTPUTS);
    assert_eq!(
        xr_render_dims(&[(2560, 1440); 3], Some((1920, 1080)), None, true),
        vec![(1920, 1080); 3]
    );
}

#[test]
fn xr_transition_override_can_force_a_shared_extent() {
    assert_eq!(
        xr_render_dims(&OUTPUTS, Some((2560, 1440)), Some("max"), true),
        vec![(3840, 2160); 3]
    );
}

#[test]
fn lost_transition_source_promotes_the_ready_target() {
    let (_tx, rx) = std::sync::mpsc::sync_channel(1);
    let mut fade = Some(FadeState {
        rx,
        abort: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        worker: None,
        cur: (crate::decode::RenderFrame::plain(ffmpeg_the_third::frame::Video::empty()), 1.25),
        queued: None,
        pts0: 1.25,
        speed: 1.0,
        t0: std::time::Instant::now(),
        dur_ms: 500,
        uvs: vec![],
        style: None,
        effect: None,
        still_b: false,
        first_frame: true,
        path: "/target.mp4".into(),
    });
    let mut pending = None;

    assert!(promote_transition_target(&mut fade, &mut pending));
    assert_eq!(pending.as_ref().map(|(_, pts)| *pts), Some(1.25));
    assert_eq!(fade.as_ref().map(|state| state.dur_ms), Some(0));
    assert_eq!(fade.as_ref().map(|state| state.first_frame), Some(false));
}
