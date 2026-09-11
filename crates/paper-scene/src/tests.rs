#![cfg(test)]

use crate::{audit, effects, model, pkg, scene, tex};

fn push_len_str(out: &mut Vec<u8>, text: &str) {
    out.extend_from_slice(&(u32::try_from(text.len()).unwrap()).to_le_bytes());
    out.extend_from_slice(text.as_bytes());
}

fn push_nul_str(out: &mut Vec<u8>, text: &str) {
    out.extend_from_slice(text.as_bytes());
    out.push(0);
}

fn push_i32(out: &mut Vec<u8>, value: i32) {
    out.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn build_pkg(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    push_len_str(&mut out, "PKGV0007");
    push_i32(&mut out, i32::try_from(files.len()).unwrap());
    let mut offset = 0u32;
    for (path, data) in files {
        push_len_str(&mut out, path);
        out.extend_from_slice(&offset.to_le_bytes());
        out.extend_from_slice(&(u32::try_from(data.len()).unwrap()).to_le_bytes());
        offset += u32::try_from(data.len()).unwrap();
    }
    for (_, data) in files {
        out.extend_from_slice(data);
    }
    out
}

fn build_tex(
    container: &str,
    format: i32,
    flags: i32,
    payload: &[u8],
    compress: bool,
    anim: Option<&str>,
) -> Vec<u8> {
    let mut out = Vec::new();
    push_nul_str(&mut out, "TEXV0005");
    push_nul_str(&mut out, "TEXI0001");
    push_i32(&mut out, format);
    push_i32(&mut out, flags);
    for dim in [4, 4, 4, 4] {
        push_i32(&mut out, dim);
    }
    push_i32(&mut out, 0);
    push_nul_str(&mut out, container);
    push_i32(&mut out, 1);
    if container >= "TEXB0003" {
        push_i32(&mut out, -1);
    }
    push_i32(&mut out, 1);
    push_i32(&mut out, 4);
    push_i32(&mut out, 4);
    let body: Vec<u8>;
    if container >= "TEXB0002" {
        if compress {
            body = lz4_flex::block::compress(payload);
            push_i32(&mut out, 1);
        } else {
            body = payload.to_vec();
            push_i32(&mut out, 0);
        }
        push_i32(&mut out, i32::try_from(payload.len()).unwrap());
    } else {
        body = payload.to_vec();
    }
    push_i32(&mut out, i32::try_from(body.len()).unwrap());
    out.extend_from_slice(&body);
    if let Some(anim_magic) = anim {
        push_nul_str(&mut out, anim_magic);
        push_i32(&mut out, 1);
        if anim_magic == "TEXS0003" {
            push_i32(&mut out, 4);
            push_i32(&mut out, 4);
        }
        push_i32(&mut out, 0);
        out.extend_from_slice(&0.1f32.to_le_bytes());
        for value in [0f32, 0.0, 4.0, 0.0, 0.0, 4.0] {
            out.extend_from_slice(&value.to_le_bytes());
        }
    }
    out
}

#[test]
fn pkg_round_trip() {
    let bytes = build_pkg(&[("a/b.json", b"{}"), ("scene.json", b"{\"objects\":[]}")]);
    let package = pkg::Package::parse(bytes).unwrap();
    assert_eq!(package.version(), "PKGV0007");
    assert_eq!(package.entries().len(), 2);
    assert_eq!(package.find("a/b.json").unwrap(), b"{}");
    assert!(package.find_json("scene.json").unwrap().unwrap().get("objects").is_some());
    assert!(package.find("missing").is_none());
}

#[test]
fn pkg_rejects_garbage() {
    assert!(pkg::Package::parse(b"nope".to_vec()).is_err());
    let mut bytes = build_pkg(&[("x", b"12345")]);
    bytes.truncate(bytes.len() - 3);
    assert!(pkg::Package::parse(bytes).is_err());
}

#[test]
fn tex_uncompressed_and_lz4() {
    let payload = [7u8; 64];
    let plain = build_tex("TEXB0001", 0, 0, &payload, false, None);
    let parsed = tex::parse(&plain).unwrap();
    assert_eq!(parsed.meta.format, tex::TexFormat::Rgba8888);
    assert_eq!(parsed.images[0][0].data, payload);

    let packed = build_tex("TEXB0002", 7, 0, &payload, true, None);
    let parsed = tex::parse(&packed).unwrap();
    assert_eq!(parsed.meta.format, tex::TexFormat::Dxt1);
    assert_eq!(parsed.images[0][0].data, payload);

    let meta = tex::parse_meta(&packed).unwrap();
    assert_eq!(meta.image_count, 1);
    assert_eq!(meta.mip_counts, vec![1]);
}

#[test]
fn tex_gif_frames() {
    let bytes =
        build_tex("TEXB0003", 0, tex::FLAG_IS_GIF as i32, &[1u8; 64], false, Some("TEXS0003"));
    let parsed = tex::parse(&bytes).unwrap();
    assert_eq!(parsed.meta.frame_count, 1);
    assert!((parsed.frames[0].width - 4.0).abs() < f32::EPSILON);
}

#[test]
fn tex_decode_argb_and_r8() {
    let mut argb = Vec::new();
    for _ in 0..16 {
        argb.extend_from_slice(&[1, 2, 3, 4]);
    }
    let bytes = build_tex("TEXB0002", 0, 0, &argb, true, None);
    let (w, h, rgba) = tex::decode_rgba(&mut tex::parse(&bytes).unwrap()).unwrap();
    assert_eq!((w, h), (4, 4));
    assert_eq!(&rgba[0..4], &[1, 2, 3, 4]);

    let bytes = build_tex("TEXB0002", 9, 0, &[9u8; 16], true, None);
    let mut parsed = tex::parse(&bytes).unwrap();
    let pixels = tex::take_pixels(&mut parsed).unwrap();
    assert_eq!(pixels.format, tex::PixelFormat::R8);
    assert_eq!(pixels.bytes(), 16);
    assert_eq!(&pixels.base_rgba().unwrap()[0..4], &[9, 0, 0, 255]);
}

fn build_tex_levels(format: i32, levels: &[(i32, i32, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    push_nul_str(&mut out, "TEXV0005");
    push_nul_str(&mut out, "TEXI0001");
    push_i32(&mut out, format);
    push_i32(&mut out, 0);
    for dim in [levels[0].0, levels[0].1, levels[0].0, levels[0].1] {
        push_i32(&mut out, dim);
    }
    push_i32(&mut out, 0);
    push_nul_str(&mut out, "TEXB0002");
    push_i32(&mut out, 1);
    push_i32(&mut out, i32::try_from(levels.len()).unwrap());
    for (width, height, payload) in levels {
        push_i32(&mut out, *width);
        push_i32(&mut out, *height);
        push_i32(&mut out, 0);
        push_i32(&mut out, i32::try_from(payload.len()).unwrap());
        push_i32(&mut out, i32::try_from(payload.len()).unwrap());
        out.extend_from_slice(payload);
    }
    out
}

#[test]
fn tex_native_levels_keep_shipped_chain_and_stop_at_bad_level() {
    let bytes = build_tex_levels(7, &[(8, 4, &[1u8; 16]), (4, 2, &[2u8; 8]), (2, 1, &[3u8; 8])]);
    let pixels = tex::take_pixels(&mut tex::parse(&bytes).unwrap()).unwrap();
    assert_eq!(pixels.format, tex::PixelFormat::Bc1);
    assert!(pixels.format.compressed());
    assert_eq!(pixels.levels.len(), 3);
    assert_eq!((pixels.width(), pixels.height()), (8, 4));
    assert_eq!(pixels.levels[2].data.len(), 8);
    assert_eq!(pixels.bytes(), 32);
    let rgba = pixels.decompressed().unwrap();
    assert_eq!(rgba.format, tex::PixelFormat::Rgba8);
    assert_eq!(rgba.levels.len(), 3);
    assert_eq!(rgba.levels[0].data.len(), 8 * 4 * 4);

    let bytes = build_tex_levels(0, &[(4, 4, &[1u8; 64]), (3, 3, &[2u8; 36]), (1, 1, &[3u8; 4])]);
    let pixels = tex::take_pixels(&mut tex::parse(&bytes).unwrap()).unwrap();
    assert_eq!(pixels.levels.len(), 1);

    let bytes = build_tex_levels(8, &[(2, 2, &[1u8; 8]), (1, 1, &[2u8; 1])]);
    let pixels = tex::take_pixels(&mut tex::parse(&bytes).unwrap()).unwrap();
    assert_eq!(pixels.format, tex::PixelFormat::Rg8);
    assert_eq!(pixels.levels.len(), 1);

    let bytes = build_tex_levels(4, &[(4, 4, &[1u8; 15])]);
    assert!(tex::take_pixels(&mut tex::parse(&bytes).unwrap()).is_none());
}

#[test]
fn scene_feature_extraction() {
    let scene_json = br#"{
        "general": {"cameraparallax": true},
        "objects": [
            {"image": "models/bg.json", "effects": [{"file": "effects/ripple/effect.json"}, {"file": "effects/shared_thing/effect.json"}]},
            {"particle": "particles/snow.json"},
            {"sound": ["audio/a.mp3"]}
        ]
    }"#;
    let model = br#"{"material": "materials/bg.json", "puppet": "models/bg.puppet"}"#;
    let material = br#"{"passes": [{"shader": "genericimage2", "combos": {"BLENDMODE": 1}}]}"#;
    let effect = br#"{"passes": [{"material": "materials/fx.json"}]}"#;
    let fx_material =
        br#"{"passes": [{"shader": "effects/ripple", "combos": {"AUDIOPROCESSING": 1}}]}"#;
    let bytes = build_pkg(&[
        ("scene.json", scene_json),
        ("models/bg.json", model),
        ("materials/bg.json", material),
        ("effects/ripple/effect.json", effect),
        ("materials/fx.json", fx_material),
        ("particles/snow.json", b"{}"),
    ]);
    let package = pkg::Package::parse(bytes).unwrap();
    let features = scene::extract(&package).unwrap();
    assert_eq!(features.objects_image, 1);
    assert_eq!(features.objects_particle, 1);
    assert_eq!(features.objects_sound, 1);
    assert!(features.effects.contains("ripple"));
    assert!(features.shared_effects.contains("shared_thing"));
    assert!(features.shaders.contains("genericimage2"));
    assert!(features.combos.contains("BLENDMODE"));
    assert!(features.particles.contains("snow"));
    assert!(features.puppet);
    assert!(features.puppet_unsupported);
    assert!(features.parallax);
    assert!(features.audio);
    assert_eq!(features.tier(), 3);

    let off = br#"{"general": {"cameraparallax": false, "cameraparallaxamount": 0.5}, "objects": [{"image": "models/bg.json", "parallaxDepth": "2 2"}]}"#;
    let package = pkg::Package::parse(build_pkg(&[
        ("scene.json", off),
        ("models/bg.json", model),
        ("materials/bg.json", material),
    ]))
    .unwrap();
    assert!(
        !scene::extract(&package).unwrap().parallax,
        "the parallax keys are present in every scene; only cameraparallax counts"
    );
}

#[test]
fn features_animated_textures() {
    let texture =
        build_tex("TEXB0003", 0, tex::FLAG_IS_GIF as i32, &[1u8; 64], false, Some("TEXS0003"));
    let bytes = build_pkg(&[
        ("scene.json", br#"{"objects":[{"image":"models/bg.json"}]}"#),
        ("models/bg.json", br#"{"material":"materials/bg.json"}"#),
        ("materials/bg.json", br#"{"passes":[{"textures":["animated"]}]}"#),
        ("materials/animated.tex", &texture),
    ]);
    let package = pkg::Package::parse(bytes).unwrap();
    let features = scene::extract(&package).unwrap();
    assert_eq!(features.tex_gif, 1);
    assert_eq!(features.animated_image_textures, ["animated".into()].into_iter().collect());
}

#[test]
fn features_unknown_objects() {
    let bytes = build_pkg(&[(
        "scene.json",
        br#"{"objects":[{"id":42,"name":"Camera","type":"camera","zoom":1.5}]}"#,
    )]);
    let package = pkg::Package::parse(bytes).unwrap();
    let features = scene::extract(&package).unwrap();
    assert_eq!(features.objects_other, 1);
    assert_eq!(
        features.other_objects,
        [scene::OtherObject {
            kind: "type:camera".into(),
            id: "42".into(),
            name: "Camera".into(),
            properties: vec!["id".into(), "name".into(), "type".into(), "zoom".into()],
        }]
    );
}

#[test]
fn audit_json_shape() {
    let mut totals = audit::Totals { scenes: 1, parsed: 1, ..Default::default() };
    totals.object_gaps.insert(
        "camera|id,type".into(),
        audit::ObjectGap {
            kind: "camera".into(),
            properties: ["id".into(), "type".into()].into_iter().collect(),
            count: 2,
            scenes: ["431960".into()].into_iter().collect(),
            examples: vec![("431960".into(), "42".into(), "Camera".into())],
        },
    );
    let report = audit::render_json(&totals);
    assert_eq!(report["schema"], 1);
    assert_eq!(report["object_shapes"][0]["count"], 2);
    assert_eq!(report["object_shapes"][0]["classification"], "native-candidate");
    assert_eq!(report["object_shapes"][0]["strict_route"], "reject scene startup");
}

#[test]
fn tier_ladder() {
    let plain = scene::SceneFeatures::default();
    assert_eq!(plain.tier(), 0);
    let mut fx = scene::SceneFeatures::default();
    fx.effects.insert("blur".into());
    assert_eq!(fx.tier(), 1);
    let particles = scene::SceneFeatures { objects_particle: 1, ..Default::default() };
    assert_eq!(particles.tier(), 2);
}

#[test]
fn tex_rejects_oversized_mip() {
    let mut bytes = build_tex("TEXB0002", 0, 0, &[1u8; 16], true, None);
    let marker = 16i32.to_le_bytes();
    let at = bytes.windows(4).position(|window| window == marker).unwrap();
    bytes[at..at + 4].copy_from_slice(&i32::MAX.to_le_bytes());
    let err = tex::parse(&bytes).unwrap_err().to_string();
    assert!(err.contains("over cap"), "{err}");
    assert!(tex::parse_meta(&bytes).is_err());
}

#[test]
fn tex_rejects_oversized_dimensions() {
    let mut bytes = build_tex("TEXB0002", 0, 0, &[1_u8; 16], false, None);
    let dimensions = 4_i32.to_le_bytes();
    let positions: Vec<usize> = bytes
        .windows(4)
        .enumerate()
        .filter_map(|(at, raw)| (raw == dimensions).then_some(at))
        .collect();
    let mip_width = positions[4];
    bytes[mip_width..mip_width + 4]
        .copy_from_slice(&(tex::MAX_TEXTURE_EDGE as i32 + 1).to_le_bytes());
    assert!(tex::parse(&bytes).unwrap_err().to_string().contains("invalid mip dimensions"));
}

#[test]
fn json_input_budget() {
    let oversized = vec![b' '; crate::json::MAX_JSON_BYTES + 1];
    let error = crate::json::parse(&oversized).unwrap_err().to_string();
    assert!(error.contains("limit"), "{error}");
}

#[test]
fn tex_gif_missing_frames() {
    let bytes = build_tex("TEXB0002", 0, tex::FLAG_IS_GIF as i32, &[1u8; 16], true, None);
    assert!(tex::parse(&bytes).is_err());
}

#[test]
fn tex_frame_rotation_dimensions() {
    let frame = tex::TexFrame {
        image_id: 0,
        frame_time: 0.1,
        x: 10.0,
        y: 20.0,
        width: 0.0,
        width_y: 100.0,
        height_x: -50.0,
        height: 0.0,
    };
    assert_eq!(frame.size(), (50.0, 100.0));
    assert!(frame.rotated());
}

#[test]
fn json_error_vs_missing() {
    let bytes = build_pkg(&[("scene.json", b"{\"objects\": [")]);
    let package = pkg::Package::parse(bytes).unwrap();
    assert!(package.find("scene.json").is_some());
    assert!(package.find_json("scene.json").is_err());
    let err = scene::extract(&package).unwrap_err();
    assert!(err.contains("parse json"), "{err}");
    assert!(package.find_json("nope.json").unwrap().is_none());
}

#[test]
fn json_bom_tolerated() {
    let mut body = vec![0xef, 0xbb, 0xbf];
    body.extend_from_slice(b"{\"objects\":[]}");
    let bytes = build_pkg(&[("scene.json", &body)]);
    let package = pkg::Package::parse(bytes).unwrap();
    assert!(scene::extract(&package).is_ok());
}

#[test]
fn malformed_tex_counted() {
    let bytes = build_pkg(&[("scene.json", b"{\"objects\":[]}"), ("bad.tex", b"XXXX\0nope")]);
    let package = pkg::Package::parse(bytes).unwrap();
    let features = scene::extract(&package).unwrap();
    assert_eq!(features.tex_failures, 1);
    assert!(features.sample_error.is_some());
}

#[test]
fn reference_budget_bounded() {
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    let objects: Vec<String> = (0..50).map(|_| "{\"image\": \"m.json\"}".to_string()).collect();
    files.push((
        "scene.json".into(),
        format!("{{\"objects\":[{}]}}", objects.join(",")).into_bytes(),
    ));
    files.push(("m.json".into(), br#"{"material": "mat.json"}"#.to_vec()));
    files.push(("mat.json".into(), br#"{"passes":[{"shader":"genericimage2"}]}"#.to_vec()));
    let refs: Vec<(&str, &[u8])> =
        files.iter().map(|(path, data)| (path.as_str(), data.as_slice())).collect();
    let package = pkg::Package::parse(build_pkg(&refs)).unwrap();
    let features = scene::extract(&package).unwrap();
    assert_eq!(features.objects_image, 50);
    assert!(features.shaders.contains("genericimage2"));
    assert!(!features.refs_truncated);
}

#[test]
fn lenient_json_dialect() {
    let value = crate::json::parse(br#"{"a": [1, 2,], "b": {"c": 3,},}"#).unwrap();
    assert_eq!(value["a"].as_array().unwrap().len(), 2);
    assert_eq!(value["b"]["c"], 3);
    let with_comment = crate::json::parse(b"{\n// note\n\"x\": 1 /* inline */ }").unwrap();
    assert_eq!(with_comment["x"], 1);
    let comma_in_string = crate::json::parse(br#"{"t": "a,]"}"#).unwrap();
    assert_eq!(comma_in_string["t"], "a,]");
    assert!(crate::json::parse(b"{not json").is_err());
}

#[test]
fn shader_translation_shape() {
    use crate::shader::{Stage, UniformKind, translate};
    use std::collections::BTreeMap;
    let src = r#"
varying vec4 v_TexCoord;
varying vec2 v_Fan[4];
attribute vec3 a_Position;
uniform sampler2D g_Texture0;
uniform mat4 g_ModelViewProjectionMatrix;
uniform float g_Speed; // {"default":5,"range":[0.01,50]}
uniform vec3 g_Tint; // {"default":"1 0.5 0"}
void main() {
    vec4 sample = texSample2D(g_Texture0, v_TexCoord.xy);
    gl_FragColor = vec4(max(0, sample.rgb) * g_Tint, 1.0);
}
"#;
    let mut combos = BTreeMap::new();
    combos.insert("BLENDMODE".to_string(), 1i64);
    let out = translate(src, Stage::Fragment, &combos);
    assert!(out.source.starts_with("#version 450"));
    assert!(out.source.contains("#define BLENDMODE 1"));
    assert!(out.source.contains("layout(location = 0) in vec4 skwd_io_v_TexCoord;"));
    assert!(out.source.contains("\nvec4 v_TexCoord;\n"));
    assert!(out.source.contains("layout(location = 1) in vec4 skwd_io_v_Fan[2];"));
    assert!(out.source.contains("\nvec2 v_Fan[4];\n"));
    assert!(out.source.contains("    v_Fan[3] = skwd_io_v_Fan[1].zw;\n"), "{}", out.source);
    assert!(out.source.contains("void skwd_main()"));
    assert!(
        out.source
            .contains("    v_TexCoord = skwd_io_v_TexCoord;\n    v_Fan[0] = skwd_io_v_Fan[0].xy;")
    );
    assert!(out.source.contains("    v_Fan[3] = skwd_io_v_Fan[1].zw;\n    skwd_main();"));
    assert!(
        out.source.contains("layout(location = 5) in vec3 a_Position;")
            || out.source.contains("in vec3 a_Position;")
    );
    assert!(out.source.contains("layout(set = 0, binding = 1) uniform sampler2D g_Texture0;"));
    assert!(out.source.contains("uniform SkwdParams"));
    assert!(out.source.contains("skwd_sample"));
    assert!(out.source.contains("max( skwd_sample.rgb,0.0)"), "{}", out.source);
    assert!(!out.source.contains("varying "));
    assert_eq!(out.uniforms.len(), 3);
    assert_eq!(out.uniforms[0].kind, UniformKind::Mat4);
    assert_eq!(out.uniforms[1].offset, 64);
    assert_eq!(out.uniforms[1].default, Some(vec![5.0]));
    assert_eq!(out.uniforms[2].default, Some(vec![1.0, 0.5, 0.0]));
    assert_eq!(out.samplers.len(), 1);
}

#[test]
fn undefined_combos_zero() {
    use crate::shader::{Stage, translate};
    let src = "#if BLENDMODE == 2\nvoid a(){}\n#endif\n#ifdef OTHER\nvoid b(){}\n#endif\n";
    let out = translate(src, Stage::Fragment, &std::collections::BTreeMap::new());
    assert!(out.source.contains("#define BLENDMODE 0"));
    assert!(!out.source.contains("#define OTHER"));
}

#[test]
fn uniform_std140_offsets() {
    use crate::shader::{Stage, pack_uniforms, translate};
    let src = "uniform float g_A; // {\"default\":2}\nuniform vec4 g_B;\nvoid main(){}\n";
    let out = translate(src, Stage::Fragment, &std::collections::BTreeMap::new());
    assert_eq!(out.uniforms[1].offset, 16);
    let mut values = std::collections::BTreeMap::new();
    values.insert("g_B".to_string(), vec![1.0, 0.0, 0.0, 1.0]);
    let packed = pack_uniforms(&out.uniforms, &values);
    assert_eq!(packed.len(), 32);
    assert_eq!(f32::from_le_bytes(packed[0..4].try_into().unwrap()), 2.0);
    assert_eq!(f32::from_le_bytes(packed[16..20].try_into().unwrap()), 1.0);
}

#[test]
fn includes_resolve_and_skip_provided() {
    use crate::shader::resolve_includes;
    let load = |name: &str| (name == "extra.h").then(|| "float helper(){return 1.0;}".to_string());
    let out =
        resolve_includes("#include \"common.h\"\n#include \"extra.h\"\nvoid main(){}", &load, 0)
            .unwrap();
    assert!(out.contains("#define M_PI 3.14159265359"));
    assert!(out.contains("vec2 rotateVec2(vec2 v, float r)"));
    assert!(out.contains("float helper()"));
    let real = |name: &str| (name == "common.h").then(|| "#define M_PI 3.0\n".to_string());
    let real_out = resolve_includes("#include \"common.h\"\nvoid main(){}", &real, 0).unwrap();
    assert!(real_out.contains("#define M_PI 3.0"));
    assert!(real_out.contains("vec2 rotateVec2(vec4 v, float r)"));
    assert!(!real_out.contains("vec2 rotateVec2(vec2 v, float r)"));
    let missing = resolve_includes("#include \"gone.h\"", &load, 0).unwrap();
    assert!(missing.contains("missing include gone.h"));
}

#[test]
fn user_bound_baked_fallback() {
    let scene = br#"{"general":{"orthogonalprojection":{"width":1000,"height":1000}},"objects":[{"id":1,"name":"L","image":"models/bg.json","size":"100.000 50.000","origin":{"user":"p","value":"10.000 20.000 0.000"},"scale":{"user":"q","value":"2.000 3.000 1.000"},"angles":{"user":"r","value":"0.000 0.000 1.5708"},"color":{"user":"c","value":"0.500 0.250 0.125"}}]}"#;
    let bytes = build_pkg(&[
        ("scene.json", scene),
        ("models/bg.json", br#"{"material":"materials/bg.json"}"#),
        ("materials/bg.json", br#"{"passes":[{"shader":"flat"}]}"#),
    ]);
    let package = pkg::Package::parse(bytes).unwrap();
    let scene = model::load(&package).unwrap();
    let layer = scene.layers.first().expect("layer built");
    assert_eq!(layer.center, (10.0, 980.0));
    assert_eq!(layer.size, (200.0, 150.0));
    assert!((layer.angle + std::f32::consts::FRAC_PI_2).abs() < 1e-5);
    assert_eq!(layer.color, [0.5, 0.25, 0.125]);
}

#[test]
fn property_overrides_replace_baked() {
    let scene = br#"{"general":{"orthogonalprojection":{"width":1000,"height":1000}},"objects":[{"id":1,"name":"L","image":"models/bg.json","size":"100.000 50.000","origin":{"user":"place","value":"10.000 20.000 0.000"},"color":{"user":"tint","value":"1.000 1.000 1.000"},"alpha":{"user":"fade","value":1.0},"visible":{"user":"shown","value":true}}]}"#;
    let build = |overrides: model::Properties| {
        let bytes = build_pkg(&[
            ("scene.json", scene),
            ("models/bg.json", br#"{"material":"materials/bg.json"}"#),
            ("materials/bg.json", br#"{"passes":[{"shader":"flat"}]}"#),
        ]);
        let package = pkg::Package::parse(bytes).unwrap();
        let assets = effects::Assets::discover(Some("")).with_overrides(overrides);
        model::load_with(&package, &assets).unwrap()
    };

    let baked = build(model::Properties::new());
    let layer = baked.layers.first().expect("layer built");
    assert_eq!(layer.center, (10.0, 980.0));
    assert_eq!(layer.color, [1.0, 1.0, 1.0]);
    assert_eq!(layer.alpha, 1.0);

    let overrides = effects::parse_property_overrides(
        serde_json::json!({"Place": "40 60 0", "tint": "0.25 0.5 0.75", "fade": 0.5})
            .as_object()
            .unwrap(),
    );
    let tuned = build(overrides);
    let layer = tuned.layers.first().expect("layer built");
    assert_eq!(layer.center, (40.0, 940.0));
    assert_eq!(layer.color, [0.25, 0.5, 0.75]);
    assert_eq!(layer.alpha, 0.5);

    let hidden = build(effects::parse_property_overrides(
        serde_json::json!({"shown": false}).as_object().unwrap(),
    ));
    assert!(hidden.layers.is_empty());
}

#[test]
fn hidden_image_referenced_as_render_target_remains_available_but_transparent() {
    let scene = br#"{
        "objects": [
            {"id": 7, "name": "mask", "image": "models/bg.json", "visible": false},
            {"id": 8, "name": "consumer", "image": "models/bg.json",
             "target": "_rt_imageLayerComposite_7_a"},
            {"id": 9, "name": "unused", "image": "models/bg.json", "visible": false,
             "target": "_rt_imageLayerComposite_9_b"}
        ]
    }"#;
    let bytes = build_pkg(&[
        ("scene.json", scene),
        ("models/bg.json", br#"{"material":"materials/bg.json"}"#),
        ("materials/bg.json", br#"{"passes":[{"shader":"flat"}]}"#),
    ]);
    let package = pkg::Package::parse(bytes).unwrap();
    let scene = model::load(&package).unwrap();

    assert_eq!(scene.layers.iter().map(|layer| layer.id.as_str()).collect::<Vec<_>>(), ["7", "8"]);
    assert!(!scene.layers[0].visible);
    assert_eq!(scene.layers[0].alpha, 1.0);
    assert!(scene.layers[1].visible);
    assert_eq!(scene.layers[1].alpha, 1.0);
}

#[test]
fn image_and_particle_layers_draw_in_authored_order_regardless_of_depth() {
    let scene = br#"{
        "objects": [
            {"id": 1, "image": "models/bg.json", "origin": "0 0 5"},
            {"id": 2, "particle": "particles/dot.json", "origin": "0 0 1"},
            {"id": 3, "image": "models/bg.json", "origin": "0 0 -2"}
        ]
    }"#;
    let particle = br#"{
        "renderer": [{"name": "sprite"}],
        "emitter": [{"name": "boxrandom", "rate": 1}]
    }"#;
    let bytes = build_pkg(&[
        ("scene.json", scene),
        ("models/bg.json", br#"{"material":"materials/bg.json"}"#),
        ("materials/bg.json", br#"{"passes":[{"shader":"flat"}]}"#),
        ("particles/dot.json", particle),
    ]);
    let package = pkg::Package::parse(bytes).unwrap();
    let scene = model::load(&package).unwrap();

    assert_eq!(scene.layers.iter().map(|layer| layer.scene_order).collect::<Vec<_>>(), [0, 2]);
    assert_eq!(scene.particles[0].scene_order, 1);
}

#[test]
fn scalar_property_broadcast() {
    let scene = br#"{"general":{"orthogonalprojection":{"width":1000,"height":1000}},"objects":[{"id":1,"name":"L","image":"models/bg.json","size":"100.000 50.000","scale":{"user":"zoom","value":"1.000 1.000 1.000"}}]}"#;
    let build = |overrides: model::Properties| {
        let bytes = build_pkg(&[
            ("scene.json", scene),
            ("models/bg.json", br#"{"material":"materials/bg.json"}"#),
            ("materials/bg.json", br#"{"passes":[{"shader":"flat"}]}"#),
        ]);
        let package = pkg::Package::parse(bytes).unwrap();
        let assets = effects::Assets::discover(Some("")).with_overrides(overrides);
        model::load_with(&package, &assets).unwrap()
    };

    let scaled = build(effects::parse_property_overrides(
        serde_json::json!({"zoom": 2.5}).as_object().unwrap(),
    ));
    assert_eq!(scaled.layers.first().expect("layer built").size, (250.0, 125.0));
    assert_eq!(scaled.layers.first().expect("layer built").scale, (2.5, 2.5));

    let explicit = build(effects::parse_property_overrides(
        serde_json::json!({"zoom": "3 4 1"}).as_object().unwrap(),
    ));
    assert_eq!(explicit.layers.first().expect("layer built").size, (300.0, 200.0));
}

#[test]
fn passthrough_util_layers_sample_the_scene_and_need_effects() {
    let scene = br#"{
        "general": {"orthogonalprojection": {"width": 800, "height": 600}},
        "objects": [
            {"id": 1, "name": "bg", "image": "models/bg.json"},
            {"id": 2, "name": "compose", "image": "models/util/composelayer.json",
             "origin": "100 200 0", "size": "300 100",
             "effects": [{"file": "effects/e.json"}]},
            {"id": 3, "name": "idle", "image": "models/util/composelayer.json", "size": "10 10"},
            {"id": 4, "name": "full", "image": "models/util/fullscreenlayer.json",
             "origin": "5 5 0", "size": "10 10", "angles": "0 0 45",
             "effects": [{"file": "effects/e.json"}]},
            {"id": 5, "name": "broken", "image": "models/util/composelayer.json",
             "effects": [{"file": "effects/missing.json"}]}
        ]
    }"#;
    let bytes = build_pkg(&[
        ("scene.json", scene),
        ("models/bg.json", br#"{"material":"materials/bg.json"}"#),
        ("materials/bg.json", br#"{"passes":[{"shader":"flat"}]}"#),
        (
            "models/util/composelayer.json",
            br#"{"material":"materials/util/composelayer.json","passthrough":true}"#,
        ),
        (
            "models/util/fullscreenlayer.json",
            br#"{"material":"materials/util/fullscreenlayer.json","fullscreen":true,"passthrough":true}"#,
        ),
        (
            "materials/util/composelayer.json",
            br#"{"passes":[{"shader":"composelayer","textures":["_rt_FullFrameBuffer"]}]}"#,
        ),
        (
            "materials/util/fullscreenlayer.json",
            br#"{"passes":[{"shader":"passthrough","textures":["_rt_FullFrameBuffer"]}]}"#,
        ),
        ("effects/e.json", br#"{"passes":[{"material":"materials/e.json"}]}"#),
        ("materials/e.json", br#"{"passes":[{"shader":"e"}]}"#),
        ("shaders/e.vert", b"attribute vec3 a_Position;\nvoid main() { gl_Position = vec4(a_Position, 1.0); }\n"),
        ("shaders/e.frag", b"uniform sampler2D g_Texture0;\nvoid main() { gl_FragColor = vec4(1.0); }\n"),
    ]);
    let package = pkg::Package::parse(bytes).unwrap();
    let scene =
        model::load_with(&package, &effects::Assets::discover(Some("/nonexistent"))).unwrap();

    let ids: Vec<&str> = scene.layers.iter().map(|layer| layer.id.as_str()).collect();
    assert_eq!(ids, ["1", "2", "4"]);
    assert!(scene.skipped.is_empty(), "{:?}", scene.skipped);
    let compose = &scene.layers[1];
    assert!(compose.passthrough);
    assert_eq!(compose.size, (300.0, 100.0));
    assert_eq!(compose.center, (100.0, 400.0));
    assert_eq!(compose.effects.len(), 1);
    let full = &scene.layers[2];
    assert!(full.passthrough);
    assert_eq!(full.size, (800.0, 600.0));
    assert_eq!(full.center, (400.0, 300.0));
    assert_eq!(full.angle, 0.0);
    assert!(!scene.layers[0].passthrough);
}

#[test]
fn copy_command_passes_become_named_target_copies() {
    let scene = br#"{"objects":[{"id":1,"name":"L","image":"models/bg.json",
        "effects":[{"file":"effects/mb.json"}]}]}"#;
    let effect = br#"{"fbos":[{"name":"_rt_A"},{"name":"_rt_B"}],"passes":[
        {"material":"materials/e.json","target":"_rt_B","bind":[{"name":"_rt_A","index":1}]},
        {"command":"copy","source":"_rt_B","target":"_rt_A"},
        {"material":"materials/e.json","bind":[{"name":"_rt_B","index":0}]}]}"#;
    let bytes = build_pkg(&[
        ("scene.json", scene),
        ("models/bg.json", br#"{"material":"materials/bg.json"}"#),
        ("materials/bg.json", br#"{"passes":[{"shader":"flat"}]}"#),
        ("effects/mb.json", effect),
        ("materials/e.json", br#"{"passes":[{"shader":"e"}]}"#),
        ("shaders/e.vert", b"attribute vec3 a_Position;\nvoid main() { gl_Position = vec4(a_Position, 1.0); }\n"),
        ("shaders/e.frag", b"uniform sampler2D g_Texture0;\nuniform sampler2D g_Texture1;\nvoid main() { gl_FragColor = vec4(1.0); }\n"),
        ("shaders/passthrough.vert", b"attribute vec3 a_Position;\nattribute vec2 a_TexCoord;\nvarying vec2 v_TexCoord;\nvoid main() { gl_Position = vec4(a_Position, 1.0); v_TexCoord = a_TexCoord; }\n"),
        ("shaders/passthrough.frag", b"varying vec2 v_TexCoord;\nuniform sampler2D g_Texture0;\nvoid main() { gl_FragColor = texSample2D(g_Texture0, v_TexCoord); }\n"),
    ]);
    let package = pkg::Package::parse(bytes).unwrap();
    let scene =
        model::load_with(&package, &effects::Assets::discover(Some("/nonexistent"))).unwrap();

    let passes = &scene.layers[0].effects[0].passes;
    let names: Vec<&str> = passes.iter().map(|pass| pass.name.as_str()).collect();
    assert_eq!(names, ["e", "copy", "e"]);
    assert_eq!(passes[1].target.as_deref(), Some("_rt_A"));
    assert_eq!(passes[1].binds, [(0, effects::EffectBind::Named("_rt_B".into()))]);
    assert_eq!(passes[0].target.as_deref(), Some("_rt_B"));
}

#[test]
fn color_blend_modes_append_a_scene_blend_pass() {
    let scene = br#"{"objects":[
        {"id":1,"name":"overlay","image":"models/bg.json","colorBlendMode":9,
         "color":"1 0.5 0.25","alpha":0.5},
        {"id":2,"name":"screen","image":"models/bg.json","colorBlendMode":6},
        {"id":3,"name":"plain","image":"models/bg.json"}]}"#;
    let bytes = build_pkg(&[
        ("scene.json", scene),
        ("models/bg.json", br#"{"material":"materials/bg.json"}"#),
        ("materials/bg.json", br#"{"passes":[{"shader":"flat"}]}"#),
        (
            "shaders/common_blending.h",
            b"vec3 ApplyBlending(const int mode, in vec3 A, in vec3 B, in float opacity) { return mix(A, B, opacity); }\n",
        ),
    ]);
    let package = pkg::Package::parse(bytes).unwrap();
    let scene =
        model::load_with(&package, &effects::Assets::discover(Some("/nonexistent"))).unwrap();

    assert!(scene.skipped.is_empty(), "{:?}", scene.skipped);
    let overlay = &scene.layers[0];
    assert_eq!(overlay.color_blend, 9);
    let blend = overlay.effects.last().unwrap();
    assert_eq!(blend.name, effects::COLOR_BLEND_EFFECT);
    let pass = &blend.passes[0];
    assert_eq!(pass.name, effects::COLOR_BLEND_SHADER);
    assert_eq!(pass.binds, [(4, effects::EffectBind::SceneUnderLayer)]);
    assert_eq!(pass.values["g_Color4"], [1.0, 1.0, 1.0, 1.0]);
    assert_eq!(overlay.color, [1.0, 0.5, 0.25]);
    assert!((overlay.alpha - 0.5).abs() < 1e-6);
    assert!(pass.fragment.source.contains("#define BLENDMODE 9"));
    assert!(pass.fragment.samplers.iter().any(|sampler| sampler.index == 4));
    assert!(scene.layers[1].effects.is_empty());
    assert!(scene.layers[2].effects.is_empty());
    assert!(!effects::shader_blend(0) && !effects::shader_blend(6) && !effects::shader_blend(8));
}

#[test]
fn varyings_keep_each_stages_width_behind_a_writable_shadow() {
    use crate::shader::{Stage, translate_with, varying_map};
    use std::collections::BTreeMap;
    let vert = "varying vec2 v_TexCoord;\n#if MODE == 0\nvarying vec2 v_Bounds;\n#else\nvarying float v_Fade;\n#endif\nattribute vec3 a_Position;\nvoid main() {\n\tv_TexCoord = a_Position.xy;\n\tgl_Position = vec4(a_Position, 1.0);\n}\n";
    let frag = "varying vec4 v_TexCoord;\n#if MODE == 0\nvarying vec2 v_Bounds;\n#endif\nvoid main() {\n\tv_TexCoord.y += 0.5;\n\tgl_FragColor = v_TexCoord;\n}\n";
    let map = varying_map(vert, frag, &BTreeMap::default());
    assert_eq!(map["v_TexCoord"].width, 4);
    assert_eq!(map["v_Bounds"].width, 2);
    let combos = BTreeMap::from([("MODE".to_string(), 0i64)]);
    let vertex = translate_with(vert, Stage::Vertex, &combos, Some(&map)).source;
    let fragment = translate_with(frag, Stage::Fragment, &combos, Some(&map)).source;

    assert!(vertex.contains("layout(location = 0) out vec4 skwd_io_v_TexCoord;"), "{vertex}");
    assert!(vertex.contains("\nvec2 v_TexCoord;\n"));
    assert!(vertex.contains("skwd_io_v_TexCoord = vec4(v_TexCoord, 0.0, 1.0);"));
    assert!(vertex.contains("#if MODE == 0\n    skwd_io_v_Bounds = v_Bounds;\n#endif"), "{vertex}");
    assert!(
        vertex.contains("#if MODE == 0\n#else\n    skwd_io_v_Fade = v_Fade;\n#endif"),
        "{vertex}"
    );
    assert!(vertex.contains("void skwd_main()"));
    assert!(vertex.ends_with("}\n"));
    assert!(fragment.contains("layout(location = 0) in vec4 skwd_io_v_TexCoord;"));
    assert!(fragment.contains("\nvec4 v_TexCoord;\n"));
    assert!(fragment.contains("    v_TexCoord = skwd_io_v_TexCoord;\n"), "{fragment}");
    assert!(fragment.contains("#if MODE == 0\n    v_Bounds = skwd_io_v_Bounds;\n#endif"));
    assert!(fragment.contains("\tv_TexCoord.y += 0.5;"));
}

#[test]
fn vec2_and_float_varying_arrays_pack_into_vec4_locations_behind_a_local_array() {
    use crate::shader::{Stage, compile, translate_with, varying_map};
    use std::collections::BTreeMap;
    let vert = "varying vec2 v_Kernel[49];\nvarying float v_Weight[6];\nvarying vec2 v_TexCoord;\nattribute vec3 a_Position;\nvoid main() {\n\tfor (int i = 0; i < 49; i++) v_Kernel[i] = a_Position.xy * float(i);\n\tfor (int i = 0; i < 6; i++) v_Weight[i] = float(i);\n\tv_TexCoord = a_Position.xy;\n\tgl_Position = vec4(a_Position, 1.0);\n}\n";
    let frag = "varying vec2 v_Kernel[49];\nvarying float v_Weight[6];\nvarying vec2 v_TexCoord;\nvoid main() {\n\tvec2 sum = v_TexCoord;\n\tfor (int i = 0; i < 49; i++) sum += v_Kernel[i] * v_Weight[i % 6];\n\tgl_FragColor = vec4(sum, 0.0, 1.0);\n}\n";
    let map = varying_map(vert, frag, &BTreeMap::default());
    assert_eq!(map["v_Kernel"].location, 0);
    assert_eq!(map["v_Weight"].location, 25);
    assert_eq!(map["v_TexCoord"].location, 27);
    let combos = BTreeMap::new();
    let vertex = translate_with(vert, Stage::Vertex, &combos, Some(&map)).source;
    let fragment = translate_with(frag, Stage::Fragment, &combos, Some(&map)).source;
    assert!(vertex.contains("layout(location = 0) out vec4 skwd_io_v_Kernel[25];"), "{vertex}");
    assert!(vertex.contains("layout(location = 25) out vec4 skwd_io_v_Weight[2];"), "{vertex}");
    assert!(vertex.contains("\nvec2 v_Kernel[49];\n"));
    assert!(vertex.contains("    skwd_io_v_Kernel[24].xy = v_Kernel[48];\n"), "{vertex}");
    assert!(vertex.contains("    skwd_io_v_Weight[1].y = v_Weight[5];\n"), "{vertex}");
    assert!(fragment.contains("layout(location = 0) in vec4 skwd_io_v_Kernel[25];"), "{fragment}");
    assert!(fragment.contains("    v_Kernel[1] = skwd_io_v_Kernel[0].zw;\n"), "{fragment}");
    assert!(fragment.contains("    v_Weight[4] = skwd_io_v_Weight[1].x;\n"), "{fragment}");
    assert!(compile(&vertex, Stage::Vertex, "pack.vert").is_ok(), "{vertex}");
    assert!(compile(&fragment, Stage::Fragment, "pack.frag").is_ok(), "{fragment}");
    let hlsl = "varying vec2 v_Kernel[49];\nvarying vec2 v_TexCoord;\nvoid main() {}\n";
    assert_eq!(crate::hlsl::varying_slots(hlsl), 50);
    assert!(crate::hlsl::varying_slots(hlsl) > crate::hlsl::MAX_VARYING_SLOTS);
    assert_eq!(
        crate::hlsl::varying_slots("varying float v_W[8];\nvarying vec4 v_T;\nvoid main() {}\n"),
        3
    );

    let macro_vert = "varying float v_Bins[BINS];\nvarying vec2 v_TexCoord;\nattribute vec3 a_Position;\nvoid main() {\n\tfor (int i = 0; i < BINS; i++) v_Bins[i] = float(i);\n\tv_TexCoord = a_Position.xy;\n\tgl_Position = vec4(a_Position, 1.0);\n}\n";
    let macro_frag = "varying float v_Bins[BINS];\nvarying vec2 v_TexCoord;\nvoid main() {\n\tfloat sum = 0.0;\n\tfor (int i = 0; i < BINS; i++) sum += v_Bins[i];\n\tgl_FragColor = vec4(sum, v_TexCoord, 1.0);\n}\n";
    let sized = BTreeMap::from([("BINS".to_string(), 32i64)]);
    let map = varying_map(macro_vert, macro_frag, &sized);
    assert_eq!(map["v_Bins"].location, 0);
    assert_eq!(map["v_TexCoord"].location, 8, "a macro-sized array must claim its real slots");
    let vertex = translate_with(macro_vert, Stage::Vertex, &sized, Some(&map)).source;
    assert!(vertex.contains("layout(location = 0) out vec4 skwd_io_v_Bins[8];"), "{vertex}");
    assert!(vertex.contains("\nfloat v_Bins[32];\n"), "{vertex}");
    assert!(compile(&vertex, Stage::Vertex, "bins.vert").is_ok(), "{vertex}");
}

#[test]
fn portability_relaxation_closes_fallthrough_functions_and_macros_uniform_backed_globals() {
    use crate::shader::{Stage, compile, relax_portability, translate};
    use std::collections::BTreeMap;
    let src = "uniform float u_Balance;\n#define balance u_Balance\nconst vec2 span = vec2(balance, 1.0 - balance);\nconst float k = 2.0;\nvec3 Mask(vec2 uv) {\n#if MODE == 4\n\treturn vec3(uv, 0.0);\n#endif\n}\nvoid main() { gl_FragColor = vec4(Mask(span) * k, 1.0); }\n";
    let relaxed = relax_portability(src);
    assert!(relaxed.contains("#define span (vec2(balance, 1.0 - balance))"), "{relaxed}");
    assert!(!relaxed.contains("const vec2 span"));
    assert!(relaxed.contains("const float k = 2.0;"), "literal globals stay const: {relaxed}");
    assert!(
        relaxed.contains("return vec3(0.0, 0.0, 0.0);\n}"),
        "splats are invalid HLSL: {relaxed}"
    );
    assert!(
        relaxed.contains("void main() { gl_FragColor"),
        "void and main are left alone: {relaxed}"
    );
    let out = translate(&relaxed, Stage::Fragment, &BTreeMap::new()).source;
    assert!(compile(&out, Stage::Fragment, "relax.frag").is_ok(), "{out}");
    assert!(
        compile(
            &translate(src, Stage::Fragment, &BTreeMap::new()).source,
            Stage::Fragment,
            "raw.frag"
        )
        .is_err()
    );
}

#[test]
fn prelude_macros_yield_to_shader_redefinitions() {
    use crate::shader::{Stage, translate};
    use std::collections::BTreeMap;
    let src = "#define M_PI 3.14159265359\n#define M_PI 3.1415926535897932384626433832795\n#define CAST2(x) (x)\nvoid main() { gl_FragColor = vec4(M_PI); }\n";
    let out = translate(src, Stage::Fragment, &BTreeMap::new()).source;
    assert!(out.contains("#define M_PI 3.14159265359\n#undef M_PI\n#define M_PI 3.1415926535897932384626433832795"), "{out}");
    assert!(out.contains("#undef CAST2\n#define CAST2(x) (x)"), "{out}");
}

#[test]
fn conditional_symbol_defaults_ignore_trailing_comments() {
    use crate::shader::{Stage, translate};
    use std::collections::BTreeMap;
    let src = "#if RESOLUTION != 64 //Resolution 64 cannot be passed in the frag\nvarying vec2 v_A;\n#endif\nattribute vec3 a_Position;\nvoid main() { vec3 color = a_Position; gl_Position = vec4(color, 1.0); }\n";
    let out = translate(src, Stage::Vertex, &BTreeMap::new()).source;
    assert!(out.contains("#define RESOLUTION 0"), "{out}");
    assert!(!out.contains("#define in 0"), "{out}");
    assert!(!out.contains("#define Resolution 0"), "{out}");
}

#[test]
fn atlas_frames_require_one_unrotated_image_with_timing() {
    use crate::model::{SpriteFrame, Texture};
    let frame = |image, rotated, time| SpriteFrame { uv: [0.0; 4], rotated, time, image };
    let mut texture = model::solid_texture();
    assert!(texture.atlas_frames().is_none());
    texture.frames = vec![frame(0, false, 0.1), frame(0, false, 0.1)];
    assert_eq!(texture.atlas_frames().map(<[SpriteFrame]>::len), Some(2));
    texture.frames = vec![frame(0, false, 0.1), frame(1, false, 0.1)];
    assert!(texture.atlas_frames().is_none());
    texture.frames = vec![frame(0, true, 0.1), frame(0, false, 0.1)];
    assert!(texture.atlas_frames().is_none());
    texture.frames = vec![frame(0, false, 0.0), frame(0, false, 0.0)];
    assert!(texture.atlas_frames().is_none());
    let _: &Texture = &texture;
}

#[test]
fn preprocessor_follows_wallpaper_engine_leniency() {
    use crate::shader::preprocess;
    use std::collections::BTreeMap;
    let combos = BTreeMap::from([("MODE".to_string(), 2i64), ("AUDIO".to_string(), 1i64)]);
    let src = "#define LOCAL 3\n#if MODE == 2 && defined(AUDIO) //comment\nA\n#elif MODE == 32;\nB\n#else\nC\n#endif\n#if UNKNOWN\nD\n#endif\n#ifdef LOCAL\nE\n#endif\n#ifndef LOCAL\nF\n#endif\n#if LOCAL > 2\nG\n#else\nH\n#endif\n#if MODE == 3\nI\n#elif MODE == 2;\nJ\n#else\nK\n#endif\n#endif\nL\n#if !AUDIO || (MODE == 1)\nM\n#endif\n";
    let out = preprocess(src, &combos);
    let lines: Vec<&str> = out.lines().filter(|l| l.len() == 1).collect();
    assert_eq!(lines, ["A", "E", "G", "J", "L"], "{out}");
    assert!(out.contains("#define LOCAL 3"));
}

#[test]
fn fragments_may_use_any_vertex_varying() {
    use crate::shader::{Stage, translate_with, varying_map};
    use std::collections::BTreeMap;
    let vert = "varying vec2 v_Bounds;\nattribute vec3 a_Position;\nvoid main() { v_Bounds = a_Position.xy; gl_Position = vec4(a_Position, 1.0); }\n";
    let frag = "void main() { gl_FragColor = vec4(v_Bounds, 0.0, 1.0); }\n";
    let map = varying_map(vert, frag, &BTreeMap::default());
    let fragment = translate_with(frag, Stage::Fragment, &BTreeMap::new(), Some(&map)).source;
    assert!(fragment.contains("layout(location = 0) in vec2 skwd_io_v_Bounds;"), "{fragment}");
    assert!(fragment.contains("\nvec2 v_Bounds;\n"));
    assert!(fragment.contains("    v_Bounds = skwd_io_v_Bounds;\n    skwd_main();"), "{fragment}");
}

#[test]
fn reserved_word_renaming_keeps_multibyte_text_intact() {
    use crate::shader::{Stage, translate};
    use std::collections::BTreeMap;
    let src = "// 注释 sample 注释\nvoid main() { float sample = max(0, 1.0); gl_FragColor = vec4(sample); }\n";
    let out = translate(src, Stage::Fragment, &BTreeMap::new()).source;
    assert!(out.contains("// 注释 skwd_sample 注释"), "{out}");
    assert!(out.contains("float skwd_sample = max(0.0, 1.0)"), "{out}");
}

#[test]
fn object_angles_are_radians() {
    let scene = br#"{"general":{"orthogonalprojection":{"width":1000,"height":1000}},"objects":[
        {"id":1,"name":"parent","image":"models/bg.json","origin":"500 500 0","angles":"0 0 1.5707964"},
        {"id":2,"name":"child","image":"models/bg.json","parent":1,"origin":"100 0 0","angles":"0 0 0.5"}]}"#;
    let bytes = build_pkg(&[
        ("scene.json", scene),
        ("models/bg.json", br#"{"material":"materials/bg.json"}"#),
        ("materials/bg.json", br#"{"passes":[{"shader":"flat"}]}"#),
    ]);
    let package = pkg::Package::parse(bytes).unwrap();
    let scene = model::load(&package).unwrap();
    assert!((scene.layers[0].angle + 1.5707964).abs() < 1e-6, "{}", scene.layers[0].angle);
    assert!((scene.layers[1].angle + 2.0707964).abs() < 1e-6, "{}", scene.layers[1].angle);
    let child = scene.layers[1].center;
    assert!((child.0 - 500.0).abs() < 1e-3, "{child:?}");
    assert!((child.1 - 400.0).abs() < 1e-3, "{child:?}");
}

#[test]
fn solid_layers_are_flagged_and_combo_keys_are_upper_cased() {
    let scene = br#"{"objects":[
        {"id":1,"name":"solid","image":"models/solid.json","size":"10 10"},
        {"id":2,"name":"img","image":"models/bg.json","effects":[{"file":"effects/e.json"}]}]}"#;
    let bytes = build_pkg(&[
        ("scene.json", scene),
        ("models/solid.json", br#"{"material":"materials/solid.json"}"#),
        ("materials/solid.json", br#"{"passes":[{"shader":"flat"}]}"#),
        ("models/bg.json", br#"{"material":"materials/bg.json"}"#),
        ("materials/bg.json", br#"{"passes":[{"shader":"flat"}]}"#),
        ("effects/e.json", br#"{"passes":[{"material":"materials/e.json"}]}"#),
        ("materials/e.json", br#"{"passes":[{"shader":"e","combos":{"version":2,"Mode":1}}]}"#),
        ("shaders/e.vert", b"attribute vec3 a_Position;\nvoid main() { gl_Position = vec4(a_Position, 1.0); }\n"),
        ("shaders/e.frag", b"uniform sampler2D g_Texture0;\nvoid main() { gl_FragColor = vec4(float(VERSION), float(MODE), 0.0, 1.0); }\n"),
    ]);
    let package = pkg::Package::parse(bytes).unwrap();
    let scene =
        model::load_with(&package, &effects::Assets::discover(Some("/nonexistent"))).unwrap();
    assert!(scene.layers[0].solid);
    assert!(scene.layers[1].solid);
    assert!(scene.layers[0].effects.is_empty());
    assert_eq!(scene.layers[1].effects.len(), 1);
    let source = &scene.layers[1].effects[0].passes[0].fragment.source;
    assert!(source.contains("#define VERSION 2"), "{source}");
    assert!(source.contains("#define MODE 1"), "{source}");
}

#[test]
fn slot_textures_define_their_engine_format_combo() {
    let scene = br#"{"objects":[{"id":1,"name":"L","image":"models/bg.json",
        "effects":[{"file":"effects/n.json"}]}]}"#;
    let normal = build_tex("TEXB0002", 4, 0, &[7u8; 16], false, None);
    let bytes = build_pkg(&[
        ("scene.json", scene),
        ("models/bg.json", br#"{"material":"materials/bg.json"}"#),
        ("materials/bg.json", br#"{"passes":[{"shader":"flat"}]}"#),
        ("effects/n.json", br#"{"passes":[{"material":"materials/e.json"}]}"#),
        ("materials/e.json", br#"{"passes":[{"shader":"e","textures":["","normal"]}]}"#),
        ("materials/normal.tex", &normal),
        ("shaders/e.vert", b"attribute vec3 a_Position;\nvoid main() { gl_Position = vec4(a_Position, 1.0); }\n"),
        ("shaders/e.frag", b"uniform sampler2D g_Texture0;\nuniform sampler2D g_Texture1;\nvoid main() { gl_FragColor = vec4(1.0); }\n"),
    ]);
    let package = pkg::Package::parse(bytes).unwrap();
    let scene =
        model::load_with(&package, &effects::Assets::discover(Some("/nonexistent"))).unwrap();
    let pass = &scene.layers[0].effects[0].passes[0];
    let slot = pass.textures[1].as_ref().expect("normal map loaded");
    assert_eq!(slot.pixels.format, tex::PixelFormat::Bc3);
    assert_eq!(slot.pixels.bytes(), 16);
    assert_eq!(slot.we_format(), 4);
    let hlsl = pass.hlsl.as_ref().expect("hlsl pair");
    assert!(hlsl.fragment.contains("#define TEX1FORMAT 4"), "{}", hlsl.fragment);
    assert!(!hlsl.fragment.contains("#define TEX0FORMAT"), "{}", hlsl.fragment);
}

#[test]
fn particle_sprite_texture_defines_tex0format_instead_of_remapping_channels() {
    let particle = br#"{"renderer":[{"name":"sprite"}],"material":"materials/p.json",
        "emitter":[{"name":"boxrandom","rate":1}]}"#;
    let object: serde_json::Value =
        serde_json::json!({"id": 1, "particle": "particles/p.json", "origin": "0 0 0"});
    let mask = build_tex("TEXB0002", 9, 0, &[9u8; 16], false, None);
    let bytes = build_pkg(&[
        ("particles/p.json", particle),
        ("materials/p.json", br#"{"passes":[{"shader":"genericparticle","textures":["mask"]}]}"#),
        ("materials/mask.tex", &mask),
        (
            "shaders/genericparticle.vert",
            b"attribute vec3 a_Position;\nvoid main() { gl_Position = vec4(a_Position, 1.0); }\n",
        ),
        (
            "shaders/genericparticle.frag",
            b"uniform sampler2D g_Texture0;\nvoid main() { gl_FragColor = vec4(1.0); }\n",
        ),
    ]);
    let package = pkg::Package::parse(bytes).unwrap();
    let assets = effects::Assets::discover(Some("/nonexistent"));
    let system = crate::particles::load(&package, &assets, &object, "particles/p.json").unwrap();
    let texture = system.texture.as_ref().expect("sprite texture");
    assert_eq!(texture.pixels.format, tex::PixelFormat::R8);
    assert_eq!(texture.pixels.levels[0].data, vec![9u8; 16]);
    let hlsl = system.pass.as_ref().and_then(|pass| pass.hlsl.as_ref()).expect("engine pass");
    assert!(hlsl.fragment.contains("#define TEX0FORMAT 9"), "{}", hlsl.fragment);
    assert_eq!(system.grab_slot, None);
    assert!(hlsl.fragment.contains("#define REFRACT 0") || !hlsl.fragment.contains("REFRACT 1"));
}

#[test]
fn refracting_particle_material_keeps_refract_and_binds_the_scene_so_far() {
    let particle = br#"{"renderer":[{"name":"sprite"}],"material":"materials/p.json",
        "emitter":[{"name":"boxrandom","rate":1}]}"#;
    let object: serde_json::Value = serde_json::json!({
        "id": 1, "particle": "particles/p.json", "origin": "0 0 0",
        "instanceoverride": {"alpha": 0.8}
    });
    let mask = build_tex("TEXB0002", 9, 0, &[9u8; 16], false, None);
    let frag = br#"uniform sampler2D g_Texture0;
#if REFRACT
uniform sampler2D g_Texture1; // {"label":"n","format":"normalmap","formatcombo":true,"combo":"NORMALMAP"}
uniform sampler2D g_Texture3; // {"default":"_rt_FullFrameBuffer","hidden":true}
#endif
void main() {
#if REFRACT
	gl_FragColor = texSample2D(g_Texture3, vec2(0.5, 0.5)) * texSample2D(g_Texture1, vec2(0.5, 0.5));
#else
	gl_FragColor = vec4(1.0);
#endif
}
"#;
    let bytes = build_pkg(&[
        ("particles/p.json", particle),
        (
            "materials/p.json",
            br#"{"passes":[{"shader":"genericparticle","combos":{"REFRACT":1},"textures":["mask","normal"]}]}"#,
        ),
        ("materials/mask.tex", &mask),
        ("materials/normal.tex", &mask),
        (
            "shaders/genericparticle.vert",
            b"attribute vec3 a_Position;
void main() { gl_Position = vec4(a_Position, 1.0); }
",
        ),
        ("shaders/genericparticle.frag", frag),
    ]);
    let package = pkg::Package::parse(bytes).unwrap();
    let assets = effects::Assets::discover(Some("/nonexistent"));
    let system = crate::particles::load(&package, &assets, &object, "particles/p.json").unwrap();
    let pass = system.pass.as_ref().expect("engine pass");
    assert_eq!(system.grab_slot, Some(3));
    assert_eq!(pass.binds, vec![(3, effects::EffectBind::SceneSoFar)]);
    assert!(pass.textures.get(1).is_some_and(Option::is_some), "normal map slot kept");
    let hlsl = pass.hlsl.as_ref().expect("hlsl pair");
    assert!(hlsl.fragment.contains("#define REFRACT 1"), "{}", hlsl.fragment);
    assert!(hlsl.fragment.contains("#define NORMALMAP 1"), "{}", hlsl.fragment);
    assert!((system.alpha - 0.8).abs() < 1e-6, "refraction no longer dims alpha: {}", system.alpha);
}

#[test]
fn lighting_module_stub_is_valid_in_both_dialects() {
    let source = "#require LightingV1\nvoid main() { gl_FragColor = CAST4(PerformLighting_V1(CAST3(0.0), CAST3(1.0), CAST3(0.0), CAST3(0.0), CAST3(0.0), CAST3(0.0), 0.5, 0.0)); }\n";
    let resolved = crate::shader::resolve_includes(source, &|_| None, 0).unwrap();
    assert!(resolved.contains("return CAST3(0.0); }"), "{resolved}");
    assert!(!resolved.contains("vec3(0.0)"), "{resolved}");
}

#[test]
fn each_backend_receives_its_own_dialect_branch() {
    let scene = br#"{"objects":[{"id":1,"name":"L","image":"models/bg.json",
        "effects":[{"file":"effects/d.json"}]}]}"#;
    let frag = b"uniform sampler2D g_Texture0;\nvoid main() {\n#if HLSL\n\tfloat k = 1.0;\n#else\n\tfloat k = 2.0;\n#endif\n#ifdef GLSL\n\tk += 4.0;\n#endif\n#if HLSL_SM30\n\tk += 8.0;\n#endif\n\tgl_FragColor = CAST4(k);\n}\n";
    let bytes = build_pkg(&[
        ("scene.json", scene),
        ("models/bg.json", br#"{"material":"materials/bg.json"}"#),
        ("materials/bg.json", br#"{"passes":[{"shader":"flat"}]}"#),
        ("effects/d.json", br#"{"passes":[{"material":"materials/d.json"}]}"#),
        ("materials/d.json", br#"{"passes":[{"shader":"d"}]}"#),
        (
            "shaders/d.vert",
            b"attribute vec3 a_Position;\nvoid main() { gl_Position = vec4(a_Position, 1.0); }\n",
        ),
        ("shaders/d.frag", frag),
    ]);
    let package = pkg::Package::parse(bytes).unwrap();
    let scene =
        model::load_with(&package, &effects::Assets::discover(Some("/nonexistent"))).unwrap();
    let pass = &scene.layers[0].effects[0].passes[0];
    let hlsl = pass.hlsl.as_ref().expect("hlsl pair");
    assert!(hlsl.fragment.contains("float k = 1.0;"), "{}", hlsl.fragment);
    assert!(!hlsl.fragment.contains("k += 4.0"), "{}", hlsl.fragment);
    assert!(!hlsl.fragment.contains("k += 8.0"), "{}", hlsl.fragment);
    assert!(pass.fragment.source.contains("float k = 2.0;"), "{}", pass.fragment.source);
    assert!(pass.fragment.source.contains("k += 4.0"), "{}", pass.fragment.source);
    assert!(!pass.fragment.source.contains("k += 8.0"), "{}", pass.fragment.source);
}

fn fluid_like_pkg() -> Vec<u8> {
    let scene = br#"{"objects":[{"id":7,"name":"L","image":"models/bg.json",
        "effects":[{"id":41,"file":"effects/fluid.json","passes":[{"combos":{"RENDERING":3}}]}]}]}"#;
    let effect = br#"{"fbos":[
        {"name":"_rt_V1","fit":256,"format":"rg1616f","unique":true,"clear":"0 0 0 0"},
        {"name":"_rt_V2","fit":256,"format":"rg1616f","unique":true},
        {"name":"_rt_Curl","fit":256,"format":"r16f"},
        {"name":"_rt_Normal","scale":2,"conditions":[{"LIGHTING":1}]},
        {"name":"_rt_Tiles","width":256,"height":256,"format":"r8","uvs":"repeat"},
        {"name":"_rt_Odd","scale":1.5,"format":"weird"}],
      "passes":[
        {"material":"materials/e.json","target":"_rt_V2","bind":[{"name":"_rt_V1","index":1},{"name":"_rt_Curl","index":2,"conditions":[{"RENDERING":3}]},{"name":"_rt_Normal","index":3,"conditions":[{"LIGHTING":1}]}]},
        {"command":"swap","source":"_rt_V1","target":"_rt_V2"},
        {"material":"materials/e.json","conditions":[{"LIGHTING":1}]},
        {"material":"materials/e.json","bind":[{"name":"_rt_V1","index":1}]}]}"#;
    build_pkg(&[
        ("scene.json", scene),
        ("models/bg.json", br#"{"material":"materials/bg.json"}"#),
        ("materials/bg.json", br#"{"passes":[{"shader":"flat"}]}"#),
        ("effects/fluid.json", effect),
        ("materials/e.json", br#"{"passes":[{"shader":"e","combos":{"LIGHTING":0}}]}"#),
        ("shaders/e.vert", b"attribute vec3 a_Position;\nvoid main() { gl_Position = vec4(a_Position, 1.0); }\n"),
        ("shaders/e.frag", b"uniform sampler2D g_Texture0;\nuniform sampler2D g_Texture1;\nuniform sampler2D g_Texture2;\nuniform sampler2D g_Texture3;\nvoid main() { gl_FragColor = vec4(1.0); }\n"),
    ])
}

#[test]
fn fbo_declarations_keep_size_format_wrap_clear_unique_and_conditions() {
    use effects::{FboFormat, FboSize};
    let package = pkg::Package::parse(fluid_like_pkg()).unwrap();
    let scene =
        model::load_with(&package, &effects::Assets::discover(Some("/nonexistent"))).unwrap();
    let effect = &scene.layers[0].effects[0];
    let names: Vec<&str> = effect.fbos.iter().map(|fbo| fbo.name.as_str()).collect();
    assert_eq!(names, ["_rt_V1", "_rt_V2", "_rt_Curl", "_rt_Tiles", "_rt_Odd"]);
    let v1 = &effect.fbos[0];
    assert_eq!(v1.size, FboSize::Fit(256));
    assert_eq!(v1.format, FboFormat::Rg16f);
    assert!(v1.unique);
    assert_eq!(v1.repeat, None);
    assert_eq!(v1.clear, [0.0; 4]);
    assert_eq!(effect.fbos[2].format, FboFormat::R16f);
    assert!(!effect.fbos[2].unique);
    let tiles = &effect.fbos[3];
    assert_eq!(tiles.size, FboSize::Fixed { width: 256, height: 256 });
    assert_eq!(tiles.format, FboFormat::R8);
    assert_eq!(tiles.repeat, Some(true));
    assert_eq!(effect.fbos[4].size, FboSize::Scale(1.5));
    assert_eq!(effect.fbos[4].format, FboFormat::Rgba8);
    assert!(scene.skipped.iter().any(|skip| skip.contains("_rt_Odd")), "{:?}", scene.skipped);
    assert_eq!(v1.extent((1749, 984)), (256, 144));
    assert_eq!(tiles.extent((1749, 984)), (256, 256));
    assert_eq!(effect.fbos[4].extent((300, 150)), (200, 100));
}

#[test]
fn swap_commands_become_steps_and_conditions_gate_passes_and_binds() {
    let package = pkg::Package::parse(fluid_like_pkg()).unwrap();
    let scene =
        model::load_with(&package, &effects::Assets::discover(Some("/nonexistent"))).unwrap();
    let effect = &scene.layers[0].effects[0];
    assert_eq!(effect.passes.len(), 2, "lighting-gated pass dropped");
    assert_eq!(effect.swaps.len(), 1);
    assert_eq!(effect.swaps[0].after_pass, Some(0));
    assert_eq!(
        (effect.swaps[0].source.as_str(), effect.swaps[0].target.as_str()),
        ("_rt_V1", "_rt_V2")
    );
    let binds: Vec<usize> = effect.passes[0].binds.iter().map(|(slot, _)| *slot).collect();
    assert_eq!(binds, [1, 2], "lighting-gated bind dropped, rendering bind kept");
    assert!(!scene.skipped.iter().any(|skip| skip.contains("swap")), "{:?}", scene.skipped);
}

#[test]
fn engine_global_shadow_atlas_defaults_do_not_become_binds() {
    let scene = br#"{"objects":[{"id":1,"name":"L","image":"models/bg.json",
        "effects":[{"file":"effects/s.json"}]}]}"#;
    let bytes = build_pkg(&[
        ("scene.json", scene),
        ("models/bg.json", br#"{"material":"materials/bg.json"}"#),
        ("materials/bg.json", br#"{"passes":[{"shader":"flat"}]}"#),
        ("effects/s.json", br#"{"fbos":[{"name":"_rt_Local"}],"passes":[{"material":"materials/s.json"}]}"#),
        ("materials/s.json", br#"{"passes":[{"shader":"s"}]}"#),
        ("shaders/s.vert", b"attribute vec3 a_Position;\nvoid main() { gl_Position = vec4(a_Position, 1.0); }\n"),
        ("shaders/s.frag", b"uniform sampler2D g_Texture0;\nuniform sampler2D g_Texture1; // {\"hidden\":true,\"default\":\"_rt_Local\"}\nuniform sampler2DComparison g_Texture6; // {\"hidden\":true,\"default\":\"_rt_shadowAtlas\"}\nvoid main() { gl_FragColor = texSample2D(g_Texture0, vec2(0.5)) + texSample2D(g_Texture1, vec2(0.5)); }\n"),
    ]);
    let package = pkg::Package::parse(bytes).unwrap();
    let scene =
        model::load_with(&package, &effects::Assets::discover(Some("/nonexistent"))).unwrap();
    let pass = &scene.layers[0].effects[0].passes[0];
    let bound: Vec<usize> = pass.binds.iter().map(|(slot, _)| *slot).collect();
    assert!(bound.contains(&1), "{bound:?}");
    assert!(!bound.contains(&6), "{bound:?}");
}

#[test]
fn legacy_image_materials_ignore_object_tint_and_use_material_constants() {
    let mask = build_tex("TEXB0002", 9, 0, &[9u8; 16], false, None);
    let scene = br#"{"general":{"orthogonalprojection":{"width":1920,"height":1080}},"objects":[
        {"id":1,"name":"legacy","image":"models/legacy.json","origin":"100 100 0","size":"10 10","color":"1 0 0","alpha":0.5},
        {"id":2,"name":"consts","image":"models/consts.json","origin":"100 100 0","size":"10 10","color":"1 0 0","alpha":0.5},
        {"id":3,"name":"versioned","image":"models/versioned.json","origin":"100 100 0","size":"10 10","color":"1 0 0","alpha":0.5},
        {"id":4,"name":"modern","image":"models/modern.json","origin":"100 100 0","size":"10 10","color":"1 0 0","alpha":0.5}]}"#;
    let bytes = build_pkg(&[
        ("scene.json", scene),
        ("models/legacy.json", br#"{"material":"materials/legacy.json"}"#),
        ("materials/legacy.json", br#"{"passes":[{"shader":"genericimage2","textures":["mask"]}]}"#),
        ("models/consts.json", br#"{"material":"materials/consts.json"}"#),
        ("materials/consts.json", br#"{"passes":[{"shader":"genericimage2","constantshadervalues":{"Brightness":0.5,"Alpha":0.25},"textures":["mask"]}]}"#),
        ("models/versioned.json", br#"{"material":"materials/versioned.json"}"#),
        ("materials/versioned.json", br#"{"passes":[{"shader":"genericimage2","combos":{"VERSION":2},"textures":["mask"]}]}"#),
        ("models/modern.json", br#"{"material":"materials/modern.json"}"#),
        ("materials/modern.json", br#"{"passes":[{"shader":"genericimage4","textures":["mask"]}]}"#),
        ("materials/mask.tex", &mask),
    ]);
    let package = pkg::Package::parse(bytes).unwrap();
    let model =
        model::load_with(&package, &effects::Assets::discover(Some("/nonexistent"))).unwrap();
    let by_name = |name: &str| model.layers.iter().find(|layer| layer.name == name).unwrap();
    assert_eq!(by_name("legacy").color, [1.0, 1.0, 1.0]);
    assert!((by_name("legacy").alpha - 1.0).abs() < 1e-6);
    assert_eq!(by_name("consts").color, [0.5, 0.5, 0.5]);
    assert!((by_name("consts").alpha - 0.25).abs() < 1e-6);
    assert_eq!(by_name("versioned").color, [1.0, 0.0, 0.0]);
    assert!((by_name("versioned").alpha - 0.5).abs() < 1e-6);
    assert_eq!(by_name("modern").color, [1.0, 0.0, 0.0]);
    assert!((by_name("modern").alpha - 0.5).abs() < 1e-6);
}

#[test]
fn bloom_scenes_get_the_engine_bloom_chain_as_a_final_passthrough_layer() {
    let assets = effects::Assets::discover(None);
    if assets.read("materials/util/downsample_quarter_bloom.json").is_none() {
        return;
    }
    let mask = build_tex("TEXB0002", 9, 0, &[9u8; 16], false, None);
    let scene = br#"{"general":{"orthogonalprojection":{"width":1920,"height":1080},"bloom":true,"bloomstrength":1.5,"bloomthreshold":0.4,"bloomtint":"1 0.5 0.25"},
        "objects":[{"id":1,"name":"bg","image":"models/bg.json","origin":"960 540 0","size":"1920 1080"}]}"#;
    let bytes = build_pkg(&[
        ("scene.json", scene),
        ("models/bg.json", br#"{"material":"materials/bg.json"}"#),
        ("materials/bg.json", br#"{"passes":[{"shader":"genericimage4","textures":["mask"]}]}"#),
        ("materials/mask.tex", &mask),
    ]);
    let package = pkg::Package::parse(bytes).unwrap();
    let model = model::load_with(&package, &assets).unwrap();
    assert_eq!(model.layers.len(), 2, "{:?}", model.skipped);
    let bloom = model.layers.last().unwrap();
    assert_eq!(bloom.id, "@bloom");
    assert!(bloom.passthrough);
    assert_eq!(bloom.size, (1920.0, 1080.0));
    assert!(bloom.scene_order > model.layers[0].scene_order);
    let effect = &bloom.effects[0];
    assert_eq!(effect.passes.len(), 4, "{:?}", model.skipped);
    assert_eq!(
        effect.fbos.iter().map(|fbo| fbo.name.as_str()).collect::<Vec<_>>(),
        ["_rt_4FrameBuffer", "_rt_8FrameBuffer", "_rt_Bloom"]
    );
    assert_eq!(effect.passes[0].target.as_deref(), Some("_rt_4FrameBuffer"));
    assert_eq!(effect.passes[0].binds, vec![(0, effects::EffectBind::SceneSoFar)]);
    assert_eq!(effect.passes[3].target, None);
    assert_eq!(
        effect.passes[3].binds,
        vec![
            (0, effects::EffectBind::SceneSoFar),
            (1, effects::EffectBind::Named("_rt_Bloom".into()))
        ]
    );
    assert_eq!(effect.passes[0].values.get("g_BloomStrength"), Some(&vec![1.5]));
    assert_eq!(effect.passes[0].values.get("g_BloomThreshold"), Some(&vec![0.4]));
    assert_eq!(effect.passes[0].values.get("g_BloomTint"), Some(&vec![1.0, 0.5, 0.25]));
}

#[test]
fn media_thumbnail_layers_are_hidden_without_playback() {
    let mask = build_tex("TEXB0002", 9, 0, &[9u8; 16], false, None);
    let scene = br#"{"general":{"orthogonalprojection":{"width":1920,"height":1080}},"objects":[
        {"id":1,"name":"bg","image":"models/bg.json","origin":"960 540 0","size":"10 10"},
        {"id":2,"name":"cover","image":"models/bg.json","origin":"100 100 0","size":"10 10","instance":{"textures":["util/white"],"usertextures":[{"name":"$mediaThumbnail","type":"system"}]}}]}"#;
    let bytes = build_pkg(&[
        ("scene.json", scene),
        ("models/bg.json", br#"{"material":"materials/bg.json"}"#),
        ("materials/bg.json", br#"{"passes":[{"shader":"genericimage4","textures":["mask"]}]}"#),
        ("materials/mask.tex", &mask),
    ]);
    let package = pkg::Package::parse(bytes).unwrap();
    let model =
        model::load_with(&package, &effects::Assets::discover(Some("/nonexistent"))).unwrap();
    assert_eq!(model.layers.len(), 1);
    assert!(
        model.skipped.iter().any(|skip| skip.contains("media thumbnail")),
        "{:?}",
        model.skipped
    );
}

#[test]
fn passes_whose_vertex_shader_ignores_the_projection_draw_an_ndc_quad() {
    let scene = br#"{"objects":[{"id":1,"name":"L","image":"models/bg.json",
        "effects":[{"file":"effects/a.json"},{"file":"effects/b.json"}]}]}"#;
    let bytes = build_pkg(&[
        ("scene.json", scene),
        ("models/bg.json", br#"{"material":"materials/bg.json"}"#),
        ("materials/bg.json", br#"{"passes":[{"shader":"flat"}]}"#),
        ("effects/a.json", br#"{"passes":[{"material":"materials/a.json"}]}"#),
        ("materials/a.json", br#"{"passes":[{"shader":"a"}]}"#),
        ("shaders/a.vert", b"attribute vec3 a_Position;\nuniform mat4 g_ModelViewProjectionMatrix;\nvoid main() { gl_Position = mul(vec4(a_Position, 1.0), g_ModelViewProjectionMatrix); }\n"),
        ("shaders/a.frag", b"void main() { gl_FragColor = vec4(1.0); }\n"),
        ("effects/b.json", br#"{"passes":[{"material":"materials/b.json"}]}"#),
        ("materials/b.json", br#"{"passes":[{"shader":"b"}]}"#),
        ("shaders/b.vert", b"attribute vec3 a_Position;\nuniform vec2 g_TexelSize;\nvoid main() { gl_Position = vec4(a_Position, 1.0); }\n"),
        ("shaders/b.frag", b"void main() { gl_FragColor = vec4(1.0); }\n"),
    ]);
    let package = pkg::Package::parse(bytes).unwrap();
    let object = package.find_json("scene.json").unwrap().unwrap()["objects"][0].clone();
    let (effects, skipped) =
        effects::load_effects(&package, &effects::Assets::discover(Some("/nonexistent")), &object);
    assert_eq!(effects.len(), 2, "{skipped:?}");
    assert!(!effects::PassMeta::of(&effects[0].passes[0]).ndc());
    assert!(effects::PassMeta::of(&effects[1].passes[0]).ndc());
}

#[test]
fn video_texture_payload_is_never_accepted_as_rgba() {
    let mut payload = vec![93_u8; 256];
    payload[..12].copy_from_slice(b"\x00\x00\x00\x20ftypisom");
    let bytes = build_tex("TEXB0003", 0, tex::FLAG_IS_VIDEO as i32, &payload, false, None);
    let mut parsed = tex::parse(&bytes).unwrap();
    assert!(tex::take_pixels(&mut parsed).is_none());
    assert!(tex::decode_rgba(&mut parsed).is_none());
    let texture = model::load_texture_bytes(&bytes).unwrap();
    assert_eq!(texture.video.as_deref(), Some(payload.as_slice()));
    assert_eq!((texture.width, texture.height), (4, 4));
    assert_eq!(texture.pixels.bytes(), 0);
    assert!(texture.frames.is_empty());
}
