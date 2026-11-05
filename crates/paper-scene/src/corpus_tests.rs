use crate::effects::Assets;
use crate::pkg::Package;
use crate::shader::{self, Stage};
use std::collections::BTreeMap;
use std::io::Write;

fn trace(log: &mut Option<std::fs::File>, stage: &str, label: &str) {
    if let Some(file) = log {
        let _ = writeln!(file, "{stage}\t{label}");
        let _ = file.flush();
    }
}

fn workshop_root() -> Option<std::path::PathBuf> {
    let root =
        std::env::var_os("SKWD_WE_WORKSHOP").map(std::path::PathBuf::from).or_else(|| {
            std::env::var_os("HOME").map(|home| {
                std::path::PathBuf::from(home)
                    .join(".steam/steam/steamapps/workshop/content/431960")
            })
        })?;
    root.is_dir().then_some(root)
}

fn glsl_pass(
    pass: &crate::effects::EffectPass,
    label: &str,
    log: &mut Option<std::fs::File>,
) -> anyhow::Result<()> {
    trace(log, "glsl vertex", label);
    shader::compile(&pass.vertex.source, Stage::Vertex, label)?;
    if let Some(geometry) = &pass.geometry {
        trace(log, "glsl geometry", label);
        shader::compile(&geometry.source, Stage::Geometry, label)?;
    }
    trace(log, "glsl fragment", label);
    shader::compile(&pass.fragment.source, Stage::Fragment, label)?;
    Ok(())
}

#[test]
fn corpus_shader_ratchet() {
    let Some(root) = workshop_root() else {
        eprintln!("no Workshop corpus, skipping");
        return;
    };
    let ceiling: usize = std::env::var("SKWD_SHADER_RATCHET_MAX")
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or(0);
    let fallback_ceiling: usize = std::env::var("SKWD_SHADER_FALLBACK_MAX")
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or(2);
    let mut dirs: Vec<_> = std::fs::read_dir(&root).unwrap().flatten().map(|e| e.path()).collect();
    dirs.sort();
    let mut passes = 0usize;
    let mut hlsl_ok = 0usize;
    let mut particles = 0usize;
    let mut unreadable = 0usize;
    let mut rejected = 0usize;
    let mut skipped = 0usize;
    let mut failed: BTreeMap<String, usize> = BTreeMap::new();
    let mut log = std::env::var_os("SKWD_SHADER_RATCHET_LOG")
        .and_then(|path| std::fs::File::create(path).ok());
    let _compilation = shader::CompilationSession::new().unwrap();
    for dir in dirs {
        let scene = dir.join("scene.pkg");
        if !scene.is_file() {
            continue;
        }
        trace(&mut log, "package", &scene.display().to_string());
        let pkg = match Package::open(&scene) {
            Ok(pkg) => pkg,
            Err(error) => {
                unreadable += 1;
                trace(&mut log, "package rejected", &format!("{}: {error:#}", scene.display()));
                continue;
            }
        };
        trace(&mut log, "model", &scene.display().to_string());
        let mut model = match crate::model::load_from_dir(&pkg, &dir) {
            Ok(model) => model,
            Err(error) => {
                rejected += 1;
                trace(&mut log, "model rejected", &format!("{}: {error:#}", dir.display()));
                continue;
            }
        };
        if let Some(scripts) = &mut model.scripts {
            model
                .particles
                .extend(scripts.take_created_particles().into_iter().map(|(_, layer)| layer));
        }
        skipped += model.skipped.len();
        for reason in &model.skipped {
            trace(&mut log, "model skipped", &format!("{}: {reason}", dir.display()));
        }
        let images = model.layers.iter().flat_map(|layer| {
            layer
                .material
                .iter()
                .map(|material| &material.pass)
                .chain(layer.material.iter().filter_map(|material| {
                    material.skin.as_ref()?.channels.as_ref().map(|channels| &channels.pass)
                }))
                .chain(layer.effects.iter().flat_map(|effect| effect.passes.iter()))
        });
        let meshes =
            model.meshes.iter().flat_map(|layer| &layer.materials).map(|material| &material.pass);
        for pass in images.chain(meshes) {
            let label = format!("{}:{}", dir.display(), pass.name);
            if pass.name == crate::effects::COLOR_BLEND_MESH_SHADER {
                if glsl_pass(pass, &label, &mut log).is_err() {
                    *failed.entry(pass.name.clone()).or_default() += 1;
                }
                continue;
            }
            passes += 1;
            let hlsl = pass.hlsl.as_ref().map(|pair| {
                trace(&mut log, "hlsl vertex", &label);
                crate::hlsl::compile(&pair.vertex, Stage::Vertex, &label).and_then(|_| {
                    trace(&mut log, "hlsl fragment", &label);
                    crate::hlsl::compile(&pair.fragment, Stage::Fragment, &label)
                })
            });
            if matches!(hlsl, Some(Ok(_))) {
                hlsl_ok += 1;
                continue;
            }
            let glsl = glsl_pass(pass, &label, &mut log);
            if let Some(log) = log.as_mut() {
                let hlsl_err = match &hlsl {
                    None => "no hlsl pair".to_string(),
                    Some(Err(err)) => format!("{err:#}"),
                    Some(Ok(_)) => String::new(),
                };
                let glsl_err = glsl.as_ref().err().map_or(String::new(), |err| format!("{err:#}"));
                let _ = writeln!(
                    log,
                    "{label}\tHLSL: {}\tGLSL: {}",
                    hlsl_err.replace('\n', " | "),
                    glsl_err.replace('\n', " | ")
                );
            }
            if glsl.is_err() {
                *failed.entry(pass.name.clone()).or_default() += 1;
            }
        }
        let mut pending: Vec<_> = model.particles.iter().map(|layer| &layer.system).collect();
        while let Some(system) = pending.pop() {
            pending.extend(&system.children);
            let Some(pass) = &system.pass else { continue };
            particles += 1;
            let label = format!("{}:particles:{}", dir.display(), pass.name);
            if let Err(error) = glsl_pass(pass, &label, &mut log) {
                *failed.entry(format!("particles:{}", pass.name)).or_default() += 1;
                trace(&mut log, "particle shader failure", &format!("{label}: {error:#}"));
            }
        }
    }
    let total: usize = failed.values().sum();
    let fallbacks = passes - hlsl_ok;
    eprintln!(
        "corpus shader ratchet: passes={passes} hlsl_ok={hlsl_ok} fallbacks={fallbacks} particles={particles} failed={total} unreadable={unreadable} rejected={rejected} skipped={skipped} ceilings={fallback_ceiling}/{ceiling}"
    );
    for (name, count) in &failed {
        eprintln!("  {count:4} {name}");
    }
    assert!(passes + particles > 0, "no shader passes examined");
    assert!(total <= ceiling, "shader failures {total} exceed ratchet ceiling {ceiling}");
    assert!(
        fallbacks <= fallback_ceiling,
        "DXC fallbacks {fallbacks} exceed ratchet ceiling {fallback_ceiling}"
    );
    let _ = Assets::discover(None);
}
