use paper_scene::effects::Assets;
use paper_scene::pkg::Package;

fn main() {
    let assets = Assets::discover(None);
    for dir in std::env::args().skip(1) {
        let path = std::path::Path::new(&dir);
        let Ok(pkg) = Package::open(&path.join("scene.pkg")) else { continue };
        let Ok(model) = paper_scene::model::load_with(&pkg, &assets) else { continue };
        let mut passes = 0usize;
        let mut audio = 0usize;
        let mut names = Vec::new();
        for layer in &model.layers {
            for effect in &layer.effects {
                for pass in &effect.passes {
                    passes += 1;
                    if paper_scene::effects::PassMeta::of(pass).audio_dependent() {
                        audio += 1;
                        names.push(pass.name.clone());
                    }
                }
            }
        }
        names.sort();
        names.dedup();
        println!(
            "{} passes={passes} audio={audio} {:?}",
            path.file_name().unwrap_or_default().to_string_lossy(),
            &names[..names.len().min(3)]
        );
    }
}
