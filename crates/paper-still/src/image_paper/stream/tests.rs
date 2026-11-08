use super::*;

#[test]
fn blends_premultiplied_pixels() {
    let mut frame = vec![100, 80, 60, 255];
    let mut canvas = paper_widgets::Canvas::new(1, 1);
    canvas.pixels[0] = [64, 0, 0, 128];
    blend(&mut frame, 1, [0, 0], &canvas);
    assert_eq!(frame, [113, 39, 29, 255]);
}

#[test]
fn clips_mismatched_frame() {
    let mut placed = scene(None);
    placed.widgets[0].pin.anchor = [0.5, 1.0];
    let output = Output {
        spec: StreamOutput { size: [192, 108], logical: [256, 160], name: "DP-1".into(), fd: None },
        base: vec![0; 192 * 108 * CHANNELS],
        sink: Box::new(Vec::<u8>::new()),
    };
    let active = ActiveScene::new(placed, &[output], "/no/image", [192, 108], FillMode::Fill)
        .unwrap()
        .unwrap();
    let layer = &active.layers[0][0];
    let painter = Scene {
        lettering: &active.lettering,
        tones: layer.tones,
        moment: Moment::civil(12, 34, 1, 2, 3),
        labels: &active.scene.labels,
    };
    let canvas = painter.paint(layer.face, layer.place, None);
    let mut bytes = vec![0; 192 * 108 * CHANNELS];
    blend(&mut bytes, 192, layer.place.origin, &canvas);
    assert!(bytes.iter().any(|&byte| byte != 0));
    let saved = bytes.clone();
    blend(&mut bytes, 192, [192, 108], &canvas);
    assert_eq!(bytes, saved);
}

#[test]
fn writes_header_once() {
    let mut bytes = Vec::new();
    write_frame(&mut bytes, 1, 1, &[1, 2, 3, 4], true).unwrap();
    write_frame(&mut bytes, 1, 1, &[5, 6, 7, 8], false).unwrap();
    assert_eq!(&bytes[..12], b"SKWP\x01\0\0\0\x01\0\0\0");
    assert_eq!(&bytes[12..], &[1, 2, 3, 4, 5, 6, 7, 8]);
}

fn scene(output: Option<&str>) -> WidgetScene {
    use paper_control::{ClockStyle, Face, Labels, Palette, Pin, Widget};
    WidgetScene {
        widgets: vec![Widget {
            face: Face::Clock(ClockStyle::Aperture),
            depth: Depth::Front,
            pin: Pin { anchor: [0.5, 0.5], offset: [0.0, 0.0] },
            height: Some(0.5),
            output: output.map(str::to_owned),
        }],
        outlines: Vec::new(),
        palette: Palette { text: [255; 3], accent: [255; 3], shade: [0; 3] },
        font: None,
        labels: Labels {
            days: std::array::from_fn(|_| String::new()),
            day_names: std::array::from_fn(|_| String::new()),
            months: std::array::from_fn(|_| String::new()),
            month_names: std::array::from_fn(|_| String::new()),
        },
    }
}

#[test]
fn filters_output_layers() {
    let outputs = ["DP-1", "HDMI-A-1"].map(|name| Output {
        spec: StreamOutput { size: [200, 100], logical: [100, 50], name: name.into(), fd: None },
        base: vec![0; 200 * 100 * CHANNELS],
        sink: Box::new(Vec::<u8>::new()) as Box<dyn Write>,
    });
    let active =
        ActiveScene::new(scene(Some("DP-1")), &outputs, "/no/image", [200, 100], FillMode::Fill)
            .unwrap()
            .unwrap();
    assert_eq!(active.layers[0].len(), 1);
    assert!(active.layers[1].is_empty());
    assert_eq!(active.layers[0][0].place.size[1], 50);
}

#[test]
fn rejects_invalid_scene() {
    let mut invalid = scene(None);
    invalid.widgets[0].pin.anchor[0] = 2.0;
    assert!(ActiveScene::new(invalid, &[], "/no/image", [1, 1], FillMode::Fill).is_err());
}
