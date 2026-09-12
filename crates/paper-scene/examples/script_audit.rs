use anyhow::Result;
use paper_scene::{pkg::Package, script::SceneScripts};
use serde_json::json;
use std::path::Path;

fn main() -> Result<()> {
    let root = std::env::args().nth(1).expect("workshop root");
    let mut rows = Vec::new();
    for entry in std::fs::read_dir(root)?.flatten() {
        let dir = entry.path();
        let Ok(bytes) = std::fs::read(dir.join("project.json")) else {
            continue;
        };
        let project: serde_json::Value = serde_json::from_slice(&bytes)?;
        if !project["type"].as_str().is_some_and(|s| s.eq_ignore_ascii_case("scene")) {
            continue;
        }
        let path = std::fs::read_dir(&dir)?
            .flatten()
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|e| e == "pkg"));
        let Some(path) = path else {
            continue;
        };
        let package = Package::open(Path::new(&path))?;
        let Some(mut scene) = package.find_json("scene.json")? else {
            continue;
        };
        let props = paper_scene::effects::parse_properties(&project);
        let start = std::time::Instant::now();
        match SceneScripts::load(&mut scene, &props, &project) {
            Ok(Some(mut host)) => {
                let load_us = start.elapsed().as_micros();
                let start = std::time::Instant::now();
                for frame in 1..=60 {
                    host.tick(frame as f32 / 60.0, 1.0 / 60.0, [0.5; 2])?;
                }
                rows.push(json!({"id":dir.file_name().unwrap().to_string_lossy(),"bindings":host.bindings,"heap_bytes":host.heap_bytes(),"load_us":load_us,"tick_us":start.elapsed().as_micros()/60,"animated":host.animated(),"errors":host.diagnostics}));
            }
            Ok(None) => {}
            Err(error) => rows.push(
                json!({"id":dir.file_name().unwrap().to_string_lossy(),"error":error.to_string()}),
            ),
        }
    }
    println!("{}", serde_json::to_string_pretty(&rows)?);
    Ok(())
}
