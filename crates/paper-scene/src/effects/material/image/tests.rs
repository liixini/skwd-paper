use super::*;
use crate::tests::{build_pkg, build_tex};
use serde_json::json;

#[test]
fn instance_slots_and_maps() {
    let mut pass = json!({
        "textures":["red","blue"],"usertextures":[{"name":"cover","type":"albumart"}],
        "combos":{"version":1,"KEEP":1},
        "constantshadervalues":{"g_Tint":"1 0 0","g_Keep":2},
        "usershadervalues":{"tint":"g_Tint","kept":"g_Keep"}
    });
    overlay(
        &mut pass,
        &json!({
            "textures":["","green",null],"usertextures":[{"name":""}],
            "combos":{"VERSION":2},"constantshadervalues":{"g_Tint":"0 1 0"}
        }),
    );
    assert_eq!(pass["textures"], json!(["red", "green", null]));
    assert_eq!(pass["usertextures"][0]["name"], "cover");
    assert_eq!(
        combos_of(pass.get(COMBOS_FIELD)),
        std::collections::BTreeMap::from([("VERSION".into(), 2), ("KEEP".into(), 1)])
    );
    assert_eq!(pass["constantshadervalues"]["g_Keep"], 2);
    assert_eq!(pass["usershadervalues"], json!({"tint":"g_Tint","kept":"g_Keep"}));
}

#[test]
fn instance_asset_material() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("shaders")).unwrap();
    std::fs::create_dir(dir.path().join("materials")).unwrap();
    std::fs::write(dir.path().join("materials/base.json"),br#"{"passes":[{"shader":"custom","textures":["red","red"],"combos":{"TINT":0},"constantshadervalues":{"Tint":"1 0 0"}}]}"#).unwrap();
    let red = build_tex("TEXB0001", 0, 0, &[255; 64], false, None);
    let green = build_tex("TEXB0001", 0, 0, &[0; 64], false, None);
    let package = Package::parse(build_pkg(&[
        ("scene.json",br#"{"objects":[{"id":1,"image":"models/image.json","size":"32 32","instance":{"textures":["green","green"],"combos":{"TINT":1},"constantshadervalues":{"Tint":"0 1 0"}}}]}"#),
        ("models/image.json",br#"{"material":"materials/base.json","instanced":true}"#),
        ("materials/red.tex",&red),("materials/green.tex",&green),
        ("shaders/custom.vert",b"attribute vec3 a_Position;\nuniform mat4 g_ModelViewProjectionMatrix;\nvoid main(){gl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);}"),
        ("shaders/custom.frag",b"uniform sampler2D g_Texture0;\nuniform sampler2D g_Texture1;\nuniform vec3 g_Tint; // {\"material\":\"Tint\"}\nvoid main(){\n#if TINT\ngl_FragColor=texSample2D(g_Texture0,vec2(0.5))*texSample2D(g_Texture1,vec2(0.5))*vec4(g_Tint,1.0);\n#else\ngl_FragColor=vec4(1.0);\n#endif\n}"),
    ])).unwrap();
    let model = crate::model::load_with(&package, &Assets::discover(dir.path().to_str())).unwrap();
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    let layer = &model.layers[0];
    assert_eq!(layer.texture_name.as_deref(), Some("green"));
    let pass = &layer.material.as_ref().unwrap().pass;
    assert_eq!(pass.values.get("g_Tint"), Some(&vec![0.0, 1.0, 0.0]));
    assert_eq!(pass.fragment.samplers.len(), 2);
    assert_eq!(layer.texture.pixels, pass.textures[1].as_ref().unwrap().pixels);
}

#[test]
fn instance_scene_input() {
    let texture = build_tex("TEXB0001", 0, 0, &[255; 64], false, None);
    let package = Package::parse(build_pkg(&[
        ("scene.json", br#"{"objects":[{"id":1,"image":"models/image.json","size":"32 32","instance":{"textures":["_rt_MipMappedFrameBuffer"]}}]}"#),
        ("models/image.json", br#"{"material":"materials/base.json","instanced":true}"#),
        ("materials/base.json", br#"{"passes":[{"shader":"custom","textures":["red"]}]}"#),
        ("materials/red.tex", &texture),
        ("shaders/custom.vert", b"attribute vec3 a_Position; uniform mat4 g_ModelViewProjectionMatrix; void main(){gl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);}"),
        ("shaders/custom.frag", b"uniform sampler2D g_Texture0; void main(){gl_FragColor=texSample2D(g_Texture0,vec2(0.5));}"),
    ])).unwrap();
    let model = crate::model::load_with(&package, &Assets::discover(Some(""))).unwrap();
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    let layer = &model.layers[0];
    assert!(layer.texture_name.is_none());
    assert!(
        layer
            .material
            .as_ref()
            .unwrap()
            .pass
            .binds
            .contains(&(0, super::super::super::EffectBind::SceneHistory,))
    );
}

#[test]
fn instance_alias_constant() {
    let texture = build_tex("TEXB0001", 0, 0, &[255; 64], false, None);
    let package = Package::parse(build_pkg(&[
        ("scene.json", br#"{"objects":[{"id":1,"image":"models/image.json","size":"32 32","instance":{"constantshadervalues":{"Tint":"0 1 0"}}}]}"#),
        ("models/image.json", br#"{"material":"materials/base.json","instanced":true}"#),
        ("materials/base.json", br#"{"passes":[{"shader":"custom","textures":["red"],"usershadervalues":{"tint":"g_Tint"}}]}"#),
        ("materials/red.tex", &texture),
        ("shaders/custom.vert", b"attribute vec3 a_Position; uniform mat4 g_ModelViewProjectionMatrix; void main(){gl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);}"),
        ("shaders/custom.frag", b"uniform vec3 g_Tint; // {\"material\":\"Tint\"}\nvoid main(){gl_FragColor=vec4(g_Tint,1.0);}"),
    ])).unwrap();
    let assets = Assets::discover(Some(""))
        .with_properties(std::collections::BTreeMap::from([("tint".into(), vec![1.0, 0.0, 0.0])]));
    let model = crate::model::load_with(&package, &assets).unwrap();
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    let pass = &model.layers[0].material.as_ref().unwrap().pass;
    assert_eq!(pass.values["g_Tint"], [0.0, 1.0, 0.0]);
}

#[test]
fn effect_media_slots_merge() {
    let package = Package::parse(build_pkg(&[
        ("shaders/media.vert", b"attribute vec3 a_Position; void main(){gl_Position=vec4(a_Position,1.0);}"),
        ("shaders/media.frag", b"uniform sampler2D g_Texture1; uniform sampler2D g_Texture2; void main(){gl_FragColor=texSample2D(g_Texture1,vec2(0.5))+texSample2D(g_Texture2,vec2(0.5));}"),
    ])).unwrap();
    let material = serde_json::json!({"passes":[{"shader":"media","usertextures":[null,{"type":"system","name":"$mediaThumbnail"}]}]});
    let overrides = serde_json::json!([{"usertextures":[null,null,{"type":"system","name":"$mediaPreviousThumbnail"}]}]);
    let passes = super::super::super::build_material(
        &package,
        &Assets::discover(Some("")),
        &material,
        Some(&overrides),
        None,
        None,
        None,
    )
    .unwrap();
    assert_eq!(
        passes[0].textures[1].as_ref().unwrap().system_texture,
        Some(crate::model::MediaTexture::Current)
    );
    assert_eq!(
        passes[0].textures[2].as_ref().unwrap().system_texture,
        Some(crate::model::MediaTexture::Previous)
    );
}
