use super::*;

pub(super) fn scene() -> WidgetScene {
    use paper_control::{ClockStyle, Depth, Face, Labels, Palette, Pin, Widget};
    WidgetScene {
        widgets: vec![Widget {
            face: Face::Clock(ClockStyle::Aperture),
            depth: Depth::Front,
            pin: Pin { anchor: [0.0; 2], offset: [0.0; 2] },
            height: Some(0.5),
            output: Some("DP-1".into()),
        }],
        outlines: Vec::new(),
        font: None,
        palette: Palette { text: [255; 3], accent: [255; 3], shade: [0; 3] },
        labels: Labels {
            days: std::array::from_fn(|_| "DAY".into()),
            day_names: std::array::from_fn(|_| "DAY".into()),
            months: std::array::from_fn(|_| "MONTH".into()),
            month_names: std::array::from_fn(|_| "MONTH".into()),
        },
    }
}

#[test]
#[ignore = "requires a Vulkan device"]
fn paints_moves_clears() -> Result<()> {
    let shared = crate::shared::test_device();
    let mut renderer = crate::shared::renderer(&shared, 320, 180)?;
    let mut export = renderer.create_export_image_opts(320, 180, false)?;
    let target = if renderer.direct_render() {
        renderer.create_export_rt(&export)?
    } else {
        renderer.create_render_target(320, 180)?
    };
    let readback = renderer.create_readback_buf(320 * 180 * 4)?;
    let background = renderer.create_scene_texture(1, 1, &[20, 40, 80, 255])?;
    let mut stream = StreamTarget::single(-1, 320, 180, 30, true);
    stream.output = "stream-7".into();
    stream.connector = "DP-1".into();
    stream.logical = [640, 360];
    let mut widgets = Widgets::default();
    let mut scene = scene();
    let mut frames = Vec::new();
    for step in 0..5 {
        match step {
            1 => scene.widgets[0].pin.anchor = [1.0; 2],
            2 => stream.connector = "DP-2".into(),
            3 => scene.widgets.clear(),
            4 => {}
            _ => {}
        }
        if step == 2 {
            widgets = Widgets::default();
        }
        widgets.sync(Some(&scene), &stream, &mut renderer)?;
        renderer.render_to(
            &target,
            &mut export,
            &crate::vk::Src::Rgba(background.view),
            [1.0, 1.0, 0.0, 0.0],
        )?;
        renderer.read_export_to(&export, &readback, 320, 180)?;
        frames.push(unsafe { std::slice::from_raw_parts(readback.ptr, 320 * 180 * 4) }.to_vec());
        widgets.pending = false;
    }
    assert_ne!(frames[0], frames[1]);
    assert_ne!(frames[0], frames[2]);
    assert_eq!(frames[2], frames[3]);
    assert_eq!(frames[3], frames[4]);
    assert!(frames[2].chunks_exact(4).all(|pixel| pixel == [80, 40, 20, 255]));
    assert!(widgets.wake_fd().is_none());
    assert!(!widgets.pending);
    Ok(())
}

#[test]
#[ignore = "requires a Vulkan device"]
fn cpu_widgets_skip_overlay() -> Result<()> {
    let shared = crate::shared::test_device();
    let mut renderer = crate::shared::renderer(&shared, 320, 180)?;
    let mut stream = StreamTarget::single(-1, 320, 180, 30, true);
    stream.connector = "DP-1".into();
    stream.logical = [640, 360];
    let mut widgets = Widgets { snapshots: Some(Snapshots::default()), ..Widgets::default() };
    let mut scene = scene();
    widgets.sync(Some(&scene), &stream, &mut renderer)?;
    assert!(!renderer.has_overlay());
    assert!(widgets.pending);
    widgets.pending = false;
    widgets.sync(Some(&scene), &stream, &mut renderer)?;
    assert!(!widgets.pending);
    scene.widgets.clear();
    widgets.sync(Some(&scene), &stream, &mut renderer)?;
    assert!(!renderer.has_overlay());
    assert!(widgets.wake_fd().is_none());
    assert!(widgets.pending);
    Ok(())
}
