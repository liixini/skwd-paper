use super::super::{Attachments, Properties, SunView};
use serde_json::json;

fn scene(light: &serde_json::Value) -> serde_json::Value {
    json!({"general":{"lightconfig":{"directional":1}},"objects":[light]})
}

fn uniforms(scene: &serde_json::Value) -> std::collections::BTreeMap<String, Vec<f32>> {
    super::super::uniforms(
        scene,
        &Properties::new(),
        &Attachments::new(),
        &SunView::of(scene, &Properties::new()),
    )
}

#[test]
fn tilted_sun_direction() {
    for angles in ["0 0.5 0", "0.5 0.5 0", "0 0.5 0.5"] {
        let values = uniforms(&scene(
            &json!({"id":1,"light":"ldirectional","angles":angles,"color":"1 0.5 0.25","intensity":2.0,"radius":500.0}),
        ));
        let direction = &values["g_LDirectional_Direction"];
        assert!((direction[2] - 0.5_f32.sin()).abs() < 1e-6, "{angles}: {direction:?}");
        assert_eq!(values["g_LDirectional_Color"], [2.0, 1.0, 0.5, 1.0]);
    }
    let values = uniforms(&scene(
        &json!({"id":1,"light":"ldirectional","angles":"0 0.5 0","intensity":1.0}),
    ));
    let direction = &values["g_LDirectional_Direction"];
    assert!((direction[0] + 0.5_f32.cos()).abs() < 1e-6, "{direction:?}");
}

#[test]
fn unfilled_slot_dark() {
    let mut scene =
        scene(&json!({"id":1,"light":"ldirectional","angles":"0 0.5 0","intensity":1.0}));
    scene["general"]["lightconfig"]["directional"] = json!(2);
    let values = uniforms(&scene);
    assert_eq!(values["g_LDirectional_Direction"][4..], [0.0, 0.0, -1.0, 0.0]);
    assert_eq!(values["g_LDirectional_Color"][4..], [0.0; 4]);
}

#[test]
fn directional_light_loads() {
    let scene = scene(
        &json!({"id":1,"name":"sun","light":"ldirectional","angles":"0 0 0","color":"1 1 1","intensity":1.0}),
    );
    let (config, skipped) = super::super::load(&scene, &Properties::new());
    assert!(skipped.is_empty(), "{skipped:?}");
    assert_eq!(config.directionals, 1);
}

#[test]
fn volumetric_sun_reported() {
    let scene = scene(&json!({"id":1,"name":"sun","light":"ldirectional","castvolumetrics":true}));
    let (_, skipped) = super::super::load(&scene, &Properties::new());
    assert_eq!(skipped, ["sun: directional light volumetrics unsupported"]);
}

#[test]
fn shadow_sun_loads() {
    let mut scene = scene(&json!({"id":1,"name":"sun","light":"ldirectional","castshadow":true}));
    scene["general"]["lightconfig"]["directionalshadow"] = json!(1);
    let (config, skipped) = super::super::load(&scene, &Properties::new());
    assert!(skipped.is_empty(), "{skipped:?}");
    assert!(config.shadows);
    assert_eq!(config.projections, 3);
}

#[test]
fn invalid_cascade_reported() {
    let scene = scene(
        &json!({"id":1,"name":"sun","light":"ldirectional","castshadow":true,"cascadedistance1":-1.0}),
    );
    let (_, skipped) = super::super::load(&scene, &Properties::new());
    assert_eq!(skipped, ["sun: invalid directional light values"]);
}

#[test]
fn degenerate_sun_dark() {
    let values =
        uniforms(&scene(&json!({"id":1,"light":"ldirectional","scale":"0 0 0","intensity":1.0})));
    assert_eq!(values["g_LDirectional_Direction"], [0.0, 0.0, -1.0, 0.0]);
    assert_eq!(values["g_LDirectional_Color"], [0.0; 4]);
}

