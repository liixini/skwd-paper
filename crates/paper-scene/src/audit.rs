use crate::pkg::Package;
use crate::scene::{self, SceneFeatures};
use crate::sound::SoundInventory;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;
use std::path::Path;

#[derive(Default)]
pub struct Totals {
    pub scenes: usize,
    pub parsed: usize,
    pub failures: Vec<(String, String)>,
    pub tiers: [usize; 4],
    pub with_effects: usize,
    pub with_shared_effects: usize,
    pub with_particles: usize,
    pub with_puppet: usize,
    pub with_audio: usize,
    pub with_scripts: usize,
    pub with_parallax: usize,
    pub with_sound_objects: usize,
    pub sound_objects: usize,
    pub sound_scenes_multiple: usize,
    pub sound_autostart: usize,
    pub sound_event_driven: usize,
    pub sound_hidden: usize,
    pub sound_multiple_clips: usize,
    pub sound_looped: usize,
    pub sound_one_shot: usize,
    pub sound_random: usize,
    pub sound_visibility_bound: usize,
    pub sound_volume_bound: usize,
    pub sound_start_bound: usize,
    pub sound_event_scenes: BTreeSet<String>,
    pub with_tex_gif: usize,
    pub with_tex_video: usize,
    pub tex_failures: usize,
    pub json_failures: usize,
    pub refs_truncated: usize,
    pub samples: Vec<(String, String)>,
    pub effects: BTreeMap<String, usize>,
    pub shared_effects: BTreeMap<String, usize>,
    pub shaders: BTreeMap<String, usize>,
    pub combos: BTreeMap<String, usize>,
    pub particles: BTreeMap<String, usize>,
    pub tex_formats: BTreeMap<String, usize>,
    pub pkg_versions: BTreeMap<String, usize>,
    pub object_gaps: BTreeMap<String, ObjectGap>,
    pub object_gap_instances: usize,
    pub gap_scenes: BTreeMap<String, usize>,
    pub gap_instances: BTreeMap<String, usize>,
    pub full_fidelity: usize,
}

#[derive(Default)]
pub struct ObjectGap {
    pub kind: String,
    pub properties: BTreeSet<String>,
    pub count: usize,
    pub scenes: BTreeSet<String>,
    pub examples: Vec<(String, String, String)>,
}

fn bump_all(map: &mut BTreeMap<String, usize>, keys: impl IntoIterator<Item = String>) {
    for key in keys {
        *map.entry(key).or_insert(0) += 1;
    }
}

fn record(totals: &mut Totals, id: &str, features: SceneFeatures, sound: &SoundInventory) {
    totals.parsed += 1;
    let compatibility = crate::capability::assess_native(&features);
    if compatibility.full_fidelity() {
        totals.full_fidelity += 1;
    }
    for gap in &compatibility.gaps {
        *totals.gap_scenes.entry(gap.code().to_string()).or_insert(0) += 1;
        *totals.gap_instances.entry(gap.code().to_string()).or_insert(0) += gap.instances();
    }
    totals.tiers[features.tier() as usize] += 1;
    if !features.effects.is_empty() {
        totals.with_effects += 1;
    }
    if !features.shared_effects.is_empty() {
        totals.with_shared_effects += 1;
    }
    if features.objects_particle > 0 {
        totals.with_particles += 1;
    }
    if features.puppet {
        totals.with_puppet += 1;
    }
    if features.audio {
        totals.with_audio += 1;
    }
    if features.scripts > 0 {
        totals.with_scripts += 1;
    }
    if features.parallax {
        totals.with_parallax += 1;
    }
    if features.objects_sound > 0 {
        totals.with_sound_objects += 1;
    }
    totals.sound_objects += sound.objects;
    totals.sound_scenes_multiple += usize::from(sound.objects > 1);
    totals.sound_autostart += sound.autostart;
    totals.sound_event_driven += sound.event_driven;
    totals.sound_hidden += sound.hidden;
    totals.sound_multiple_clips += sound.multiple_clips;
    totals.sound_looped += sound.looped;
    totals.sound_one_shot += sound.one_shot;
    totals.sound_random += sound.random;
    totals.sound_visibility_bound += sound.visibility_bound;
    totals.sound_volume_bound += sound.volume_bound;
    totals.sound_start_bound += sound.start_bound;
    if sound.event_driven > 0 {
        totals.sound_event_scenes.insert(id.to_string());
    }
    if features.tex_gif > 0 {
        totals.with_tex_gif += 1;
    }
    if features.tex_video > 0 {
        totals.with_tex_video += 1;
    }
    totals.tex_failures += features.tex_failures;
    totals.json_failures += features.json_failures;
    if features.refs_truncated {
        totals.refs_truncated += 1;
    }
    totals.object_gap_instances += features.objects_other;
    if let Some(sample) = features.sample_error
        && totals.samples.len() < 15
    {
        totals.samples.push((id.to_string(), sample));
    }
    for object in features.other_objects {
        let gap = totals
            .object_gaps
            .entry(object.kind.clone())
            .or_insert_with(|| ObjectGap { kind: object.kind.clone(), ..ObjectGap::default() });
        gap.properties.extend(object.properties);
        gap.count += 1;
        gap.scenes.insert(id.to_string());
        if gap.examples.len() < 3 {
            gap.examples.push((id.to_string(), object.id, object.name));
        }
    }
    bump_all(&mut totals.effects, features.effects);
    bump_all(&mut totals.shared_effects, features.shared_effects);
    bump_all(&mut totals.shaders, features.shaders);
    bump_all(&mut totals.combos, features.combos);
    bump_all(
        &mut totals.particles,
        features.particles.into_iter().chain(features.shared_particles),
    );
    bump_all(&mut totals.tex_formats, features.tex_formats.into_keys());
}

