use super::*;

const WE_FONT_TEST_ENV: &str = "SKWD_REQUIRE_WE_FONT";
const TEXT_ORACLE_FIXTURES_ENV: &str = "SKWD_TEXT_ORACLE_FIXTURES";
const TEXT_RASTER_FIXTURE_ENV: &str = "SKWD_TEXT_RASTER_FIXTURE";
const C08_ADVANCE_FIXTURE_ENV: &str = "SKWD_C08_ADVANCE_FIXTURE";
const C08_SPACE_FIXTURE_ENV: &str = "SKWD_C08_SPACE_FIXTURE";
const C08_COVERAGE_FIXTURE_ENV: &str = "SKWD_C08_COVERAGE_FIXTURE";

#[test]
fn preset_text_binds() {
    let project = serde_json::json!({"general":{"properties":{
        "Caption":{"type":"textinput","value":"Selected caption"},
        "notepad":{"type":"textinput","value":"First|Second"},
        "heading":{"type":"text","value":"Settings heading"}
    }}});
    let mut scene = serde_json::json!({"objects":[
        {"text":{"user":"caption","value":"Base caption"}},
        {"text":{"user":"notepad","value":"-","script":"export function update(value){return thisLayer.text.toString().split('|').join('\\n');}"}},
        {"text":{"user":"heading","value":"Base heading"}},
        {"text":{"user":"missing","value":"Keep me"}}
    ]});
    bind_user_values(&mut scene, &project);
    assert_eq!(scene["objects"][0]["text"]["value"], "Selected caption");
    assert_eq!(scene["objects"][1]["text"]["value"], "First|Second");
    assert!(scene["objects"][1]["text"]["script"].is_string());
    assert_eq!(scene["objects"][2]["text"]["value"], "Base heading");
    assert_eq!(scene["objects"][3]["text"]["value"], "Keep me");
}

#[test]
fn notepad_lines() {
    let project = serde_json::json!({"general":{"properties":{
        "notepad":{"type":"textinput","value":"First|Second"}
    }}});
    let mut scene = serde_json::json!({"objects":[
        {"id":1,"text":{"user":"notepad","value":"-","script":"export function update(value){return thisLayer.text.toString().split('|').join('\\n');}"}}
    ]});
    bind_user_values(&mut scene, &project);
    let mut host =
        crate::script::SceneScripts::load(&mut scene, &crate::model::Properties::new(), &project)
            .unwrap()
            .unwrap();
    host.tick(0.0625, 0.0625, [0.5; 2]).unwrap();
    assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
    assert_eq!(host.scene["objects"][0]["text"], "First\nSecond");
}

#[test]
fn repeated_text_is_bounded() {
    let selected = "界".repeat(MAX_TEXT_CHARS + 1);
    let project = serde_json::json!({"general":{"properties":{
        "caption":{"type":"textinput","value":selected}
    }}});
    let mut scene = serde_json::json!({"objects":[
        {"text":{"user":"caption","value":"one"}},
        {"text":{"user":"caption","value":"two"}}
    ]});
    bind_user_values(&mut scene, &project);
    for object in scene["objects"].as_array().unwrap() {
        let text = object["text"]["value"].as_str().unwrap();
        assert_eq!(text.chars().count(), MAX_TEXT_CHARS);
        assert_eq!(text.len(), "界".len() * MAX_TEXT_CHARS);
    }
}

#[test]
fn text_box_wraps() {
    let object = serde_json::json!({"limitwidth":true,"maxwidth":7,"limitrows":true,"maxrows":2});
    let limits = TextLimits::of(&object, &Properties::new());
    let width = |line: &str| Some(line.chars().count() as f32);
    assert_eq!(
        wrap_text("One two three four", limits.width, limits.rows, width).unwrap(),
        "One two\nthree"
    );
    assert_eq!(wrap_text("界界界界界", Some(2.0), None, width).unwrap(), "界界\n界界\n界");
    assert_eq!(wrap_text("ABCDE", Some(0.5), None, width).unwrap(), "A\nB\nC\nD\nE");
    assert_eq!(wrap_text("ABCDE", Some(0.5), Some(2), width).unwrap(), "A\nB");
    assert_eq!(wrap_text("A  B\nC D", Some(4.0), None, width).unwrap(), "A  B\nC D");
    assert_eq!(wrap_text("A\nB\nC", None, Some(2), width).unwrap(), "A\nB");
    assert!(wrap_text("A", Some(1.0), None, |_| None).is_none());
}

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
    assert!((x + 74.0).abs() < 1e-4 && (y + 107.0).abs() < 1e-4);
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
fn gpos_kerns_latin_pairs() {
    let Some(face) = we_face() else {
        return;
    };
    let mut kerned_advance = 0.0;
    face.font.shape("AV", |glyph| kerned_advance += glyph.advance);
    let mut plain_advance = 0.0;
    face.font.shape("A", |glyph| plain_advance += glyph.advance);
    face.font.shape("V", |glyph| plain_advance += glyph.advance);
    assert!(kerned_advance + 1.0 < plain_advance, "GPOS kerning");
}

fn we_face() -> Option<Face> {
    let assets = crate::effects::Assets::discover(None);
    let bytes = assets.read_bytes(FALLBACK_FONT);
    if std::env::var_os(WE_FONT_TEST_ENV).is_some() {
        assert!(bytes.is_some(), "WE font asset");
    }
    Some(Face::parse(&bytes?, 100.0).expect("font parses"))
}

fn oracle_face(fixture: &str) -> Option<Face> {
    let root = std::env::var_os(TEXT_ORACLE_FIXTURES_ENV)?;
    let path = std::path::Path::new(&root).join(fixture).join("fonts/Probe.ttf");
    let bytes = std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    Some(Face::parse(&bytes, 100.0).expect("oracle font parses"))
}

fn glyph_ids(face: &Face, text: &str) -> Vec<u16> {
    let mut ids = Vec::new();
    face.font.shape(text, |glyph| ids.push(glyph.id));
    ids
}

#[test]
fn hebrew_first_strong_direction() {
    let Some(face) = oracle_face("text-hebrew-bidi") else {
        return;
    };
    for text in ["אבג", "גבא", "Aאב", "אבA", "A1אב", "אב12", "A B", "AאB", "123"] {
        let ids = glyph_ids(&face, text);
        assert!(!ids.is_empty() && ids.iter().all(|id| *id != 0), "{text}: {ids:?}");
        assert!(face.rasterize(text, HAlign::Left).is_some(), "{text}");
    }
    assert_eq!(
        glyph_ids(&face, "אבג"),
        glyph_ids(&face, "גבא").into_iter().rev().collect::<Vec<_>>()
    );
    let mixed = glyph_ids(&face, "Aאב");
    assert_eq!(mixed[0], glyph_ids(&face, "A")[0]);
    assert_eq!(&mixed[1..], glyph_ids(&face, "אב").iter().rev().copied().collect::<Vec<_>>());
    let rtl_numbers = glyph_ids(&face, "אב12");
    assert_eq!(&rtl_numbers[..2], &glyph_ids(&face, "21"));
}

