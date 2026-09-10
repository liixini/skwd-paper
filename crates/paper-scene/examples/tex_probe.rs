fn main() {
    let path = std::env::args().nth(1).expect("tex path");
    let bytes = std::fs::read(&path).expect("read");
    match paper_scene::model::load_texture_bytes(&bytes) {
        Some(texture) => {
            println!(
                "ok {}x{} img {}x{} format {:?} frames {} clamp {} nearest {}",
                texture.width,
                texture.height,
                texture.img_width,
                texture.img_height,
                texture.pixels.format,
                texture.frames.len(),
                texture.clamp,
                texture.nearest
            );
            for frame in texture.frames.iter().take(4) {
                println!("  {frame:?}");
            }
        }
        None => println!("undecodable"),
    }
}
