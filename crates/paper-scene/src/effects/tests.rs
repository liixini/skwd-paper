use super::{
    Assets, CompositeBuffer, EffectBind, EffectPass, FrameClock, PassMeta, daytime, daytime_now,
};
use crate::shader::{Uniform, UniformKind};
use std::collections::BTreeMap;

fn pass_meta(names: &[&str]) -> PassMeta {
    PassMeta {
        property_source: None,
        name: String::new(),
        uniforms: names
            .iter()
            .map(|name| Uniform {
                name: (*name).to_string(),
                kind: UniformKind::Float,
                default: None,
                material: None,
                offset: 0,
                count: 1,
            })
            .collect(),
        constants: BTreeMap::new(),
        resolutions: Vec::new(),
        ndc: false,
    }
}

#[test]
fn live_color_vectors_refresh_the_material_uniform_bytes() {
    let mut meta = pass_meta(&["g_BarColor"]);
    meta.uniforms[0].kind = UniformKind::Vec3;
    meta.uniforms[0].material = Some("Bar Color".into());
    meta.constants.insert("g_BarColor".into(), vec![1.0; 3]);
    for color in [[1.0, 0.0, 0.0], [0.25, 0.5, 0.75], [1.0; 3]] {
        meta.apply_script_values(&serde_json::json!({"Bar Color": color}), &BTreeMap::new());
        let bytes = meta.uniform_bytes(FrameClock::default(), None, 8, 8, (8, 8), &[]);
        let actual: Vec<_> = bytes[..12]
            .chunks_exact(4)
            .map(|part| f32::from_le_bytes(part.try_into().unwrap()))
            .collect();
        assert_eq!(actual, color);
    }
    assert!(super::json_numbers(&serde_json::json!([1, "invalid", 0])).is_none());
}

#[test]
fn live_uniforms_keep_source_indices_after_effect_and_pass_filtering() {
    let definition = br#"{"passes":[{"material":"materials/expanded.json","conditions":[{"DISABLED":1}]},{"material":"materials/expanded.json"}]}"#;
    let material = br#"{"passes":[{"shader":"color","constantshadervalues":{"Bar Color":"1 1 1"}},{"shader":"color","constantshadervalues":{"Bar Color":"1 1 1"}}]}"#;
    let vertex = b"attribute vec3 a_Position; void main() { gl_Position = vec4(a_Position, 1.0); }";
    let fragment = br#"uniform vec3 g_BarColor; // {"material":"Bar Color","default":"1 1 1"}
void main() { gl_FragColor = vec4(g_BarColor, 1.0); }"#;
    let package = crate::pkg::Package::parse(crate::tests::build_pkg(&[
        ("effects/live/effect.json", definition),
        ("materials/expanded.json", material),
        ("shaders/color.vert", vertex),
        ("shaders/color.frag", fragment),
    ]))
    .unwrap();
    let mut object = serde_json::json!({"effects":[
        {"file":"effects/live/effect.json","visible":false},
        {"file":"effects/missing/effect.json"},
        {"file":"effects/live/effect.json","passes":[
            {"constantshadervalues":{"Bar Color":"0 1 0"}},
            {"constantshadervalues":{"Bar Color":"0.25 0.5 0.75"}}
        ]}
    ]});
    let (effects, skipped) = super::load_effects(&package, &Assets::discover(Some("")), &object);
    assert_eq!(effects.len(), 1);
    assert_eq!(skipped.len(), 1);
    assert_eq!(effects[0].passes.len(), 2);
    assert_eq!(effects[0].passes[0].property_source, Some((2, 1)));
    assert_eq!(effects[0].passes[1].property_source, None);
    let mut direct = PassMeta::of(&effects[0].passes[0]);
    let mut expanded = PassMeta::of(&effects[0].passes[1]);
    assert_eq!(direct.constants["g_BarColor"], [0.25, 0.5, 0.75]);
    assert_eq!(expanded.constants["g_BarColor"], [1.0; 3]);
    object["effects"][2]["passes"][1]["constantshadervalues"]["Bar Color"] =
        serde_json::json!([1, 0, 0]);
    direct.apply_object_values(&object, &BTreeMap::new());
    expanded.apply_object_values(&object, &BTreeMap::new());
    assert_eq!(direct.constants["g_BarColor"], [1.0, 0.0, 0.0]);
    assert_eq!(expanded.constants["g_BarColor"], [1.0; 3]);
}