#[test]
fn arabic_joins_clusters() {
    let Some(face) = oracle_face("text-arabic-shape") else {
        return;
    };
    for text in ["مرحبا", "اب", "با", "لا", "لام", "مـ", "1اب", "اب1", "123"] {
        let ids = glyph_ids(&face, text);
        assert!(!ids.is_empty() && ids.iter().all(|id| *id != 0), "{text}: {ids:?}");
        assert!(face.rasterize(text, HAlign::Left).is_some(), "{text}");
    }
    assert_ne!(glyph_ids(&face, "لا"), [glyph_ids(&face, "ا"), glyph_ids(&face, "ل")].concat());
    assert_ne!(glyph_ids(&face, "با"), [glyph_ids(&face, "ا"), glyph_ids(&face, "ب")].concat());
    assert_eq!(glyph_ids(&face, "اب"), [glyph_ids(&face, "ب"), glyph_ids(&face, "ا")].concat());
}

fn fallback_face(text: &str, em: f32) -> Option<Face> {
    let assets = crate::effects::Assets::discover(None);
    let bytes = assets.read_bytes(FALLBACK_FONT);
    if std::env::var_os(WE_FONT_TEST_ENV).is_some() {
        assert!(bytes.is_some(), "WE font asset");
    }
    let mut face = Face::parse(&bytes?, em).unwrap();
    face.load_fallbacks(&assets, text);
    assert!(face.uses_fallback(text), "prefix fallback missing");
    Some(face)
}

#[test]
fn fallback_advances_round() {
    for (em, text, expected) in [
        (50.0, "AאבB", 123.0),
        (100.0, "AאבB", 249.0),
        (50.0, "AגדB", 111.0),
        (100.0, "AגדB", 225.0),
        (50.0, "AאבגB", 144.0),
        (100.0, "AאבגB", 291.0),
        (50.0, "A漢B", 113.0),
        (100.0, "A漢B", 228.0),
        (50.0, "A漢字B", 163.0),
        (100.0, "A漢字B", 328.0),
    ] {
        let Some(face) = fallback_face(text, em) else {
            return;
        };
        let mut advance = 0.0;
        face.shape(text, |_, glyph| advance += glyph.advance);
        assert_eq!(advance, expected, "{em}: {text}");
    }
}

#[test]
fn fallback_runs_keep_direction() {
    let Some(face) = fallback_face("AאבB", 100.0) else {
        return;
    };
    let mut actual = Vec::new();
    face.shape("AאבB", |font, glyph| {
        actual.push((std::ptr::eq(font, &raw const face.font), glyph.id));
    });
    let mut expected = Vec::new();
    face.font.shape("A", |glyph| expected.push((true, glyph.id)));
    face.fallbacks[0].shape("אב", |glyph| expected.push((false, glyph.id)));
    face.font.shape("B", |glyph| expected.push((true, glyph.id)));
    assert_eq!(actual, expected);
}

#[test]
fn primary_pixels_stay_identical() {
    let Some(mut face) = we_face() else { return };
    let before = face.rasterize("AV fi Á", HAlign::Left).unwrap();
    let assets = crate::effects::Assets::discover(None);
    face.load_fallbacks(&assets, "AאבB");
    assert!(face.uses_fallback("AאבB"));
    let after = face.rasterize("AV fi Á", HAlign::Left).unwrap();
    assert_eq!(before.0, after.0);
    assert_eq!(before.1.pixels.levels[0].data, after.1.pixels.levels[0].data);
}

#[test]
fn text_baselines_round() {
    for (em, ascent) in [(50.0, 54.0), (100.0, 107.0)] {
        let Some(face) = fallback_face("A漢B", em) else {
            return;
        };
        assert_eq!(face.measure("A漢B").unwrap().ascent, ascent);
        assert_eq!(face.measure("ABC").unwrap().ascent, ascent);
    }
}

#[test]
fn fallback_uses_coverage() {
    let Some(mut face) = we_face() else { return };
    let Some(arabic) = oracle_face("text-arabic-shape") else {
        return;
    };
    let before = face.rasterize("AابB 😀", HAlign::Left).unwrap();
    face.fallbacks.push(arabic.font);
    assert!(face.uses_fallback("AابB 😀"));
    let after = face.rasterize("AابB 😀", HAlign::Left).unwrap();
    assert_ne!(before.0.width, after.0.width);
    assert_ne!(before.1.pixels.levels[0].data, after.1.pixels.levels[0].data);
}

#[test]
fn prepared_preserves_font_caches() {
    let Some(face) = fallback_face("AאבB", 100.0) else {
        return;
    };
    let prepared = Prepared {
        face: std::cell::RefCell::new(face),
        assets: crate::effects::Assets::discover(None),
        halign: HAlign::Left,
        width: 400,
        height: 200,
        placeholder: String::new(),
        raster: Raster::of(RasterMode::Direct, &Value::Null),
        weekday: None,
        clock: None,
        date: None,
    };
    let first = prepared.rasterize("AאבB").unwrap();
    let key = prepared.face.borrow().fallbacks[0].cache_key();
    let second = prepared.rasterize("AאבB").unwrap();
    assert_eq!(key, prepared.face.borrow().fallbacks[0].cache_key());
    assert_eq!(first.pixels.levels[0].data, second.pixels.levels[0].data);
}

#[test]
fn zwnj_does_not_draw() {
    let Some(face) = we_face() else {
        return;
    };
    let (_, fi) = face.rasterize("fi", HAlign::Left).expect("rasterizes");
    let (_, separated) = face.rasterize("f\u{200c}i", HAlign::Left).expect("rasterizes");
    assert_eq!(fi.pixels.levels[0].data, separated.pixels.levels[0].data);
}

#[test]
fn combining_accent_matches_precomposed() {
    let Some(face) = we_face() else {
        return;
    };
    let (_, composed) = face.rasterize("Á", HAlign::Left).expect("rasterizes");
    let (_, combining) = face.rasterize("A\u{301}", HAlign::Left).expect("rasterizes");
    assert_eq!(composed.pixels.levels[0].data, combining.pixels.levels[0].data);
}

#[test]
fn noto_hint_matches_edge() {
    let Some(face) = we_face() else {
        return;
    };
    let mut id = None;
    face.font.shape("Á", |glyph| id = Some(glyph.id));
    let (glyph, bitmap) = face.font.rasterize(id.expect("glyph id")).expect("glyph rasterizes");
    assert_eq!((glyph.width, glyph.height), (64, 95));
    assert_eq!(bitmap[40], u8::MAX);
}

#[test]
#[ignore = "requires SKWD_TEXT_RASTER_FIXTURE"]
fn freetype_golden_mask() {
    use sha2::{Digest, Sha256};

    let root = std::env::var_os(TEXT_RASTER_FIXTURE_ENV).expect(TEXT_RASTER_FIXTURE_ENV);
    let path = std::path::Path::new(&root).join("fonts/probe.ttf");
    let bytes = std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        "ffc549b7fc28057d624086111f1ba0b1f752d2d7ff7c5363dfa064e4dca17d47"
    );
    let face = Face::parse(&bytes, 400.0).expect("FreeType face parses");
    for (text, size, expected) in [
        ("T", (176, 294), "fc451cb169d81d6c733c061f51a12013c70556d33472da4fff5564078b610c40"),
        ("E", (167, 294), "e94b839c6397b5e7e0d878f1ceb823dc027925cdfbe7e3b0e7b93cac8ed78995"),
        ("S", (177, 302), "7b11b540cba50f462d8940828ee4795566dab6554a9feb5349a3ba222171ad75"),
    ] {
        let mut id = None;
        face.font.shape(text, |glyph| id = Some(glyph.id));
        let (glyph, bitmap) =
            face.font.rasterize(id.expect("shaped glyph")).expect("rasterized glyph");
        assert_eq!((glyph.width, glyph.height), size, "{text}");
        assert_eq!(format!("{:x}", Sha256::digest(bitmap)), expected, "{text}");
    }
}

