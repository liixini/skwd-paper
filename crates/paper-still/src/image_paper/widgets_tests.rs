use super::*;
use paper_control::{ClockStyle, Face, Labels, Palette, Pin, Widget};
use std::path::PathBuf;

fn blank<const N: usize>() -> [String; N] {
    std::array::from_fn(|_| String::new())
}

fn scene() -> WidgetScene {
    WidgetScene {
        widgets: vec![Widget {
            face: Face::Clock(ClockStyle::Aperture),
            depth: Depth::Behind,
            pin: Pin { anchor: [0.5, 0.5], offset: [0.0, 0.0] },
            height: None,
            output: None,
        }],
        outlines: vec![PathBuf::from("/cache/a/manifest.json")],
        palette: Palette { text: [236, 235, 231], accent: [90, 110, 200], shade: [18, 19, 22] },
        font: None,
        labels: Labels { days: blank(), day_names: blank(), months: blank(), month_names: blank() },
    }
}

#[test]
fn equal_scene_same() {
    assert_eq!(Change::between(&scene(), &scene()), Change::Same);
}

#[test]
fn widgets_rebuild_panels() {
    let mut moved = scene();
    moved.widgets[0].pin.offset = [12.0, 0.0];
    assert_eq!(Change::between(&scene(), &moved), Change::Panels);
    moved.outlines.clear();
    assert_eq!(Change::between(&scene(), &moved), Change::Panels);
}

#[test]
fn outlines_recut() {
    let mut recut = scene();
    recut.outlines = vec![PathBuf::from("/cache/b/manifest.json")];
    assert_eq!(Change::between(&scene(), &recut), Change::Recut);
}

#[test]
fn palette_repaints() {
    let mut toned = scene();
    toned.palette.accent = [200, 90, 110];
    assert_eq!(Change::between(&scene(), &toned), Change::Repaint);
}

#[test]
fn font_rebuilds() {
    let mut lettered = scene();
    lettered.font = Some(PathBuf::from("/fonts/a.ttf"));
    assert_eq!(Change::between(&scene(), &lettered), Change::Rebuild);
}
