use super::*;

fn bindings(scene: &Value, project: &Value) -> PropertyBindings {
    let mut resolved = scene.clone();
    let mut scripts = Vec::new();
    source::collect(&mut resolved, "", &mut scripts, &Properties::new());
    PropertyBindings::new(scene, project, &scripts)
}

#[test]
fn layer_values_and_shader_uniforms_update_without_accepting_effect_topology() {
    let scene = json!({"objects":[{"id":1,"image":"layer.json","alpha":{"user":"opacity","value":1},"color":{"user":"color","value":"1 1 1"},"effects":[{"visible":{"user":"effect","value":true},"passes":[{"constantshadervalues":{"Tint":{"user":"tint","value":"1 1 1"}}}]}]}]});
    let bindings = bindings(&scene, &Value::Null);
    for name in ["opacity", "color", "tint"] {
        assert!(
            bindings
                .update(&Properties::new(), &Properties::from([(name.into(), vec![0.5])]))
                .is_some(),
            "{name}"
        );
    }
    for name in ["effect", "unknown"] {
        assert!(
            bindings
                .update(&Properties::new(), &Properties::from([(name.into(), vec![0.5])]))
                .is_none(),
            "{name}"
        );
    }
}

#[test]
fn one_unsupported_binding_requires_a_full_reload() {
    let scene = json!({"objects":[{"id":1,"alpha":{"user":"shared","value":1},"size":{"user":"shared","value":"100 100"}}]});
    assert!(
        bindings(&scene, &Value::Null)
            .update(&Properties::new(), &Properties::from([("shared".into(), vec![2.0])]))
            .is_none()
    );
}

#[test]
fn shared_asset_dependencies_and_unknown_assets_require_a_reload() {
    let scene = json!({"objects":[{"alpha":{"user":"opacity","value":1},"color":{"user":"tint","value":"1 1 1"}}]});
    let mut bindings = bindings(&scene, &Value::Null);
    bindings.restrict_assets(Some(&BTreeSet::from(["opacity".into()])));
    assert!(
        bindings
            .update(&Properties::new(), &Properties::from([("opacity".into(), vec![0.5])]))
            .is_none()
    );
    assert!(
        bindings
            .update(&Properties::new(), &Properties::from([("tint".into(), vec![0.5])]))
            .is_some()
    );
    bindings.restrict_assets(None);
    assert!(
        bindings
            .update(&Properties::new(), &Properties::from([("tint".into(), vec![0.5])]))
            .is_none()
    );
}

#[test]
fn particle_and_sound_ancestry_stays_on_the_full_reload_path() {
    let scene = json!({"objects":[{"id":1,"visible":{"user":"language","value":true}}, {"id":2,"parent":1}, {"id":3,"parent":2,"particle":"particles.json"}, {"id":4,"visible":{"user":"clock","value":true},"text":"Clock"}]});
    let bindings = bindings(&scene, &Value::Null);
    assert!(
        bindings
            .update(&Properties::new(), &Properties::from([("language".into(), vec![2.0])]))
            .is_none()
    );
    assert!(
        bindings
            .update(&Properties::new(), &Properties::from([("clock".into(), vec![0.0])]))
            .is_some()
    );
}

#[test]
fn clearing_overrides_restores_authored_defaults() {
    let project = json!({"general":{"properties":{"opacity":{"type":"slider","value":0.75}}}});
    let scene = json!({"objects":[{"alpha":{"user":"opacity","value":1}}]});
    let bindings = bindings(&scene, &project);
    let changed = Properties::from([("opacity".into(), vec![0.25])]);
    let desired = bindings.desired(&Properties::new());
    assert_eq!(desired["opacity"], vec![0.75]);
    let update = bindings.update(&changed, &desired).unwrap();
    assert_eq!(update["values"], json!([["/objects/0/alpha", 0.75]]));
}

#[test]
fn package_assets_can_disable_an_otherwise_safe_property() {
    let scene = json!({"objects":[{"alpha":{"user":"opacity","value":1}}]});
    let mut bindings = bindings(&scene, &Value::Null);
    let asset = br#"{"size":{"user":"opacity","value":1}}"#;
    let path = b"models/custom.json";
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&5u32.to_le_bytes());
    bytes.extend_from_slice(b"PKGV1");
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&(path.len() as u32).to_le_bytes());
    bytes.extend_from_slice(path);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&(asset.len() as u32).to_le_bytes());
    bytes.extend_from_slice(asset);
    let package = crate::pkg::Package::parse(bytes).unwrap();
    bindings.restrict_package(&package);
    assert!(
        bindings
            .update(&Properties::new(), &Properties::from([("opacity".into(), vec![0.5])]))
            .is_none()
    );
}