#[test]
fn text_objects_become_layers_anchored_at_the_origin() {
    let assets = crate::effects::Assets::discover(None);
    if assets.read_bytes(FALLBACK_FONT).is_none() {
        return;
    }
    let scene = br#"{"general":{"orthogonalprojection":{"width":1920,"height":1080}},
        "objects":[{"id":1,"name":"t","origin":"300 540 0","text":"H","pointsize":48,
        "font":"fonts/NotoSans-Regular.ttf","horizontalalign":"left","verticalalign":"top","color":"1 0 0","alpha":0.5}]}"#;
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

#[test]
fn text_blend_modes() {
    for (mode, effect, blended, additive) in [
        (0, false, false, false),
        (6, false, true, false),
        (31, false, false, true),
        (31, true, false, false),
    ] {
        let effects = if effect {
            serde_json::json!([{"file":"effects/probe.json"}])
        } else {
            serde_json::json!([])
        };
        let scene = serde_json::json!({"objects":[{"id":1,"text":"H","pointsize":24,
            "font":"fonts/latin.ttf","colorBlendMode":mode,"effects":effects}]})
        .to_string();
        let package = Package::parse(crate::tests::build_pkg(&[
            ("scene.json", scene.as_bytes()),
            ("fonts/latin.ttf", include_bytes!("../fixtures/fonts/latin.ttf")),
            ("effects/probe.json", br#"{"passes":[{"material":"materials/probe.json"}]}"#),
            ("materials/probe.json", br#"{"passes":[{"shader":"probe"}]}"#),
            ("shaders/probe.vert", b"attribute vec3 a_Position;\nvoid main(){gl_Position=vec4(a_Position,1.0);}"),
            ("shaders/probe.frag", b"uniform sampler2D g_Texture0;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,vec2(0.5,0.5));}"),
            (
                "shaders/common_blending.h",
                b"vec3 ApplyBlending(const int mode, in vec3 A, in vec3 B, in float opacity) { return mix(A, B, opacity); }",
            ),
        ]))
        .unwrap();
        let model =
            crate::model::load_with(&package, &Assets::discover(Some("/nonexistent"))).unwrap();
        let layer = &model.layers[0];
        let blend =
            layer.effects.iter().any(|effect| effect.name == crate::effects::COLOR_BLEND_EFFECT);
        assert_eq!(blend, blended, "{mode} {:?}", model.skipped);
        assert_eq!(layer.additive(), additive, "{mode} {effect}");
        assert_eq!(layer.additive_text(), mode == crate::effects::ADDITIVE_COLOR_BLEND);
    }
}

#[test]
fn centered_blocks_share_the_middle_line_with_the_engine() {
    let one = Metrics { width: 10.0, ascent: 40.0, descent: 10.0, line_height: 91.0, lines: 1 };
    let three = Metrics { lines: 3, ..one };
    let (_, single) = box_offset(&one, HAlign::Left, VAlign::Center);
    let (_, block) = box_offset(&three, HAlign::Left, VAlign::Center);
    assert!((single - -20.0).abs() < 1e-6);
    assert!((block - (single - 91.0)).abs() < 1e-6);
    let (_, top) = box_offset(&three, HAlign::Left, VAlign::Top);
    let (_, bottom) = box_offset(&three, HAlign::Left, VAlign::Bottom);
    assert!((top - 0.0).abs() < 1e-6);
    assert!((bottom - -three.height()).abs() < 1e-6);
}

#[test]
fn fractional_center_placement() {
    for (ascent, expected) in [
        (48.752604, -24.5),
        (69.761_46, -35.0),
        (125.047_92, -63.0),
        (241.149_48, -121.0),
        (386.100_52, -193.5),
    ] {
        let metrics = Metrics { ascent, width: 100.0, descent: 10.0, line_height: 120.0, lines: 1 };
        assert_eq!(box_offset(&metrics, HAlign::Center, VAlign::Center).1, expected);
        assert_eq!(metrics.ascent, ascent);
        assert_eq!(box_offset(&metrics, HAlign::Left, VAlign::Top).1, 0.0);
        assert_eq!(box_offset(&metrics, HAlign::Left, VAlign::Bottom).1, -metrics.height());
    }
}

#[test]
#[ignore = "requires pinned C08 advance fixture"]
fn ve_size_advance() {
    for (points, expected_advance) in [(24.0, 60.0), (40.0, 100.0)] {
        let face = pinned_text_font(
            C08_ADVANCE_FIXTURE_ENV,
            "fonts/Probe.ttf",
            "6b04c8dd65af6b73eb4279472ed1580b29102d6496a377340e80a40cdb3b22c9",
            em_from_points(points),
        );
        let mut advances = Vec::new();
        face.shape("VE", |_, glyph| advances.push(glyph.advance));
        assert_eq!(advances.first().copied(), Some(expected_advance));
    }
}

#[test]
#[ignore = "requires pinned C08 space fonts"]
fn fallback_space_kerning() {
    use sha2::{Digest, Sha256};

    let em = em_from_points(24.0);
    let mut face = pinned_text_font(
        C08_SPACE_FIXTURE_ENV,
        "fonts/Probe.ttf",
        "bdff3e5659d67e67def05b33f749683b9376ae819d65d3dd62ac4640b3aaef48",
        em,
    );
    let root = std::path::PathBuf::from(std::env::var_os(C08_SPACE_FIXTURE_ENV).expect("fixture"));
    let arial = std::fs::read(root.join("fonts/Arial.ttf")).expect("Arial font");
    assert_eq!(
        format!("{:x}", Sha256::digest(&arial)),
        "30328e88b08753dc63b14d84ce882ce504d53e27a56e637f5a068fdedc6ae385"
    );
    face.fallbacks.push(paper_text::Font::parse(arial, em).expect("Arial face"));

    for (text, expected) in [
        ("A A A", vec![(36, 0.0), (3, 61.0), (36, 83.0), (3, 144.0), (36, 166.0)]),
        ("AAA", vec![(36, 0.0), (36, 66.0), (36, 132.0)]),
    ] {
        let mut cursor = 0.0;
        let mut glyphs = Vec::new();
        face.shape(text, |font, glyph| {
            assert!(std::ptr::eq(font, &raw const face.fallbacks[0]));
            glyphs.push((glyph.id, cursor + glyph.x));
            cursor += glyph.advance;
        });
        assert_eq!(glyphs, expected, "{text}");
    }
    let mut cursor = 0.0;
    let mut glyphs = Vec::new();
    face.shape("LIVE TEXT", |font, glyph| {
        assert!(std::ptr::eq(font, &raw const face.fallbacks[0]));
        glyphs.push((glyph.id, cursor + glyph.x));
        cursor += glyph.advance;
    });
    assert_eq!(glyphs[4..6], [(3, 214.0), (55, 239.0)]);
}

#[test]
#[ignore = "requires pinned C08 no-space font"]
fn fallback_space_coverage() {
    use sha2::{Digest, Sha256};

    let em = em_from_points(24.0);
    let mut face = pinned_text_font(
        C08_COVERAGE_FIXTURE_ENV,
        "fonts/Probe.ttf",
        "bdff3e5659d67e67def05b33f749683b9376ae819d65d3dd62ac4640b3aaef48",
        em,
    );
    let root = std::path::PathBuf::from(
        std::env::var_os(C08_COVERAGE_FIXTURE_ENV).expect("coverage fixture"),
    );
    let arial = std::fs::read(root.join("fonts/ArialNoSpace.ttf")).expect("Arial no-space font");
    assert_eq!(
        format!("{:x}", Sha256::digest(&arial)),
        "7f5fe67fed84e0d5e5e773b7e77c23abac0425bdf33e682d04f31659bc9d38e9"
    );
    face.fallbacks.push(paper_text::Font::parse(arial, em).expect("Arial no-space face"));

    let mut space = Vec::new();
    face.font.shape(" ", |glyph| space.push(glyph));
    assert_eq!(space.len(), 1);
    assert!(space[0].advance > 0.0);
    let mut missing = Vec::new();
    face.fallbacks[0].shape(" ", |glyph| missing.push(glyph));
    assert!(missing.is_empty());
    let mut letter = Vec::new();
    face.fallbacks[0].shape("A", |glyph| letter.push(glyph));
    assert_eq!(letter.len(), 1);

    let mut cursor = 0.0;
    let mut actual = Vec::new();
    face.shape("A A A", |font, glyph| {
        actual.push((
            std::ptr::eq(font, &raw const face.font),
            glyph.id,
            cursor + glyph.x,
            glyph.advance,
        ));
        cursor += glyph.advance;
    });
    let letter = letter[0];
    let space = space[0];
    assert_eq!(
        actual,
        [
            (false, letter.id, letter.x, letter.advance),
            (true, space.id, letter.advance + space.x, space.advance),
            (false, letter.id, letter.advance + space.advance + letter.x, letter.advance),
            (true, space.id, 2.0 * letter.advance + space.advance + space.x, space.advance),
            (false, letter.id, 2.0 * (letter.advance + space.advance) + letter.x, letter.advance),
        ]
    );
    assert_eq!(cursor, 3.0 * letter.advance + 2.0 * space.advance);
}

fn pinned_text_font(env: &str, path: &str, hash: &str, em: f32) -> Face {
    use sha2::{Digest, Sha256};

    let root = std::env::var_os(env).expect(env);
    let bytes = std::fs::read(std::path::Path::new(&root).join(path)).expect("fixture font");
    assert_eq!(format!("{:x}", Sha256::digest(&bytes)), hash);
    Face::parse(&bytes, em).expect("fixture face")
}

#[test]
#[ignore = "requires pinned text fixture roots"]
fn blank_text_layers() {
    use sha2::{Digest, Sha256};

    let root = std::env::var_os(TEXT_RASTER_FIXTURE_ENV).expect(TEXT_RASTER_FIXTURE_ENV);
    let font =
        std::fs::read(std::path::Path::new(&root).join("fonts/probe.ttf")).expect("fixture font");
    assert_eq!(
        format!("{:x}", Sha256::digest(&font)),
        "ffc549b7fc28057d624086111f1ba0b1f752d2d7ff7c5363dfa064e4dca17d47"
    );
    let scene = br#"{"objects":[
        {"id":1,"text":"","font":"fonts/probe.ttf"},
        {"id":2,"text":"   ","font":"fonts/probe.ttf"}]}"#;
    let package = Package::parse(crate::tests::build_pkg(&[
        ("scene.json", scene),
        ("fonts/probe.ttf", &font),
    ]))
    .expect("package");
    let model = crate::model::load_with(&package, &Assets::discover(None)).expect("model");
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    assert_eq!(model.layers.len(), 2);
    for layer in &model.layers {
        assert!(layer.texture.text_batch.as_ref().expect("glyph batch").quads.is_empty());
        assert!(layer.texture.pixels.levels[0].data.chunks_exact(4).all(|rgba| rgba[3] == 0));
    }
}

