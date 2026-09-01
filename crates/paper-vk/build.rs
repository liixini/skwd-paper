use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=FFMPEG_DIR");
    if std::env::var_os("FFMPEG_DIR").is_some() {
        println!("cargo:rustc-link-arg-bins=-Wl,-rpath,$ORIGIN/../lib/skwd-paper");
    }
    if std::env::var_os("CARGO_FEATURE_SHARED_DEVICE").is_some() {
        compile_ffmpeg_vulkan();
    }
    let out = std::env::var("OUT_DIR").unwrap();
    for (src, dst) in [
        ("shaders/fullscreen.vert", "fullscreen.vert.spv"),
        ("shaders/nv12.frag", "nv12.frag.spv"),
        ("shaders/rgba.frag", "rgba.frag.spv"),
        ("shaders/layer.vert", "layer.vert.spv"),
        ("shaders/puppet.vert", "puppet.vert.spv"),
        ("shaders/layer.frag", "layer.frag.spv"),
        ("shaders/sand_base.vert", "sand_base.vert.spv"),
    ] {
        println!("cargo:rerun-if-changed={src}");
        compile(src, &format!("{out}/{dst}"));
    }
    let sand_vert = format!("{out}/sand_grain.vert");
    let sand_frag = format!("{out}/sand_grain.frag");
    std::fs::write(
        &sand_vert,
        paper_shaders::sand_grain_point_vert(paper_shaders::Dialect::Vulkan450),
    )
    .expect("write composed sand vert");
    std::fs::write(
        &sand_frag,
        paper_shaders::sand_grain_point_frag(paper_shaders::Dialect::Vulkan450),
    )
    .expect("write composed sand frag");
    compile(&sand_vert, &format!("{out}/sand_grain.vert.spv"));
    compile(&sand_frag, &format!("{out}/sand_grain.frag.spv"));
    let base_frag = format!("{out}/sand_base.frag");
    std::fs::write(&base_frag, paper_shaders::SAND_BASE_FRAG_VK).expect("write sand base frag");
    compile(&base_frag, &format!("{out}/sand_base.frag.spv"));

    let mut table = String::from("pub static EFFECT_SPV: &[&[u8]] = &[\n");
    for (idx, (name, gl_src)) in paper_shaders::EFFECTS.iter().enumerate() {
        let frag = format!("{out}/fx_{idx}.frag");
        std::fs::write(&frag, paper_shaders::effect_frag_vk(gl_src))
            .unwrap_or_else(|_| panic!("write fx {name}"));
        compile(&frag, &format!("{out}/fx_{idx}.frag.spv"));
        table.push_str(&format!(
            "    include_bytes!(concat!(env!(\"OUT_DIR\"), \"/fx_{idx}.frag.spv\")),\n"
        ));
    }
    table.push_str("];\n");
    std::fs::write(format!("{out}/effects_gen.rs"), table).expect("write effects table");
}

fn compile_ffmpeg_vulkan() {
    println!("cargo:rerun-if-changed=native/ffmpeg_vulkan.c");
    println!("cargo:rerun-if-env-changed=FFMPEG_DIR");
    println!("cargo:rerun-if-env-changed=VULKAN_INCLUDE_DIR");
    let mut build = cc::Build::new();
    build.file("native/ffmpeg_vulkan.c");
    build.flag_if_supported("-Wno-deprecated-declarations");
    if let Ok(root) = std::env::var("FFMPEG_DIR") {
        build.include(std::path::Path::new(&root).join("include"));
    }
    if let Ok(library) = pkg_config::Config::new().cargo_metadata(false).probe("libavutil") {
        for include in library.include_paths {
            build.include(include);
        }
    }
    let vulkan = std::env::var_os("VULKAN_INCLUDE_DIR")
        .map(std::path::PathBuf::from)
        .filter(|path| path.join("vulkan/vulkan.h").is_file())
        .or_else(|| {
            let path = std::path::PathBuf::from("/usr/include");
            path.join("vulkan/vulkan.h").is_file().then_some(path)
        })
        .or_else(|| {
            let path = std::env::var_os("HOME")
                .map(std::path::PathBuf::from)?
                .join(".cache/skwd-build/Vulkan-Headers/include");
            path.join("vulkan/vulkan.h").is_file().then_some(path)
        })
        .expect("Vulkan headers not found; set VULKAN_INCLUDE_DIR");
    build.include(vulkan);
    build.compile("skwd-paper-ffmpeg-vulkan");
}

fn compile(src: &str, dst: &str) {
    let status = Command::new("glslc")
        .arg(src)
        .arg("-o")
        .arg(dst)
        .status()
        .expect("glslc not found: install shaderc (pacman -S shaderc) to build skwd-wall-vk");
    assert!(status.success(), "glslc failed on {src}");
}
