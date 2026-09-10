use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).expect("workspace root").to_path_buf()
}

fn rust_files(directory: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("read {}: {error}", directory.display()))
    {
        let path = entry.expect("directory entry").path();
        if path.is_dir() {
            rust_files(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
}

fn assert_private_checkout(workflow: &str, repository: &str, revision: &str, path: &str) {
    let checkout = format!(
        "        with:\n          repository: {repository}\n          ref: {revision}\n          path: {path}\n          token: ${{{{ secrets.SKWD_SUITE_READ_TOKEN }}}}\n          persist-credentials: false"
    );
    assert!(workflow.contains(&checkout), "{repository} checkout is not securely wired");
}

#[test]
fn renderer_packages_keep_stable_compatibility_names() {
    let root = root();
    for (directory, package, binary) in [
        ("crates/paper-vk", "skwd-wall-vk", "skwd-wall-vk"),
        ("crates/paper-still", "skwd-wall-still", "skwd-wall-still"),
    ] {
        let manifest = fs::read_to_string(root.join(directory).join("Cargo.toml"))
            .unwrap_or_else(|error| panic!("read {directory}/Cargo.toml: {error}"));
        assert!(manifest.contains(&format!("name = \"{package}\"")));
        assert!(manifest.contains(&format!("name = \"{binary}\"")));
        for forbidden in [
            "skwd-wall-core",
            "skwd-walld",
            "skwd-config",
            "skwd-wall-scan",
            "skwd-wall-effects",
            "skwd-steam",
            "wall-proto",
            "wall-geom",
            "skwd-log",
        ] {
            assert!(!manifest.contains(forbidden), "{package} depends on {forbidden}");
        }
    }
}

#[test]
fn tinier_worker_stays_private_and_paper_owned() {
    let root = root();
    let manifest = fs::read_to_string(root.join("crates/paper-tinier/Cargo.toml"))
        .expect("read paper-tinier manifest");
    assert!(manifest.contains("name = \"paper-tinier\""));
    assert!(manifest.contains("name = \"skwd-paper-tinier\""));
    assert!(manifest.contains("paper-control = { path = \"../paper-control\" }"));
    let package_manifest =
        fs::read_to_string(root.join("packaging/manifest.txt")).expect("read package manifest");
    assert!(package_manifest.contains("usr/lib/skwd-paper/skwd-paper-tinier"));
    assert!(!package_manifest.contains("usr/bin/skwd-paper-tinier"));
}

#[test]
fn plasma_vk_stream_falls_back_when_external_export_is_unavailable() {
    let source = fs::read_to_string(root().join("crates/paper-vk/src/preview.rs"))
        .expect("read Vulkan preview stream");
    let fallback = source
        .find("external stream unavailable, using CPU frames")
        .expect("external stream fallback warning");
    assert!(
        source[fallback..].contains("return video_stream(path, width, height, fps, write_header);")
    );
    let control = source[fallback..].find("Ctl::start").expect("stream control after fallback");
    assert!(control > 0);
}

#[test]
fn local_dependencies_stay_inside_the_repository() {
    let root = root().canonicalize().expect("canonical workspace root");
    let mut manifests = Vec::new();
    for entry in fs::read_dir(root.join("crates")).expect("read crates") {
        let path = entry.expect("crate entry").path().join("Cargo.toml");
        if path.is_file() {
            manifests.push(path);
        }
    }
    for manifest_path in manifests {
        let manifest = fs::read_to_string(&manifest_path).expect("read manifest");
        for line in manifest.lines().filter(|line| line.contains("path = \"")) {
            let path = line.split("path = \"").nth(1).expect("path value");
            let path = path.split('"').next().expect("path terminator");
            let resolved = manifest_path
                .parent()
                .expect("manifest directory")
                .join(path)
                .canonicalize()
                .unwrap_or_else(|error| {
                    panic!("resolve {} from {}: {error}", path, manifest_path.display())
                });
            assert!(
                resolved.starts_with(&root),
                "{} escapes {}",
                resolved.display(),
                root.display()
            );
        }
    }
}

#[test]
fn ffmpeg_bindings_are_stock_current_and_not_vendored() {
    let root = root();
    let manifest = fs::read_to_string(root.join("Cargo.toml")).expect("read workspace manifest");
    let lockfile = fs::read_to_string(root.join("Cargo.lock")).expect("read workspace lockfile");
    assert!(!manifest.contains("ffmpeg-sys-the-third = { git"));
    assert!(!manifest.contains("skwd-ffmpeg-sys"));
    for crate_manifest in ["crates/paper-audio/Cargo.toml", "crates/paper-vk/Cargo.toml"] {
        let contents = fs::read_to_string(root.join(crate_manifest)).expect("read FFmpeg consumer");
        assert!(contents.contains("ffmpeg-the-third = { version = \"6\""));
    }
    let package = lockfile
        .split("[[package]]")
        .find(|entry| entry.contains("name = \"ffmpeg-sys-the-third\""))
        .expect("locked FFmpeg sys package");
    assert!(package.contains("version = \"6.0.0+ffmpeg-9.0\""));
    assert!(package.contains("source = \"registry+https://github.com/rust-lang/crates.io-index\""));
    assert!(!root.join("vendor").exists(), "third-party forks must not be vendored");
    let shim = fs::read_to_string(root.join("crates/paper-vk/native/ffmpeg_vulkan.c"))
        .expect("read Paper Vulkan shim");
    assert!(shim.contains("#include <libavutil/hwcontext_vulkan.h>"));
    assert!(shim.contains("LIBAVUTIL_VERSION_MAJOR >= 61"));
    assert!(shim.contains("AV_VERSION_INT(57, 28, 100)"));
    assert!(shim.contains("AV_VERSION_INT(58, 2, 100)"));
    assert!(shim.contains("AV_VERSION_INT(59, 34, 100)"));
    assert!(shim.contains("SKWD_AV_VK_HAS_FIXED_QUEUES"));
    assert!(shim.contains("SKWD_AV_VK_HAS_SYNC_QUEUES"));
    assert!(shim.contains("return queue_sync ? 2 : 1;"));
    assert!(shim.contains("sizeof(context->qf) / sizeof(context->qf[0])"));
    assert!(!shim.contains("nb_qf >= 64"));
    let shared = fs::read_to_string(root.join("crates/paper-vk/src/shared.rs"))
        .expect("read shared device policy");
    assert!(shared.contains("pub queue_sync: bool"));
    assert!(shared.contains("software_decode_required"));
    let upload = fs::read_to_string(root.join("crates/paper-vk/src/app/shared_upload.rs"))
        .expect("read shared upload policy");
    assert!(upload.contains("if !sd.queue_sync"));
    assert!(upload.contains("super::upload::run_upload"));
    let dmabuf = fs::read_to_string(root.join("crates/paper-vk/src/app/dmabuf_present.rs"))
        .expect("read dmabuf decoder policy");
    assert!(dmabuf.contains("software_decode_required(sd.software, sd.queue_sync)"));
    assert!(dmabuf.contains("start_swap(&req, sd, force_software_decode"));
    assert_eq!(dmabuf.matches("pending_swap = Some(begin_swap(").count(), 1);
    let preview = fs::read_to_string(root.join("crates/paper-vk/src/preview.rs"))
        .expect("read preview decoder policy");
    assert_eq!(
        preview.matches("software_decode_required(shared.software, shared.queue_sync)").count(),
        2
    );
    assert_eq!(
        preview.matches("crate::decode::open_decoder(").count(),
        2,
        "both preview streams must reach VAAPI through the decode cascade"
    );
    let zero_copy_build =
        fs::read_to_string(root.join("scripts/build-vk-zerocopy.sh")).expect("read build helper");
    assert!(!zero_copy_build.contains("SKWD_FFMPEG_VULKAN"));
    assert!(zero_copy_build.contains("path = shared-device (zero-copy)"));
    let perf = fs::read_to_string(root.join("scripts/perf-sweep.py")).expect("read perf sweep");
    assert!(perf.contains("ZERO_COPY_MARKER = b\"path = shared-device (zero-copy)\""));
}

#[test]
fn presentation_paths_open_decoders_through_the_fallback_cascade() {
    let root = root();
    for relative in [
        "crates/paper-vk/src/app/shared_upload.rs",
        "crates/paper-vk/src/app/upload.rs",
        "crates/paper-vk/src/app/dmabuf_helpers.rs",
        "crates/paper-vk/src/preview.rs",
    ] {
        let source = fs::read_to_string(root.join(relative)).expect("read renderer path");
        assert!(source.contains("open_decoder("), "{relative} does not use the decode cascade");
        assert!(
            !source.contains("VulkanDecoder::open"),
            "{relative} opens a Vulkan decoder with no VAAPI or software fallback"
        );
    }
    let cascade = fs::read_to_string(root.join("crates/paper-vk/src/decode.rs"))
        .expect("read decoder cascade");
    assert!(cascade.contains("Ok(AnyDecoder::Vaapi(decoder))"));
    assert!(cascade.contains("Ok(AnyDecoder::Sw(SwDecoder::open(path)?))"));
}

#[test]
fn scene_capture_readback_uses_drop_owned_vulkan_resources() {
    let source = fs::read_to_string(root().join("crates/paper-vk/src/vk/scene.rs"))
        .expect("read native scene renderer");
    let readback = source
        .split_once("pub fn read_scene_target")
        .map(|(_, body)| body)
        .expect("native scene readback function");

    assert!(
        readback.contains("let readback = self.create_readback_buf(size)?;"),
        "scene capture must acquire the drop-owned readback allocation before fallible GPU work"
    );
    assert!(readback.contains("readback.buffer"));
    assert!(readback.contains("readback.ptr"));
    for raw_lifecycle in [
        "create_buffer(",
        "allocate_memory(",
        "map_memory(",
        "unmap_memory(",
        "destroy_buffer(",
        "free_memory(",
    ] {
        assert!(
            !readback.contains(raw_lifecycle),
            "scene capture reintroduced an early-return leak through {raw_lifecycle}"
        );
    }
}

#[test]
fn nv12_direct_present_uses_the_decode_cascade_without_losing_the_requested_source() {
    let source = fs::read_to_string(root().join("crates/paper-vk/src/app/dmabuf_present.rs"))
        .expect("read nv12 presentation path");
    assert!(
        !source.contains("VulkanDecoder::open"),
        "run_nv12 must not hard-open a Vulkan decoder it cannot fall back from"
    );
    assert!(
        source.contains("decode::open_decoder("),
        "run_nv12 must retain the Vulkan, VAAPI, and software fallback cascade"
    );
    assert!(
        source.contains("decode::AnyDecoder::Vaapi(_)")
            && source.contains("DirectPresentation::Vaapi("),
        "run_nv12 must directly present VAAPI source DMA-BUFs"
    );
    assert!(
        source.contains("release_vaapi_frames(target, &mut presentation)"),
        "run_nv12 must release retained VAAPI frames after compositor buffer release"
    );
    let compact = source.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        compact.contains("run_shared_dmabuf_with( target, &path,"),
        "the nv12 hand-off must carry the requested source, not the source it started on"
    );
}

#[test]
fn private_ci_pins_system_verifier_checkout() {
    let workflow = fs::read_to_string(root().join(".forgejo/workflows/deep-verification.yml"))
        .expect("read Forgejo workflow");
    assert_private_checkout(
        &workflow,
        "liixini/skwd-verify",
        "bf9f7f4b86c2e5ac8fcef7f2abcdc3a6e5b2cc15",
        ".ci/checkouts/skwd-verify",
    );
    assert_eq!(workflow.matches("token: ${{ secrets.SKWD_SUITE_READ_TOKEN }}").count(), 1);
    assert_eq!(workflow.matches("persist-credentials: false").count(), 2);
    for required in [
        "working-directory: .ci/checkouts/skwd-verify",
        "pkg-config --exists dav1d",
        "test \"$(cc -print-file-name=libyuv.so)\" != libyuv.so",
    ] {
        assert!(workflow.contains(required), "private CI is missing {required}");
    }
    assert!(!workflow.contains("skwd-ffmpeg-sys"));
}

#[test]
fn live_system_tests_are_owned_by_the_verifier() {
    let root = root();
    for retired in ["scripts/e2e-scene.py", "scripts/e2e_lib.py"] {
        assert!(!root.join(retired).exists(), "retired local system test returned: {retired}");
    }
}

#[test]
fn control_crate_contains_only_renderer_contract_modules() {
    let source = fs::read_to_string(root().join("crates/paper-control/src/lib.rs"))
        .expect("read paper-control map");
    for required in [
        "media",
        "multi_video",
        "output_target",
        "paper_command",
        "protocol",
        "socket",
        "stdin_reader",
        "still_command",
    ] {
        assert!(source.contains(&format!("mod {required};")), "missing {required}");
    }
    for unrelated in [
        "analysis_status",
        "client",
        "download",
        "playlist",
        "rpc",
        "sources",
        "task_status",
        "wallpaper_item",
        "workspace",
    ] {
        assert!(!source.contains(unrelated), "unrelated control module {unrelated}");
    }
}

#[test]
fn repository_source_has_no_retired_local_crate_imports() {
    let root = root();
    let mut files = Vec::new();
    rust_files(&root.join("crates"), &mut files);
    for path in files {
        if path.ends_with("tests/repository_guard.rs") {
            continue;
        }
        let source = fs::read_to_string(&path).expect("read Rust source");
        for retired in ["wall_proto::", "wall_geom::", "skwd_log::"] {
            assert!(!source.contains(retired), "{} imports {retired}", path.display());
        }
    }
}

#[test]
fn video_renderer_keeps_detached_standalone_mode() {
    let root = root();
    let bootstrap = fs::read_to_string(root.join("crates/paper-vk/src/app/bootstrap.rs"))
        .expect("read video renderer bootstrap");
    let control =
        fs::read_to_string(root.join("crates/paper-vk/src/ctl.rs")).expect("read control adapter");
    assert!(bootstrap.contains("--standalone"));
    assert!(bootstrap.contains("ctl::set_stdin_enabled"));
    assert!(control.contains("with_stdin && stdin_enabled()"));
    assert!(!bootstrap.contains("args[1] == \"--multi\""));
}