#[test]
#[ignore = "requires pinned text fixture roots"]
fn fractional_text_layout() {
    for (points, advances, width) in [
        (17.35, [43.0; 5], 210.0),
        (31.1, [77.0; 5], 376.0),
        (42.0, [105.0; 5], 511.0),
        (96.0, [240.0; 5], 1168.0),
    ] {
        let face = pinned_text_font(
            TEXT_RASTER_FIXTURE_ENV,
            "fonts/probe.ttf",
            "ffc549b7fc28057d624086111f1ba0b1f752d2d7ff7c5363dfa064e4dca17d47",
            em_from_points(points),
        );
        let mut actual = Vec::new();
        face.shape("Hello", |_, glyph| actual.push(glyph.advance));
        assert_eq!(actual, advances);
        let metrics = face.measure("Hello").expect("Hello metrics");
        assert_eq!(metrics.width, width);
        assert_eq!(box_offset(&metrics, HAlign::Center, VAlign::Top).0, -width * 0.5);
        assert_eq!(box_offset(&metrics, HAlign::Right, VAlign::Top).0, -width);
    }
}

#[test]
#[ignore = "requires pinned text fixture roots"]
fn kerned_text_layout() {
    let face = pinned_text_font(
        TEXT_ORACLE_FIXTURES_ENV,
        "text-fallback-metrics-hebrew-24/fonts/primary.ttf",
        "6b04c8dd65af6b73eb4279472ed1580b29102d6496a377340e80a40cdb3b22c9",
        100.0,
    );
    let mut pair = Vec::new();
    face.shape("AV", |_, glyph| pair.push((glyph.id, glyph.advance, glyph.x)));
    assert_eq!(pair, [(36, 59.0, 0.0), (57, 60.0, 0.0)]);
    assert_eq!(face.measure("AV").expect("pair metrics").width, 119.0);
}

#[test]
#[ignore = "requires pinned text fixture roots"]
fn rtl_mark_layout() {
    let rtl = pinned_text_font(
        TEXT_ORACLE_FIXTURES_ENV,
        "text-fallback-metrics-hebrew-24/fonts/hebrew.ttf",
        "30328e88b08753dc63b14d84ce882ce504d53e27a56e637f5a068fdedc6ae385",
        100.0,
    );
    for (text, base_x, ids) in [("אְ", 0.0, vec![1280]), ("אְב", 59.0, vec![1281, 1280])] {
        let mut bases = Vec::new();
        let mut marks = Vec::new();
        let mut cursor = 0.0;
        rtl.shape(text, |_, glyph| {
            if glyph.advance == 0.0 {
                let ink = rtl.font.metrics(glyph.id).expect("mark ink");
                marks.push((glyph.id, cursor + glyph.x, glyph.y, ink));
            } else {
                bases.push(glyph.id);
            }
            cursor += glyph.advance;
        });
        assert_eq!(bases, ids);
        assert_eq!(marks.len(), 1);
        assert_eq!(marks[0].0, 1256);
        assert!((marks[0].1 - base_x - 32.234_375).abs() <= 0.015_625);
        assert!((marks[0].2 + 0.968_75).abs() <= 0.015_625);
        assert!((marks[0].1 + marks[0].3.xmin as f32 - base_x - 28.234_375).abs() <= 0.015_625);
        let top = marks[0].2 + (marks[0].3.ymin + marks[0].3.height as i32) as f32;
        assert!((top + 1.968_75).abs() <= 0.015_625);
    }
}

