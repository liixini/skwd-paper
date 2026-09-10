use paper_scene::effects::Assets;
use paper_scene::pkg::Package;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (dir, needle) = (&args[1], &args[2]);
    let pkg = Package::open(&std::path::Path::new(dir).join("scene.pkg")).expect("pkg");
    let model = paper_scene::model::load_with(&pkg, &Assets::discover(None)).expect("model");
    println!("---- skipped\n{:?}", model.skipped);
    for layer in &model.layers {
        let effects: Vec<(String, usize, usize, usize)> = layer
            .effects
            .iter()
            .map(|effect| {
                (effect.name.clone(), effect.passes.len(), effect.fbos.len(), effect.swaps.len())
            })
            .collect();
        println!("---- layer {} effects {:?}", layer.name, effects);
    }
    for layer in &model.layers {
        for effect in &layer.effects {
            for pass in &effect.passes {
                if !pass.name.contains(needle.as_str()) {
                    continue;
                }
                println!("==== {} (layer {})", pass.name, layer.name);
                println!("---- constants\n{:?}", pass.values);
                println!(
                    "---- offsets\n{:?}",
                    pass.fragment
                        .uniforms
                        .iter()
                        .map(|u| (u.name.clone(), u.offset, u.std140_span()))
                        .collect::<Vec<_>>()
                );
                let meta = paper_scene::effects::PassMeta::of(pass);
                let bytes = meta.uniform_bytes(
                    paper_scene::effects::FrameClock::default(),
                    None,
                    8,
                    8,
                    (8, 8),
                    &[],
                );
                println!("---- packed {} bytes", bytes.len());
                for u in &pass.fragment.uniforms {
                    let lanes: Vec<f32> = (0..4)
                        .filter_map(|i| {
                            bytes
                                .get(u.offset + i * 4..u.offset + i * 4 + 4)
                                .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
                        })
                        .collect();
                    println!("  {} @{} = {:?}", u.name, u.offset, lanes);
                }
                println!(
                    "---- uniform defaults\n{:?}",
                    pass.fragment
                        .uniforms
                        .iter()
                        .map(|u| (u.name.clone(), u.default.clone(), u.material.clone()))
                        .collect::<Vec<_>>()
                );
                println!("---- GLSL vertex\n{}", pass.vertex.source);
                println!("---- GLSL fragment\n{}", pass.fragment.source);
                if let Some(pair) = &pass.hlsl {
                    println!("---- HLSL vertex\n{}", pair.vertex);
                    println!("---- HLSL fragment\n{}", pair.fragment);
                    if let Some(out) = args.get(3) {
                        for (stage, text, suffix) in [
                            (paper_scene::shader::Stage::Vertex, &pair.vertex, "vert"),
                            (paper_scene::shader::Stage::Fragment, &pair.fragment, "frag"),
                        ] {
                            match paper_scene::hlsl::compile(text, stage, &pass.name) {
                                Ok(words) => {
                                    let bytes: Vec<u8> =
                                        words.iter().flat_map(|w| w.to_le_bytes()).collect();
                                    std::fs::write(format!("{out}.{suffix}.spv"), bytes)
                                        .expect("write spv");
                                }
                                Err(err) => eprintln!("dxc {suffix}: {err:#}"),
                            }
                        }
                    }
                }
                return;
            }
        }
    }
    eprintln!("no pass matching {needle}");
}
