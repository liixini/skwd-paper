use super::*;

#[test]
fn malformed_fonts_and_invalid_sizes_are_rejected() {
    for bytes in [&[][..], &[0, 1, 0, 0][..], b"not a font".as_slice()] {
        assert!(Face::parse(bytes, 200.0).is_none());
    }
    let assets = crate::effects::Assets::discover(None);
    if let Some(bytes) = assets.read_bytes(FALLBACK_FONT) {
        for em in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert!(Face::parse(&bytes, em).is_none());
        }
    }
}

#[test]
fn repeated_text_rendering_preserves_pixels_and_multiline_spacing() {
    let assets = crate::effects::Assets::discover(None);
    let Some(bytes) = assets.read_bytes(FALLBACK_FONT) else {
        return;
    };
    let face = Face::parse(&bytes, 200.0).expect("font parses");
    let single = face.measure("J H").expect("single-line metrics");
    for align in [HAlign::Left, HAlign::Center, HAlign::Right] {
        let (metrics, first) = face.rasterize("J H\n12:34", align).expect("rasterizes");
        let (_, second) = face.rasterize("J H\n12:34", align).expect("rasterizes again");
        assert_eq!(metrics.lines, 2);
        assert!((metrics.height() - single.height() - single.line_height).abs() < 0.001);
        assert_eq!(first.pixels.levels[0].data, second.pixels.levels[0].data);
        assert!(first.pixels.levels[0].data.chunks_exact(4).any(|pixel| pixel[3] > 0));
    }
}

fn noto_like() -> Metrics {
    Metrics { width: 148.0, ascent: 213.8, descent: 58.6, line_height: 272.4, lines: 1 }
}

#[test]
fn point_size_maps_to_a_300_dpi_em() {
    assert!((EM_PER_POINT * 48.0 - 200.0).abs() < 1e-3);
    assert!((EM_PER_POINT * 96.0 - 400.0).abs() < 1e-3);
}

#[test]
fn box_offsets_follow_engine_alignment_rules() {
    let m = noto_like();
    assert_eq!(box_offset(&m, HAlign::Left, VAlign::Top), (0.0, 0.0));
    let (x, y) = box_offset(&m, HAlign::Center, VAlign::Center);
    assert!((x + 74.0).abs() < 1e-4 && (y + 106.9).abs() < 1e-4);
    let (x, y) = box_offset(&m, HAlign::Right, VAlign::Bottom);
    assert!((x + 148.0).abs() < 1e-4 && (y + 272.4).abs() < 1e-3);
    let cap = 142.8;
    let (_, y) = box_offset(&m, HAlign::Center, VAlign::Center);
    let cap_centre = y + m.ascent - cap * 0.5;
    assert!((cap_centre - 35.5).abs() < 1.0, "{cap_centre}");
    let (_, y) = box_offset(&m, HAlign::Center, VAlign::Bottom);
    assert!((y + m.ascent + 58.6).abs() < 1e-3);
}

#[test]
fn text_value_reads_strings_numbers_and_script_placeholders() {
    assert_eq!(text_value(Some(&serde_json::json!("MEW"))).as_deref(), Some("MEW"));
    assert_eq!(text_value(Some(&serde_json::json!(12))).as_deref(), Some("12"));
    let scripted = serde_json::json!({"script": "export function update() {}", "value": "12:34"});
    assert_eq!(text_value(Some(&scripted)).as_deref(), Some("12:34"));
    assert_eq!(text_value(Some(&serde_json::json!({"script": "x"}))), None);
    assert_eq!(text_value(None), None);
}