#[test]
#[ignore = "requires pinned text fixture roots"]
fn mixed_fallback_layout() {
    let mut face = pinned_text_font(
        TEXT_ORACLE_FIXTURES_ENV,
        "text-fallback-metrics-hebrew-24/fonts/primary.ttf",
        "6b04c8dd65af6b73eb4279472ed1580b29102d6496a377340e80a40cdb3b22c9",
        100.0,
    );
    let rtl = pinned_text_font(
        TEXT_ORACLE_FIXTURES_ENV,
        "text-fallback-metrics-hebrew-24/fonts/hebrew.ttf",
        "30328e88b08753dc63b14d84ce882ce504d53e27a56e637f5a068fdedc6ae385",
        100.0,
    );
    face.fallbacks.push(rtl.font);
    let mut mixed = Vec::new();
    let mut cursor = 0.0;
    face.shape("AאבB", |_, glyph| {
        mixed.push((glyph.id, cursor + glyph.x));
        cursor += glyph.advance;
    });
    assert_eq!(mixed, [(36, 0.0), (1281, 63.0), (1280, 122.0), (37, 184.0)]);
    assert_eq!(cursor, 249.0);
    let metrics = face.measure("AאבB").expect("mixed metrics");
    assert_eq!(metrics.width, 244.0);
    assert_eq!(box_offset(&metrics, HAlign::Center, VAlign::Top).0, -122.0);
    assert_eq!(box_offset(&metrics, HAlign::Right, VAlign::Top).0, -244.0);
}

#[test]
#[ignore = "requires pinned text fixture roots"]
fn pinned_text_baselines() {
    for (em, ascent, top) in [(50.0, 54.0, 20), (100.0, 107.0, 36)] {
        let face = pinned_text_font(
            TEXT_ORACLE_FIXTURES_ENV,
            "text-fallback-metrics-hebrew-24/fonts/primary.ttf",
            "6b04c8dd65af6b73eb4279472ed1580b29102d6496a377340e80a40cdb3b22c9",
            em,
        );
        let (metrics, texture) = face.rasterize("A", HAlign::Left).expect("primary raster");
        assert_eq!(metrics.ascent, ascent);
        let first = texture.pixels.levels[0]
            .data
            .chunks_exact(4)
            .position(|rgba| rgba[3] != 0)
            .expect("primary ink");
        assert_eq!(first / texture.width as usize, top);
    }
    for (em, ascent, top, height) in [(175.0, 169.0, 41, 221), (400.0, 386.0, 93, 502)] {
        let face = pinned_text_font(
            TEXT_RASTER_FIXTURE_ENV,
            "fonts/probe.ttf",
            "ffc549b7fc28057d624086111f1ba0b1f752d2d7ff7c5363dfa064e4dca17d47",
            em,
        );
        let (metrics, texture) = face.rasterize("A", HAlign::Left).expect("probe raster");
        assert_eq!(metrics.ascent, ascent);
        assert_eq!(texture.height, height);
        let first = texture.pixels.levels[0]
            .data
            .chunks_exact(4)
            .position(|rgba| rgba[3] != 0)
            .expect("probe ink");
        assert_eq!(first / texture.width as usize, top);
    }
}

fn trimmed_ink(texture: &Texture) -> (Vec<u8>, [usize; 2]) {
    let width = texture.width as usize;
    let pixels = &texture.pixels.levels[0].data;
    let mut bounds = [width, texture.height as usize, 0, 0];
    for (index, pixel) in pixels.chunks_exact(4).enumerate() {
        if pixel[3] != 0 {
            let x = index % width;
            let y = index / width;
            bounds[0] = bounds[0].min(x);
            bounds[1] = bounds[1].min(y);
            bounds[2] = bounds[2].max(x + 1);
            bounds[3] = bounds[3].max(y + 1);
        }
    }
    assert!(bounds[0] < bounds[2] && bounds[1] < bounds[3]);
    let mut alpha = Vec::new();
    for y in bounds[1]..bounds[3] {
        for x in bounds[0]..bounds[2] {
            alpha.push(pixels[(y * width + x) * 4 + 3]);
        }
    }
    (alpha, [bounds[2] - bounds[0], bounds[3] - bounds[1]])
}

#[test]
#[ignore = "requires pinned text fixture roots"]
fn native_mark_masks() {
    use sha2::{Digest, Sha256};

    for (em, text, size, expected) in [
        (50.0, "אְב", [58, 40], "ec1517a97091fb3abf6dcfddab76b820276f10b908ca8d822d66e8fae35452d6"),
        (50.0, "אְבְ", [58, 40], "ce8b45aa21effe7761fed6c4460612fcb41ba60c08ff215837724899f6683bde"),
        (50.0, "אֲב", [58, 40], "5cdb55efa6f5ace5a9fd357050064e7142c9a2b67f3ed274ed73fa40c04756ec"),
        (
            72.291_664,
            "אְב",
            [86, 58],
            "ea8f77b2569e62da7cf819a714dad3d9a4a55007970b3ae5bf0ff103d5099507",
        ),
        (
            72.291_664,
            "אְבְ",
            [86, 58],
            "ba6a3f64a19d0850302444418bbbeb217ddba89cac019998558bb158f2b5a470",
        ),
        (
            72.291_664,
            "אֲב",
            [86, 58],
            "c7c5fbc606612eefd08582d4007ba242b15558c56237c35323aa347c0cd13c90",
        ),
    ] {
        let face = pinned_text_font(
            TEXT_ORACLE_FIXTURES_ENV,
            "text-hebrew-bidi/fonts/Probe.ttf",
            "baccc64becc3eb7d104b7c84d99f5314a0a1f896e2b3ea6c2f22fc08d2003bee",
            em,
        );
        for align in [HAlign::Left, HAlign::Center, HAlign::Right] {
            let (_, texture) = face.rasterize(text, align).expect("native mark raster");
            let (alpha, bounds) = trimmed_ink(&texture);
            assert_eq!(bounds, size);
            assert_eq!(format!("{:x}", Sha256::digest(alpha)), expected);
            let texture = face
                .rasterize_in(text, align, 400, 200, Raster::of(RasterMode::Direct, &Value::Null))
                .expect("aligned mark raster");
            let (alpha, bounds) = trimmed_ink(&texture);
            assert_eq!(bounds, size);
            assert_eq!(format!("{:x}", Sha256::digest(alpha)), expected);
        }
    }
}

