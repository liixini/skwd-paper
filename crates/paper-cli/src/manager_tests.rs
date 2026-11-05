use super::*;
use paper_control::{
    AudioSetRequest, FillMode, Layer, RendererPolicy, SandPolicy, SandQuality, SandScope,
    ScenePolicy, Source, SourceKind, TransitionPolicy,
};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(future)
}

fn fake_worker(directory: &Path) -> PathBuf {
    let path = directory.join("worker");
    let mut file = std::fs::File::create(&path).unwrap();
    file.write_all(
        concat!(
            "#!/bin/sh\n",
            "umask 077\n",
            "base=$(dirname \"$0\")\n",
            "printf '%s\\n' \"$@\" > \"$base/args-$$.part\"\n",
            "mv \"$base/args-$$.part\" \"$base/args-$$\"\n",
            "env | sort > \"$base/env-$$\"\n",
            "trap 'exit 0' TERM\n",
            "while IFS= read -r line; do\n",
            "  printf '%s\\n' \"$line\" >> \"$base/stdin-$$\"\n",
            "  case \"$line\" in\n",
            "    *'\"freeze\":\"'*|*'\"snapshot\":{'*) if [ -f \"$base/finish-on-freeze\" ]; then exit 0; fi; path=${line#*'\"freeze\":\"'}; case \"$line\" in *'\"snapshot\":{'*) path=${line#*'\"path\":\"'};; esac; path=${path%%'\"'*}; printf 'P6\\n1 1\\n255\\n\\000\\000\\000' > \"$path.part\"; ln \"$path.part\" \"$path\"; rm \"$path.part\";;\n",
            "  esac\n",
            "done\n",
            "printf 'eof' > \"$base/eof-$$\"\n",
        )
        .as_bytes(),
    )
    .unwrap();
    file.sync_all().unwrap();
    drop(file);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(25));
    path
}

fn freeze_error_worker(directory: &Path) -> PathBuf {
    let path = directory.join("error-worker");
    let mut file = std::fs::File::create(&path).unwrap();
    file.write_all(
        concat!(
            "#!/bin/sh\n",
            "umask 077\n",
            "base=$(dirname \"$0\")\n",
            "printf '%s\\n' \"$@\" > \"$base/args-$$\"\n",
            "env | sort > \"$base/env-$$\"\n",
            "trap 'exit 0' TERM\n",
            "while IFS= read -r line; do\n",
            "  printf '%s\\n' \"$line\" >> \"$base/stdin-$$\"\n",
            "  case \"$line\" in\n",
            "    *'\"freeze\":\"'*|*'\"snapshot\":{'*) path=${line#*'\"freeze\":\"'}; case \"$line\" in *'\"snapshot\":{'*) path=${line#*'\"path\":\"'};; esac; path=${path%%'\"'*}; printf 'capture failed' > \"$path.error.part\"; ln \"$path.error.part\" \"$path.error\"; rm \"$path.error.part\";;\n",
            "  esac\n",
            "done\n",
        )
        .as_bytes(),
    )
    .unwrap();
    file.sync_all().unwrap();
    drop(file);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(25));
    path
}

fn wait_log(directory: &Path, kind: &str, pid: u32, pattern: &str) -> String {
    let path = directory.join(format!("{kind}-{pid}"));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        let contents = std::fs::read_to_string(&path).unwrap_or_default();
        if contents.contains(pattern) {
            return contents;
        }
        assert!(std::time::Instant::now() < deadline, "missing {pattern:?}");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

fn assignment(output: &str, source: Source) -> Assignment {
    Assignment::new(vec![output.to_string()], source)
}

fn apply(assignments: Vec<Assignment>, replace_all: bool) -> ApplyRequest {
    ApplyRequest { assignments, replace_all, policy: None }
}

async fn ready(transaction: &mut ApplyTransaction) {
    loop {
        for readiness in transaction.candidate_readiness() {
            assert!(transaction.note_ready(&readiness));
        }
        assert!(transaction.all_ready());
        if !transaction.prepare_next().await.unwrap() {
            break;
        }
    }
}

struct Fixture {
    directory: tempfile::TempDir,
    manager: Manager,
    socket: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let worker = fake_worker(directory.path());
        let backends = BackendPaths::from_executables(worker.clone(), worker);
        let socket = directory.path().join("paper.sock");
        let manager = Manager::with_backends(backends, directory.path().to_path_buf());
        Self { directory, manager, socket }
    }

    async fn commit(&mut self, request: &ApplyRequest) -> Vec<WorkerStatus> {
        let mut transaction = self.manager.begin_apply(request, &self.socket).await.unwrap();
        ready(&mut transaction).await;
        self.manager.commit(transaction).await.unwrap()
    }
}

#[test]
fn span_patch_stop() {
    block_on(async {
        for kind in [SourceKind::Static, SourceKind::Video, SourceKind::WallpaperEngine] {
            let mut fixture = Fixture::new();
            let source = match kind {
                SourceKind::Static => Source::static_file("/wall/one.png"),
                SourceKind::Video => Source::video("/wall/one.mp4"),
                SourceKind::WallpaperEngine => {
                    let project = fixture.directory.path().join("scene");
                    std::fs::create_dir(&project).unwrap();
                    std::fs::write(
                        project.join("project.json"),
                        r#"{"type":"scene","file":"scene.pkg"}"#,
                    )
                    .unwrap();
                    std::fs::write(project.join("scene.pkg"), []).unwrap();
                    Source::wallpaper_engine(project.display().to_string())
                }
            };
            let mut span =
                Assignment::new(vec!["DP-1".into(), "DP-2".into(), "DP-3".into()], source);
            span.fill_mode = FillMode::Span;
            span.mute = false;
            span.volume = 45;
            let first = fixture.commit(&apply(vec![span], false)).await;
            assert_eq!(first.len(), 1);
            assert_eq!(first[0].assignment.outputs, ["DP-1", "DP-2", "DP-3"]);
            let original_pid = first[0].pid;
            let args = wait_log(fixture.directory.path(), "args", original_pid, "DP-1,DP-2,DP-3");
            assert!(args.contains("--fill-mode\nspan"));

            let next = apply(vec![assignment("DP-1", Source::static_file("/wall/two.png"))], false);
            let mut transaction =
                fixture.manager.begin_apply(&next, &fixture.socket).await.unwrap();
            assert_eq!(fixture.manager.status()[0].pid, original_pid);
            ready(&mut transaction).await;
            let patched = fixture.manager.commit(transaction).await.unwrap();
            assert_eq!(patched.len(), 2);
            let remaining =
                patched.iter().find(|entry| entry.assignment.outputs == ["DP-2", "DP-3"]).unwrap();
            assert_ne!(remaining.pid, original_pid);
            assert!(
                wait_log(fixture.directory.path(), "args", remaining.pid, "DP-2,DP-3")
                    .contains("--fill-mode\nspan")
            );

            let stop = StopRequest { outputs: vec!["DP-2".into()] };
            let (mut transaction, count) =
                fixture.manager.begin_stop(&stop, &fixture.socket).await.unwrap().unwrap();
            assert_eq!(count, 1);
            assert_eq!(fixture.manager.status().len(), 2);
            ready(&mut transaction).await;
            let stopped = fixture.manager.commit(transaction).await.unwrap();
            assert_eq!(stopped.len(), 2);
            assert!(stopped.iter().any(|entry| entry.assignment.outputs == ["DP-1"]));
            let final_worker =
                stopped.iter().find(|entry| entry.assignment.outputs == ["DP-3"]).unwrap();
            assert!(
                wait_log(fixture.directory.path(), "args", final_worker.pid, "DP-3")
                    .contains("--fill-mode\nspan")
            );
            assert!(stopped.iter().all(|entry| !entry.assignment.outputs.contains(&"DP-2".into())));
        }
    });
}

