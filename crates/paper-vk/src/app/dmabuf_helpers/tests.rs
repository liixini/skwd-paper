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
    assert_eq!(xr_render_dims(&OUTPUTS, Some((2560, 1440)), None, true, None), OUTPUTS);
    assert_eq!(
        xr_render_dims(&[(2560, 1440); 3], Some((1920, 1080)), None, true, None),
        vec![(1920, 1080); 3]
    );
}

#[test]
fn xr_transition_override_can_force_a_shared_extent() {
    assert_eq!(
        xr_render_dims(&OUTPUTS, Some((2560, 1440)), Some("max"), true, None),
        vec![(3840, 2160); 3]
    );
}

#[test]
fn span_preserves_desktop_aspect() {
    assert_eq!(
        xr_render_dims(&[(800, 600); 2], Some((1600, 900)), None, true, Some((1600, 600))),
        vec![(1600, 600); 2]
    );
    assert_eq!(
        xr_render_dims(&[(800, 600); 2], Some((800, 450)), None, true, Some((1600, 600))),
        vec![(1600, 600); 2]
    );
}

#[test]
fn span_retains_output_layout() {
    let outputs = [(800, 600), (600, 800)];
    assert_eq!(
        xr_render_dims(&outputs, Some((1920, 1080)), None, true, Some((1400, 800))),
        vec![(1400, 800); 2]
    );
    assert_eq!(
        xr_render_dims(&outputs, Some((1920, 1080)), None, false, Some((1400, 800))),
        outputs
    );
}

#[test]
fn lost_transition_source_promotes_the_ready_target() {
    let (_tx, rx) = std::sync::mpsc::sync_channel(1);
    let mut fade = Some(FadeState {
        drawing: None,
        drawing_texture: None,
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

#[test]
#[ignore = "requires a Vulkan device"]
fn snapshot_error_releases_upload() {
    let shared = crate::shared::create(std::ptr::null_mut()).unwrap();
    let renderer = crate::shared::renderer(&shared, 64, 64).unwrap();
    let directory = tempfile::tempdir().unwrap();
    for paused in [false, true] {
        let path = directory.path().join(format!("frame-{paused}.ppm"));
        let (tx, rx) = std::sync::mpsc::channel();
        let mut ctl = crate::ctl::Ctl::from_channel(rx, None, "", true, 80, false);
        ctl.set_paused(paused);
        tx.send(paper_control::PaperCommand::snapshot(path.to_str().unwrap(), "DP-1")).unwrap();
        assert!(ctl.poll().is_none());
        let frame = crate::decode::RenderFrame::plain(ffmpeg_the_third::frame::Video::new(
            ffmpeg_the_third::format::Pixel::NV12,
            64,
            64,
        ));
        let mut slot = None;
        let captured = (|| -> anyhow::Result<()> {
            crate::app::shared_support::ensure_frame_slot(&mut slot, &renderer, &frame, 0.0)?;
            anyhow::bail!("injected capture failure")
        })();
        assert!(slot.is_some());
        assert!(captured.is_err());
        super::finish_snapshot(&mut ctl, &mut slot, &renderer, captured);
        assert!(slot.is_none());
        assert!(!ctl.freeze_pending());
        assert_eq!(ctl.paused, paused);
        assert!(ctl.frozen_at.is_none());
        assert!(!path.exists());
        assert!(
            std::fs::read_to_string(path.with_extension("ppm.error"))
                .unwrap()
                .contains("injected capture failure")
        );
    }
}
