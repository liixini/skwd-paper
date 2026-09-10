fn main() {
    let dir = std::env::args().nth(1).expect("scene dir");
    let pkg =
        paper_scene::pkg::Package::open(std::path::Path::new(&dir).join("scene.pkg").as_path())
            .expect("pkg");
    let model = paper_scene::model::load(&pkg).expect("model");
    for layer in &model.layers {
        println!(
            "{} center={:?} size={:?} scale={:?} angle={} alpha={} color={:?} tex={}x{} visible={} order={}",
            layer.name,
            layer.center,
            layer.size,
            layer.scale,
            layer.angle,
            layer.alpha,
            layer.color,
            layer.texture.width,
            layer.texture.height,
            layer.visible,
            layer.scene_order
        );
    }
    for particle in &model.particles {
        let system = &particle.system;
        println!(
            "PARTICLE order={} origin={:?} scale={:?} max={} start={} emitters={} inits={} ops={} texture={} pass={} blend={:?} persp={} rate_scale={} size_scale={} alpha={} tint={:?}",
            particle.scene_order,
            system.origin,
            system.scale3,
            system.max_count,
            system.start_time,
            system.emitters.len(),
            system.initializers.len(),
            system.operators.len(),
            system
                .texture
                .as_ref()
                .map_or("none".to_string(), |t| format!("{}x{}", t.width, t.height)),
            system.pass.as_ref().map_or("none".to_string(), |p| p.name.clone()),
            system.blend,
            system.perspective,
            system.rate_scale,
            system.size_scale,
            system.alpha,
            system.tint
        );
    }
    println!("skipped: {:?}", model.skipped);
}
