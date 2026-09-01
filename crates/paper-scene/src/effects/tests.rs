use super::{Assets, CompositeBuffer, EffectBind, PassMeta};
use crate::shader::{Uniform, UniformKind};
use std::collections::BTreeMap;

fn pass_meta(names: &[&str]) -> PassMeta {
    PassMeta {
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
    }
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