#[test]
fn span_groups_compatible_assignments() {
    let source = Source::video("/wall/one.mp4");
    let mut first = assignment("DP-1", source.clone());
    first.fill_mode = FillMode::Span;
    first.mute = false;
    first.volume = 45;
    let mut second = assignment("DP-2", source.clone());
    second.fill_mode = FillMode::Span;
    second.mute = true;
    second.volume = 100;
    let grouped = expand(&[first.clone(), second.clone()], None);
    assert_eq!(grouped.len(), 1);
    assert_eq!(grouped[0].0.outputs, ["DP-1", "DP-2"]);
    assert!(!grouped[0].0.mute);
    assert_eq!(grouped[0].0.volume, 45);

    let mut different_source = second.clone();
    different_source.source = Source::video("/wall/two.mp4");
    let mut different_mode = second.clone();
    different_mode.fill_mode = FillMode::Fit;
    assert_eq!(expand(&[first.clone(), different_source], None).len(), 2);
    assert_eq!(expand(&[first.clone(), different_mode], None).len(), 2);
    first.fill_mode = FillMode::Fill;
    second.fill_mode = FillMode::Fill;
    assert_eq!(expand(&[first, second], None).len(), 2);
}

#[test]
fn span_mixed_hints_cut() {
    let mut first = assignment("DP-1", Source::video("/wall/new.mp4"));
    first.fill_mode = FillMode::Span;
    first.transition = Some(TransitionPolicy {
        drawing_source: None,
        drawing: None,
        from: Some("/wall/old-one.mp4".into()),
        ..Default::default()
    });
    let mut second = first.clone();
    second.outputs = vec!["DP-2".into()];
    second.transition.as_mut().unwrap().from = Some("/wall/old-two.mp4".into());

    let cut = expand(&[first.clone(), second.clone()], None);
    assert_eq!(cut.len(), 1);
    assert_eq!(cut[0].0.outputs, ["DP-1", "DP-2"]);
    assert!(cut[0].0.transition.is_none());

    second.transition = first.transition.clone();
    let fade = expand(&[first, second], None);
    assert_eq!(fade.len(), 1);
    assert_eq!(fade[0].0.transition.as_ref().unwrap().from.as_deref(), Some("/wall/old-one.mp4"));
}

#[test]
fn span_mixed_incumbents_cut() {
    block_on(async {
        let mut fixture = Fixture::new();
        fixture
            .commit(&apply(
                vec![
                    assignment("DP-1", Source::video("/wall/old-one.mp4")),
                    assignment("DP-2", Source::video("/wall/old-two.mp4")),
                ],
                true,
            ))
            .await;
        let mut wildcard = assignment("*", Source::video("/wall/another.mp4"));
        wildcard.transition = Some(TransitionPolicy::default());
        let error = fixture
            .manager
            .begin_apply(&apply(vec![wildcard], true), &fixture.socket)
            .await
            .err()
            .unwrap();
        assert!(error.to_string().contains("explicit from hint"));
        let mut span =
            Assignment::new(vec!["DP-1".into(), "DP-2".into()], Source::video("/wall/new.mp4"));
        span.fill_mode = FillMode::Span;
        span.transition = Some(TransitionPolicy::default());
        let mut transaction =
            fixture.manager.begin_apply(&apply(vec![span], true), &fixture.socket).await.unwrap();
        assert_eq!(transaction.candidates.len(), 1);
        assert!(transaction.candidates[0].assignment.transition.is_none());
        ready(&mut transaction).await;
        let status = fixture.manager.commit(transaction).await.unwrap();
        assert_eq!(status[0].assignment.outputs, ["DP-1", "DP-2"]);
        let args = wait_log(fixture.directory.path(), "args", status[0].pid, "DP-1,DP-2");
        assert!(args.contains("--fill-mode\nspan"));
        assert!(!args.contains("--transition-from"));
    });
}