#[test]
fn time_dependency_exact_uniform() {
    assert!(!pass_meta(&[]).time_dependent());
    assert!(!pass_meta(&["g_TimeDelta", "g_Daytime"]).time_dependent());
    assert!(pass_meta(&["g_Alpha", "g_Time"]).time_dependent());
}

#[test]
fn assets_dir_discovered() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("shaders")).unwrap();
    assert!(Assets::discover(directory.path().to_str()).available());
}

#[test]
fn asset_reads_confined() {
    let directory = tempfile::tempdir().unwrap();
    let assets_root = directory.path().join("assets");
    std::fs::create_dir_all(assets_root.join("shaders")).unwrap();
    std::fs::write(assets_root.join("shaders/inside.vert"), "inside").unwrap();
    std::fs::write(directory.path().join("outside.vert"), "outside").unwrap();
    let assets = Assets::discover(assets_root.to_str());

    assert_eq!(assets.read("shaders/inside.vert").as_deref(), Some("inside"));
    assert!(assets.read("../outside.vert").is_none());
}

#[test]
fn loaded_shared_assets_record_property_dependencies_and_unknown_json() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("shaders")).unwrap();
    std::fs::write(
        directory.path().join("material.json"),
        r#"{"size":{"user":"Opacity","value":1}}"#,
    )
    .unwrap();
    let assets = Assets::discover(directory.path().to_str());
    assert_eq!(assets.property_dependencies(), Some(std::collections::BTreeSet::new()));
    assert!(assets.read("material.json").is_some());
    assert_eq!(
        assets.property_dependencies().unwrap().into_iter().collect::<Vec<_>>(),
        ["opacity"]
    );
    std::fs::write(directory.path().join("unknown.json"), "{broken").unwrap();
    assert!(assets.read("unknown.json").is_some());
    assert_eq!(assets.property_dependencies(), None);
}

#[test]
fn asset_reads_reject_symlink_escapes() {
    let directory = tempfile::tempdir().unwrap();
    let assets_root = directory.path().join("assets");
    std::fs::create_dir_all(assets_root.join("shaders")).unwrap();
    let outside = directory.path().join("outside.vert");
    std::fs::write(&outside, "outside").unwrap();
    std::os::unix::fs::symlink(&outside, assets_root.join("shaders/escape.vert")).unwrap();
    let assets = Assets::discover(assets_root.to_str());

    assert!(assets.read("shaders/escape.vert").is_none());
}

#[test]
fn render_target_names_have_explicit_scene_semantics() {
    assert_eq!(EffectBind::parse("previous", Some("42")), EffectBind::Previous);
    assert_eq!(EffectBind::parse("_rt_imageLayerComposite_42_a", Some("42")), EffectBind::Previous);
    assert_eq!(
        EffectBind::parse("_rt_imageLayerComposite_7_b", Some("42")),
        EffectBind::LayerComposite { layer: "7".into(), buffer: CompositeBuffer::B }
    );
    assert_eq!(EffectBind::parse("_rt_FullFrameBuffer", Some("42")), EffectBind::SceneSoFar);
    assert_eq!(
        EffectBind::parse("_rt_HalfCompoBuffer1", Some("42")),
        EffectBind::Named("_rt_HalfCompoBuffer1".into())
    );
}

#[test]
fn malformed_layer_target_names_are_not_misclassified() {
    assert_eq!(
        EffectBind::parse("_rt_imageLayerComposite_7_c", Some("42")),
        EffectBind::Named("_rt_imageLayerComposite_7_c".into())
    );
    assert_eq!(
        EffectBind::parse("_rt_imageLayerComposite__a", Some("42")),
        EffectBind::Named("_rt_imageLayerComposite__a".into())
    );
}

