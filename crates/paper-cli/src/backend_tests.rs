use super::*;
use std::fs;
use std::os::fd::AsRawFd;

fn executable(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

fn command_parts(command: &std::process::Command) -> (String, Vec<String>) {
    (
        command.get_program().to_string_lossy().into_owned(),
        command.get_args().map(|argument| argument.to_string_lossy().into_owned()).collect(),
    )
}

#[test]
fn plasma_presentation_uses_paper_backend_routing() {
    let temp = tempfile::tempdir().unwrap();
    let vk = temp.path().join("skwd-wall-vk");
    let still = temp.path().join("skwd-wall-still");
    let tinier = temp.path().join("skwd-paper-tinier");
    for path in [&vk, &still, &tinier] {
        executable(path, "#!/bin/sh\nexit 0\n");
    }
    let backends =
        BackendPaths::from_executables_with_tinier(vk.clone(), still.clone(), tinier.clone());

    let mut static_assignment =
        Assignment::new(vec!["DP-1".into()], Source::static_file("/wall/a.png"));
    static_assignment.fill_mode = paper_control::FillMode::Fit;
    let command =
        super::plasma_command(&backends, &static_assignment, "1920x1080", 60, 3, false, true)
            .unwrap();
    let (program, arguments) = command_parts(&command);
    assert_eq!(program, still.to_string_lossy());
    assert_eq!(
        arguments,
        ["*", "/wall/a.png", "--frame-stream", "1920x1080", "--fill-mode", "fit"]
    );

    let tinier_assignment =
        Assignment::new(vec!["DP-1".into()], Source::tinier_video("/wall/loop.ivf", "30000/1001"));
    let command =
        super::plasma_command(&backends, &tinier_assignment, "1280x720", 60, 3, true, true)
            .unwrap();
    let (program, arguments) = command_parts(&command);
    assert_eq!(program, tinier.to_string_lossy());
    assert_eq!(
        arguments,
        [
            "--frame-stream",
            "1280x720",
            "--fill-mode",
            "fill",
            "--paused",
            "/wall/loop.ivf",
            "30000/1001",
            "bt709",
        ]
    );

    let video_assignment =
        Assignment::new(vec!["DP-1".into()], Source::video("/wall/loop.mp4", None));
    let command =
        super::plasma_command(&backends, &video_assignment, "2560x1440", 144, 3, false, true)
            .unwrap();
    let (program, arguments) = command_parts(&command);
    assert_eq!(program, vk.to_string_lossy());
    assert!(arguments.starts_with(&["--video-stream".into(), "/wall/loop.mp4".into()]));
    assert!(arguments.windows(2).any(|pair| pair == ["--stream-fd", "3"]));
}

#[test]
fn plasma_presentation_does_not_require_a_second_wayland_surface() {
    let temp = tempfile::tempdir().unwrap();
    let still = temp.path().join("skwd-wall-still");
    executable(&still, "#!/bin/sh\nexit 0\n");
    let mut backends = BackendPaths::from_executables(temp.path().join("vk"), still.clone());
    backends.still.capability.dependencies.push(RuntimeDependencyStatus {
        name: "wayland_connection".into(),
        available: false,
        detail: "unavailable by design".into(),
    });
    let assignment = Assignment::new(vec!["DP-1".into()], Source::static_file("/wall/a.png"));
    let command =
        super::plasma_command(&backends, &assignment, "1920x1080", 60, 3, false, true).unwrap();
    assert_eq!(command.get_program(), still);
}

#[test]
fn plasma_transition_is_a_one_shot_paper_prelude() {
    let temp = tempfile::tempdir().unwrap();
    let vk = temp.path().join("skwd-wall-vk");
    let still = temp.path().join("skwd-wall-still");
    executable(&vk, "#!/bin/sh\nexit 0\n");
    executable(&still, "#!/bin/sh\nexit 0\n");
    let backends = BackendPaths::from_executables(vk.clone(), still.clone());
    let mut assignment = Assignment::new(vec!["DP-1".into()], Source::static_file("/wall/b.png"));
    assignment.transition = Some(paper_control::TransitionPolicy {
        from: Some("/wall/a.png".into()),
        effect: Some("inkwell-drop".into()),
        duration_ms: Some(700),
    });
    let transition =
        super::plasma_transition_command(&backends, &assignment, "1920x1080", 60).unwrap().unwrap();
    let (program, arguments) = command_parts(&transition);
    assert_eq!(program, vk.to_string_lossy());
    assert!(arguments.windows(2).any(|pair| pair == ["--preview-stream", "/wall/b.png"]));
    assert!(arguments.windows(2).any(|pair| pair == ["--transition-from", "/wall/a.png"]));
    assert!(arguments.windows(2).any(|pair| pair == ["--shader", "inkwell-drop"]));
    assert!(arguments.contains(&"--preview-once".to_string()));
    assert!(arguments.contains(&"--stream-no-header".to_string()));

    let steady =
        super::plasma_command(&backends, &assignment, "1920x1080", 60, 3, false, false).unwrap();
    let (_, arguments) = command_parts(&steady);
    assert!(arguments.contains(&"--stream-no-header".to_string()));
    assert!(!arguments.contains(&"--transition-from".to_string()));
}

#[test]
fn tinier_worker_argv() {
    let temp = tempfile::tempdir().unwrap();
    let trace = temp.path().join("trace");
    let worker = temp.path().join("skwd-paper-tinier");
    executable(
        &worker,
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nprintf '%s\\n' \"$SKWD_PAPER_GENERATION\" >> '{}'\nwhile IFS= read -r line; do :; done\n",
            trace.display(),
            trace.display()
        ),
    );
    let backends = BackendPaths::from_executables_with_tinier(
        temp.path().join("vk"),
        temp.path().join("still"),
        worker,
    );
    let assignment =
        Assignment::new(vec!["DP-2".into()], Source::tinier_video("/wall/loop.ivf", "30000/1001"));
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
        let spawned = backends
            .spawn(assignment, "DP-2".into(), Path::new("/tmp/paper.sock"), 91, None)
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        while !trace.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        spawned.stop().await;
    });
    assert_eq!(
        fs::read_to_string(trace).unwrap(),
        "--output\nDP-2\n/wall/loop.ivf\n30000/1001\nbt709\n91\n"
    );
}

