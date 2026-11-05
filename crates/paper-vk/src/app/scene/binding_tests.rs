use super::effect_lifetime_tests::{center, package_files};
use super::*;
use paper_scene::model::Properties;

#[test]
#[ignore = "requires Vulkan"]
fn instance_binding_updates() {
    for (base, written) in [(false, false), (true, false), (false, true)] {
        binding_updates(base, written);
    }
}

fn binding_updates(base: bool, written: bool) {
    let mut scene = serde_json::json!({
        "general":{"orthogonalprojection":{"width":64,"height":64},"clearcolor":"0 0 0"},
        "objects":[{"id":1,"image":"models/flat.json","origin":"32 32 0","size":"64 64",
            "visible":{"value":true,"script":"export function init(value) { thisLayer.name=String(thisLayer.getEffect(0).getMaterial(0).amount); return value; } export function update(value) { return value; }"},
            "effects":[{"file":"effects/test.json","passes":[{"usershadervalues":{"Strength":"amount"},"constantshadervalues":{"amount":0.25}}]}]},
            {"id":2,"alpha":{"user":"strength","value":1.0}},
            {"id":3,"alpha":{"user":"opacity","value":1.0}}]
    });
    let mut material = serde_json::json!({"passes":[{"shader":"test"}]});
    if base {
        material["passes"][0]["usershadervalues"] = serde_json::json!({"strength":"amount"});
        scene["objects"][0]["effects"][0]["passes"][0]
            .as_object_mut()
            .unwrap()
            .remove("usershadervalues");
    }
    if written {
        let script = scene["objects"][0]["visible"]["script"].as_str().unwrap();
        scene["objects"][0]["visible"]["script"] = script
            .replace(
                "thisLayer.name=",
                "thisLayer.getEffect(0).getMaterial(0).amount=0.875; thisLayer.name=",
            )
            .into();
    }
    let scene = scene.to_string();
    let material = material.to_string();
    let package = package_files(&[
        ("scene.json", scene.as_bytes()),
        ("models/flat.json", br#"{"material":"materials/flat.json"}"#),
        ("materials/flat.json", br#"{"passes":[{"shader":"flat"}]}"#),
        ("effects/test.json", br#"{"passes":[{"material":"materials/test.json"}]}"#),
        ("materials/test.json", material.as_bytes()),
        ("shaders/test.vert", b"attribute vec3 a_Position;\nuniform mat4 g_ModelViewProjectionMatrix;\nvoid main(){gl_Position=mul(vec4(a_Position,1.0),g_ModelViewProjectionMatrix);}"),
        ("shaders/test.frag", b"uniform float g_Value; // {\"material\":\"amount\",\"default\":0.125}\nvoid main(){gl_FragColor=vec4(g_Value,0.0,0.0,1.0);}"),
    ]);
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("scene.pkg"), b"fixture").unwrap();
    std::fs::write(
        root.path().join("project.json"),
        br#"{"general":{"properties":{"strength":{"type":"slider","value":0.75},"opacity":{"type":"slider","value":1.0}}}}"#,
    )
    .unwrap();
    let shared = crate::shared::test_device();
    let build = |properties: &Properties| {
        let mut model =
            paper_scene::model::load_from_dir_with(&package, root.path(), properties).unwrap();
        build_group(&shared, &mut model, true, &[(64, 64)], FillMode::Fit).unwrap()
    };
    let mut group = build(&Properties::new());
    assert!(!group.frozen);
    assert!(group.baked_fx_outputs.is_empty());
    assert_eq!(center(&mut group), [if written { 223 } else { 191 }, 0, 0, 255]);
    let directory = root.path().to_str().unwrap();
    let mut source = properties::PropertySource::new(directory, &Properties::new());
    let mut ctl = crate::ctl::Ctl::start_opts("", true, 100, false, false);
    let allocations =
        (vk::Renderer::scene_target_live_allocations(), vk::Renderer::effect_live_allocations());
    assert!(group.update_properties(
        &mut source,
        directory,
        &Properties::from([("opacity".into(), vec![0.5])]),
        &mut ctl,
    ));
    assert_eq!(center(&mut group), [if written { 223 } else { 191 }, 0, 0, 255]);
    assert_eq!(
        (vk::Renderer::scene_target_live_allocations(), vk::Renderer::effect_live_allocations()),
        allocations
    );
    for (properties, pixel, getter) in [
        (Properties::from([("strength".into(), vec![0.625])]), 159, "0.625"),
        (Properties::new(), 191, "0.75"),
    ] {
        let accepted = group.update_properties(&mut source, directory, &properties, &mut ctl);
        assert!(!accepted);
        let next = build(&properties);
        std::mem::replace(&mut group, next).destroy();
        source = properties::PropertySource::new(directory, &properties);
        let host = group.scripts.as_ref().unwrap().host.as_ref().unwrap();
        assert!(host.diagnostics.is_empty(), "{:?}", host.diagnostics);
        assert_eq!(host.scene["objects"][0]["name"], if written { "0.875" } else { getter });
        assert_eq!(center(&mut group), [if written { 223 } else { pixel }, 0, 0, 255]);
    }
    group.destroy();
}