pub fn render_json(totals: &Totals) -> serde_json::Value {
    let object_gaps = totals
        .object_gaps
        .values()
        .map(|gap| {
            let (classification, parser_outcome, default_route, strict_route) = match gap
                .kind
                .as_str()
            {
                "transform" => (
                    "already-supported-under-another-representation",
                    "scene parsed; object contributes parent transform state without a render node",
                    "retain for descendant transform resolution",
                    "accepted",
                ),
                "camera" | "shape" | "attachment" => (
                    "native-candidate",
                    "scene parsed; object is not represented by the native model",
                    "warn and skip object",
                    "reject scene startup",
                ),
                _ => (
                    "intentionally-unsupported-until-measured",
                    "scene parsed; object type is unknown to the native model",
                    "warn and skip object",
                    "reject scene startup",
                ),
            };
            serde_json::json!({
                "type": gap.kind,
                "count": gap.count,
                "affected_scenes": gap.scenes,
                "properties": gap.properties,
                "representatives": gap.examples.iter().map(|(scene, id, name)| serde_json::json!({
                    "scene": scene,
                    "id": id,
                    "name": name,
                })).collect::<Vec<_>>(),
                "parser_outcome": parser_outcome,
                "classification": classification,
                "default_route": default_route,
                "strict_route": strict_route,
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "schema": 1,
        "scenes": totals.scenes,
        "parsed": totals.parsed,
        "failed": totals.failures.len(),
        "remaining_object_instances": totals.object_gap_instances,
        "object_shapes": object_gaps,
    })
}

fn scene_pkg_path(dir: &Path) -> Option<std::path::PathBuf> {
    for name in ["scene.pkg", "gifscene.pkg"] {
        let cand = dir.join(name);
        if cand.is_file() {
            return Some(cand);
        }
    }
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.extension().and_then(|ext| ext.to_str()) == Some("pkg") && path.is_file())
}

pub fn audit_workshop(root: &Path) -> Totals {
    let mut totals = Totals::default();
    let Ok(entries) = std::fs::read_dir(root) else {
        return totals;
    };
    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let Ok(project_raw) = std::fs::read(dir.join("project.json")) else {
            continue;
        };
        let Ok(project) = serde_json::from_slice::<Value>(&project_raw) else {
            continue;
        };
        let kind = project.get("type").and_then(Value::as_str).unwrap_or("");
        if !kind.eq_ignore_ascii_case("scene") {
            continue;
        }
        totals.scenes += 1;
        let id =
            dir.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let Some(pkg_path) = scene_pkg_path(&dir) else {
            totals.failures.push((id, "no pkg file".into()));
            continue;
        };
        match Package::open(&pkg_path) {
            Ok(pkg) => {
                if let Some(count) = totals.pkg_versions.get_mut(pkg.version()) {
                    *count += 1;
                } else {
                    totals.pkg_versions.insert(pkg.version().to_string(), 1);
                }
                match scene::extract(&pkg) {
                    Ok(mut features) => {
                        features.audio |= project
                            .get("general")
                            .and_then(|general| general.get("supportsaudioprocessing"))
                            .and_then(Value::as_bool)
                            .unwrap_or(false);
                        let sound = crate::sound::sound_inventory(&pkg).unwrap_or_default();
                        record(&mut totals, &id, features, &sound);
                    }
                    Err(err) => totals.failures.push((id, err)),
                }
            }
            Err(err) => totals.failures.push((id, format!("{err:#}"))),
        }
    }
    totals
}

fn top(map: &BTreeMap<String, usize>, count: usize) -> Vec<(&str, usize)> {
    let mut rows: Vec<(&str, usize)> =
        map.iter().map(|(key, &count)| (key.as_str(), count)).collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    rows.truncate(count);
    rows
}

fn table(out: &mut String, title: &str, map: &BTreeMap<String, usize>, limit: usize, total: usize) {
    if map.is_empty() {
        return;
    }
    let _ = writeln!(out, "\n## {title} ({} distinct)\n", map.len());
    for (name, count) in top(map, limit) {
        let pct = (count * 100).checked_div(total).unwrap_or(0);
        let _ = writeln!(out, "{count:>5}  {pct:>3}%  {name}");
    }
}