#[test]
fn tinier_worker_missing() {
    let temp = tempfile::tempdir().unwrap();
    let backends =
        BackendPaths::from_executables(temp.path().join("vk"), temp.path().join("still"));
    let assignment =
        Assignment::new(vec!["DP-2".into()], Source::tinier_video("/wall/loop.ivf", "30"));
    let error = backends
        .spawn(assignment, "DP-2".into(), Path::new("/tmp/paper.sock"), 92, None)
        .err()
        .unwrap();
    assert!(error.to_string().contains("tinier video media capability requires skwd-paper-tinier"));
}

#[test]
fn still_package_works_without_vk_package() {
    let temp = tempfile::tempdir().unwrap();
    let worker = temp.path().join("skwd-wall-still");
    executable(&worker, "#!/bin/sh\nwhile IFS= read -r line; do :; done\n");
    let missing = temp.path().join("skwd-wall-vk");
    let backends = BackendPaths::from_executables(missing.clone(), worker);
    let capabilities = backends.capabilities();
    assert!(!capabilities[0].present);
    assert!(capabilities[1].executable_file);
    let error = backends
        .spawn(
            Assignment::new(vec!["DP-1".into()], Source::video("/wall/loop.mp4", None)),
            "DP-1".into(),
            Path::new("/tmp/paper.sock"),
            93,
            None,
        )
        .err()
        .unwrap();
    assert!(error.to_string().contains("video media capability requires skwd-wall-vk"));
    assert!(error.to_string().contains(&missing.display().to_string()));
    let assignment = Assignment::new(vec!["DP-1".into()], Source::static_file("/wall/a.png"));
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
        let worker = backends
            .spawn(assignment, "DP-1".into(), Path::new("/tmp/paper.sock"), 94, None)
            .unwrap();
        worker.stop().await;
    });
}

#[test]
fn vk_package_works_without_still_package() {
    let temp = tempfile::tempdir().unwrap();
    let worker = temp.path().join("skwd-wall-vk");
    executable(&worker, "#!/bin/sh\nwhile IFS= read -r line; do :; done\n");
    let missing = temp.path().join("skwd-wall-still");
    let backends = BackendPaths::from_executables(worker, missing.clone());
    let capabilities = backends.capabilities();
    assert!(capabilities[0].executable_file);
    assert!(!capabilities[1].present);
    let error = backends
        .spawn(
            Assignment::new(vec!["DP-1".into()], Source::static_file("/wall/a.png")),
            "DP-1".into(),
            Path::new("/tmp/paper.sock"),
            95,
            None,
        )
        .err()
        .unwrap();
    assert!(error.to_string().contains("static image media capability requires skwd-wall-still"));
    assert!(error.to_string().contains(&missing.display().to_string()));
    let assignment = Assignment::new(vec!["DP-1".into()], Source::video("/wall/loop.mp4", None));
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
        let worker = backends
            .spawn(assignment, "DP-1".into(), Path::new("/tmp/paper.sock"), 96, None)
            .unwrap();
        worker.stop().await;
    });
}

