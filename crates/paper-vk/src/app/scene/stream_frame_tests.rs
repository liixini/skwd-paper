use super::*;
use crate::app::scene::{FadeFrom, FadeSource, build_group, build_presenter};

#[test]
#[ignore = "requires Vulkan"]
fn holds_delivered_transition() -> Result<()> {
    let shared = crate::shared::test_device();
    let mut model = crate::app::scene::effect_lifetime_tests::model_files(&[
        ("scene.json", br#"{"general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"1 0 0"},"objects":[{"id":1,"image":"models/flat.json","origin":"32 32 0","size":"8 8"}]}"#),
        ("models/flat.json", br#"{"material":"materials/flat.json"}"#),
        ("materials/flat.json", br#"{"passes":[{"shader":"flat"}]}"#),
    ]);
    let mut group =
        build_group(&shared, &mut model, true, &[(64, 64)], paper_geom::FillMode::Fill)?;
    let old = group.renderer.create_scene_target(64, 64)?;
    group.renderer.render_scene(&old, [0.0, 0.0, 1.0, 1.0], &[], &[])?;
    group.from = Some(FadeSource {
        source: FadeFrom::Scene(vk::PresentedTarget::new(old)),
        width: 64,
        height: 64,
    });
    let mut presenter = build_presenter(&shared, 64, 64, 1, None)?;
    presenter.fade(&group, 0, 0.25, TransitionStyle::Fade, "")?;
    let readback = presenter.renderer.create_readback_buf(64 * 64 * 4)?;
    presenter.read_export_to(0, &readback)?;
    let expected = unsafe { std::slice::from_raw_parts(readback.ptr, 64 * 64 * 4) }.to_vec();
    let mut frame = StreamFrame { progress: 0.25, ..StreamFrame::default() };
    frame.hold(
        FrameStatus { emitted: true, paused: false, available: true },
        &mut presenter,
        &group,
        TransitionStyle::Fade,
    )?;
    assert!(frame.frozen.is_none());
    frame.hold(
        FrameStatus { emitted: true, paused: false, available: false },
        &mut presenter,
        &group,
        TransitionStyle::Fade,
    )?;
    group.renderer.render_scene(&group.target, [0.0, 1.0, 0.0, 1.0], &[], &[])?;
    group.drop_from();
    frame.hold(
        FrameStatus { emitted: true, paused: true, available: true },
        &mut presenter,
        &group,
        TransitionStyle::Fade,
    )?;
    let frozen = frame.frozen.as_ref().unwrap();
    presenter.present(frozen, 0)?;
    presenter.read_export_to(0, &readback)?;
    let actual = unsafe { std::slice::from_raw_parts(readback.ptr, 64 * 64 * 4) };
    assert_eq!(actual, expected);
    group.destroy();
    Ok(())
}