#[test]
fn engine_uniforms_follow_the_documented_definitions() {
    let source = "uniform float g_Time;\nuniform float g_Daytime;\nuniform float g_Frametime;\nuniform vec2 g_TexelSize;\nuniform vec3 g_Screen;\nuniform vec4 g_Texture0Rotation;\nvoid main() { gl_FragColor = vec4(g_Time); }\n";
    let fragment =
        crate::shader::translate(source, crate::shader::Stage::Fragment, &BTreeMap::new());
    let mut vertex =
        crate::shader::translate("void main() {}", crate::shader::Stage::Vertex, &BTreeMap::new());
    let mut fragment = fragment;
    crate::shader::unify_uniforms(&mut vertex, &mut fragment);
    let pass = EffectPass {
        property_source: None,
        name: "probe".into(),
        vertex,
        fragment,
        hlsl: None,
        textures: Vec::new(),
        values: BTreeMap::new(),
        target: None,
        binds: Vec::new(),
    };
    let meta = PassMeta::of(&pass);
    let clock = FrameClock { time: 3.0, dt: 0.5, daytime: 0.25 };
    let bytes = meta.uniform_bytes(clock, None, 256, 128, (1920, 1080), &[]);
    let at = |name: &str| {
        let uniform = pass.fragment.uniforms.iter().find(|u| u.name == name).unwrap();
        let word = |lane: usize| {
            let start = uniform.offset + lane * 4;
            f32::from_le_bytes(bytes[start..start + 4].try_into().unwrap())
        };
        (word(0), word(1), word(2), word(3))
    };
    assert_eq!(at("g_Time").0, 3.0);
    assert_eq!(at("g_Frametime").0, 0.5);
    assert_eq!(at("g_Daytime").0, 0.25);
    assert!((at("g_TexelSize").0 - 1.0 / 1920.0).abs() < 1e-9);
    assert!((at("g_TexelSize").1 - 1.0 / 1080.0).abs() < 1e-9);
    assert_eq!((at("g_Screen").0, at("g_Screen").1), (1920.0, 1080.0));
    assert_eq!(at("g_Texture0Rotation"), (1.0, 0.0, 0.0, 1.0));
    let daytime = daytime_now();
    assert!((0.0..1.0).contains(&daytime));
}

#[test]
fn daytime_matches_the_engine_formula() {
    assert_eq!(daytime(0, 0, 0, 0), 0.0);
    assert!((daytime(12, 0, 0, 0) - 0.5).abs() < 1e-7);
    assert!((daytime(23, 59, 59, 999) - 0.999_999_988).abs() < 1e-6);
    assert!((daytime(6, 30, 0, 500) - (6.5 / 24.0 + 0.5 / 86_400.0)).abs() < 1e-7);
}

#[test]
fn effect_projection_follows_the_backend_clip_convention() {
    let source = "uniform mat4 g_ModelViewProjectionMatrix;\nuniform mat4 g_EffectModelViewProjectionMatrix;\nuniform vec2 g_ParallaxPosition;\nvoid main() { gl_FragColor = mul(vec4(g_ParallaxPosition, 0.0, 1.0), g_ModelViewProjectionMatrix) + mul(CAST4(1.0), g_EffectModelViewProjectionMatrix); }";
    let fragment =
        crate::shader::translate(source, crate::shader::Stage::Fragment, &BTreeMap::new());
    let mut vertex =
        crate::shader::translate("void main() {}", crate::shader::Stage::Vertex, &BTreeMap::new());
    let mut fragment = fragment;
    crate::shader::unify_uniforms(&mut vertex, &mut fragment);
    let pass = EffectPass {
        property_source: None,
        name: "probe".into(),
        vertex,
        fragment,
        hlsl: None,
        textures: Vec::new(),
        values: BTreeMap::new(),
        target: None,
        binds: Vec::new(),
    };
    let meta = PassMeta::of(&pass);
    let clock = FrameClock { time: 0.0, dt: 0.0, daytime: 0.0 };
    let lane = |bytes: &[u8], name: &str, index: usize| {
        let uniform = pass.fragment.uniforms.iter().find(|u| u.name == name).unwrap();
        let start = uniform.offset + index * 4;
        f32::from_le_bytes(bytes[start..start + 4].try_into().unwrap())
    };
    let gl = meta.uniform_bytes_with(
        clock,
        Some((200, 100)),
        200,
        100,
        (200, 100),
        &[],
        &BTreeMap::new(),
        false,
    );
    let d3d = meta.uniform_bytes_with(
        clock,
        Some((200, 100)),
        200,
        100,
        (200, 100),
        &[],
        &BTreeMap::new(),
        true,
    );
    assert_eq!(lane(&gl, "g_ModelViewProjectionMatrix", 5), -0.02);
    assert_eq!(lane(&gl, "g_ModelViewProjectionMatrix", 13), 1.0);
    assert_eq!(lane(&d3d, "g_ModelViewProjectionMatrix", 5), 0.02);
    assert_eq!(lane(&d3d, "g_ModelViewProjectionMatrix", 13), -1.0);
    assert_eq!(lane(&d3d, "g_EffectModelViewProjectionMatrix", 5), 0.02);
    assert_eq!(lane(&gl, "g_EffectModelViewProjectionMatrix", 0), 0.01);
    assert_eq!(
        (lane(&gl, "g_ParallaxPosition", 0), lane(&gl, "g_ParallaxPosition", 1)),
        (0.5, 0.5)
    );
    let named_gl =
        meta.uniform_bytes_with(clock, None, 200, 100, (200, 100), &[], &BTreeMap::new(), false);
    let named_d3d =
        meta.uniform_bytes_with(clock, None, 200, 100, (200, 100), &[], &BTreeMap::new(), true);
    assert_eq!(lane(&named_gl, "g_ModelViewProjectionMatrix", 5), 1.0);
    assert_eq!(lane(&named_d3d, "g_ModelViewProjectionMatrix", 5), 1.0);
    assert_eq!(lane(&named_d3d, "g_EffectModelViewProjectionMatrix", 5), 1.0);
}