#[test]
fn configured_override_is_authoritative() {
    let temp = tempfile::tempdir().unwrap();
    let fallback = temp.path().join("fallback");
    executable(&fallback, "#!/bin/sh\nexit 0\n");
    let configured = temp.path().join("configured-missing");
    let backend = super::select_backend(
        "skwd-wall-vk",
        &[SourceKind::Video],
        &[VideoEngine::Default],
        Some(("SKWD_PAPER_VK_BIN".into(), configured.clone())),
        vec![(fallback, RendererDiscovery::Path)],
        Vec::new(),
    );
    assert_eq!(backend.capability.discovery, RendererDiscovery::Configured);
    assert_eq!(backend.capability.path.as_deref(), Some(configured.to_str().unwrap()));
    assert!(!backend.capability.present);
    assert!(!backend.capability.executable_file);
    assert!(backend.capability.diagnostic.unwrap().contains("SKWD_PAPER_VK_BIN"));
}

#[test]
fn discovery_uses_first_executable_candidate() {
    let temp = tempfile::tempdir().unwrap();
    let sibling = temp.path().join("sibling");
    let path = temp.path().join("path");
    executable(&sibling, "#!/bin/sh\nexit 0\n");
    executable(&path, "#!/bin/sh\nexit 0\n");
    let backend = super::select_backend(
        "skwd-wall-still",
        &[SourceKind::Static],
        &[],
        None,
        vec![(sibling.clone(), RendererDiscovery::Sibling), (path, RendererDiscovery::Path)],
        Vec::new(),
    );
    assert_eq!(backend.capability.discovery, RendererDiscovery::Sibling);
    assert_eq!(
        backend.capability.path.as_deref(),
        Some(std::fs::canonicalize(sibling).unwrap().to_str().unwrap())
    );
}

#[test]
fn non_executable_candidate_is_reported_separately() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("skwd-wall-still");
    fs::write(&path, "not executable").unwrap();
    let backend = super::select_backend(
        "skwd-wall-still",
        &[SourceKind::Static],
        &[],
        None,
        vec![(path.clone(), RendererDiscovery::Path)],
        Vec::new(),
    );
    assert!(backend.capability.present);
    assert!(!backend.capability.executable_file);
    assert!(backend.capability.path.is_some());
    assert!(backend.capability.diagnostic.unwrap().contains("not executable"));
}

#[test]
fn missing_runtime_dependency_rejects_the_media_capability() {
    let temp = tempfile::tempdir().unwrap();
    let worker = temp.path().join("skwd-wall-vk");
    executable(&worker, "#!/bin/sh\nexit 0\n");
    let mut backend = super::injected_backend(
        "skwd-wall-vk",
        worker,
        &[SourceKind::Video],
        &[VideoEngine::Default],
    );
    backend.capability.dependencies.push(RuntimeDependencyStatus {
        name: "vulkan_loader".into(),
        available: false,
        detail: "libvulkan.so.1 is not loadable".into(),
    });
    assert!(!backend.capability.available());
    let error = backend.require("video").unwrap_err();
    assert!(error.downcast_ref::<RendererUnavailable>().is_some());
    assert!(error.to_string().contains("runtime dependency vulkan_loader"));
    assert!(error.to_string().contains("libvulkan.so.1 is not loadable"));
}

#[test]
fn wayland_display_requires_a_reachable_socket() {
    let temp = tempfile::tempdir().unwrap();
    let display = OsString::from("wayland-test");
    let socket = temp.path().join(&display);
    let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    let available = super::wayland_dependency(
        None,
        Some(display.clone()),
        Some(temp.path().as_os_str().to_owned()),
    );
    assert!(available.available);
    assert!(available.detail.contains(&socket.display().to_string()));
    drop(listener);
    std::fs::remove_file(&socket).unwrap();
    let unavailable =
        super::wayland_dependency(None, Some(display), Some(temp.path().as_os_str().to_owned()));
    assert!(!unavailable.available);
    assert!(unavailable.detail.contains("not reachable"));
}

#[test]
fn wayland_socket_requires_a_connected_unix_descriptor() {
    let (left, _right) = std::os::unix::net::UnixStream::pair().unwrap();
    let available =
        super::wayland_dependency(Some(OsString::from(left.as_raw_fd().to_string())), None, None);
    assert!(available.available);
    let file = std::fs::File::open("/dev/null").unwrap();
    let unavailable =
        super::wayland_dependency(Some(OsString::from(file.as_raw_fd().to_string())), None, None);
    assert!(!unavailable.available);
    assert!(unavailable.detail.contains("not a connected Unix socket descriptor"));
}

#[test]
fn scene_properties_populated() {
    assert_eq!(super::scene_properties_arg(&Source::wallpaper_engine("/wall/item")), None);
    assert_eq!(
        super::scene_properties_arg(
            &Source::wallpaper_engine("/wall/item").with_properties(serde_json::Map::new())
        ),
        None
    );
    let mut properties = serde_json::Map::new();
    properties.insert("tint".into(), serde_json::json!("1 0 0"));
    assert_eq!(
        super::scene_properties_arg(
            &Source::wallpaper_engine("/wall/item").with_properties(properties)
        )
        .as_deref(),
        Some(r#"{"tint":"1 0 0"}"#)
    );
}
