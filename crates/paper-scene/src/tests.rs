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

fn build_pkg(files: &[(&str, &[u8])]) -> Vec<u8> {
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
    let (w, h, rgba) = tex::decode_rgba(&tex::parse(&bytes).unwrap()).unwrap();
    assert_eq!((w, h), (4, 4));
    assert_eq!(&rgba[0..4], &[1, 2, 3, 4]);

    let bytes = build_tex("TEXB0002", 9, 0, &[9u8; 16], true, None);
    let (_, _, rgba) = tex::decode_rgba(&tex::parse(&bytes).unwrap()).unwrap();
    assert_eq!(&rgba[0..4], &[9, 9, 9, 255]);
}

#[test]
fn scene_feature_extraction() {
    let scene_json = br#"{
        "camera": {"parallax": 1},
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
    assert!(out.source.contains("layout(location = 0) in vec4 v_TexCoord;"));
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
    assert!(!out.contains("common.h"));
    assert!(out.contains("float helper()"));
    let missing = resolve_includes("#include \"gone.h\"", &load, 0).unwrap();
    assert!(missing.contains("missing include gone.h"));
}

#[test]
fn user_bound_baked_fallback() {
    let scene = br#"{"general":{"orthogonalprojection":{"width":1000,"height":1000}},"objects":[{"id":1,"name":"L","image":"models/bg.json","size":"100.000 50.000","origin":{"user":"p","value":"10.000 20.000 0.000"},"scale":{"user":"q","value":"2.000 3.000 1.000"},"angles":{"user":"r","value":"0.000 0.000 90.000"},"color":{"user":"c","value":"0.500 0.250 0.125"}}]}"#;
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
    assert!((layer.angle + 90.0_f32.to_radians()).abs() < 1e-5);
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
fn image_and_particle_layers_share_depth_then_authored_order() {
    let scene = br#"{
        "objects": [
            {"id": 1, "image": "models/bg.json", "origin": "0 0 0"},
            {"id": 2, "particle": "particles/dot.json", "origin": "0 0 1"},
            {"id": 3, "image": "models/bg.json", "origin": "0 0 2"}
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

    let explicit = build(effects::parse_property_overrides(
        serde_json::json!({"zoom": "3 4 1"}).as_object().unwrap(),
    ));
    assert_eq!(explicit.layers.first().expect("layer built").size, (300.0, 200.0));
}
