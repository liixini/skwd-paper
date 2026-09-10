use crate::effects::Assets;
use crate::pkg::Package;
use crate::shader::{self, Stage};
use std::collections::BTreeMap;

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
    let mut failed: BTreeMap<String, usize> = BTreeMap::new();
    let mut log = std::env::var_os("SKWD_SHADER_RATCHET_LOG")
        .and_then(|path| std::fs::File::create(path).ok());
    for dir in dirs {
        let scene = dir.join("scene.pkg");
        if !scene.is_file() {
            continue;
        }
        let Ok(pkg) = Package::open(&scene) else { continue };
        let Ok(model) = crate::model::load_from_dir(&pkg, &dir) else { continue };
        for layer in &model.layers {
            for effect in &layer.effects {
                for pass in &effect.passes {
                    passes += 1;
                    let label = format!("{}:{}", dir.display(), pass.name);
                    let hlsl = pass.hlsl.as_ref().map(|pair| {
                        crate::hlsl::compile(&pair.vertex, Stage::Vertex, &label).and_then(|_| {
                            crate::hlsl::compile(&pair.fragment, Stage::Fragment, &label)
                        })
                    });
                    if matches!(hlsl, Some(Ok(_))) {
                        hlsl_ok += 1;
                        continue;
                    }
                    let glsl = shader::compile(&pass.vertex.source, Stage::Vertex, &label)
                        .and_then(|_| {
                            shader::compile(&pass.fragment.source, Stage::Fragment, &label)
                        });
                    if let Some(log) = log.as_mut() {
                        use std::io::Write;
                        let hlsl_err = match &hlsl {
                            None => "no hlsl pair".to_string(),
                            Some(Err(err)) => format!("{err:#}"),
                            Some(Ok(_)) => String::new(),
                        };
                        let glsl_err =
                            glsl.as_ref().err().map_or(String::new(), |err| format!("{err:#}"));
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
            }
        }
    }
    let total: usize = failed.values().sum();
    let fallbacks = passes - hlsl_ok;
    eprintln!(
        "corpus shader ratchet: passes={passes} hlsl_ok={hlsl_ok} fallbacks={fallbacks} failed={total} ceilings={fallback_ceiling}/{ceiling}"
    );
    for (name, count) in &failed {
        eprintln!("  {count:4} {name}");
    }
    assert!(total <= ceiling, "shader failures {total} exceed ratchet ceiling {ceiling}");
    assert!(
        fallbacks <= fallback_ceiling,
        "DXC fallbacks {fallbacks} exceed ratchet ceiling {fallback_ceiling}"
    );
    let _ = Assets::discover(None);
}