#[test]
#[ignore = "requires pinned text fixture roots"]
fn missing_text_clusters() {
    use sha2::{Digest, Sha256};

    let assets = crate::effects::Assets::discover(None);
    let emoji = assets.read_bytes("fonts/TwemojiMozilla.ttf").expect("native emoji font");
    assert_eq!(
        format!("{:x}", Sha256::digest(&emoji)),
        "063b2a145c2331e1aede451242b5107a0f29a44d782fdb24a6adb9754dc48d14"
    );
    for (text, width, expected) in [
        ("AB", 123.0, "041d1a0913ff6d40c4e944059a484fe4c40ff410e57cb8e9b9c673e5846a2355"),
        ("AابB", 123.0, "041d1a0913ff6d40c4e944059a484fe4c40ff410e57cb8e9b9c673e5846a2355"),
        ("A\u{0378}B", 123.0, "041d1a0913ff6d40c4e944059a484fe4c40ff410e57cb8e9b9c673e5846a2355"),
        ("A\u{FFFF}B", 123.0, "041d1a0913ff6d40c4e944059a484fe4c40ff410e57cb8e9b9c673e5846a2355"),
        ("A\u{10000}B", 123.0, "041d1a0913ff6d40c4e944059a484fe4c40ff410e57cb8e9b9c673e5846a2355"),
        ("A\u{20000}B", 123.0, "041d1a0913ff6d40c4e944059a484fe4c40ff410e57cb8e9b9c673e5846a2355"),
        ("A\u{10FFFF}B", 123.0, "041d1a0913ff6d40c4e944059a484fe4c40ff410e57cb8e9b9c673e5846a2355"),
        ("A😀B", 223.0, "2e5d0a4c89de16e742409916566e6b1f62fec42ed389e329dbaf674809b5b00c"),
        ("A👩B", 223.0, "2e5d0a4c89de16e742409916566e6b1f62fec42ed389e329dbaf674809b5b00c"),
        ("A👩‍💻B", 223.0, "2e5d0a4c89de16e742409916566e6b1f62fec42ed389e329dbaf674809b5b00c"),
        ("A😀😀B", 323.0, "06497da604c2258918c58aef6acd60a4934a7ff554d498bc88547714dcee9739"),
    ] {
        let mut face = pinned_text_font(
            TEXT_ORACLE_FIXTURES_ENV,
            "text-fallback-metrics-hebrew-24/fonts/primary.ttf",
            "6b04c8dd65af6b73eb4279472ed1580b29102d6496a377340e80a40cdb3b22c9",
            100.0,
        );
        face.load_fallbacks(&assets, text);
        let (metrics, texture) =
            face.rasterize(text, HAlign::Left).expect("missing cluster raster");
        assert_eq!(metrics.width, width, "{text}");
        let (alpha, bounds) = trimmed_ink(&texture);
        assert_eq!(bounds, [width as usize, 72], "{text}");
        assert_eq!(format!("{:x}", Sha256::digest(alpha)), expected, "{text}");
    }
}

#[test]
#[ignore = "requires pinned text fixture roots"]
fn native_overlap_masks() {
    use sha2::{Digest, Sha256};

    let face = pinned_text_font(
        TEXT_ORACLE_FIXTURES_ENV,
        "text-arabic-shape/fonts/Probe.ttf",
        "bdff3e5659d67e67def05b33f749683b9376ae819d65d3dd62ac4640b3aaef48",
        100.0,
    );
    for (text, dimensions, expected) in [
        ("لا", [48, 72], "a14207933fc1c1a4bd08a18113ec77c132e34a5750abe9b66d364a3ec9dd187e"),
        ("لام", [98, 112], "098549b3da5eedec202fdce80b079bdc9ebd341118cd4fce18bf9ae5b20210f1"),
        ("مـ", [74, 37], "6b636152154bba2392afdfc552d5c82fe09308ed2e195655bd9459f7ae8c8b87"),
        ("1اب", [155, 94], "9c73c83e35819ecb391522be03e55d5e72a5bc7f36e8a57d6f3c2282f64b9480"),
        ("اب1", [166, 94], "4162000d6e07fce5ec220ae922c37d6f7a5dbc6402b4bc05e3bab493c614276d"),
        ("123", [158, 74], "ab33c566f6f1c199242dbbc97c37aee69b4ad9542e897f318d741fe0fb06a3eb"),
    ] {
        let (_, texture) = face.rasterize(text, HAlign::Left).expect("native overlap raster");
        let (alpha, bounds) = trimmed_ink(&texture);
        assert_eq!(bounds, dimensions, "{text}");
        assert_eq!(format!("{:x}", Sha256::digest(alpha)), expected, "{text}");
    }
}

#[test]
#[ignore = "requires pinned text fixture roots"]
fn native_glyph_bounds() {
    let face = pinned_text_font(
        TEXT_ORACLE_FIXTURES_ENV,
        "text-fallback-metrics-hebrew-24/fonts/primary.ttf",
        "6b04c8dd65af6b73eb4279472ed1580b29102d6496a377340e80a40cdb3b22c9",
        100.0,
    );
    for (text, expected) in [
        ("ffi", vec![[2, 31, 38, 77], [36, 31, 38, 77], [76, 34, 12, 74]]),
        ("AV", vec![[1, 36, 64, 72], [60, 36, 60, 72]]),
    ] {
        let (_, texture) = face.rasterize(text, HAlign::Left).unwrap();
        let batch = texture.text_batch.unwrap();
        assert_eq!(batch.canvas, [texture.width, texture.height]);
        assert_eq!(
            batch.quads.iter().map(|quad| quad.bounds).collect::<Vec<_>>(),
            expected,
            "{text}"
        );
    }
}

#[test]
#[ignore = "requires pinned text fixture roots"]
fn glyph_atlas_masks() {
    use sha2::{Digest, Sha256};

    let face = pinned_text_font(
        TEXT_ORACLE_FIXTURES_ENV,
        "text-fallback-metrics-hebrew-24/fonts/primary.ttf",
        "6b04c8dd65af6b73eb4279472ed1580b29102d6496a377340e80a40cdb3b22c9",
        100.0,
    );
    let (_, texture) = face.rasterize("fiAV", HAlign::Left).unwrap();
    let batch = texture.text_batch.unwrap();
    let expected = [
        "205fc7302ca86fe62fe3f682be76e557a3d9ae137646410345de1460119dd982",
        "5338072cb65fd10581473038444648b7374693cfcd246a41bb256ca90149f4d1",
        "3cd3123b5f2c7326acc26ba8e313432e3ebd5e05d132aa9a61478687d41dc911",
        "c91f3295869c6cf253a94e36b4498e122b3e18b2e5f8a2488501139e51addb42",
    ];
    assert_eq!(batch.quads.len(), expected.len());
    let atlas = &batch.pixels.levels[0];
    for (quad, expected) in batch.quads.iter().zip(expected) {
        let [left, top, width, height] = quad.uv;
        let mut alpha = Vec::new();
        for y in top..top + height {
            for x in left..left + width {
                alpha.push(atlas.data[((y * atlas.width + x) * 4 + 3) as usize]);
            }
        }
        assert_eq!(format!("{:x}", Sha256::digest(alpha)), expected);
    }
}