pub fn render_markdown(totals: &Totals) -> String {
    let mut out = String::new();
    let total = totals.parsed.max(1);
    let _ = writeln!(out, "# Scene audit\n");
    let _ = writeln!(
        out,
        "scenes: {}  parsed: {}  failed: {}",
        totals.scenes,
        totals.parsed,
        totals.failures.len()
    );
    let cumulative: Vec<usize> = (0..4).map(|tier| totals.tiers[..=tier].iter().sum()).collect();
    let _ = writeln!(
        out,
        "\ntier ladder (cumulative coverage of parsed scenes):\n  t0 static compose only: {} ({}%)\n  t1 + effects:           {} ({}%)\n  t2 + particles:         {} ({}%)\n  t3 + puppet warp:       {} ({}%)",
        cumulative[0],
        cumulative[0] * 100 / total,
        cumulative[1],
        cumulative[1] * 100 / total,
        cumulative[2],
        cumulative[2] * 100 / total,
        cumulative[3],
        cumulative[3] * 100 / total
    );
    let _ = writeln!(
        out,
        "\nfeature incidence (scenes): effects={} shared-effects={} particles={} puppet={} audio={} scripts={} parallax={} sound-objects={} gif-tex={} video-tex={}",
        totals.with_effects,
        totals.with_shared_effects,
        totals.with_particles,
        totals.with_puppet,
        totals.with_audio,
        totals.with_scripts,
        totals.with_parallax,
        totals.with_sound_objects,
        totals.with_tex_gif,
        totals.with_tex_video
    );
    if totals.sound_objects > 0 {
        let _ = writeln!(
            out,
            "\nsound semantics (objects): total={} autostart={} event-driven={} hidden={} multi-clip={} loop={} one-shot={} random={} visibility-bound={} volume-bound={} start-bound={}",
            totals.sound_objects,
            totals.sound_autostart,
            totals.sound_event_driven,
            totals.sound_hidden,
            totals.sound_multiple_clips,
            totals.sound_looped,
            totals.sound_one_shot,
            totals.sound_random,
            totals.sound_visibility_bound,
            totals.sound_volume_bound,
            totals.sound_start_bound,
        );
        let _ = writeln!(
            out,
            "sound semantics (scenes): with-sounds={} multiple-sounds={} event-driven={} event-scene-ids={}",
            totals.with_sound_objects,
            totals.sound_scenes_multiple,
            totals.sound_event_scenes.len(),
            totals.sound_event_scenes.iter().cloned().collect::<Vec<_>>().join(","),
        );
    }
    if totals.tex_failures > 0 || totals.json_failures > 0 || totals.refs_truncated > 0 {
        let _ = writeln!(
            out,
            "\ndegraded parses: tex-header={} json-entry={} ref-budget-hit={}",
            totals.tex_failures, totals.json_failures, totals.refs_truncated
        );
    }
    let _ = writeln!(
        out,
        "\nfull native fidelity: {} of {} parsed scenes ({}%)",
        totals.full_fidelity,
        totals.parsed,
        totals.full_fidelity * 100 / total
    );
    if !totals.gap_scenes.is_empty() {
        let _ = writeln!(out, "\n## native gaps (scenes affected, instances)\n");
        let mut rows: Vec<(&str, usize, usize)> = totals
            .gap_scenes
            .iter()
            .map(|(code, &scenes)| {
                (code.as_str(), scenes, totals.gap_instances.get(code).copied().unwrap_or(0))
            })
            .collect();
        rows.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        for (code, scenes, instances) in rows {
            let pct = (scenes * 100).checked_div(total).unwrap_or(0);
            let _ = writeln!(out, "{scenes:>5}  {pct:>3}%  {instances:>6} inst  {code}");
        }
    }
    table(&mut out, "pkg versions", &totals.pkg_versions, 12, total);
    table(&mut out, "tex formats (textures, count = scenes using)", &totals.tex_formats, 20, total);
    table(&mut out, "effects (in-pkg)", &totals.effects, 40, total);
    table(&mut out, "effects (shared assets, not in pkg)", &totals.shared_effects, 40, total);
    table(&mut out, "shaders", &totals.shaders, 40, total);
    table(&mut out, "combos", &totals.combos, 40, total);
    table(&mut out, "particles", &totals.particles, 25, total);
    if !totals.failures.is_empty() {
        let _ = writeln!(out, "\n## failures (first 15)\n");
        for (id, err) in totals.failures.iter().take(15) {
            let _ = writeln!(out, "{id}: {err}");
        }
    }
    if !totals.samples.is_empty() {
        let _ = writeln!(out, "\n## degraded-parse samples\n");
        for (id, err) in &totals.samples {
            let _ = writeln!(out, "{id}: {err}");
        }
    }
    out
}
