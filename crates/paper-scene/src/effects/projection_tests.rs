use super::{Assets, EffectPass};
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn projection_pass(scene: Option<&Value>, authored: i64) -> EffectPass {
    let object = json!({"id":1,"shape":"quad","effects":[{"file":"effects/view.json"}]});
    let mut definition = scene.cloned().unwrap_or_else(|| json!({}));
    definition["objects"] = json!([object]);
    let material = json!({"passes":[{"shader":"view","combos":{"SCENE_ORTHO":authored}}]});
    let fragment = br"
void main() {
#if SCENE_ORTHO
    vec4 ortho_color = vec4(0.25);
    gl_FragColor = ortho_color;
#else
    vec4 perspective_color = vec4(0.75);
    gl_FragColor = perspective_color;
#endif
}
";
    let package = crate::pkg::Package::parse(crate::tests::build_pkg(&[
        ("scene.json", definition.to_string().as_bytes()),
        ("effects/view.json", br#"{"passes":[{"material":"materials/view.json"}]}"#),
        ("materials/view.json", material.to_string().as_bytes()),
        ("shaders/view.vert", b"void main() { gl_Position = vec4(0.0); }\n"),
        ("shaders/view.frag", fragment),
    ]))
    .unwrap();
    let assets = Assets::discover(Some("/nonexistent"));
    let mut effects = if scene.is_some_and(|scene| {
        crate::model::ProjectionMode::of(scene) != crate::model::ProjectionMode::Other
    }) {
        let mut model = crate::model::load_with(&package, &assets).unwrap();
        assert!(model.skipped.is_empty(), "{:?}", model.skipped);
        assert_eq!(model.layers.len(), 1);
        model.layers.remove(0).effects
    } else {
        let assets = assets.with_scene(super::SceneContext {
            projection: scene.map(crate::model::ProjectionMode::of),
            ..Default::default()
        });
        let (effects, skipped) = super::load_effects(&package, &assets, &object, false);
        assert!(skipped.is_empty(), "{skipped:?}");
        effects
    };
    assert_eq!(effects.len(), 1);
    effects.remove(0).passes.remove(0)
}

#[test]
fn scene_projection_selects_branch() {
    let cases = [
        (Some(json!({"general":{"orthogonalprojection":{"width":256,"height":256}}})), true),
        (Some(json!({"general":{"orthogonalprojection":{"auto":true}}})), true),
        (
            Some(
                json!({"camera":{"eye":"0 0 10","center":"0 0 0","up":"0 1 0"},"general":{"fov":60,"nearz":0.1,"farz":100}}),
            ),
            false,
        ),
        (None, false),
    ];
    for (scene, orthographic) in cases {
        for authored in [0, 1] {
            let pass = projection_pass(scene.as_ref(), authored);
            let hlsl = pass.hlsl.unwrap();
            for source in [&pass.fragment.source, &hlsl.fragment] {
                let active = crate::shader::preprocess(source, &BTreeMap::new());
                assert_eq!(active.contains("ortho_color"), orthographic);
                assert_eq!(active.contains("perspective_color"), !orthographic);
            }
        }
    }
}
