use super::{
    Assets, CompositeBuffer, EffectBind, EffectPass, FrameClock, PassMeta, daytime, daytime_now,
};
use crate::shader::{Uniform, UniformKind};
use std::collections::BTreeMap;

fn pass_meta(names: &[&str]) -> PassMeta {
    PassMeta {
        property_source: None,
        user_uniforms: std::collections::BTreeSet::new(),
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
fn material_user_bindings() {
    let package = crate::pkg::Package::parse(crate::tests::build_pkg(&[
        ("shaders/bound.vert", b"attribute vec3 a_Position;\nvoid main(){gl_Position=vec4(a_Position,1.0);}"),
        ("shaders/bound.frag", b"uniform vec3 g_Color; // {\"material\":\"color\"}\nuniform float g_Alpha; // {\"material\":\"alpha\"}\nuniform float g_Missing; // {\"material\":\"missing\"}\nvoid main(){gl_FragColor=vec4(g_Color*g_Missing,g_Alpha);}"),
    ])).unwrap();
    let material = serde_json::json!({"passes":[{"shader":"bound",
        "constantshadervalues":{"color":"1 1 1","alpha":0.25,"missing":0.75},
        "usershadervalues":{"Tint":"color","enabled":"alpha","absent":"missing"}}]});
    let assets = Assets::discover(Some("")).with_properties(BTreeMap::from([
        ("tint".into(), vec![0.25, 0.5, 1.0]),
        ("enabled".into(), vec![1.0]),
    ]));
    let passes =
        super::build_material(&package, &assets, &material, None, None, None, None).unwrap();
    assert_eq!(passes[0].values["g_Color"], [0.25, 0.5, 1.0]);
    assert_eq!(passes[0].values["g_Alpha"], [1.0]);
    assert_eq!(passes[0].values["g_Missing"], [0.75]);
    let overrides = serde_json::json!([{"constantshadervalues":{"color":"1 0 0"}}]);
    let passes =
        super::build_material(&package, &assets, &material, Some(&overrides), None, None, None)
            .unwrap();
    assert_eq!(passes[0].values["g_Color"], [0.25, 0.5, 1.0]);
    assert_eq!(
        super::property_dependencies(&material),
        std::collections::BTreeSet::from(["tint".into(), "enabled".into(), "absent".into(),])
    );
}

#[test]
fn bound_constants_survive_frames() {
    let mut meta = pass_meta(&["g_Value", "g_Other"]);
    meta.property_source = Some((0, 0));
    meta.uniforms[0].material = Some("amount".into());
    meta.uniforms[1].material = Some("other".into());
    meta.user_uniforms.insert("g_Value".into());
    meta.constants.insert("g_Value".into(), vec![0.75]);
    let object = serde_json::json!({"effects":[{"passes":[{
        "constantshadervalues":{"amount":0.25,"other":0.5}
    }]}]});
    meta.apply_object_values(&object, &BTreeMap::new());
    assert_eq!(meta.constants["g_Value"], [0.75]);
    assert_eq!(meta.constants["g_Other"], [0.5]);
    meta.apply_script_values(&serde_json::json!({"g_Value":0.875}), &BTreeMap::new());
    meta.apply_object_values(&object, &BTreeMap::new());
    assert_eq!(meta.constants["g_Value"], [0.875]);
}

#[test]
fn authored_aliases_are_exact() {
    let assets =
        Assets::discover(Some("")).with_properties(BTreeMap::from([("level".into(), vec![0.75])]));
    let uniforms = [("g_Value", Some("amount")), ("g_Plain", None)];
    for key in ["amount", "Amount", "g_Value", "Value", "value", "g_Plain"] {
        let expected = if key == "amount" { Some(vec![0.75]) } else { None };
        for pass in [
            serde_json::json!({"constantshadervalues":{key:0.75}}),
            serde_json::json!({"usershadervalues":{"level":key}}),
        ] {
            let values = super::material_values(&pass, None, &assets, &uniforms.into_iter());
            assert_eq!(values.get("g_Value"), expected.as_ref(), "{pass}");
            assert!(!values.contains_key("g_Plain"));
        }
    }
}

#[test]
fn user_binding_overrides() {
    let assets =
        Assets::discover(Some("")).with_properties(BTreeMap::from([("level".into(), vec![0.75])]));
    let uniforms = [("g_Value", Some("amount")), ("g_Other", Some("other"))];
    let pass = serde_json::json!({"usershadervalues":{"level":"amount"}});
    for (bindings, expected) in [
        (serde_json::json!({"level":"bad"}), None),
        (serde_json::json!({"level":"g_Value"}), None),
        (serde_json::json!({"level":"other"}), Some("g_Other")),
        (serde_json::json!({}), Some("g_Value")),
    ] {
        let overrides = serde_json::json!({"usershadervalues":bindings});
        let values =
            super::material_values(&pass, Some(&overrides), &assets, &uniforms.into_iter());
        let expected = expected.map(|name| (name.to_owned(), vec![0.75])).into_iter().collect();
        assert_eq!(values, expected);
    }
}

#[test]
fn runtime_uniforms_stay_explicit() {
    let mut meta = pass_meta(&["g_Value"]);
    meta.uniforms[0].material = Some("amount".into());
    meta.property_source = Some((0, 0));
    meta.constants.insert("g_Value".into(), vec![0.25]);
    for key in ["g_Value", "Amount", "Value"] {
        let object =
            serde_json::json!({"effects":[{"passes":[{"constantshadervalues":{key:0.75}}]}]});
        meta.apply_object_values(&object, &BTreeMap::new());
        assert_eq!(meta.constants["g_Value"], [0.25]);
    }
    meta.apply_script_values(&serde_json::json!({"g_Value":0.75}), &BTreeMap::new());
    assert_eq!(meta.constants["g_Value"], [0.75]);
    meta.apply_script_values(&serde_json::json!({"amount":0.5}), &BTreeMap::new());
    assert_eq!(meta.constants["g_Value"], [0.5]);
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
    let (effects, skipped) =
        super::load_effects(&package, &Assets::discover(Some("")), &object, false);
    assert_eq!(effects.len(), 1);
    assert_eq!(skipped.len(), 1);
    assert_eq!(effects[0].passes.len(), 2);
    assert_eq!(effects[0].passes[0].property_source, Some((2, 1)));
    assert_eq!(effects[0].passes[1].property_source, None);
    let (retained, _) = super::load_effects(&package, &Assets::discover(Some("")), &object, true);
    assert_eq!(retained.len(), 2);
    assert_eq!(retained[0].state.source, Some(0));
    assert!(!retained[0].state.visible);
    assert_eq!(retained[1].state.source, Some(2));
    assert!(retained[1].state.visible);
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
    assert_eq!(EffectBind::parse("_rt_MipMappedFrameBuffer", None), EffectBind::SceneHistory);
    assert_eq!(EffectBind::parse("_RT_MIPMAPPEDFRAMEBUFFER", None), EffectBind::SceneHistory);
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
        blend: crate::effects::PassBlend::Copy,
        property_source: None,
        user_uniforms: std::collections::BTreeSet::new(),
        name: "probe".into(),
        vertex,
        geometry: None,
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
fn upper_texture_metadata() {
    let source = "uniform vec4 g_Texture8Resolution;\nuniform vec4 g_Texture12Resolution;\nuniform vec4 g_Texture15Resolution;\nuniform vec4 g_Texture12Rotation;\nuniform vec2 g_Texture12Translation;\nvoid main() { gl_FragColor = g_Texture12Resolution; }";
    let translated =
        crate::shader::translate(source, crate::shader::Stage::Fragment, &BTreeMap::new());
    let mut resolutions = vec![None; 16];
    resolutions[8] = Some([512.0, 256.0, 480.0, 240.0]);
    let meta = PassMeta {
        property_source: None,
        user_uniforms: std::collections::BTreeSet::new(),
        name: "metadata".into(),
        uniforms: translated.uniforms,
        constants: BTreeMap::from([("g_Texture12Translation".into(), vec![0.25, 0.5])]),
        resolutions,
        ndc: true,
    };
    let mut slots = vec![None; 16];
    slots[12] = Some([128.0, 64.0, 120.0, 60.0]);
    for size in [[128.0, 64.0, 120.0, 60.0], [256.0, 128.0, 240.0, 120.0]] {
        slots[12] = Some(size);
        let bytes = meta.uniform_bytes(FrameClock::default(), None, 800, 600, (800, 600), &slots);
        for (name, expected) in [
            ("g_Texture8Resolution", &[512.0, 256.0, 480.0, 240.0][..]),
            ("g_Texture12Resolution", &size[..]),
            ("g_Texture15Resolution", &[800.0, 600.0, 800.0, 600.0][..]),
            ("g_Texture12Rotation", &[1.0, 0.0, 0.0, 1.0][..]),
            ("g_Texture12Translation", &[0.25, 0.5][..]),
        ] {
            let uniform = meta.uniforms.iter().find(|uniform| uniform.name == name).unwrap();
            let actual: Vec<_> = bytes[uniform.offset..]
                .chunks_exact(size_of::<f32>())
                .take(expected.len())
                .map(|word| f32::from_le_bytes(word.try_into().unwrap()))
                .collect();
            assert_eq!(actual, expected, "{name}");
        }
    }
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
        blend: crate::effects::PassBlend::Copy,
        property_source: None,
        user_uniforms: std::collections::BTreeSet::new(),
        name: "probe".into(),
        vertex,
        geometry: None,
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
        blend: crate::effects::PassBlend::Copy,
        property_source: None,
        user_uniforms: std::collections::BTreeSet::new(),
        name: "probe".into(),
        vertex,
        geometry: None,
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

#[test]
fn effect_visibility_sources() {
    let mut state = super::EffectState { source: Some(2), visible: true, available: true };
    for (visible, expected) in [
        (serde_json::json!(false), false),
        (serde_json::json!({"value":1}), true),
        (serde_json::json!("0"), false),
    ] {
        state.apply_object(
            &serde_json::json!({"effects":[{"visible":true},{"visible":true},{"visible":visible}]}),
            &BTreeMap::new(),
        );
        assert_eq!(state.visible, expected);
    }
    state.apply_object(&serde_json::json!({"effects":[{}, {}, {}]}), &BTreeMap::new());
    assert!(state.visible);
}

#[test]
fn hidden_loader_failures() {
    let package = crate::pkg::Package::parse(crate::tests::build_pkg(&[
        ("effects/empty.json", b"{}"),
        ("effects/material.json", br#"{"passes":[{"material":"materials/missing.json"}]}"#),
    ]))
    .unwrap();
    for file in ["effects/missing.json", "effects/empty.json", "effects/material.json"] {
        let object = serde_json::json!({"effects":[{"file":file,"visible":false}]});
        let (effects, skipped) =
            super::load_effects(&package, &Assets::discover(Some("")), &object, true);
        assert!(skipped.is_empty());
        assert_eq!(effects.len(), 1);
        assert!(!effects[0].state.available);
        assert!(!effects[0].state.visible);
        assert!(effects[0].passes.is_empty());
    }
}

#[test]
fn hidden_recoverable_warnings() {
    let package = crate::pkg::Package::parse(crate::tests::build_pkg(&[
        ("effects/probe.json", br#"{"fbos":[{"name":"cache","format":"unknown"}],"passes":[{"material":"materials/probe.json","target":"cache"},{"command":"swap","source":"cache"}]}"#),
        ("materials/probe.json", br#"{"passes":[{"shader":"probe"}]}"#),
        ("shaders/probe.vert", b"attribute vec3 a_Position;\nvoid main(){\ngl_Position=vec4(a_Position,1.0);\n}"),
        ("shaders/probe.frag", b"void main(){\ngl_FragColor=vec4(1.0);\n}"),
    ])).unwrap();
    for visible in [false, true] {
        let object =
            serde_json::json!({"effects":[{"file":"effects/probe.json","visible":visible}]});
        let (effects, skipped) =
            super::load_effects(&package, &Assets::discover(Some("")), &object, true);
        assert_eq!(effects.len(), 1);
        assert!(effects[0].state.available);
        assert_eq!(effects[0].state.visible, visible);
        assert_eq!(effects[0].passes.len(), 1);
        assert_eq!(effects[0].fbos.len(), 1);
        assert_eq!(effects[0].fbos[0].format, super::FboFormat::Rgba8);
        assert!(effects[0].swaps.is_empty());
        assert_eq!(skipped.len(), 2);
    }
}

#[test]
fn explicit_bind_scope() {
    let package = crate::pkg::Package::parse(crate::tests::build_pkg(&[
        ("effects/probe.json", br#"{"fbos":[{"name":"cache"}],"passes":[{"material":"materials/probe.json","bind":[{"index":0,"name":"_rt_FullFrameBuffer"},{"index":1,"name":"cache"},{"index":2,"name":"missing"}]}]}"#),
        ("materials/probe.json", br#"{"passes":[{"shader":"probe","textures":["_rt_FullFrameBuffer","","","_rt_FullFrameBuffer"]}]}"#),
        ("shaders/probe.vert", b"attribute vec3 a_Position;\nvoid main(){\ngl_Position=vec4(a_Position,1.0);\n}"),
        ("shaders/probe.frag", b"void main(){\ngl_FragColor=vec4(1.0);\n}"),
    ])).unwrap();
    let object = serde_json::json!({"effects":[{"file":"effects/probe.json"}]});
    let (effects, skipped) =
        super::load_effects(&package, &Assets::discover(Some("")), &object, false);
    assert!(skipped.is_empty());
    let binds = &effects[0].passes[0].binds;
    assert!(binds.contains(&(0, EffectBind::Previous)));
    assert!(binds.contains(&(1, EffectBind::Named("cache".into()))));
    assert!(binds.contains(&(2, EffectBind::Previous)));
    assert!(binds.contains(&(3, EffectBind::SceneSoFar)));
}

#[test]
fn fitted_source_dimensions() {
    let mut fbo = super::Fbo {
        name: "cache".into(),
        size: super::FboSize::Fit(128),
        format: super::FboFormat::Rgba8,
        repeat: None,
        clear: [0.0; 4],
        clear_on_setup: true,
        unique: true,
    };
    assert_eq!(fbo.extent((32, 32)), (32, 32));
    assert_eq!(fbo.extent((256, 128)), (128, 64));
    fbo.size = super::FboSize::Fit(16);
    assert_eq!(fbo.extent((31, 17)), (16, 8));
    assert_eq!(fbo.extent((17, 31)), (8, 16));
}

#[test]
fn projection_uniform_shape() {
    let mut meta = super::PassMeta {
        property_source: None,
        user_uniforms: std::collections::BTreeSet::new(),
        name: "projection".into(),
        uniforms: vec![crate::shader::Uniform {
            name: "g_ModelViewProjectionMatrix".into(),
            kind: crate::shader::UniformKind::Mat4,
            default: None,
            material: None,
            offset: 16,
            count: 1,
        }],
        constants: std::collections::BTreeMap::new(),
        resolutions: Vec::new(),
        ndc: false,
    };
    assert_eq!(meta.matrix_offset("g_ModelViewProjectionMatrix"), Some(16));
    meta.uniforms[0].kind = crate::shader::UniformKind::Vec4;
    assert_eq!(meta.matrix_offset("g_ModelViewProjectionMatrix"), None);
    meta.uniforms[0].kind = crate::shader::UniformKind::Mat4;
    meta.uniforms[0].count = 2;
    assert_eq!(meta.matrix_offset("g_ModelViewProjectionMatrix"), None);
}

#[test]
fn particle_scene_binds() {
    let package = crate::pkg::Package::parse(crate::tests::build_pkg(&[
        ("shaders/probe.vert", b"attribute vec3 a_Position;\nvoid main(){gl_Position=vec4(a_Position,1.0);}"),
        ("shaders/probe.frag", b"#if REFRACT\nuniform sampler2D g_Texture3;\n#endif\nvoid main(){\n#if REFRACT\ngl_FragColor=texture2D(g_Texture3,vec2(0.5));\n#else\ngl_FragColor=vec4(1.0);\n#endif\n}"),
    ])).unwrap();
    let material = serde_json::json!({"passes":[{"shader":"probe","textures":["","","","_rt_FullFrameBuffer"]}]});
    let assets = Assets::discover(Some("/nonexistent"));
    for active in [false, true] {
        let combos = BTreeMap::from([("REFRACT".into(), i64::from(active))]);
        let pass = super::particle_pass(&package, &assets, &material, &combos).unwrap();
        assert_eq!(pass.binds.contains(&(3, EffectBind::SceneSoFar)), active);
        assert_eq!(pass.fragment.samplers.iter().any(|sampler| sampler.index == 3), active);
    }
}

#[test]
fn reflection_binding() {
    for name in ["_rt_Reflection", "_RT_REFLECTION"] {
        assert_eq!(EffectBind::parse(name, None), EffectBind::Reflection);
    }
    assert_eq!(
        EffectBind::parse("_rt_ReflectionOther", None),
        EffectBind::Named("_rt_ReflectionOther".into())
    );
}

#[test]
fn authored_scale_truncates() {
    for (scale, canvas, expected) in [
        (1.0, (52, 211), (52, 211)),
        (2.0, (52, 211), (26, 105)),
        (2.0, (74, 210), (37, 105)),
        (3.0, (74, 211), (24, 70)),
        (1.5, (52, 211), (34, 140)),
        (3.0, (1, 2), (1, 1)),
    ] {
        let value = serde_json::json!({"name":"probe", "scale":scale, "format":"rgba_backbuffer"});
        let mut skipped = Vec::new();
        let mut fbo = super::parse_fbo(&value, &mut skipped, "probe").unwrap();
        assert!(skipped.is_empty());
        assert_eq!(fbo.extent(canvas), expected);
        fbo.size = super::FboSize::Scale(2.0);
        assert_eq!(fbo.extent((52, 211)), (26, 106));
    }
}

#[test]
fn frame_inputs_preserve_scheduling() {
    for (name, expected, frame) in [
        ("g_Time", [true, false, false], true),
        ("g_Frametime", [true, false, false], true),
        ("g_Daytime", [false, false, false], true),
        ("g_PointerPosition", [false, true, false], true),
        ("g_PointerPositionLast", [false, true, false], true),
        ("g_PointerState", [false, true, false], true),
        ("g_ParallaxPosition", [false, true, false], true),
        ("g_AudioSpectrum16Left", [false, false, true], true),
        ("g_AudioUnknown", [false, false, false], false),
        ("g_TimeDelta", [false, false, false], false),
        ("g_PointerPositionOffset", [false, false, false], false),
    ] {
        let meta = pass_meta(&[name]);
        assert_eq!(
            [meta.time_dependent(), meta.pointer_dependent(), meta.audio_dependent()],
            expected
        );
        assert_eq!(super::frame_input(name).is_some(), frame);
    }
}

#[test]
fn clear_function_admission() {
    let definition = serde_json::json!({
        "fbos": [{"name": "_rt_A"}, {"name": "_rt_B"}],
        "functions": {
            "clearProbe": {"action": "clear", "fbos": ["_rt_A", "_rt_B"]},
            "unsupported": {"action": "blur", "fbos": ["_rt_A"]},
            "foreign": {"action": "clear", "fbos": ["_rt_Elsewhere"]},
            "duplicate": {"action": "clear", "fbos": ["_rt_A", "_rt_A"]},
            "empty": {"action": "clear", "fbos": []}
        }
    });
    let functions = super::clear_functions(&definition);
    assert_eq!(functions.len(), 1);
    assert_eq!(functions["clearProbe"], vec!["_rt_A".to_owned(), "_rt_B".to_owned()]);
}

#[test]
#[ignore = "requires original Wallpaper Engine effect assets"]
fn fluid_effect_graph() {
    let root = std::env::var("SKWD_WE_ENGINE_ASSETS").unwrap();
    let package = crate::pkg::Package::parse(crate::tests::build_pkg(&[])).unwrap();
    let assets = Assets::discover(Some(&root));
    let object = serde_json::json!({"effects": [{"file": "effects/fluidsimulation/effect.json"}]});
    let (effects, skipped) = super::load_effects(&package, &assets, &object, false);
    assert!(skipped.is_empty(), "{skipped:?}");
    assert_eq!(effects.len(), 1);
    let effect = &effects[0];
    assert_eq!(effect.passes.len(), 17);
    assert_eq!(effect.fbos.len(), 8);
    assert_eq!(effect.swaps.len(), 2);
    assert_eq!(effect.swaps[0].after_pass, Some(16));
    assert_eq!(effect.swaps[1].after_pass, Some(16));
    assert_eq!(effect.clear_functions.len(), 2);
    assert!(effect.fbos.iter().all(|fbo| fbo.unique));
    assert_eq!(effect.fbos[0].format, super::FboFormat::Rg16f);
    assert_eq!(effect.fbos[2].format, super::FboFormat::R16f);
}

#[test]
fn local_effect_includes() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/fixtures/effects/local");
    let package = crate::pkg::Package::parse(crate::tests::build_pkg(&[])).unwrap();
    let assets = Assets::discover(root.to_str());
    let object = serde_json::json!({"effects": [{"file": "effects/nested/effect.json"}]});
    let (effects, skipped) = super::load_effects(&package, &assets, &object, false);
    assert!(skipped.is_empty(), "{skipped:?}");
    assert_eq!(effects.len(), 1);
    assert_eq!(effects[0].passes.len(), 1);
    assert!(effects[0].passes[0].fragment.source.contains("vec4 sharedColor()"));
}

#[test]
fn scoped_property_dependencies() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("shaders")).unwrap();
    let local = directory.path().join("effects/nested");
    std::fs::create_dir_all(local.join("materials")).unwrap();
    std::fs::write(local.join("effect.json"), "{}").unwrap();
    std::fs::write(
        local.join("materials/tint.json"),
        r#"{"constants":{"tint":{"user":"Opacity","value":1}}}"#,
    )
    .unwrap();
    let assets = Assets::discover(directory.path().to_str());
    let scoped = assets.with_effect("effects/nested/effect.json");
    assert!(scoped.read("materials/tint.json").is_some());
    assert_eq!(
        assets.property_dependencies().unwrap().into_iter().collect::<Vec<_>>(),
        ["opacity"]
    );
    std::fs::write(local.join("materials/tint.json"), "{broken").unwrap();
    assert!(scoped.read("materials/tint.json").is_some());
    assert_eq!(assets.property_dependencies(), None);
}

#[test]
fn texture_component_combos() {
    let fragment = b"uniform sampler2D g_Texture2; // {\"components\":[{\"combo\":\"C0\"},{\"combo\":\"C1\"},{\"combo\":\"C2\"},{\"combo\":\"C3\"}]}\n#if C0\nuniform float g_C0;\n#endif\n#if C1\nuniform float g_C1;\n#endif\n#if C2\nuniform float g_C2;\n#endif\n#if C3\nuniform float g_C3;\n#endif\nvoid main(){gl_FragColor=vec4(1.0);}";
    for (slot, name) in [(0, "mask"), (0, "mask.tex"), (0, "materials/mask.tex"), (2, "mask")] {
        let fragment = String::from_utf8(fragment.to_vec())
            .unwrap()
            .replace("g_Texture2", &format!("g_Texture{slot}"));
        for (mask, authored) in [(0, 1), (1, 0), (2, 1), (4, 0), (8, 0), (15, 0)] {
            let texture =
                crate::tests::build_tex("TEXB0001", 0, mask << 20, &[255; 64], false, None);
            let decoded = crate::model::load_texture_bytes(&texture).unwrap();
            assert_eq!(decoded.component_mask, mask as u8);
            let package = crate::pkg::Package::parse(crate::tests::build_pkg(&[
                ("materials/mask.tex", &texture),
                (
                    "shaders/mask.vert",
                    b"attribute vec3 a_Position;\nvoid main(){gl_Position=vec4(a_Position,1.0);}",
                ),
                ("shaders/mask.frag", fragment.as_bytes()),
            ]))
            .unwrap();
            let mut slots = vec![serde_json::Value::Null; slot + 1];
            slots[slot] = serde_json::json!({"name":name,"type":"texture"});
            let material = serde_json::json!({"passes":[{"shader":"mask","textures":slots,"combos":{"C0":authored}}]});
            let passes = super::build_material(
                &package,
                &Assets::discover(Some("")),
                &material,
                None,
                None,
                None,
                None,
            )
            .unwrap();
            for component in 0..4 {
                let expected = mask & (1 << component) != 0 || (component == 0 && authored != 0);
                assert_eq!(
                    passes[0]
                        .fragment
                        .uniforms
                        .iter()
                        .any(|uniform| uniform.name == format!("g_C{component}")),
                    expected
                );
            }
        }
    }
}

#[test]
fn texture_asset_aliases() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("shaders")).unwrap();
    std::fs::create_dir(directory.path().join("materials")).unwrap();
    let bytes = crate::tests::build_tex("TEXB0001", 0, 3 << 20, &[255; 64], false, None);
    std::fs::write(directory.path().join("materials/mask.tex"), bytes).unwrap();
    let package = crate::pkg::Package::parse(crate::tests::build_pkg(&[])).unwrap();
    let assets = Assets::discover(directory.path().to_str());
    for name in ["mask", "mask.tex", "materials/mask.tex"] {
        let texture = super::texture_slot(&package, &assets, Some(name)).unwrap();
        assert_eq!(texture.component_mask, 3);
        assert_eq!(super::texture_components(&package, &assets, name), Some(3));
    }
}

#[test]
fn vertex_component_combos() {
    let texture = crate::tests::build_tex("TEXB0001", 0, 1 << 20, &[255; 64], false, None);
    let package = crate::pkg::Package::parse(crate::tests::build_pkg(&[
        ("materials/mask.tex", &texture),
        ("shaders/mask.vert", b"uniform sampler2D g_Texture0; // {\"components\":[{\"combo\":\"C0\"}]}\nattribute vec3 a_Position;\nvoid main(){gl_Position=vec4(a_Position,1.0);}"),
        ("shaders/mask.frag", b"#if C0\nuniform float g_Component;\n#endif\nvoid main(){gl_FragColor=vec4(1.0);}"),
    ])).unwrap();
    let material = serde_json::json!({"passes":[{"shader":"mask","textures":["mask"]}]});
    for (input, expected) in [(None, true), (Some(0), false), (Some(1), true)] {
        let input =
            input.map(|component_mask| super::MaterialInput { bones: None, component_mask });
        let passes = super::build_material(
            &package,
            &Assets::discover(Some("")),
            &material,
            None,
            None,
            None,
            input,
        )
        .unwrap();
        assert_eq!(
            passes[0].fragment.uniforms.iter().any(|uniform| uniform.name == "g_Component"),
            expected
        );
    }
}

#[test]
fn fbo_format_names() {
    for (name, format) in [
        ("rgb888", super::FboFormat::Rgba8),
        ("rgb161616f", super::FboFormat::Rgba16f),
        ("rgba16161616", super::FboFormat::Rgba16),
        ("rgb161616", super::FboFormat::Rgba16),
    ] {
        assert_eq!(super::FboFormat::parse(name), Some(format), "{name}");
    }
    assert_eq!(super::FboFormat::Rgba16.bytes_per_pixel(), 8);
}

#[test]
fn unguarded_default_binds() {
    let declare = "uniform sampler2D g_Texture1; // {\"combo\":\"MASK\",\"default\":\"mask\"}\n";
    let read = "gl_FragColor = texSample2D(g_Texture1, vec2(0.5, 0.5));";
    for (body, bound) in [
        (format!("void main(){{ {read} }}"), true),
        (format!("void main(){{\n#if MASK\n{read}\n#endif\n}}"), false),
    ] {
        let fragment = format!("{declare}{body}");
        let texture = crate::tests::build_tex("TEXB0001", 0, 0, &[255; 64], false, None);
        let package = crate::pkg::Package::parse(crate::tests::build_pkg(&[
            ("materials/mask.tex", &texture),
            (
                "shaders/mask.vert",
                b"attribute vec3 a_Position;\nvoid main(){gl_Position=vec4(a_Position,1.0);}",
            ),
            ("shaders/mask.frag", fragment.as_bytes()),
        ]))
        .unwrap();
        let material = serde_json::json!({"passes":[{"shader":"mask"}]});
        let passes = super::build_material(
            &package,
            &Assets::discover(Some("")),
            &material,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        assert_eq!(passes[0].textures.get(1).is_some_and(Option::is_some), bound);
    }
}