#[test]
fn sun_loop_emitted() {
    let scene = json!({"general":{"lightconfig":{"directional":1}},"objects":[
        {"id":1,"light":"ldirectional","angles":"0 0.5 0","intensity":1.0},
        {"id":2,"shape":"quad","effects":[{"file":"effects/light.json"}]}
    ]})
    .to_string();
    let fragment = b"#include \"common.h\"\nvec3 ComputePBRLightShadowInfinite(vec3 n, vec3 l, vec3 v, vec3 a, vec3 c, vec3 s, vec3 b, float r, float m, float f) { return c; }\n#require LightingV1\nvoid main() { gl_FragColor = CAST4(PerformLighting_V1(CAST3(0.0), CAST3(1.0), CAST3(0.0), CAST3(0.0), CAST3(1.0), CAST3(0.0), 0.5, 0.0).x); }\n";
    let package = crate::pkg::Package::parse(crate::tests::build_pkg(&[
        ("scene.json", scene.as_bytes()),
        ("effects/light.json", br#"{"passes":[{"material":"materials/light.json"}]}"#),
        ("materials/light.json", br#"{"passes":[{"shader":"light","combos":{"LIGHTING":1}}]}"#),
        ("shaders/light.vert", b"void main() { gl_Position=vec4(0.0); }\n"),
        ("shaders/light.frag", fragment),
    ]))
    .unwrap();
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    let model = crate::model::load_with(&package, &assets).unwrap();
    let pass = &model.layers[0].effects[0].passes[0];
    let source = &pass.hlsl.as_ref().unwrap().fragment;
    assert!(source.contains("g_LDirectional_Direction[1]"), "{source}");
    assert!(source.contains("ComputePBRLightShadowInfinite(normal, g_LDirectional_Direction[i]"));
}

#[test]
fn cascade_loop_emitted() {
    let scene =
        json!({"general":{"lightconfig":{"directional":1,"directionalshadow":1}},"objects":[
            {"id":1,"light":"ldirectional","angles":"0 0.5 0","intensity":1.0,"castshadow":true},
            {"id":2,"shape":"quad","effects":[{"file":"effects/light.json"}]}
        ]})
        .to_string();
    let fragment = b"#include \"common.h\"\nvec3 ComputePBRLightShadowInfinite(vec3 n, vec3 l, vec3 v, vec3 a, vec3 c, vec3 s, vec3 b, float r, float m, float f) { return c * f; }\nvec3 ComputePBRLightShadow(vec3 n, vec3 d, vec3 v, vec3 a, vec3 c, float e, float k, vec3 s, vec3 b, float r, float m, float f) { return c; }\n#require LightingV1\nvoid main() { gl_FragColor = CAST4(PerformLighting_V1(CAST3(0.0), CAST3(1.0), CAST3(0.0), CAST3(0.0), CAST3(0.0), CAST3(0.0), 0.5, 0.0).x); }\n";
    let package = crate::pkg::Package::parse(crate::tests::build_pkg(&[
        ("scene.json", scene.as_bytes()),
        ("effects/light.json", br#"{"passes":[{"material":"materials/light.json"}]}"#),
        ("materials/light.json", br#"{"passes":[{"shader":"light","combos":{"LIGHTING":1}}]}"#),
        ("shaders/light.vert", b"void main() { gl_Position=vec4(0.0); }\n"),
        ("shaders/light.frag", fragment),
    ]))
    .unwrap();
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    let model = crate::model::load_with(&package, &assets).unwrap();
    assert!(model.skipped.is_empty(), "{:?}", model.skipped);
    let pass = &model.layers[0].effects[0].passes[0];
    let names: Vec<_> =
        pass.fragment.uniforms.iter().map(|uniform| uniform.name.as_str()).collect();
    for name in [
        "g_LDirectional_Shadow[1]",
        "g_LFeature_ShadowProjection[3]",
        "g_LFeature_ShadowProjectionTransform[3]",
    ] {
        assert!(names.contains(&name), "{names:?}");
    }
}