#[test]
fn span_missing_preview_cut() {
    block_on(async {
        let mut fixture = Fixture::new();
        let project = fixture.directory.path().join("scene");
        std::fs::create_dir(&project).unwrap();
        std::fs::write(project.join("project.json"), r#"{"type":"scene","file":"scene.pkg"}"#)
            .unwrap();
        std::fs::write(project.join("scene.pkg"), []).unwrap();
        fixture
            .commit(&apply(
                vec![assignment("DP-1", Source::wallpaper_engine(project.display().to_string()))],
                true,
            ))
            .await;

        let mut span =
            Assignment::new(vec!["DP-1".into(), "DP-2".into()], Source::video("/wall/new.mp4"));
        span.fill_mode = FillMode::Span;
        span.transition = Some(TransitionPolicy::default());
        let transaction =
            fixture.manager.begin_apply(&apply(vec![span], true), &fixture.socket).await.unwrap();
        assert_eq!(transaction.candidates.len(), 1);
        assert!(transaction.candidates[0].assignment.transition.is_none());
        transaction.rollback().await;

        std::fs::write(project.join("preview.gif"), b"gif").unwrap();
        let mut span =
            Assignment::new(vec!["DP-1".into(), "DP-2".into()], Source::video("/wall/new.mp4"));
        span.fill_mode = FillMode::Span;
        span.transition = Some(TransitionPolicy::default());
        let transaction =
            fixture.manager.begin_apply(&apply(vec![span], true), &fixture.socket).await.unwrap();
        assert!(transaction.candidates[0].assignment.transition.is_some());
        transaction.rollback().await;
    });
}

#[test]
fn span_spawn_failure() {
    block_on(async {
        let mut fixture = Fixture::new();
        let mut span = Assignment::new(
            vec!["DP-1".into(), "DP-2".into()],
            Source::static_file("/wall/one.png"),
        );
        span.fill_mode = FillMode::Span;
        let first = fixture.commit(&apply(vec![span], false)).await;
        let original_pid = first[0].pid;
        wait_log(fixture.directory.path(), "args", original_pid, "DP-1");
        std::fs::remove_file(fixture.directory.path().join("worker")).unwrap();

        let replacement =
            apply(vec![assignment("DP-1", Source::static_file("/wall/two.png"))], false);
        assert!(fixture.manager.begin_apply(&replacement, &fixture.socket).await.is_err());
        assert_eq!(fixture.manager.status()[0].pid, original_pid);
        assert!(
            fixture
                .manager
                .begin_stop(&StopRequest { outputs: vec!["DP-1".into()] }, &fixture.socket)
                .await
                .is_err()
        );
        assert_eq!(fixture.manager.status()[0].pid, original_pid);
    });
}

#[test]
fn span_pause_resume() {
    block_on(async {
        let mut fixture = Fixture::new();
        let mut assignment = Assignment::new(
            vec!["DP-1".into(), "DP-2".into(), "DP-3".into()],
            Source::video("/wall/one.mp4"),
        );
        assignment.fill_mode = FillMode::Span;
        let initial = fixture.commit(&apply(vec![assignment], false)).await;
        let initial_pid = initial[0].pid;

        let mut pause = fixture.manager.begin_pause(&fixture.socket).await.unwrap().unwrap();
        ready(&mut pause).await;
        let paused = fixture.manager.commit_pause(pause).await.unwrap();
        assert_eq!(paused.len(), 1);
        assert_ne!(paused[0].pid, initial_pid);
        assert_eq!(paused[0].assignment.outputs, ["DP-1", "DP-2", "DP-3"]);

        let mut resume = fixture.manager.begin_resume(&fixture.socket).await.unwrap().unwrap();
        ready(&mut resume).await;
        let resumed = fixture.manager.commit_resume(resume).await.unwrap();
        assert_eq!(resumed.len(), 1);
        assert_ne!(resumed[0].pid, paused[0].pid);
        assert_eq!(resumed[0].assignment.outputs, ["DP-1", "DP-2", "DP-3"]);
    });
}

#[test]
fn patch_vs_replace_all() {
    block_on(async {
        let mut fixture = Fixture::new();
        fixture
            .commit(&apply(vec![assignment("DP-1", Source::static_file("/wall/one.png"))], false))
            .await;
        let first_pid = fixture.manager.status()[0].pid;

        fixture
            .commit(&apply(vec![assignment("DP-2", Source::video("/wall/two.mp4"))], false))
            .await;
        let status = fixture.manager.status();
        assert_eq!(status.len(), 2);
        assert!(status.iter().any(|entry| entry.pid == first_pid));

        let replacement =
            apply(vec![assignment("HDMI-A-1", Source::video("/wall/three.mp4"))], true);
        let mut transaction =
            fixture.manager.begin_apply(&replacement, &fixture.socket).await.unwrap();
        assert_eq!(fixture.manager.status().len(), 2);
        ready(&mut transaction).await;
        fixture.manager.commit(transaction).await.unwrap();
        let status = fixture.manager.status();
        assert_eq!(status.len(), 1);
        assert_eq!(status[0].assignment.outputs, ["HDMI-A-1"]);
    });
}

#[test]
fn rollback_keeps_prior() {
    block_on(async {
        let mut fixture = Fixture::new();
        fixture
            .commit(&apply(vec![assignment("DP-1", Source::static_file("/wall/one.png"))], false))
            .await;
        let first_pid = fixture.manager.status()[0].pid;

        let replacement = apply(
            vec![
                assignment("DP-1", Source::video("/wall/two.mp4")),
                assignment("DP-2", Source::static_file("/wall/three.png")),
            ],
            false,
        );
        let transaction = fixture.manager.begin_apply(&replacement, &fixture.socket).await.unwrap();
        transaction.rollback().await;
        let status = fixture.manager.status();
        assert_eq!(status.len(), 1);
        assert_eq!(status[0].pid, first_pid);
        assert_eq!(status[0].assignment.source.path, "/wall/one.png");
    });
}

#[test]
fn spawn_failure_keeps_incumbents() {
    block_on(async {
        let temp = tempfile::tempdir().unwrap();
        let worker = fake_worker(temp.path());
        let missing = temp.path().join("missing-worker");
        let backends = BackendPaths::from_executables(missing, worker);
        let mut manager = Manager::with_backends(backends, temp.path().to_path_buf());
        let socket = temp.path().join("paper.sock");

        let initial = apply(vec![assignment("DP-1", Source::static_file("/wall/one.png"))], false);
        let mut transaction = manager.begin_apply(&initial, &socket).await.unwrap();
        ready(&mut transaction).await;
        manager.commit(transaction).await.unwrap();
        let first_pid = manager.status()[0].pid;

        let mixed = apply(
            vec![
                assignment("DP-1", Source::static_file("/wall/two.png")),
                assignment("DP-2", Source::video("/wall/three.mp4")),
            ],
            false,
        );
        let error = manager.begin_apply(&mixed, &socket).await.err().unwrap();
        assert!(error.to_string().contains("video media capability requires skwd-wall-vk"));
        let status = manager.status();
        assert_eq!(status.len(), 1);
        assert_eq!(status[0].pid, first_pid);
    });
}

#[test]
fn missing_still_replacement_keeps_video_incumbent() {
    block_on(async {
        let temp = tempfile::tempdir().unwrap();
        let worker = fake_worker(temp.path());
        let missing = temp.path().join("missing-still-worker");
        let backends = BackendPaths::from_executables(worker, missing);
        let mut manager = Manager::with_backends(backends, temp.path().to_path_buf());
        let socket = temp.path().join("paper.sock");

        let initial = apply(vec![assignment("DP-1", Source::video("/wall/one.mp4"))], false);
        let mut transaction = manager.begin_apply(&initial, &socket).await.unwrap();
        ready(&mut transaction).await;
        manager.commit(transaction).await.unwrap();
        let first_pid = manager.status()[0].pid;

        let replacement =
            apply(vec![assignment("DP-1", Source::static_file("/wall/two.png"))], false);
        let error = manager.begin_apply(&replacement, &socket).await.err().unwrap();
        assert!(
            error.to_string().contains("static image media capability requires skwd-wall-still")
        );
        let status = manager.status();
        assert_eq!(status.len(), 1);
        assert_eq!(status[0].pid, first_pid);
        assert_eq!(status[0].assignment.source.path, "/wall/one.mp4");
    });
}

#[test]
fn stop_one_or_all() {
    block_on(async {
        let mut fixture = Fixture::new();
        let request = apply(
            vec![
                assignment("DP-1", Source::static_file("/wall/one.png")),
                assignment("DP-2", Source::static_file("/wall/two.png")),
            ],
            false,
        );
        fixture.commit(&request).await;
        assert_eq!(
            fixture.manager.stop(&StopRequest { outputs: vec!["DP-1".into()] }).await.unwrap(),
            1
        );
        assert_eq!(fixture.manager.status()[0].assignment.outputs, ["DP-2"]);
        assert_eq!(fixture.manager.stop(&StopRequest::default()).await.unwrap(), 1);
        assert!(fixture.manager.is_empty());
    });
}

#[test]
fn pause_freeze_resume() {
    block_on(async {
        let mut fixture = Fixture::new();
        let mut initial = assignment("DP-1", Source::video("/wall/one.mp4"));
        initial.layer = Layer::Top;
        fixture.commit(&apply(vec![initial], false)).await;
        let first_pid = fixture.manager.status()[0].pid;
        let mut pause = fixture.manager.begin_pause(&fixture.socket).await.unwrap().unwrap();
        ready(&mut pause).await;
        let paused = fixture.manager.commit_pause(pause).await.unwrap();
        assert!(fixture.manager.paused());
        wait_log(fixture.directory.path(), "stdin", first_pid, "\"freeze\":");
        assert_ne!(paused[0].pid, first_pid);
        assert_eq!(paused[0].assignment.source.path, "/wall/one.mp4");
        let frozen_args = wait_log(fixture.directory.path(), "args", paused[0].pid, ".ppm");
        let frozen_path = PathBuf::from(frozen_args.lines().nth(1).unwrap());
        assert!(frozen_path.is_file());
        assert!(frozen_args.contains("--layer\ntop\n"));

        let mut replacement_assignment = assignment("DP-1", Source::video("/wall/two.mp4"));
        replacement_assignment.transition = Some(TransitionPolicy::default());
        let replacement = apply(vec![replacement_assignment], false);
        assert!(fixture.manager.begin_apply(&replacement, &fixture.socket).await.is_err());

        let mut resume = fixture.manager.begin_resume(&fixture.socket).await.unwrap().unwrap();
        ready(&mut resume).await;
        let resumed = fixture.manager.commit_resume(resume).await.unwrap();
        assert!(!fixture.manager.paused());
        assert_eq!(resumed[0].assignment.source.path, "/wall/one.mp4");
        assert!(!frozen_path.exists());

        let mut transaction =
            fixture.manager.begin_apply(&replacement, &fixture.socket).await.unwrap();
        ready(&mut transaction).await;
        let status = fixture.manager.commit(transaction).await.unwrap();
        assert_eq!(status.len(), 1);
        assert!(status[0].assignment.transition.is_none());
        assert_eq!(status[0].assignment.source.path, "/wall/two.mp4");
    });
}

#[test]
fn freeze_failure_unpauses() {
    block_on(async {
        let directory = tempfile::tempdir().unwrap();
        let worker = freeze_error_worker(directory.path());
        let backends = BackendPaths::from_executables(worker.clone(), worker);
        let mut manager = Manager::with_backends(backends, directory.path().to_path_buf());
        let socket = directory.path().join("paper.sock");
        let initial = apply(vec![assignment("DP-1", Source::video("/wall/one.mp4"))], false);
        let mut transaction = manager.begin_apply(&initial, &socket).await.unwrap();
        ready(&mut transaction).await;
        let status = manager.commit(transaction).await.unwrap();
        let pid = status[0].pid;

        let error = match manager.begin_pause(&socket).await {
            Err(error) => error.to_string(),
            Ok(_) => panic!("freeze capture unexpectedly succeeded"),
        };
        assert_eq!(error, "capture failed");
        assert!(!manager.paused());
        assert_eq!(manager.status()[0].pid, pid);
        wait_log(directory.path(), "stdin", pid, "\"pause\":false");
    });
}

#[test]
fn audio_updates_state() {
    block_on(async {
        let mut fixture = Fixture::new();
        let request = apply(
            vec![
                assignment("DP-1", Source::video("/wall/one.mp4")),
                assignment("DP-2", Source::static_file("/wall/two.png")),
            ],
            false,
        );
        fixture.commit(&request).await;
        let dynamic_pid = fixture
            .manager
            .status()
            .into_iter()
            .find(|status| status.assignment.outputs == ["DP-1"])
            .unwrap()
            .pid;

        let update = AudioSetRequest {
            duck: None,
            outputs: vec!["DP-1".into(), "DP-2".into()],
            mute: Some(false),
            volume: Some(42),
            system_audio: Some(false),
        };
        let (updated, status) = fixture.manager.audio(&update).await.unwrap();
        assert_eq!(updated, 2);
        assert!(status.iter().all(|entry| !entry.assignment.mute));
        assert!(status.iter().all(|entry| entry.assignment.volume == 42));
        let input = wait_log(fixture.directory.path(), "stdin", dynamic_pid, "\"volume\":42");
        assert!(input.contains("\"mute\":false"));
        assert!(input.contains("\"system_audio\":false"));
        assert!(status.iter().all(|entry| entry.assignment.system_audio == Some(false)));
    });
}

#[test]
fn policy_inheritance() {
    block_on(async {
        let mut fixture = Fixture::new();
        let policy = RendererPolicy {
            transition_fps: Some(120),
            load_timeout_ms: Some(30_000),
            surface: None,
            idle_seconds: Some(30),
            transitions_enabled: Some(true),
            sand: Some(SandPolicy {
                quality: Some(SandQuality::Low),
                fps: Some(24),
                ..Default::default()
            }),
            scene: Some(ScenePolicy { fps: Some(60), strict: Some(true), ..Default::default() }),
            output_fps: [("DP-1".into(), 60)].into(),
        };
        let mut initial = apply(vec![assignment("DP-1", Source::video("/wall/one.mp4"))], false);
        initial.policy = Some(policy.clone());
        let status = fixture.commit(&initial).await;
        let environment =
            wait_log(fixture.directory.path(), "env", status[0].pid, "SKWD_PAPER_IDLE_SEC=30");
        assert!(environment.contains("SKWD_PAPER_SAND_QUALITY=low"));
        assert!(environment.contains("SKWD_PAPER_LOAD_TIMEOUT_MS=30000"));
        assert!(environment.contains("SKWD_PAPER_SAND_FPS=24"));
        assert!(environment.contains("SKWD_PAPER_TRANSITION_FPS=120"));
        assert!(environment.contains("SKWD_PAPER_WE_FPS=60"));
        assert!(environment.contains("SKWD_VK_SCENE_STRICT=1"));
        assert!(environment.contains("SKWD_PAPER_OUTPUT_FPS=DP-1=60"));
        assert_eq!(fixture.manager.policy(), Some(policy.clone()));

        let mut changed = apply(vec![assignment("DP-2", Source::video("/wall/two.mp4"))], false);
        changed.policy = Some(RendererPolicy { idle_seconds: Some(60), ..Default::default() });
        assert!(fixture.manager.begin_apply(&changed, &fixture.socket).await.is_err());

        let inherited = apply(vec![assignment("DP-2", Source::video("/wall/two.mp4"))], false);
        let status = fixture.commit(&inherited).await;
        let second = status.iter().find(|status| status.assignment.outputs == ["DP-2"]).unwrap();
        wait_log(fixture.directory.path(), "env", second.pid, "SKWD_PAPER_IDLE_SEC=30");
    });
}

#[test]
fn sand_primary_scope() {
    block_on(async {
        let mut fixture = Fixture::new();
        let transition = Some(TransitionPolicy {
            drawing_source: None,
            drawing: None,
            fps: None,
            from: Some("/wall/old.png".into()),
            effect: Some("sand-donut".into()),
            duration_ms: Some(800),
        });
        let mut first = assignment("DP-1", Source::video("/wall/new.mp4"));
        first.transition.clone_from(&transition);
        let mut second = assignment("DP-2", Source::video("/wall/new.mp4"));
        second.transition = transition;
        let mut request = apply(vec![first, second], true);
        request.policy = Some(RendererPolicy {
            sand: Some(SandPolicy {
                scope: Some(SandScope::Primary),
                primary: Some("DP-2".into()),
                ..Default::default()
            }),
            ..Default::default()
        });

        let status = fixture.commit(&request).await;
        let first = status.iter().find(|status| status.assignment.outputs == ["DP-1"]).unwrap();
        let second = status.iter().find(|status| status.assignment.outputs == ["DP-2"]).unwrap();
        let first_args = wait_log(fixture.directory.path(), "args", first.pid, "--persist");
        let second_args =
            wait_log(fixture.directory.path(), "args", second.pid, "--transition-from");
        assert!(!first_args.contains("--transition-from"));
        assert!(second_args.contains("--shader\nsand-donut"));
    });
}

#[test]
fn sand_primary_fallback() {
    let transition = Some(TransitionPolicy {
        drawing_source: None,
        drawing: None,
        effect: Some("sand-donut".into()),
        ..Default::default()
    });
    let mut first = assignment("DP-4", Source::static_file("/wall/new.png"));
    first.transition.clone_from(&transition);
    let mut second = assignment("eDP-1", Source::static_file("/wall/new.png"));
    second.transition = transition;
    let policy = RendererPolicy {
        sand: Some(SandPolicy {
            scope: Some(SandScope::Primary),
            primary: Some("e-DP1".into()),
            ..Default::default()
        }),
        ..Default::default()
    };

    assert_eq!(
        primary_sand_transition_output(Some(&policy), &[first, second]).as_deref(),
        Some("DP-4")
    );
}

#[test]
fn sand_primary_ignores_fade() {
    let mut first = assignment("DP-1", Source::video("/wall/new.mp4"));
    first.transition =
        Some(TransitionPolicy { drawing: None, effect: Some("fade".into()), ..Default::default() });
    let policy = RendererPolicy {
        sand: Some(SandPolicy {
            scope: Some(SandScope::Primary),
            primary: Some("DP-1".into()),
            ..Default::default()
        }),
        ..Default::default()
    };

    assert_eq!(primary_sand_transition_output(Some(&policy), &[first]), None);
}

#[test]
fn transition_from_incumbent() {
    block_on(async {
        let mut fixture = Fixture::new();
        fixture
            .commit(&apply(vec![assignment("DP-1", Source::static_file("/wall/one.png"))], false))
            .await;

        let mut next = assignment("DP-1", Source::video("/wall/two.mp4"));
        next.transition = Some(TransitionPolicy {
            drawing_source: None,
            drawing: None,
            fps: None,
            from: None,
            effect: Some("fade".into()),
            duration_ms: Some(800),
        });
        let mut transaction =
            fixture.manager.begin_apply(&apply(vec![next], false), &fixture.socket).await.unwrap();
        ready(&mut transaction).await;
        let status = fixture.manager.commit(transaction).await.unwrap();
        let args = wait_log(fixture.directory.path(), "args", status[0].pid, "--transition-from");
        assert!(args.contains("--persist\n"));
        assert!(args.contains("/wall/one.png"));
        assert!(args.contains("--duration-ms\n800"));
    });
}

#[test]
fn static_transition_prepares_below_overlay_and_waits_for_reveal() {
    block_on(async {
        let mut fixture = Fixture::new();
        let initial = fixture
            .commit(&apply(vec![assignment("DP-1", Source::static_file("/wall/one.png"))], false))
            .await;
        let mut next = assignment("DP-1", Source::static_file("/wall/two.png"));
        next.transition = Some(TransitionPolicy::default());
        let mut transaction =
            fixture.manager.begin_apply(&apply(vec![next], false), &fixture.socket).await.unwrap();
        let steady = transaction.candidates[0].pid();
        let args = wait_log(fixture.directory.path(), "args", steady, "--persist");
        assert!(args.contains("/wall/two.png"));
        assert!(args.contains("--layer\nbackground"));
        wait_log(fixture.directory.path(), "env", steady, "SKWD_PAPER_PREPARE_HIDDEN=1");
        assert!(transaction.overlays.is_empty());
        assert_eq!(fixture.manager.status()[0].pid, initial[0].pid);
        for notification in transaction.candidate_readiness() {
            transaction.note_ready(&notification);
        }
        assert!(transaction.prepare_next().await.unwrap());
        let overlay = transaction.candidates[0].pid();
        let args = wait_log(fixture.directory.path(), "args", overlay, "--transition-hold");
        assert!(args.contains("--transition-from\n/wall/one.png"));
        assert!(args.contains("--layer\nbackground"));
        assert!(!args.contains("--persist"));
        assert!(!transaction.all_ready());
        assert!(!fixture.directory.path().join(format!("stdin-{steady}")).exists());
        for notification in transaction.candidate_readiness() {
            transaction.note_ready(&notification);
        }
        assert!(transaction.prepare_next().await.unwrap());
        assert_eq!(transaction.candidates[0].pid(), steady);
        assert_eq!(transaction.overlays[0].pid(), overlay);
        wait_log(fixture.directory.path(), "stdin", steady, "\"reveal\":true");
        assert!(!transaction.all_ready());
        assert!(!fixture.directory.path().join(format!("stdin-{overlay}")).exists());
        ready(&mut transaction).await;
        let status = fixture.manager.commit(transaction).await.unwrap();
        assert_eq!(status[0].pid, steady);
        assert!(status[0].assignment.transition.is_none());
        wait_log(fixture.directory.path(), "stdin", overlay, "\"pause\":false");
        assert_eq!(fixture.manager.overlays.len(), 1);
        fixture.manager.stop(&StopRequest::default()).await.unwrap();
        assert!(fixture.manager.is_empty());
    });
}

#[test]
fn static_transition_rollback_preserves_prior_composition() {
    block_on(async {
        let mut fixture = Fixture::new();
        let initial = fixture
            .commit(&apply(vec![assignment("DP-1", Source::video("/wall/one.mp4"))], false))
            .await;
        let mut next = assignment("DP-1", Source::static_file("/wall/two.png"));
        next.transition = Some(TransitionPolicy::default());
        let mut transaction =
            fixture.manager.begin_apply(&apply(vec![next], false), &fixture.socket).await.unwrap();
        for notification in transaction.candidate_readiness() {
            transaction.note_ready(&notification);
        }
        transaction.prepare_next().await.unwrap();
        transaction.rollback().await;
        assert_eq!(fixture.manager.status()[0].pid, initial[0].pid);
        assert!(fixture.manager.overlays.is_empty());
    });
}

#[test]
fn failed_overlay_spawn_retires_the_hidden_steady_on_rollback() {
    block_on(async {
        let mut fixture = Fixture::new();
        let initial = fixture
            .commit(&apply(vec![assignment("DP-1", Source::video("/wall/one.mp4"))], false))
            .await;
        let mut next = assignment("DP-1", Source::static_file("/wall/two.png"));
        next.transition = Some(TransitionPolicy::default());
        let mut transaction =
            fixture.manager.begin_apply(&apply(vec![next], false), &fixture.socket).await.unwrap();
        for notification in transaction.candidate_readiness() {
            transaction.note_ready(&notification);
        }
        let worker = fixture.directory.path().join("worker");
        std::fs::remove_file(&worker).unwrap();
        assert!(transaction.prepare_next().await.is_err());
        transaction.rollback().await;
        assert_eq!(fixture.manager.status()[0].pid, initial[0].pid);
    });
}

#[test]
fn overview_surface_pause_retains_video_player() {
    block_on(async {
        let mut fixture = Fixture::new();
        let project = fixture.directory.path().join("project");
        std::fs::create_dir(&project).unwrap();
        std::fs::write(project.join("project.json"), r#"{"type":"video","file":"clip.mp4"}"#)
            .unwrap();
        std::fs::write(project.join("clip.mp4"), []).unwrap();
        for source in [
            Source::video("/wall/clip.mp4"),
            Source::wallpaper_engine(project.display().to_string()),
        ] {
            let mut request = apply(vec![assignment("DP-1", source)], true);
            request.policy = Some(RendererPolicy {
                surface: Some(Box::new(paper_control::SurfacePolicy {
                    namespace: "skwd-paper-backdrop".into(),
                    blur: 12,
                    dim: 20,
                })),
                ..Default::default()
            });
            let initial = fixture.commit(&request).await;
            for _ in 0..3 {
                assert!(fixture.manager.begin_pause(&fixture.socket).await.unwrap().is_none());
                assert!(fixture.manager.paused());
                assert_eq!(fixture.manager.status()[0].pid, initial[0].pid);
                assert!(fixture.manager.begin_resume(&fixture.socket).await.unwrap().is_none());
                assert!(!fixture.manager.paused());
                assert_eq!(fixture.manager.status()[0].pid, initial[0].pid);
            }
            let cycle = "{\"to\":\"\",\"pause\":true}\n{\"to\":\"\",\"pause\":false}\n";
            let commands =
                wait_log(fixture.directory.path(), "stdin", initial[0].pid, &cycle.repeat(3));
            assert!(!commands.contains("freeze"));
        }
    });
}

#[test]
fn static_transition_rollback_preserves_incumbents_at_each_stage() {
    block_on(async {
        for stage in 0..3 {
            let mut fixture = Fixture::new();
            let initial = fixture
                .commit(&apply(
                    vec![
                        assignment("DP-1", Source::video("/wall/old.mp4")),
                        assignment("DP-2", Source::static_file("/wall/other.png")),
                    ],
                    false,
                ))
                .await;
            let mut next = assignment("DP-1", Source::static_file("/wall/new.png"));
            next.transition = Some(TransitionPolicy::default());
            let mut transaction = fixture
                .manager
                .begin_apply(&apply(vec![next], false), &fixture.socket)
                .await
                .unwrap();
            for _ in 0..stage {
                for notification in transaction.candidate_readiness() {
                    transaction.note_ready(&notification);
                }
                assert!(transaction.prepare_next().await.unwrap());
            }
            let owned: Vec<_> = transaction
                .candidates
                .iter()
                .chain(&transaction.overlays)
                .map(Worker::pid)
                .collect();
            transaction.rollback().await;
            assert_eq!(
                fixture.manager.status().iter().map(|worker| worker.pid).collect::<Vec<_>>(),
                initial.iter().map(|worker| worker.pid).collect::<Vec<_>>()
            );
            for pid in owned {
                assert!(!Path::new(&format!("/proc/{pid}")).exists());
            }
        }
    });
}

#[test]
fn mixed_transition_waits_for_reveal_on_every_static_output() {
    block_on(async {
        let mut fixture = Fixture::new();
        fixture
            .commit(&apply(vec![assignment("*", Source::static_file("/wall/old.png"))], false))
            .await;
        let mut assignments = vec![
            assignment("DP-1", Source::static_file("/wall/a.png")),
            assignment("DP-2", Source::static_file("/wall/b.png")),
            assignment("DP-3", Source::video("/wall/c.mp4")),
        ];
        for assignment in &mut assignments {
            assignment.transition = Some(TransitionPolicy::default());
        }
        let mut transaction =
            fixture.manager.begin_apply(&apply(assignments, true), &fixture.socket).await.unwrap();
        assert_eq!(transaction.candidates.len(), 3);
        let initial_pids: Vec<_> = transaction.candidates.iter().map(Worker::pid).collect();
        for notification in transaction.candidate_readiness() {
            transaction.note_ready(&notification);
        }
        assert!(transaction.prepare_next().await.unwrap());
        assert_eq!(transaction.candidates.len(), 2);
        for notification in transaction.candidate_readiness() {
            transaction.note_ready(&notification);
        }
        assert!(transaction.prepare_next().await.unwrap());
        assert_eq!(
            transaction.candidates.iter().map(Worker::pid).collect::<Vec<_>>(),
            initial_pids
        );
        assert!(!transaction.all_ready());
        let notifications = transaction.candidate_readiness();
        transaction.note_ready(&notifications[0]);
        assert!(!transaction.all_ready());
        transaction.note_ready(&notifications[1]);
        assert!(transaction.all_ready());
        assert!(!transaction.prepare_next().await.unwrap());
        assert_eq!(fixture.manager.commit(transaction).await.unwrap().len(), 3);
    });
}

#[test]
fn same_static_source_and_nontransition_applies_do_not_prepare_hidden() {
    block_on(async {
        let mut fixture = Fixture::new();
        fixture
            .commit(&apply(vec![assignment("DP-1", Source::static_file("/wall/old.png"))], false))
            .await;
        for (path, transition) in
            [("/wall/new.png", None), ("/wall/old.png", Some(TransitionPolicy::default()))]
        {
            let mut next = assignment("DP-1", Source::static_file(path));
            next.transition = transition;
            let transaction = fixture
                .manager
                .begin_apply(&apply(vec![next], false), &fixture.socket)
                .await
                .unwrap();
            assert!(transaction.next.is_none());
            let environment = wait_log(
                fixture.directory.path(),
                "env",
                transaction.candidates[0].pid(),
                "SKWD_PAPER_GENERATION",
            );
            assert!(!environment.contains("SKWD_PAPER_PREPARE_HIDDEN"));
            transaction.rollback().await;
        }
    });
}

#[test]
fn video_transitions_preserve_all_requested_layers_without_hidden_preparation() {
    block_on(async {
        for layer in [Layer::Background, Layer::Bottom, Layer::Top, Layer::Overlay] {
            let mut fixture = Fixture::new();
            fixture
                .commit(&apply(
                    vec![assignment("DP-1", Source::static_file("/wall/old.png"))],
                    false,
                ))
                .await;
            let mut next = assignment("DP-1", Source::video("/wall/new.mp4"));
            next.layer = layer;
            next.transition = Some(TransitionPolicy::default());
            let transaction = fixture
                .manager
                .begin_apply(&apply(vec![next], false), &fixture.socket)
                .await
                .unwrap();
            assert!(transaction.next.is_none());
            let worker = &transaction.candidates[0];
            let args =
                wait_log(fixture.directory.path(), "args", worker.pid(), "--transition-from");
            assert!(args.contains(&format!(
                "--layer\n{}",
                serde_json::to_value(layer).unwrap().as_str().unwrap()
            )));
            assert!(!args.contains("--transition-hold"));
            let environment =
                wait_log(fixture.directory.path(), "env", worker.pid(), "SKWD_PAPER_GENERATION");
            assert!(!environment.contains("SKWD_PAPER_PREPARE_HIDDEN"));
            transaction.rollback().await;
        }
    });
}

#[test]
fn commit_rejects_each_unfinished_static_transition_stage() {
    block_on(async {
        for stage in 0..3 {
            let mut fixture = Fixture::new();
            let initial = fixture
                .commit(&apply(
                    vec![assignment("DP-1", Source::static_file("/wall/old.png"))],
                    false,
                ))
                .await;
            let mut next = assignment("DP-1", Source::static_file("/wall/new.png"));
            next.transition = Some(TransitionPolicy::default());
            let mut transaction = fixture
                .manager
                .begin_apply(&apply(vec![next], false), &fixture.socket)
                .await
                .unwrap();
            for _ in 0..stage {
                for notification in transaction.candidate_readiness() {
                    transaction.note_ready(&notification);
                }
                transaction.prepare_next().await.unwrap();
            }
            assert!(fixture.manager.commit(transaction).await.is_err());
            assert_eq!(fixture.manager.status()[0].pid, initial[0].pid);
        }
    });
}

#[test]
fn live_surface_effects_preserve_workers_and_follow_replacement() {
    block_on(async {
        let mut fixture = Fixture::new();
        let sharp =
            paper_control::SurfacePolicy { namespace: "stationary".into(), blur: 0, dim: 0 };
        let blurred = paper_control::SurfacePolicy { blur: 20, dim: 15, ..sharp.clone() };
        for source in [Source::static_file("/wall/still.png"), Source::video("/wall/clip.mp4")] {
            let mut request = apply(vec![assignment("DP-1", source)], true);
            request.policy = Some(RendererPolicy {
                surface: Some(Box::new(sharp.clone())),
                ..Default::default()
            });
            let initial = fixture.commit(&request).await;
            fixture.manager.set_surface(blurred.clone()).await.unwrap();
            assert_eq!(fixture.manager.status()[0].pid, initial[0].pid);
            let lines = wait_log(fixture.directory.path(), "stdin", initial[0].pid, "surface");
            assert!(lines.contains("\"blur\":20"));
            assert_eq!(fixture.manager.policy().unwrap().surface.as_deref(), Some(&blurred));
            let replaced = fixture.commit(&request).await;
            assert_ne!(replaced[0].pid, initial[0].pid);
            assert_eq!(fixture.manager.policy().unwrap().surface.as_deref(), Some(&blurred));
            fixture.manager.set_surface(sharp.clone()).await.unwrap();
            assert_eq!(fixture.manager.status()[0].pid, replaced[0].pid);
            let lines = wait_log(fixture.directory.path(), "stdin", replaced[0].pid, "surface");
            assert!(lines.contains("\"blur\":0"));
            let mismatch =
                paper_control::SurfacePolicy { namespace: "another".into(), blur: 80, dim: 0 };
            assert!(
                fixture
                    .manager
                    .set_surface(mismatch)
                    .await
                    .unwrap_err()
                    .to_string()
                    .contains("namespace")
            );
            assert_eq!(fixture.manager.status()[0].pid, replaced[0].pid);
            assert_eq!(fixture.manager.policy().unwrap().surface.as_deref(), Some(&sharp));
            assert_eq!(fixture.manager.surface_override.as_ref(), Some(&sharp));
            let lines = std::fs::read_to_string(
                fixture.directory.path().join(format!("stdin-{}", replaced[0].pid)),
            )
            .unwrap();
            assert!(!lines.contains("\"blur\":80"));
        }
    });
}

#[test]
fn mirror_routes_output() {
    block_on(async {
        for source in [Source::static_file("/wall/a.png"), Source::video("/wall/a.mp4")] {
            let mut fixture = Fixture::new();
            let status = fixture
                .commit(&apply(
                    vec![
                        assignment("DP-1", source),
                        assignment("DP-2", Source::static_file("/wall/b.png")),
                    ],
                    false,
                ))
                .await;
            let target = paper_control::MirrorTarget {
                output: "DP-1".into(),
                socket: "/run/lock.sock".into(),
            };
            fixture.manager.mirror(&target).await.unwrap();
            let pid =
                status.iter().find(|status| status.assignment.outputs == ["DP-1"]).unwrap().pid;
            let lines = wait_log(fixture.directory.path(), "stdin", pid, "mirror");
            let command: serde_json::Value =
                serde_json::from_str(lines.lines().last().unwrap()).unwrap();
            assert_eq!(
                command["mirror"],
                serde_json::json!({"output":"DP-1","socket":"/run/lock.sock"})
            );
            let other =
                status.iter().find(|status| status.assignment.outputs == ["DP-2"]).unwrap().pid;
            assert!(!fixture.directory.path().join(format!("stdin-{other}")).exists());
            assert!(
                fixture
                    .manager
                    .mirror(&paper_control::MirrorTarget {
                        output: "missing".into(),
                        socket: target.socket
                    })
                    .await
                    .is_err()
            );
        }
    });
}

#[test]
fn mirrors_follow_replacement() {
    block_on(async {
        let mut fixture = Fixture::new();
        let socket = fixture.directory.path().join("mirror.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        fixture.commit(&apply(vec![assignment("DP-1", Source::video("/wall/a.mp4"))], false)).await;
        let target = paper_control::MirrorTarget {
            output: "DP-1".into(),
            socket: socket.display().to_string(),
        };
        fixture.manager.mirror(&target).await.unwrap();
        let mut pause = fixture.manager.begin_pause(&fixture.socket).await.unwrap().unwrap();
        ready(&mut pause).await;
        let paused = fixture.manager.commit_pause(pause).await.unwrap();
        assert!(
            wait_log(fixture.directory.path(), "stdin", paused[0].pid, "mirror")
                .contains(&target.socket)
        );
        let mut resume = fixture.manager.begin_resume(&fixture.socket).await.unwrap().unwrap();
        ready(&mut resume).await;
        let resumed = fixture.manager.commit_resume(resume).await.unwrap();
        assert_ne!(paused[0].pid, resumed[0].pid);
        assert!(
            wait_log(fixture.directory.path(), "stdin", resumed[0].pid, "mirror")
                .contains(&target.socket)
        );
        let replaced = fixture
            .commit(&apply(vec![assignment("DP-1", Source::static_file("/wall/b.png"))], false))
            .await;
        assert_ne!(resumed[0].pid, replaced[0].pid);
        assert!(
            wait_log(fixture.directory.path(), "stdin", replaced[0].pid, "mirror")
                .contains(&target.socket)
        );
        drop(listener);
        assert!(socket.exists());
        fixture
            .commit(&apply(vec![assignment("DP-1", Source::static_file("/wall/c.png"))], false))
            .await;
        assert!(fixture.manager.mirrors.is_empty());
    });
}

#[test]
fn disable_removes_registration() {
    block_on(async {
        let mut fixture = Fixture::new();
        fixture.commit(&apply(vec![assignment("DP-1", Source::video("/wall/a.mp4"))], false)).await;
        let mut target =
            paper_control::MirrorTarget { output: "DP-1".into(), socket: "/run/lock.sock".into() };
        fixture.manager.mirror(&target).await.unwrap();
        assert_eq!(fixture.manager.mirrors.len(), 1);
        target.socket.clear();
        fixture.manager.mirror(&target).await.unwrap();
        assert!(fixture.manager.mirrors.is_empty());
        fixture.manager.stop(&StopRequest::default()).await.unwrap();
        fixture.manager.mirror(&target).await.unwrap();
        assert!(fixture.manager.is_empty());
    });
}

#[test]
fn span_mirror_survives_stop() {
    block_on(async {
        let mut fixture = Fixture::new();
        let socket = fixture.directory.path().join("mirror.sock");
        let _listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let mut span = Assignment::new(
            vec!["DP-1".into(), "DP-2".into()],
            Source::static_file("/wall/span.png"),
        );
        span.fill_mode = FillMode::Span;
        fixture.commit(&apply(vec![span], false)).await;
        for output in ["DP-1", "DP-2"] {
            fixture
                .manager
                .mirror(&paper_control::MirrorTarget {
                    output: output.into(),
                    socket: socket.display().to_string(),
                })
                .await
                .unwrap();
        }
        let (mut transaction, _) = fixture
            .manager
            .begin_stop(&StopRequest { outputs: vec!["DP-1".into()] }, &fixture.socket)
            .await
            .unwrap()
            .unwrap();
        ready(&mut transaction).await;
        let remaining = fixture.manager.commit(transaction).await.unwrap();
        assert_eq!(remaining[0].assignment.outputs, ["DP-2"]);
        assert_eq!(fixture.manager.mirrors.keys().collect::<Vec<_>>(), ["DP-2"]);
        let commands = wait_log(fixture.directory.path(), "stdin", remaining[0].pid, "mirror");
        assert!(commands.contains("DP-2"));
        assert!(!commands.contains("DP-1"));
        fixture.manager.stop(&StopRequest::default()).await.unwrap();
        assert!(fixture.manager.mirrors.is_empty());
        assert!(fixture.manager.is_empty());
    });
}

#[path = "handoff_tests.rs"]
mod handoff;

#[path = "playback_tests.rs"]
mod playback_tests;

#[path = "surface_tests.rs"]
mod surface_tests;
