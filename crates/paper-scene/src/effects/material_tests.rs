use super::*;
use crate::tests::{build_pkg, build_tex};
use serde_json::json;

fn package(passes: &Value) -> Package {
    let material = json!({"passes":passes}).to_string();
    let texture_a = build_tex("TEXB0001", 0, 0, &[255; 64], false, None);
    let texture_b = build_tex("TEXB0001", 0, 0, &[0; 64], false, None);
    Package::parse(build_pkg(&[
        ("scene.json", br#"{"objects":[{"id":1,"image":"models/image.json","size":"32 32"}]}"#),
        ("models/image.json", br#"{"material":"materials/image.json"}"#),
        ("materials/image.json", material.as_bytes()),
        ("materials/a.tex", &texture_a),
        ("materials/b.tex", &texture_b),
        ("shaders/custom.vert", b"uniform mat4 g_ModelViewProjectionMatrix;\nattribute vec3 a_Position;\nvoid main() { gl_Position = mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix); }\n"),
        ("shaders/custom.frag", b"uniform sampler2D g_Texture0;\nuniform sampler2D g_Texture1;\nuniform vec3 g_Tint; // {\"material\":\"Tint\"}\nvoid main() { gl_FragColor=texture2D(g_Texture1,vec2(0.5,0.5))*vec4(g_Tint,1.0); }\n"),
        ("shaders/mix.vert", b"uniform mat4 g_ModelViewProjectionMatrix;\nattribute vec3 a_Position;\nvoid main() { gl_Position = mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix); }\n"),
        ("shaders/mix.frag", b"uniform sampler2D g_Texture0;\nuniform sampler2D g_Texture1;\nvoid main() { gl_FragColor=mix(texture2D(g_Texture0,vec2(0.5)),texture2D(g_Texture1,vec2(0.5)),0.8); }\n"),
    ])).unwrap()
}

#[test]
fn history_base_input() {
    let package =
        package(&json!([{"shader":"custom","textures":["_rt_MipMappedFrameBuffer","b"]}]));
    let model = crate::model::load_with(&package, &Assets::discover(Some("/nonexistent"))).unwrap();
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    let layer = &model.layers[0];
    assert_eq!(layer.size, (32.0, 32.0));
    let material = layer.material.as_ref().unwrap();
    assert!(material.pass.binds.contains(&(0, super::super::EffectBind::SceneHistory)));
    assert!(material.pass.textures[0].is_none());
    assert!(material.pass.textures[1].is_some());
}

#[test]
fn first_base_pass_only() {
    let package = package(&json!([
        {"shader":"custom","textures":["a","b"],"constantshadervalues":{"Tint":"0.25 0.5 0.75"}},
        {"shader":"mix","textures":["a","a"],"blending":"additive"}
    ]));
    let assets = Assets::discover(Some("/nonexistent"));
    let model = crate::model::load_with(&package, &assets).unwrap();
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    let material = model.layers[0].material.as_ref().unwrap();
    assert_eq!(material.pass.textures.len(), 2);
    assert!(material.pass.textures[1].is_some());
    assert_eq!(material.pass.name, "custom");
    let meta = super::super::PassMeta::of(&material.pass);
    let bytes =
        meta.uniform_bytes(super::super::FrameClock::default(), None, 32, 32, (32, 32), &[]);
    let uniform =
        material.pass.fragment.uniforms.iter().find(|uniform| uniform.name == "g_Tint").unwrap();
    let expected: Vec<_> = [0.25_f32, 0.5, 0.75].into_iter().flat_map(f32::to_ne_bytes).collect();
    assert_eq!(&bytes[uniform.offset..uniform.offset + expected.len()], expected);
}

#[test]
fn two_sampler_base() {
    let package = package(&json!([{"shader":"mix","textures":["a","b"]}]));
    let model = crate::model::load_with(&package, &Assets::discover(Some("/nonexistent"))).unwrap();
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    let layer = &model.layers[0];
    let pass = &layer.material.as_ref().unwrap().pass;
    assert_eq!(
        pass.fragment.samplers.iter().map(|sampler| sampler.index).collect::<Vec<_>>(),
        [0, 1]
    );
    assert_ne!(layer.texture.pixels, pass.textures[1].as_ref().unwrap().pixels);
}

#[test]
fn ignores_missing_second_pass() {
    let package = package(&json!([
        {"shader":"custom","textures":["a","b"]},
        {"shader":"missing","textures":["a","b"]}
    ]));
    let model = crate::model::load_with(&package, &Assets::discover(Some("/nonexistent"))).unwrap();
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    assert_eq!(model.layers[0].material.as_ref().unwrap().pass.name, "custom");
}

#[test]
fn ignores_second_source() {
    let package = package(&json!([
        {"shader":"custom","textures":["a","b"]},
        {"shader":"mix","textures":["b","a"]}
    ]));
    let model = crate::model::load_with(&package, &Assets::discover(Some("/nonexistent"))).unwrap();
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    assert_eq!(model.layers[0].texture_name.as_deref(), Some("a"));
}

#[test]
fn ignores_extra_passes() {
    let package = package(&json!([
        {"shader":"custom","textures":["a","b"]},
        {"shader":"missing","textures":["a","b"]},
        null,
        "malformed"
    ]));
    let model = crate::model::load_with(&package, &Assets::discover(Some("/nonexistent"))).unwrap();
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    assert_eq!(model.layers[0].material.as_ref().unwrap().pass.name, "custom");
}

#[test]
fn mesh_material_textures() {
    let package = package(&json!([{"shader":"custom","textures":["a","b"]}]));
    let assets = Assets::discover(Some("/nonexistent"));
    let material = load_mesh(&package, &assets, "materials/image.json", None).unwrap();
    assert!(material.pass.textures[0].is_some());
    assert!(material.pass.textures[1].is_some());
    assert_eq!(material.pass.fragment.samplers.len(), 2);
}

#[test]
fn inactive_scene_bind() {
    let assets = Assets::discover(Some("/nonexistent"));
    for binding in ["_rt_Reflection", "_rt_MipMappedFrameBuffer"] {
        let inactive = package(&json!([{
            "shader":"custom",
            "textures":["a","b",binding]
        }]));
        let material = load_mesh(&inactive, &assets, "materials/image.json", None).unwrap();
        assert!(material.pass.binds.is_empty());
        let model = crate::model::load_with(&inactive, &assets).unwrap();
        assert!(model.skipped.is_empty(), "{:?}", model.skipped);
        assert!(model.layers[0].material.as_ref().unwrap().pass.binds.is_empty());
    }
    let active = package(&json!([{
        "shader":"custom",
        "textures":["a","_rt_Reflection"]
    }]));
    let material = load_mesh(&active, &assets, "materials/image.json", None).unwrap();
    assert_eq!(material.pass.binds, [(1, super::super::EffectBind::Reflection)]);
    let model = crate::model::load_with(&active, &assets).unwrap();
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    assert!(!model.reflection.bound);
    let pass = &model.layers[0].material.as_ref().unwrap().pass;
    assert!(pass.binds.is_empty());
    let fallback = pass.textures[1].as_ref().unwrap();
    assert_eq!(fallback.pixels, crate::model::error_texture().pixels);
}

#[test]
fn ordinary_base_fastpath() {
    for pass in [
        json!({"textures":["a"]}),
        json!({"shader":"genericimage4","textures":["a"]}),
        json!({"shader":"genericimage2","textures":["a"],"combos":{"VERSION":2},"blending":"translucent"}),
    ] {
        let package = package(&json!([pass]));
        let assets = Assets::discover(Some("/nonexistent"));
        let model = crate::model::load_with(&package, &assets).unwrap();
        assert!(model.skipped.is_empty(), "{:?}", model.skipped);
        assert!(model.layers[0].material.is_none());
    }
}

#[test]
#[ignore = "requires Wallpaper Engine assets"]
fn image_shaders_authored() {
    let assets = Assets::discover(Some("/nonexistent"));
    for pass in [
        json!({"shader":"genericimage3","textures":["a"],"combos":{"VERSION":2}}),
        json!({"shader":"genericimage4","textures":["a"],"combos":{"LIGHTING":0,"REFLECTION":0}}),
        json!({"shader":"genericimage3","textures":["a"],"combos":{"LIGHTING":1}}),
    ] {
        let model = crate::model::load_with(&package(&json!([pass])), &assets).unwrap();
        assert!(model.skipped.is_empty(), "{:?}", model.skipped);
        assert!(model.layers[0].material.is_some(), "{pass}");
    }
}

#[test]
fn image_depth_state() {
    let assets = Assets::discover(Some("/nonexistent"));
    for (test, write, cull) in [(true, true, true), (true, false, true), (false, true, false)] {
        let pass = json!({
            "shader":"custom",
            "textures":["a","b"],
            "depthtest":if test { ENABLED } else { DISABLED },
            "depthwrite":if write { ENABLED } else { DISABLED },
            "cullmode":if cull { NORMAL_CULL } else { NO_CULL }
        });
        let model = crate::model::load_with(&package(&json!([pass])), &assets).unwrap();
        assert!(model.skipped.is_empty(), "{:?}", model.skipped);
        let state = model.layers[0].material.as_ref().unwrap().state;
        assert_eq!(
            (
                state.test == DepthMode::Enabled,
                state.write == DepthMode::Enabled,
                state.cull == CullMode::Normal,
            ),
            (test, write, cull)
        );
        assert_eq!(state.has_depth(), test || write);
    }
}

#[test]
fn image_state_fastpath() {
    let directory = tempfile::tempdir().unwrap();
    let shaders = directory.path().join("shaders");
    std::fs::create_dir(&shaders).unwrap();
    std::fs::write(shaders.join("genericimage2.vert"), b"uniform mat4 g_ModelViewProjectionMatrix;\nattribute vec3 a_Position;\nvoid main() { gl_Position = mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix); }\n").unwrap();
    std::fs::write(shaders.join("genericimage2.frag"), b"uniform sampler2D g_Texture0;\nvoid main() { gl_FragColor=texture2D(g_Texture0,vec2(0.5)); }\n").unwrap();
    let assets = Assets::discover(directory.path().to_str());
    let canonical = std::fs::canonicalize(directory.path()).unwrap();
    assert_eq!(assets.root.as_deref(), Some(canonical.as_path()));
    for (test, write, cull) in [
        (ENABLED, ENABLED, NORMAL_CULL),
        (DISABLED, DISABLED, NORMAL_CULL),
        (ENABLED, ENABLED, NO_CULL),
        (ENABLED, DISABLED, NO_CULL),
    ] {
        let package = package(&json!([{
            "shader":"genericimage2",
            "textures":["a"],
            "depthtest":test,
            "depthwrite":write,
            "cullmode":cull
        }]));
        let model = crate::model::load_with(&package, &assets).unwrap();
        assert!(model.skipped.is_empty(), "{:?}", model.skipped);
        let state = model.layers[0].material.as_ref().unwrap().state;
        assert_eq!(state.test == DepthMode::Enabled, test == ENABLED);
        assert_eq!(state.write == DepthMode::Enabled, write == ENABLED);
        assert_eq!(state.cull == CullMode::Normal, cull == NORMAL_CULL);
    }
}

#[test]
fn ignores_second_state() {
    let package = package(&json!([
        {"shader":"custom","textures":["a","b"],"depthtest":"enabled"},
        {"shader":"mix","textures":["a","b"],"depthtest":"disabled"}
    ]));
    let model = crate::model::load_with(&package, &Assets::discover(Some("/nonexistent"))).unwrap();
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    assert!(model.layers[0].material.as_ref().unwrap().state.test == DepthMode::Enabled);
}

#[test]
fn unknown_image_state() {
    let assets = Assets::discover(Some("/nonexistent"));
    for (field, value) in [
        (DEPTH_STATES[0], json!("reversed")),
        (DEPTH_STATES[1], json!(true)),
        (CULL, json!("front")),
    ] {
        let pass = json!({"shader":"custom","textures":["a","b"],(field):value});
        let model = crate::model::load_with(&package(&json!([pass])), &assets).unwrap();
        assert!(model.layers.is_empty());
        assert!(model.skipped[0].contains("unsupported"));
    }
}

#[test]
fn implicit_shader_limits() {
    for pass in [
        json!({"textures":["a"],"blending":"additive"}),
        json!({"textures":["a","b"]}),
        json!({"textures":["a"],"combos":{"LIGHTING":1}}),
        json!({"textures":["a"],"usershadervalues":{"g_Tint":{"user":"color"}}}),
    ] {
        let package = package(&json!([pass]));
        let assets = Assets::discover(Some("/nonexistent"));
        let model = crate::model::load_with(&package, &assets).unwrap();
        assert!(model.layers.is_empty());
        assert!(model.skipped[0].contains("shader unavailable"));
    }
}

#[test]
fn base_shader_failure() {
    for shader in [json!("missing"), json!({"user":"undeclared"}), json!(false), Value::Null] {
        let package = package(&json!([{"shader":shader,"textures":["a"]}]));
        let assets = Assets::discover(Some("/nonexistent"));
        let model = crate::model::load_with(&package, &assets).unwrap();
        assert!(model.layers.is_empty());
        assert!(model.skipped[0].contains("shader unavailable"));
    }
}

#[test]
fn sampler_zero_only() {
    let package = package(&json!([{"shader":"custom","textures":[null,"a"]},{"textures":["a"]}]));
    let assets = Assets::discover(Some("/nonexistent"));
    let model = crate::model::load_with(&package, &assets).unwrap();
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    let layer = &model.layers[0];
    assert_eq!((layer.texture_name.as_deref(), layer.texture.width), (None, 40));
}

#[test]
fn authored_blending() {
    for (name, expected) in [
        ("normal", PassBlend::Copy),
        ("alphatocoverage", PassBlend::AlphaCoverage),
        ("translucent", PassBlend::Alpha),
        ("additive", PassBlend::Add),
        ("screen", PassBlend::Screen),
    ] {
        let package = package(&json!([{"shader":"custom","textures":["a","b"],"blending":name}]));
        let model =
            crate::model::load_with(&package, &Assets::discover(Some("/nonexistent"))).unwrap();
        let material = model.layers[0].material.as_ref().unwrap();
        assert_eq!(material.blend, expected);
        assert_eq!(material.pass.blend, expected);
    }
    assert_eq!(PassBlend::parse(None), Some(PassBlend::Copy));
    assert_eq!(PassBlend::parse(Some("unknown")), None);
}

#[test]
fn base_keeps_padding() {
    let texture = build_tex("TEXB0001", 0, 0, &[255; 64], false, None);
    let package = Package::parse(build_pkg(&[
        ("scene.json", br#"{"objects":[{"image":"models/a.json","effects":[{"file":"effects/a.json"}]}]}"#),
        ("models/a.json", br#"{"material":"materials/a.json"}"#),
        ("materials/a.json", br#"{"passes":[{"shader":"custom","textures":["a"]}]}"#),
        ("materials/a.tex", &texture),
        ("effects/a.json", br#"{"passes":[{"material":"materials/a.json"}]}"#),
        ("shaders/custom.vert", b"attribute vec3 a_Position;\nuniform mat4 g_ModelViewProjectionMatrix;\nvoid main(){\ngl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);\n}\n"),
        ("shaders/custom.frag", b"uniform sampler2D g_Texture0;\nvoid main(){\ngl_FragColor=texSample2D(g_Texture0,vec2(0.5,0.5));\n}\n"),
    ])).unwrap();
    let model = crate::model::load_with(&package, &Assets::discover(Some("/nonexistent"))).unwrap();
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    assert!(model.layers[0].material.is_some());
    assert!(!model.layers[0].effects.is_empty());
    assert!(model.layers[0].pad_uv);
}

fn optional_package(fragment: &[u8], pass: &Value) -> Package {
    let material = json!({"passes":[pass]}).to_string();
    let texture = build_tex("TEXB0001", 0, 0, &[255; 64], false, None);
    let blank = build_tex("TEXB0001", 0, 0, &[0; 64], false, None);
    Package::parse(build_pkg(&[
        ("materials/image.json", material.as_bytes()),
        ("materials/a.tex", &texture),
        ("materials/b.tex", &texture),
        ("materials/blank.tex", &blank),
        (
            "shaders/custom.vert",
            b"attribute vec3 a_Position;
void main() { gl_Position=vec4(a_Position,1.0); }
",
        ),
        ("shaders/custom.frag", fragment),
    ]))
    .unwrap()
}

#[test]
fn absent_combo_sampler() {
    let fragment = br#"uniform sampler2D g_Texture0;
uniform sampler2D g_Texture1; // {"combo":"NORMALMAP"}
uniform sampler2D g_Texture2; // {"combo":"PBRMASKS"}
uniform sampler2D g_Texture3; // {"default":"b"}
void main() { gl_FragColor=texSample2D(g_Texture0,vec2(0.5)); }
"#;
    let package = optional_package(fragment, &json!({"shader":"custom","textures":["a"]}));
    let assets = Assets::discover(Some("/nonexistent"));
    let material = load_mesh(&package, &assets, "materials/image.json", None).unwrap();
    let samplers: Vec<_> =
        material.pass.fragment.samplers.iter().map(|sampler| sampler.index).collect();
    assert_eq!(samplers, [0, 3]);
    assert!(material.pass.textures[3].is_some());
    assert!(!material.pass.fragment.source.contains("uniform sampler2D g_Texture1;"));
    assert!(!material.pass.fragment.source.contains("uniform sampler2D g_Texture2;"));
    let hlsl = material.pass.hlsl.as_ref().unwrap();
    assert!(!hlsl.fragment.contains("Texture2D g_Texture1"));
    assert!(!hlsl.fragment.contains("Texture2D g_Texture2"));
}

#[test]
fn live_combo_sampler() {
    let fragment = br#"uniform sampler2D g_Texture0;
uniform sampler2D g_Texture1; // {"combo":"NORMALMAP"}
void main() { gl_FragColor=texSample2D(g_Texture0,vec2(0.5));
#if NORMALMAP
 gl_FragColor+=texSample2D(g_Texture1,vec2(0.5));
#endif
}
"#;
    let assets = Assets::discover(Some("/nonexistent"));
    let missing = optional_package(
        fragment,
        &json!({"shader":"custom","combos":{"NORMALMAP":1},"textures":["a"]}),
    );
    assert_eq!(
        load_mesh(&missing, &assets, "materials/image.json", None).err().as_deref(),
        Some("mesh material materials/image.json sampler texture missing")
    );
    let present = optional_package(fragment, &json!({"shader":"custom","textures":["a","b"]}));
    let material = load_mesh(&present, &assets, "materials/image.json", None).unwrap();
    assert!(material.pass.fragment.samplers.iter().any(|sampler| sampler.index == 1));
}

#[test]
fn unset_albedo_defaults() {
    let fragment = br#"uniform sampler2D g_Texture0; // {"default":"blank"}
void main() { gl_FragColor=texSample2D(g_Texture0,vec2(0.5)); }
"#;
    let assets = Assets::discover(Some("/nonexistent"));
    for textures in [json!([null]), json!([]), json!([""]), json!(["models/absent"])] {
        let package =
            optional_package(fragment, &json!({"shader":"custom","textures":textures.clone()}));
        let material = load_mesh(&package, &assets, "materials/image.json", None)
            .unwrap_or_else(|err| panic!("{textures}: {err}"));
        let albedo = material.pass.textures[0].as_ref().unwrap();
        assert!(albedo.pixels.levels[0].data.iter().all(|&byte| byte == 0), "{textures}");
    }
}

#[test]
fn absent_slot_defaults() {
    let fragment = br#"uniform sampler2D g_Texture0;
uniform sampler2D g_Texture1; // {"default":"blank"}
void main() { gl_FragColor=texSample2D(g_Texture0,vec2(0.5))*texSample2D(g_Texture1,vec2(0.5)); }
"#;
    let package =
        optional_package(fragment, &json!({"shader":"custom","textures":["a","models/absent"]}));
    let assets = Assets::discover(Some("/nonexistent"));
    let material = load_mesh(&package, &assets, "materials/image.json", None).unwrap();
    let slot = material.pass.textures[1].as_ref().unwrap();
    assert!(slot.pixels.levels[0].data.iter().all(|&byte| byte == 0));
}

#[test]
fn absent_normal_unset() {
    let fragment = br#"uniform sampler2D g_Texture0;
uniform sampler2D g_Texture1; // {"combo":"NORMALMAP"}
void main() { gl_FragColor=texSample2D(g_Texture0,vec2(0.5));
#if NORMALMAP
 gl_FragColor+=texSample2D(g_Texture1,vec2(0.5));
#endif
}
"#;
    let package =
        optional_package(fragment, &json!({"shader":"custom","textures":["a","models/absent"]}));
    let assets = Assets::discover(Some("/nonexistent"));
    let material = load_mesh(&package, &assets, "materials/image.json", None).unwrap();
    assert!(material.pass.fragment.samplers.iter().all(|sampler| sampler.index != 1));
}

#[test]
fn used_combo_sampler() {
    let fragment = br#"uniform sampler2D g_Texture0;
uniform sampler2D g_Texture1; // {"combo":"NORMALMAP"}
void main() { gl_FragColor=texSample2D(g_Texture1,vec2(0.5)); }
"#;
    let package = optional_package(fragment, &json!({"shader":"custom","textures":["a"]}));
    let assets = Assets::discover(Some("/nonexistent"));
    assert_eq!(
        load_mesh(&package, &assets, "materials/image.json", None).err().as_deref(),
        Some("mesh material materials/image.json sampler texture missing")
    );
}

#[test]
fn hlsl_only_sampler() {
    let fragment = br#"uniform sampler2D g_Texture0;
uniform sampler2D g_Texture1; // {"combo":"OPTION"}
#if HLSL
void main() { gl_FragColor=texSample2D(g_Texture1,vec2(0.5)); }
#else
void main() { gl_FragColor=texSample2D(g_Texture0,vec2(0.5)); }
#endif
"#;
    let package = optional_package(fragment, &json!({"shader":"custom","textures":["a"]}));
    let assets = Assets::discover(Some("/nonexistent"));
    assert_eq!(
        load_mesh(&package, &assets, "materials/image.json", None).err().as_deref(),
        Some("mesh material materials/image.json sampler texture missing")
    );
}

#[test]
fn framebuffer_base_routes_first() {
    let white = build_tex("TEXB0001", 0, 0, &[255; 64], false, None);
    let package = Package::parse(build_pkg(&[
        ("scene.json", br#"{"objects":[{"image":"models/overlay.json","size":"8 8","effects":[{"file":"effects/tint.json"}]}]}"#),
        ("models/overlay.json", br#"{"material":"materials/overlay.json"}"#),
        ("materials/overlay.json", br#"{"passes":[{"shader":"sample","textures":["_rt_FullFrameBuffer"],"blending":"normal"}]}"#),
        ("effects/tint.json", br#"{"passes":[{"material":"materials/tint.json"}]}"#),
        ("materials/tint.json", br#"{"passes":[{"shader":"flat","textures":["util/white"]}]}"#),
        ("materials/util/white.tex", &white),
        ("shaders/flat.vert", b"uniform mat4 g_ModelViewProjectionMatrix;\nattribute vec3 a_Position;\nvoid main(){gl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);}"),
        ("shaders/flat.frag", b"uniform sampler2D g_Texture0;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,vec2(0.5,0.5));}"),
        ("shaders/sample.vert", b"uniform mat4 g_ModelViewProjectionMatrix;\nattribute vec3 a_Position;\nvoid main(){gl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);}"),
        ("shaders/sample.frag", b"uniform sampler2D g_Texture0;\nuniform vec4 g_Color4;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,vec2(0.5,0.5))*g_Color4;}"),
    ])).unwrap();
    let model = crate::model::load_with(&package, &Assets::discover(Some("/nonexistent"))).unwrap();
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    let layer = &model.layers[0];
    assert!(layer.material.is_none());
    assert_eq!(layer.effects.len(), 2);
    assert_eq!(layer.effects[0].passes[0].binds, vec![(0, super::super::EffectBind::SceneSoFar)]);
    assert!(layer.base_scene_input);
    let mut meta = super::super::PassMeta::of(&layer.effects[0].passes[0]);
    meta.set_source_tint([0.25, 0.5, 0.75, 0.6]);
    let bytes = meta.uniform_bytes(super::super::FrameClock::default(), None, 8, 8, (8, 8), &[]);
    let uniform = layer.effects[0].passes[0]
        .fragment
        .uniforms
        .iter()
        .find(|uniform| uniform.name == super::super::SOURCE_COLOR_UNIFORM)
        .unwrap();
    let expected: Vec<_> =
        [0.25_f32, 0.5, 0.75, 0.6].into_iter().flat_map(f32::to_ne_bytes).collect();
    assert_eq!(&bytes[uniform.offset..uniform.offset + expected.len()], expected);
}

#[test]
fn missing_sampler_is_strict() {
    let package = Package::parse(build_pkg(&[
        ("scene.json", br#"{"objects":[{"image":"models/overlay.json","size":"8 8"}]}"#),
        ("models/overlay.json", br#"{"material":"materials/overlay.json"}"#),
        ("materials/overlay.json", br#"{"passes":[{"shader":"sample","textures":["_rt_FullFrameBuffer",null]}]}"#),
        ("shaders/sample.vert", b"uniform mat4 g_ModelViewProjectionMatrix;\nattribute vec3 a_Position;\nvoid main(){gl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);}"),
        ("shaders/sample.frag", b"uniform sampler2D g_Texture0;\nuniform sampler2D g_Texture1;\nvoid main(){gl_FragColor=texSample2D(g_Texture1,vec2(0.5,0.5));}"),
    ])).unwrap();
    let model = crate::model::load_with(&package, &Assets::discover(Some("/nonexistent"))).unwrap();
    assert!(model.layers.is_empty());
    assert!(model.skipped[0].contains("sampler texture missing"));
}

#[test]
fn missing_shader_is_strict() {
    let package = Package::parse(build_pkg(&[
        ("scene.json", br#"{"objects":[{"image":"models/overlay.json","size":"8 8"}]}"#),
        ("models/overlay.json", br#"{"material":"materials/overlay.json"}"#),
        ("materials/overlay.json", br#"{"passes":[{"shader":"genericimage","textures":["_rt_FullFrameBuffer"],"blending":"normal"}]}"#),
    ])).unwrap();
    let mut assets = Assets::discover(Some("/nonexistent"));
    assets.root = None;
    let model = crate::model::load_with(&package, &assets).unwrap();
    assert!(model.layers.is_empty());
    assert!(
        model.skipped[0].contains("base material materials/overlay.json shader unavailable"),
        "{:?}",
        model.skipped
    );
}

#[test]
fn textured_binding_is_strict() {
    let package = package(&json!([{"shader":"custom","textures":["a","_rt_FullFrameBuffer"]}]));
    let model = crate::model::load_with(&package, &Assets::discover(Some("/nonexistent"))).unwrap();
    assert!(model.layers.is_empty());
    assert!(model.skipped[0].contains("render target inputs unsupported"));
}

#[test]
fn scene_blend_admission() {
    for (blend, post, color_blend, pass_count, admitted) in [
        ("normal", false, 0, 1, true),
        ("translucent", false, 0, 1, true),
        ("additive", false, 0, 1, true),
        ("screen", false, 0, 1, false),
        ("translucent", false, 0, 2, true),
        ("additive", false, 0, 2, true),
        ("translucent", true, 0, 1, false),
        ("additive", true, 0, 1, false),
        ("translucent", false, 6, 1, false),
        ("additive", false, 8, 1, false),
    ] {
        let mut object = json!({"image":"models/overlay.json","size":"8 8",
            "colorBlendMode":color_blend});
        if post {
            object["effects"] = json!([{"file":"effects/tint.json"}]);
        }
        let scene = json!({"objects":[object]}).to_string();
        let pass = json!({"shader":"sample", "textures":["_rt_FullFrameBuffer"],"blending":blend});
        let material = json!({"passes":vec![pass; pass_count]}).to_string();
        let package = Package::parse(build_pkg(&[
            ("scene.json", scene.as_bytes()),
            ("models/overlay.json", br#"{"material":"materials/overlay.json"}"#),
            ("materials/overlay.json", material.as_bytes()),
            ("effects/tint.json", br#"{"passes":[{"material":"materials/tint.json"}]}"#),
            ("materials/tint.json", br#"{"passes":[{"shader":"tint"}]}"#),
            ("shaders/tint.vert", b"attribute vec3 a_Position;\nvoid main(){gl_Position=vec4(a_Position,1.0);}"),
            ("shaders/tint.frag", b"void main(){gl_FragColor=vec4(1.0);}"),
            ("shaders/sample.vert", b"uniform mat4 g_ModelViewProjectionMatrix;\nattribute vec3 a_Position;\nvoid main(){gl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);}"),
            ("shaders/sample.frag", b"uniform sampler2D g_Texture0;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,vec2(0.5,0.5));}"),
        ])).unwrap();
        let mut assets = Assets::discover(Some("/nonexistent"));
        assets.root = None;
        let model = crate::model::load_with(&package, &assets).unwrap();
        assert_eq!(!model.layers.is_empty(), admitted, "{blend} {post} {color_blend}");
        if admitted {
            assert!(model.skipped.is_empty(), "{:?}", model.skipped);
            assert!(model.layers[0].base_scene_input);
            assert_eq!(model.layers[0].effects.len(), 1);
        } else {
            assert!(
                model
                    .skipped
                    .iter()
                    .any(|reason| reason.contains("scene input blending unsupported")),
                "{:?}",
                model.skipped
            );
        }
    }
}

#[test]
fn dynamic_material_rejected() {
    let package = package(&json!([{"shader":"custom","textures":["a","b"]}]));
    let assets = Assets::discover(Some("/nonexistent"));
    let model = crate::model::load_with(&package, &assets).unwrap();
    assert!(model.layers[0].material.is_some());
    let resources = crate::model::ImageResources::new(&package, &assets);
    let before = (resources.payload_bytes(), resources.revision());
    assert!(resources.load(&package.scene_json().unwrap(), 0).is_err());
    assert_eq!((resources.payload_bytes(), resources.revision()), before);
}

#[test]
fn primary_file_components() {
    let directory = tempfile::tempdir().unwrap();
    ::image::RgbaImage::from_pixel(4, 4, ::image::Rgba([0, 255, 0, 255]))
        .save(directory.path().join("custom.png"))
        .unwrap();
    let texture = build_tex("TEXB0001", 0, 1 << 20, &[255; 64], false, None);
    let package = Package::parse(build_pkg(&[
        ("scene.json", br#"{"objects":[{"id":1,"image":"models/image.json","size":"32 32"}]}"#),
        ("models/image.json", br#"{"material":"materials/image.json"}"#),
        ("materials/image.json", br#"{"passes":[{"shader":"mask","textures":["mask"],"usertextures":[{"name":"photo"}]}]}"#),
        ("materials/mask.tex", &texture),
        ("shaders/mask.vert", b"attribute vec3 a_Position;\nvoid main(){gl_Position=vec4(a_Position,1.0);}"),
        ("shaders/mask.frag", b"uniform sampler2D g_Texture0; // {\"components\":[{\"combo\":\"C0\"}]}\n#if C0\nuniform float g_Component;\n#endif\nvoid main(){gl_FragColor=vec4(1.0);}"),
    ])).unwrap();
    for (path, expected) in [("", true), ("custom.png", false)] {
        let document =
            json!({"general":{"properties":{"photo":{"type":"scenetexture","value":path}}}});
        let assets = Assets::discover(Some(""))
            .with_scene_textures(&document, directory.path(), &[])
            .unwrap();
        let model = crate::model::load_with(&package, &assets).unwrap();
        assert!(model.skipped.is_empty(), "{:?}", model.skipped);
        let layer = &model.layers[0];
        assert_eq!(layer.texture.component_mask != 0, expected);
        assert_eq!(
            layer
                .material
                .as_ref()
                .unwrap()
                .pass
                .fragment
                .uniforms
                .iter()
                .any(|uniform| uniform.name == "g_Component"),
            expected
        );
    }
}

#[test]
fn cpu_skinned_morphs() {
    let mut mesh = crate::puppet::parse(&crate::puppet::tests::fixture()).unwrap();
    assert!(super::cpu_skinned(&mesh));
    let shape = |modifier| crate::puppet::morph::Shape {
        id: 1,
        name: "shape".into(),
        deltas: vec![[0; 3]],
        alpha: Vec::new(),
        modifier,
    };
    let bone = crate::puppet::morph::Modifier { bone: 0, mode: 0, range: [0.0, 1.0] };
    for (shape, cpu) in [(shape(None), true), (shape(Some(bone)), false)] {
        mesh.morph = Some(crate::puppet::morph::Morph {
            scale: 1.0,
            samples: 1,
            indices: vec![1, 0, 0],
            shapes: vec![shape],
        });
        assert_eq!(super::cpu_skinned(&mesh), cpu);
    }
}

#[test]
#[ignore = "requires Wallpaper Engine assets"]
fn texture_copy_flag() {
    let assets = Assets::discover(Some("/nonexistent"));
    for (pass, plain) in [
        (
            json!({"shader":"genericimage3","combos":{"VERSION":2,"LIGHTING":0},"textures":["a"]}),
            true,
        ),
        (json!({"shader":"genericimage4","depthtest":"enabled","textures":["a"]}), true),
        (json!({"shader":"custom","textures":["a","b"]}), false),
        (json!({"shader":"genericimage3","combos":{"LIGHTING":1},"textures":["a"]}), false),
        (json!({"shader":"flat","depthtest":"enabled"}), false),
    ] {
        let model = crate::model::load_with(&package(&json!([pass])), &assets).unwrap();
        let material = model.layers[0].material.as_ref();
        assert_eq!(material.map(super::Material::copyable), Some(plain), "{pass}");
    }
}

fn rest_vertex(position: [f32; 2], uv: [f32; 2]) -> crate::puppet::Vertex {
    crate::puppet::Vertex {
        position: [position[0], position[1], 0.0],
        uv,
        bones: [0; 4],
        weights: [1.0, 0.0, 0.0, 0.0],
    }
}

#[test]
fn prelit_projection_maps_uv() {
    let vertices = [
        rest_vertex([-1000.0, -1028.5], [0.0353, 1.0114]),
        rest_vertex([1152.0, 982.5], [1.0353, 0.0114]),
        rest_vertex([76.0, -23.0], [0.5353, 0.5114]),
    ];
    let matrix = puppet::prelit_projection(&vertices).unwrap();
    for vertex in &vertices {
        let x = matrix[0] * vertex.position[0] + matrix[12];
        let y = matrix[5] * vertex.position[1] + matrix[13];
        assert!((x - (2.0 * vertex.uv[0] - 1.0)).abs() < 0.001, "x {x}");
        assert!((y - (1.0 - 2.0 * vertex.uv[1])).abs() < 0.001, "y {y}");
    }
}

#[test]
fn prelit_projection_rejects() {
    let rearranged = [
        rest_vertex([0.0, 0.0], [0.0, 1.0]),
        rest_vertex([10.0, 10.0], [1.0, 0.0]),
        rest_vertex([5.0, 5.0], [0.1, 0.9]),
    ];
    assert!(puppet::prelit_projection(&rearranged).is_err());
    let flat = [rest_vertex([3.0, 0.0], [0.0, 1.0]), rest_vertex([3.0, 10.0], [1.0, 0.0])];
    assert!(puppet::prelit_projection(&flat).is_err());
}
