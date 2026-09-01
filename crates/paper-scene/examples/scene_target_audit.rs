use paper_scene::effects::EffectBind;
use paper_scene::model::SceneModel;
use paper_scene::pkg::Package;
use paper_scene::scene_targets::{LayerTargetNode, PassiveLayerTarget, plan_scene_targets};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Default)]
struct SceneCounts {
    owning: usize,
    foreign: usize,
    full_framebuffer: usize,
}

struct OwnedNode {
    id: String,
    scene_order: usize,
    local_targets: Vec<String>,
    binds: Vec<Vec<(usize, EffectBind)>>,
}

fn id_of(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn count_strings(value: &Value, own_id: Option<&str>, counts: &mut SceneCounts) {
    match value {
        Value::String(text) => {
            if text.eq_ignore_ascii_case("_rt_FullFrameBuffer") {
                counts.full_framebuffer += 1;
            }
            let Some((layer, suffix)) = text
                .strip_prefix("_rt_imageLayerComposite_")
                .and_then(|rest| rest.rsplit_once('_'))
            else {
                return;
            };
            if matches!(suffix, "a" | "A" | "b" | "B") {
                if own_id == Some(layer) {
                    counts.owning += 1;
                } else {
                    counts.foreign += 1;
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                count_strings(value, own_id, counts);
            }
        }
        Value::Object(values) => {
            for value in values.values() {
                count_strings(value, own_id, counts);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn raw_scene_counts(pkg: &Package) -> SceneCounts {
    let mut counts = SceneCounts::default();
    let Some(scene) = pkg.find_json("scene.json").ok().flatten() else {
        return counts;
    };
    for object in scene.get("objects").and_then(Value::as_array).into_iter().flatten() {
        count_strings(object, id_of(object.get("id")).as_deref(), &mut counts);
    }
    counts
}

fn nodes(model: &SceneModel) -> Vec<OwnedNode> {
    model
        .layers
        .iter()
        .filter(|layer| layer.visible && !layer.effects.is_empty())
        .map(|layer| OwnedNode {
            id: layer.id.clone(),
            scene_order: layer.scene_order,
            local_targets: layer
                .effects
                .iter()
                .flat_map(|effect| effect.fbos.iter().map(|(name, _)| name.clone()))
                .collect(),
            binds: layer
                .effects
                .iter()
                .flat_map(|effect| effect.passes.iter().map(|pass| pass.binds.clone()))
                .collect(),
        })
        .collect()
}

fn scene_pkg(dir: &Path) -> Option<PathBuf> {
    ["scene.pkg", "gifscene.pkg"].into_iter().map(|name| dir.join(name)).find(|path| path.is_file())
}

fn main() {
    let Some(root) = std::env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("usage: scene_target_audit <workshop-root>");
        std::process::exit(2);
    };
    let mut directories: Vec<PathBuf> = std::fs::read_dir(&root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    directories.sort();

    let mut scenes = 0usize;
    let mut loaded = 0usize;
    let mut self_references = 0usize;
    let mut foreign_references = 0usize;
    let mut full_framebuffer_references = 0usize;
    let mut target_scenes = BTreeSet::new();
    let mut foreign_target_scenes = BTreeSet::new();
    let mut active_foreign_target_scenes = BTreeSet::new();
    let mut passive_target_providers = BTreeMap::new();
    let mut snapshot_lower_particles = BTreeMap::new();
    let mut scene_snapshot_scenes = BTreeSet::new();
    let mut planned_scenes = BTreeSet::new();
    let mut failures = BTreeMap::new();
    let mut plan_failures = BTreeMap::new();

    for directory in directories {
        let Some(pkg_path) = scene_pkg(&directory) else {
            continue;
        };
        scenes += 1;
        let id = directory
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
        let pkg = match Package::open(&pkg_path) {
            Ok(pkg) => pkg,
            Err(error) => {
                failures.insert(id, format!("{error:#}"));
                continue;
            }
        };
        let raw = raw_scene_counts(&pkg);
        if raw.foreign > 0 {
            foreign_target_scenes.insert(id.clone());
        }
        self_references += raw.owning;
        foreign_references += raw.foreign;
        let model = match paper_scene::model::load_from_dir(&pkg, &directory) {
            Ok(model) => model,
            Err(error) => {
                failures.insert(id, format!("{error:#}"));
                continue;
            }
        };
        loaded += 1;
        let owned = nodes(&model);
        let runtime_full = owned
            .iter()
            .flat_map(|node| &node.binds)
            .flatten()
            .filter(|(_, binding)| matches!(binding, EffectBind::SceneSoFar))
            .count();
        if runtime_full > 0 {
            scene_snapshot_scenes.insert(id.clone());
            let count = owned
                .iter()
                .filter(|node| {
                    node.binds
                        .iter()
                        .flatten()
                        .any(|(_, binding)| matches!(binding, EffectBind::SceneSoFar))
                })
                .map(|node| {
                    model
                        .particles
                        .iter()
                        .filter(|particle| {
                            particle.scene_order < node.scene_order
                                && particle.system.texture.is_some()
                        })
                        .count()
                })
                .sum::<usize>();
            if count > 0 {
                snapshot_lower_particles.insert(id.clone(), count);
            }
        }
        full_framebuffer_references += runtime_full.max(raw.full_framebuffer);
        let runtime_foreign = owned
            .iter()
            .flat_map(|node| &node.binds)
            .flatten()
            .filter(|(_, binding)| matches!(binding, EffectBind::LayerComposite { .. }))
            .count();
        if runtime_full > 0 || runtime_foreign > 0 {
            target_scenes.insert(id.clone());
        }
        let borrowed: Vec<LayerTargetNode<'_>> = owned
            .iter()
            .map(|node| LayerTargetNode {
                id: &node.id,
                scene_order: node.scene_order,
                local_targets: &node.local_targets,
                binds: &node.binds,
                dynamic: false,
                prefix_dynamic: false,
            })
            .collect();
        let active_ids: BTreeSet<&str> = borrowed.iter().map(|node| node.id).collect();
        let referenced_ids: BTreeSet<&str> = owned
            .iter()
            .flat_map(|node| &node.binds)
            .flatten()
            .filter_map(|(_, binding)| match binding {
                EffectBind::LayerComposite { layer, .. } => Some(layer.as_str()),
                _ => None,
            })
            .collect();
        let passive_evidence: Vec<Value> = model
            .layers
            .iter()
            .filter(|layer| {
                referenced_ids.contains(layer.id.as_str())
                    && !active_ids.contains(layer.id.as_str())
            })
            .map(|layer| {
                serde_json::json!({
                    "id": layer.id,
                    "visible": layer.visible,
                    "texture": [layer.texture.width, layer.texture.height],
                    "content": [layer.texture.img_width, layer.texture.img_height],
                    "layer_size": [layer.size.0, layer.size.1],
                    "padded": layer.texture.width != layer.texture.img_width
                        || layer.texture.height != layer.texture.img_height,
                })
            })
            .collect();
        if !passive_evidence.is_empty() {
            passive_target_providers.insert(id.clone(), passive_evidence);
        }
        let passive: Vec<PassiveLayerTarget<'_>> = model
            .layers
            .iter()
            .filter(|layer| !active_ids.contains(layer.id.as_str()))
            .map(|layer| PassiveLayerTarget { id: &layer.id, dynamic: false })
            .collect();
        match plan_scene_targets(&borrowed, &passive) {
            Ok(plan) => {
                if runtime_full > 0 || runtime_foreign > 0 {
                    if plan.sampled.iter().any(|targets| !targets.is_empty()) {
                        active_foreign_target_scenes.insert(id.clone());
                    }
                    planned_scenes.insert(id);
                }
            }
            Err(error) => {
                plan_failures.insert(id, error.to_string());
            }
        }
    }

    let report = serde_json::json!({
        "schema": 1,
        "corpus": root,
        "scenes": scenes,
        "loaded": loaded,
        "load_failures": failures,
        "references": {
            "owning_layer": self_references,
            "foreign_layer": foreign_references,
            "full_framebuffer": full_framebuffer_references,
        },
        "target_scenes": target_scenes,
        "foreign_target_scenes": foreign_target_scenes,
        "active_foreign_target_scenes": active_foreign_target_scenes,
        "passive_target_providers": passive_target_providers,
        "snapshot_lower_particles": snapshot_lower_particles,
        "scene_snapshot_scenes": scene_snapshot_scenes,
        "planned_scenes": planned_scenes,
        "plan_failures": plan_failures,
    });
    println!("{}", serde_json::to_string_pretty(&report).expect("serialize audit"));
}