#[test]
fn noto_cap_height_matches_the_engine_probe() {
    let assets = crate::effects::Assets::discover(None);
    let Some(bytes) = assets.read_bytes(FALLBACK_FONT) else {
        return;
    };
    let face = Face::parse(&bytes, 200.0).expect("font parses");
    let (metrics, texture) = face.rasterize("H", HAlign::Center).expect("rasterizes");
    assert!((metrics.ascent - 213.8).abs() < 1.5, "{}", metrics.ascent);
    assert!((metrics.descent - 58.6).abs() < 1.5, "{}", metrics.descent);
    let rgba = &texture.pixels.levels[0].data;
    let width = texture.width as usize;
    let rows: Vec<usize> = (0..texture.height as usize)
        .filter(|row| (0..width).any(|col| rgba[(row * width + col) * 4 + 3] > 64))
        .collect();
    let ink = rows.last().unwrap() - rows.first().unwrap() + 1;
    assert!((ink as i64 - 143).abs() <= 3, "cap height {ink}");
    let cols: Vec<usize> = (0..width)
        .filter(|col| {
            (0..texture.height as usize).any(|row| rgba[(row * width + col) * 4 + 3] > 64)
        })
        .collect();
    let ink_w = cols.last().unwrap() - cols.first().unwrap() + 1;
    assert!((ink_w as i64 - 110).abs() <= 4, "ink width {ink_w}");
    let baseline_row = rows.last().unwrap() + 1;
    assert!((baseline_row as f32 - (1.0 + metrics.ascent)).abs() < 2.0, "{baseline_row}");
    assert!((metrics.width - 128.6).abs() < 1.5, "box drops the right bearing: {}", metrics.width);
    assert!((cols.first().copied().unwrap() as f32 - 20.4).abs() < 2.0, "{:?}", cols.first());
}

#[test]
fn line_box_starts_at_pen_or_ink_and_ends_at_ink() {
    let assets = crate::effects::Assets::discover(None);
    let Some(bytes) = assets.read_bytes(FALLBACK_FONT) else {
        return;
    };
    let face = Face::parse(&bytes, 200.0).expect("font parses");
    let j = face.measure("J").expect("metrics");
    assert!((j.width - 52.0).abs() < 1.5, "J spans its negative bearing: {}", j.width);
    let t = face.measure("T").expect("metrics");
    assert!((t.width - 109.0).abs() < 1.5, "{}", t.width);
    let hl = face.measure("HL").expect("metrics");
    assert!((hl.width - 248.0).abs() < 2.0, "{}", hl.width);
    let (_, texture) = face.rasterize("J", HAlign::Left).expect("rasterizes");
    let rgba = &texture.pixels.levels[0].data;
    let width = texture.width as usize;
    let first_ink = (0..width)
        .find(|col| (0..texture.height as usize).any(|row| rgba[(row * width + col) * 4 + 3] > 64))
        .unwrap();
    assert!(first_ink <= 2, "negative bearing glyph is not clipped: {first_ink}");
}

#[test]
fn text_objects_become_layers_anchored_at_the_origin() {
    let assets = crate::effects::Assets::discover(None);
    if assets.read_bytes(FALLBACK_FONT).is_none() {
        return;
    }
    let scene = br#"{"general":{"orthogonalprojection":{"width":1920,"height":1080}},
        "objects":[{"id":1,"name":"t","origin":"300 540 0","text":"H","pointsize":48,
        "font":"systemfont_consolas","horizontalalign":"left","verticalalign":"top","color":"1 0 0","alpha":0.5}]}"#;
    let bytes = crate::tests::build_pkg(&[("scene.json", scene)]);
    let package = crate::pkg::Package::parse(bytes).unwrap();
    let model = crate::model::load_with(&package, &assets).unwrap();
    assert_eq!(model.layers.len(), 1);
    let layer = &model.layers[0];
    assert_eq!(layer.color, [1.0, 0.0, 0.0]);
    assert!((layer.alpha - 0.5).abs() < 1e-6);
    let left = layer.center.0 - layer.size.0 * 0.5;
    let top = layer.center.1 - layer.size.1 * 0.5;
    assert!((left - 299.0).abs() < 1.0, "left {left}");
    assert!((top - 539.0).abs() < 1.0, "top {top}");
    assert!(layer.size.1 > 270.0 && layer.size.1 < 278.0, "{:?}", layer.size);
}