#[test]
#[ignore = "requires pinned text fixture roots"]
fn native_effect_raster() {
    let face = pinned_text_font(
        TEXT_ORACLE_FIXTURES_ENV,
        "text-arabic-shape/fonts/Probe.ttf",
        "bdff3e5659d67e67def05b33f749683b9376ae819d65d3dd62ac4640b3aaef48",
        100.0,
    );
    for (text, width) in [("لا", 52), ("لام", 100), ("مـ", 74)] {
        let (_, plain) = face.rasterize(text, HAlign::Center).unwrap();
        let (metrics, effect) = face
            .rasterize_mode(text, HAlign::Center, Raster::of(RasterMode::Effect, &Value::Null))
            .unwrap();
        assert_eq!([effect.width, effect.height], [width, 211]);
        for y in 0..effect.height as usize {
            let old = ((y + 1) * plain.width as usize + 1) * 4;
            let new = y * effect.width as usize * 4;
            let row = effect.width as usize * 4;
            assert_eq!(
                &plain.pixels.levels[0].data[old..old + row],
                &effect.pixels.levels[0].data[new..new + row]
            );
        }
        let padded = plain.text_batch.unwrap();
        let compact = effect.text_batch.unwrap();
        assert_eq!(compact.canvas, [width, 211]);
        assert_eq!(padded.quads.len(), compact.quads.len());
        for (old, new) in padded.quads.iter().zip(&compact.quads) {
            assert_eq!(
                new.bounds,
                [old.bounds[0] - 1, old.bounds[1] - 1, old.bounds[2], old.bounds[3]]
            );
        }
        let plain_offset = box_offset(&metrics, HAlign::Center, VAlign::Center);
        let effect_offset = render_offset(
            &metrics,
            HAlign::Center,
            VAlign::Center,
            Raster::of(RasterMode::Effect, &Value::Null),
        );
        assert_eq!(effect_offset, (plain_offset.0 + 1.0, plain_offset.1 + 1.0));
    }
}
#[test]
#[ignore = "requires pinned text fixture roots"]
fn inactive_effects_keep_raster() {
    use sha2::{Digest, Sha256};

    let root = std::env::var_os(TEXT_ORACLE_FIXTURES_ENV).expect(TEXT_ORACLE_FIXTURES_ENV);
    let font = std::fs::read(std::path::Path::new(&root).join("text-arabic-shape/fonts/Probe.ttf"))
        .unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&font)),
        "bdff3e5659d67e67def05b33f749683b9376ae819d65d3dd62ac4640b3aaef48"
    );
    for scripted in [false, true] {
        let text = if scripted {
            serde_json::json!({"value":"لا","script":"export function update(){return 'لا';}"})
        } else {
            serde_json::json!("لا")
        };
        let mut objects = Vec::new();
        for (index, effects) in [
            serde_json::json!([]),
            serde_json::json!([{"file":"effects/missing.json"}]),
            serde_json::json!([{"file":"effects/probe.json","visible":false}]),
            serde_json::json!([{"file":"effects/probe.json","visible":true}]),
        ]
        .into_iter()
        .enumerate()
        {
            objects.push(serde_json::json!({"id":index+1,"text":text,"font":"fonts/Probe.ttf","pointsize":24,"effects":effects}));
        }
        let scene = serde_json::to_vec(&serde_json::json!({"objects":objects})).unwrap();
        let package = Package::parse(crate::tests::build_pkg(&[
            ("scene.json", &scene),
            ("fonts/Probe.ttf", &font),
            ("effects/probe.json", br#"{"passes":[{"material":"materials/probe.json"}]}"#),
            ("materials/probe.json", br#"{"passes":[{"shader":"probe","blending":"normal"}]}"#),
            ("shaders/probe.vert", b"uniform mat4 g_ModelViewProjectionMatrix;\nattribute vec3 a_Position;\nattribute vec2 a_TexCoord;\nvarying vec2 v_TexCoord;\nvoid main(){v_TexCoord=a_TexCoord; gl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);}"),
            ("shaders/probe.frag", b"uniform sampler2D g_Texture0;\nvarying vec2 v_TexCoord;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,v_TexCoord);}"),
        ])).unwrap();
        let model = crate::model::load_with(&package, &Assets::discover(None)).unwrap();
        assert_eq!(model.layers.len(), 4);
        assert_eq!(model.scripts.is_some(), scripted);
        let direct = &model.layers[0];
        for layer in &model.layers[1..3] {
            assert_eq!(layer.texture.text_batch, direct.texture.text_batch);
            assert_eq!(layer.texture.pixels, direct.texture.pixels);
            assert_eq!(layer.geometry, direct.geometry);
            assert_eq!(layer.size, direct.size);
            assert_eq!(layer.center, direct.center);
        }
        assert!(model.layers[1].effects.is_empty());
        if scripted {
            assert_eq!(model.layers[2].effects.len(), 1);
            assert!(!model.layers[2].effects[0].passes.is_empty());
            assert!(!model.layers[2].effects[0].state.visible);
        } else {
            assert!(model.layers[2].effects.is_empty());
        }
        let active = &model.layers[3];
        assert_eq!(active.effects.len(), 1);
        assert!(!active.effects[0].passes.is_empty());
        assert_eq!([active.texture.width, active.texture.height], [52, 211]);
        assert_ne!(active.texture.text_batch, direct.texture.text_batch);
    }
}

#[test]
#[ignore = "requires pinned C08 space fonts"]
fn native_backdrop_metrics() {
    use sha2::{Digest, Sha256};

    let root = std::path::PathBuf::from(std::env::var_os(C08_SPACE_FIXTURE_ENV).expect("fixture"));
    let arial = std::fs::read(root.join("fonts/Arial.ttf")).expect("Arial font");
    assert_eq!(
        format!("{:x}", Sha256::digest(&arial)),
        "30328e88b08753dc63b14d84ce882ce504d53e27a56e637f5a068fdedc6ae385"
    );
    for (points, text, height, center, width, canvas) in [
        (24.0, "A A", 211.0, 36.5, Some(150.0), 214),
        (26.0, "A A", 229.0, 40.0, None, 231),
        (12.0, "A A", 106.0, 18.5, Some(75.0), 108),
        (12.0, "A A\nA A\nA A\nA A", 424.0, 18.5, Some(75.0), 426),
    ] {
        let em = em_from_points(points);
        let mut face = pinned_text_font(
            C08_SPACE_FIXTURE_ENV,
            "fonts/Probe.ttf",
            "bdff3e5659d67e67def05b33f749683b9376ae819d65d3dd62ac4640b3aaef48",
            em,
        );
        face.fallbacks.push(paper_text::Font::parse(arial.clone(), em).unwrap());
        for mode in [RasterMode::Direct, RasterMode::Effect] {
            let (metrics, texture) =
                face.rasterize_mode(text, HAlign::Left, Raster::of(mode, &Value::Null)).unwrap();
            let bounds = face.bounds(&metrics, HAlign::Left, VAlign::Center);
            assert_eq!(bounds.size[1], height);
            assert_eq!(bounds.offset[1], center);
            if let Some(width) = width {
                assert_eq!(bounds.size[0], width);
                assert_eq!(bounds.offset[0], width * 0.5);
            }
            assert_eq!(metrics.line_height, height / metrics.lines as f32);
            if mode == RasterMode::Direct {
                assert_eq!(texture.height, canvas);
                if metrics.lines > 1 {
                    let batch = texture.text_batch.as_ref().unwrap();
                    assert_eq!(batch.quads.len(), 8);
                    let top = render_offset(
                        &metrics,
                        HAlign::Left,
                        VAlign::Center,
                        Raster::of(mode, &Value::Null),
                    )
                    .1 - RASTER_GUARD;
                    for (pair, expected) in
                        batch.quads.chunks_exact(2).zip([201.5, 307.5, 413.5, 519.5])
                    {
                        for glyph in pair {
                            assert_eq!(360.0 + top + glyph.bounds[1] as f32, expected);
                        }
                    }
                }
            }
        }
    }
}

#[test]
#[ignore = "requires pinned dynamic text package"]
fn padded_effect_canvas() {
    let path = std::env::var_os("SKWD_DYNAMIC_TEXT_PACKAGE").expect("pinned text package");
    let package = Package::open(std::path::Path::new(&path)).unwrap();
    let assets = Assets::discover(Some("/nonexistent"));
    let props = Properties::new();
    let mut object = serde_json::json!({
        "text":"A A\nA A\nA A\nA A", "font":"fonts/Probe.ttf",
        "pointsize":12, "horizontalalign":"left", "verticalalign":"center",
        "padding":0, "opaquebackground":true
    });
    let source = std::sync::Arc::new(script::Source::new(&package, &assets));
    let (plain, mut text) =
        script::render(&package, &assets, &object, &props, source, RasterMode::Effect).unwrap();
    object["padding"] = serde_json::json!(12);
    let previous = text.update_style(&object, &props).unwrap();
    let padded = text.render(&text.shown).unwrap();
    assert_eq!([padded.texture.width, padded.texture.height], [99, 448]);
    assert_eq!(padded.bounds.size, [75.0, 424.0]);
    assert_eq!(padded.bounds.offset, [37.5, 18.5]);
    assert_eq!(padded.layout(), ([99.0, 448.0], [37.5, 18.5]));
    let static_text = render(&package, &assets, &object, &props, RasterMode::Effect).unwrap();
    assert_eq!(static_text.layout(), padded.layout());
    assert_eq!(static_text.texture.pixels.levels[0].data, padded.texture.pixels.levels[0].data);
    let plain_batch = plain.texture.text_batch.as_ref().unwrap();
    let padded_batch = padded.texture.text_batch.as_ref().unwrap();
    assert_eq!(plain_batch.quads.len(), padded_batch.quads.len());
    for (plain, padded) in plain_batch.quads.iter().zip(&padded_batch.quads) {
        assert_eq!(
            padded.bounds,
            [plain.bounds[0] + 12, plain.bounds[1] + 12, plain.bounds[2], plain.bounds[3]]
        );
    }
    text.reject(object["text"].as_str().unwrap(), None);
    text.restore(previous);
    assert!(text.rejected(object["text"].as_str().unwrap(), &object, &props, 0));
    let restored = text.render(&text.shown).unwrap();
    assert_eq!(restored.layout(), plain.layout());
    assert_eq!(restored.texture.pixels.levels[0].data, plain.texture.pixels.levels[0].data);
}

#[test]
#[ignore = "requires pinned dynamic text package"]
fn effect_padding_style() {
    let path = std::env::var_os("SKWD_DYNAMIC_TEXT_PACKAGE").expect("pinned text package");
    let package = Package::open(std::path::Path::new(&path)).unwrap();
    let assets = Assets::discover(Some("/nonexistent"));
    let props = Properties::new();
    let source = std::sync::Arc::new(script::Source::new(&package, &assets));
    let mut object = serde_json::json!({"text":"A A", "font":"fonts/Probe.ttf",
        "pointsize":26, "horizontalalign":"left", "verticalalign":"center"});
    let (rendered, mut effect) =
        script::render(&package, &assets, &object, &props, source.clone(), RasterMode::Effect)
            .unwrap();
    assert_eq!([rendered.texture.width, rendered.texture.height], [162, 229]);
    let (_, mut direct) =
        script::render(&package, &assets, &object, &props, source, RasterMode::Direct).unwrap();
    object["padding"] = serde_json::json!(12);
    object["backgroundcolor"] = serde_json::json!("0 1 0");
    assert!(direct.update_style(&object, &props).is_none());
    assert!(!direct.needs_render);
    assert!(effect.update_style(&object, &props).is_some());
    let padded = effect.render(&effect.shown).unwrap();
    assert_eq!([padded.texture.width, padded.texture.height], [186, 253]);
    effect.needs_render = false;
    object["opaquebackground"] = serde_json::json!(true);
    object["backgroundbrightness"] = serde_json::json!(0.5);
    assert!(effect.update_style(&object, &props).is_none());
    assert!(!effect.needs_render);
    for value in [serde_json::json!(-1), serde_json::json!("NaN"), serde_json::json!(8192)] {
        object["padding"] = value;
        let previous = effect.update_style(&object, &props).unwrap();
        assert!(effect.render(&effect.shown).is_none());
        let shown = effect.shown.clone();
        effect.reject(&shown, None);
        effect.restore(previous);
        assert!(effect.rejected(&shown, &object, &props, 0));
        assert_eq!(effect.render(&shown).unwrap().layout(), padded.layout());
    }
}

#[test]
fn padding_reads_pairs() {
    for (value, expected) in [
        (serde_json::json!(12), Some([12.0; 2])),
        (serde_json::json!("12 24"), Some([12.0, 24.0])),
        (serde_json::json!([12]), Some([12.0; 2])),
        (serde_json::json!({"value":[12, 24]}), Some([12.0, 24.0])),
        (serde_json::json!([]), None),
        (serde_json::json!([12, "bad"]), None),
    ] {
        assert_eq!(script::padding_of(&serde_json::json!({"padding":value})), expected);
    }
    for value in [
        serde_json::json!(-1),
        serde_json::json!("NaN 12"),
        serde_json::json!("inf"),
        serde_json::json!([]),
    ] {
        let object = serde_json::json!({"padding":value});
        assert!(Raster::of(RasterMode::Effect, &object).padding.is_none());
        assert_eq!(Raster::of(RasterMode::Direct, &object).padding, Some([0.0; 2]));
    }
}

#[test]
#[ignore = "requires pinned dynamic text package"]
fn effect_canvas_budget() {
    let path = std::env::var_os("SKWD_DYNAMIC_TEXT_PACKAGE").expect("pinned text package");
    let package = Package::open(std::path::Path::new(&path)).unwrap();
    let assets = Assets::discover(Some("/nonexistent"));
    let props = Properties::new();
    let mut object = serde_json::json!({"text":"A A", "font":"fonts/Probe.ttf",
        "pointsize":12, "padding":12});
    assert!(render(&package, &assets, &object, &props, RasterMode::Effect).is_some());
    object["padding"] = serde_json::json!(2000);
    assert!(render(&package, &assets, &object, &props, RasterMode::Effect).is_none());
    assert!(render(&package, &assets, &object, &props, RasterMode::Direct).is_some());
    let face = Face::parse(package.find("fonts/Probe.ttf").unwrap(), em_from_points(12.0)).unwrap();
    assert!(
        face.rasterize_in(
            "م",
            HAlign::Left,
            4096,
            4096,
            Raster::of(RasterMode::Effect, &Value::Null)
        )
        .is_none()
    );
}