#[test]
fn scalar_constants_splat_across_vector_lanes() {
    let source = "uniform vec2 g_Scale;\nuniform vec4 g_Tint;\nuniform float g_K;\nuniform mat4 g_M;\nvoid main() { gl_FragColor = vec4(g_Scale, g_K, 1.0) * g_Tint * g_M[0]; }";
    let fragment =
        crate::shader::translate(source, crate::shader::Stage::Fragment, &BTreeMap::new());
    let values = BTreeMap::from([
        ("g_Scale".to_string(), vec![0.25]),
        ("g_Tint".to_string(), vec![0.5, 0.75]),
        ("g_K".to_string(), vec![2.0]),
        ("g_M".to_string(), vec![3.0]),
    ]);
    let bytes = crate::shader::pack_uniforms(&fragment.uniforms, &values);
    let lane = |name: &str, index: usize| {
        let uniform = fragment.uniforms.iter().find(|u| u.name == name).unwrap();
        let start = uniform.offset + index * 4;
        f32::from_le_bytes(bytes[start..start + 4].try_into().unwrap())
    };
    assert_eq!((lane("g_Scale", 0), lane("g_Scale", 1)), (0.25, 0.25));
    assert_eq!((lane("g_Tint", 0), lane("g_Tint", 1), lane("g_Tint", 2)), (0.5, 0.75, 0.0));
    assert_eq!(lane("g_K", 0), 2.0);
    assert_eq!((lane("g_M", 0), lane("g_M", 1)), (3.0, 0.0));
}

#[test]
fn annotated_vector_defaults_keep_every_lane() {
    let source = "uniform vec2 u_FixedSize; // {\"default\":\"1 0.59\",\"material\":\"Fixed Size\",\"position\":true}\nuniform float u_Radius; // {\"material\":\"Radius\",\"default\":0.35,\"range\":[0,1]}\nvoid main() { gl_FragColor = vec4(u_FixedSize, u_Radius, 1.0); }";
    let fragment =
        crate::shader::translate(source, crate::shader::Stage::Fragment, &BTreeMap::new());
    let mut vertex =
        crate::shader::translate("void main() {}", crate::shader::Stage::Vertex, &BTreeMap::new());
    let mut fragment = fragment;
    crate::shader::unify_uniforms(&mut vertex, &mut fragment);
    let size = fragment.uniforms.iter().find(|u| u.name == "u_FixedSize").unwrap();
    assert_eq!(size.default, Some(vec![1.0, 0.59]), "{:?}", size.default);
    let pass = EffectPass {
        property_source: None,
        name: "probe".into(),
        vertex,
        fragment,
        hlsl: None,
        textures: Vec::new(),
        values: BTreeMap::new(),
        target: None,
        binds: Vec::new(),
    };
    let meta = PassMeta::of(&pass);
    let bytes = meta.uniform_bytes(FrameClock::default(), None, 8, 8, (8, 8), &[]);
    let lane = |name: &str, index: usize| {
        let uniform = pass.fragment.uniforms.iter().find(|u| u.name == name).unwrap();
        let start = uniform.offset + index * 4;
        f32::from_le_bytes(bytes[start..start + 4].try_into().unwrap())
    };
    assert_eq!((lane("u_FixedSize", 0), lane("u_FixedSize", 1)), (1.0, 0.59));
    assert_eq!(lane("u_Radius", 0), 0.35);
}
