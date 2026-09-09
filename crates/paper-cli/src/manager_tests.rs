use super::*;
use paper_control::{
    AudioSetRequest, Layer, RendererPolicy, SandPolicy, SandQuality, SandScope, ScenePolicy,
    Source, TransitionPolicy, VideoEngine,
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
            "printf '%s\\n' \"$@\" > \"$base/args-$$\"\n",
            "env | sort > \"$base/env-$$\"\n",
            "trap 'exit 0' TERM\n",
            "while IFS= read -r line; do\n",
            "  printf '%s\\n' \"$line\" >> \"$base/stdin-$$\"\n",
            "  case \"$line\" in\n",
            "    *'\"freeze\":\"'*) path=${line#*'\"freeze\":\"'}; path=${path%%'\"'*}; printf 'P6\\n1 1\\n255\\n\\000\\000\\000' > \"$path.part\"; ln \"$path.part\" \"$path\"; rm \"$path.part\";;\n",
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
            "    *'\"freeze\":\"'*) path=${line#*'\"freeze\":\"'}; path=${path%%'\"'*}; printf 'capture failed' > \"$path.error.part\"; ln \"$path.error.part\" \"$path.error\"; rm \"$path.error.part\";;\n",
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

fn ready(transaction: &mut ApplyTransaction) {
    loop {
        for readiness in transaction.candidate_readiness() {
            assert!(transaction.note_ready(&readiness));
        }
        assert!(transaction.all_ready());
        if !transaction.prepare_next().unwrap() {
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

    fn new_with_tinier() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let worker = fake_worker(directory.path());
        let backends =
            BackendPaths::from_executables_with_tinier(worker.clone(), worker.clone(), worker);
        let socket = directory.path().join("paper.sock");
        let manager = Manager::with_backends(backends, directory.path().to_path_buf());
        Self { directory, manager, socket }
    }

    async fn commit(&mut self, request: &ApplyRequest) -> Vec<WorkerStatus> {
        let mut transaction = self.manager.begin_apply(request, &self.socket).await.unwrap();
        ready(&mut transaction);
        self.manager.commit(transaction).await.unwrap()
    }
}

#[test]
fn tinier_group_retarget() {
    block_on(async {
        let mut fixture = Fixture::new_with_tinier();
        let source = Source::tinier_video("/wall/one.ivf", "30000/1001");
        let status = fixture
            .commit(&apply(
                vec![
                    assignment("DP-1", source.clone()),
                    assignment("DP-2", source.clone()),
                    assignment("DP-3", source),
                ],
                false,
            ))
            .await;
        assert_eq!(status.len(), 1);
        assert_eq!(status[0].assignment.outputs, ["DP-1", "DP-2", "DP-3"]);
        let pid = status[0].pid;
        let args = wait_log(fixture.directory.path(), "args", pid, "DP-1,DP-2,DP-3");
        assert!(args.contains("--output\nDP-1,DP-2,DP-3"));

        fixture
            .commit(&apply(vec![assignment("DP-1", Source::static_file("/wall/two.png"))], false))
            .await;
        let status = fixture.manager.status();
        assert_eq!(status.len(), 2);
        let retained = status.iter().find(|entry| entry.pid == pid).unwrap();
        assert_eq!(retained.assignment.outputs, ["DP-2", "DP-3"]);
        wait_log(fixture.directory.path(), "stdin", pid, "\"outputs\":[\"DP-2\",\"DP-3\"]");
        assert_eq!(
            fixture.manager.stop(&StopRequest { outputs: vec!["DP-2".into()] }).await.unwrap(),
            1
        );
        let status = fixture.manager.status();
        assert_eq!(status.len(), 2);
        assert_eq!(
            status.iter().find(|entry| entry.pid == pid).unwrap().assignment.outputs,
            ["DP-3"]
        );
    });
}

#[test]
fn tinier_pause_resume() {
    block_on(async {
        let mut fixture = Fixture::new_with_tinier();
        let assignment = Assignment::new(
            vec!["DP-1".into(), "DP-2".into(), "DP-3".into()],
            Source::tinier_video("/wall/one.ivf", "30000/1001"),
        );
        let initial = fixture.commit(&apply(vec![assignment], false)).await;
        let initial_pid = initial[0].pid;

        let mut pause = fixture.manager.begin_pause(&fixture.socket).await.unwrap().unwrap();
        ready(&mut pause);
        let paused = fixture.manager.commit_pause(pause).await.unwrap();
        assert_eq!(paused.len(), 1);
        assert_ne!(paused[0].pid, initial_pid);
        assert_eq!(paused[0].assignment.outputs, ["DP-1", "DP-2", "DP-3"]);

        let mut resume = fixture.manager.begin_resume(&fixture.socket).await.unwrap().unwrap();
        ready(&mut resume);
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
            .commit(&apply(vec![assignment("DP-2", Source::video("/wall/two.mp4", None))], false))
            .await;
        let status = fixture.manager.status();
        assert_eq!(status.len(), 2);
        assert!(status.iter().any(|entry| entry.pid == first_pid));

        let replacement =
            apply(vec![assignment("HDMI-A-1", Source::video("/wall/three.mp4", None))], true);
        let mut transaction =
            fixture.manager.begin_apply(&replacement, &fixture.socket).await.unwrap();
        assert_eq!(fixture.manager.status().len(), 2);
        ready(&mut transaction);
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
                assignment("DP-1", Source::video("/wall/two.mp4", Some(VideoEngine::Default))),
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
        ready(&mut transaction);
        manager.commit(transaction).await.unwrap();
        let first_pid = manager.status()[0].pid;

        let mixed = apply(
            vec![
                assignment("DP-1", Source::static_file("/wall/two.png")),
                assignment("DP-2", Source::video("/wall/three.mp4", None)),
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

        let initial = apply(vec![assignment("DP-1", Source::video("/wall/one.mp4", None))], false);
        let mut transaction = manager.begin_apply(&initial, &socket).await.unwrap();
        ready(&mut transaction);
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
        let mut initial = assignment("DP-1", Source::video("/wall/one.mp4", None));
        initial.layer = Layer::Top;
        fixture.commit(&apply(vec![initial], false)).await;
        let first_pid = fixture.manager.status()[0].pid;
        let mut pause = fixture.manager.begin_pause(&fixture.socket).await.unwrap().unwrap();
        ready(&mut pause);
        let paused = fixture.manager.commit_pause(pause).await.unwrap();
        assert!(fixture.manager.paused());
        wait_log(fixture.directory.path(), "stdin", first_pid, "\"freeze\":");
        assert_ne!(paused[0].pid, first_pid);
        assert_eq!(paused[0].assignment.source.path, "/wall/one.mp4");
        let frozen_args = wait_log(fixture.directory.path(), "args", paused[0].pid, ".ppm");
        let frozen_path = PathBuf::from(frozen_args.lines().nth(1).unwrap());
        assert!(frozen_path.is_file());
        assert!(frozen_args.contains("--layer\ntop\n"));

        let mut replacement_assignment = assignment("DP-1", Source::video("/wall/two.mp4", None));
        replacement_assignment.transition = Some(TransitionPolicy::default());
        let replacement = apply(vec![replacement_assignment], false);
        assert!(fixture.manager.begin_apply(&replacement, &fixture.socket).await.is_err());

        let mut resume = fixture.manager.begin_resume(&fixture.socket).await.unwrap().unwrap();
        ready(&mut resume);
        let resumed = fixture.manager.commit_resume(resume).await.unwrap();
        assert!(!fixture.manager.paused());
        assert_eq!(resumed[0].assignment.source.path, "/wall/one.mp4");
        assert!(!frozen_path.exists());

        let mut transaction =
            fixture.manager.begin_apply(&replacement, &fixture.socket).await.unwrap();
        ready(&mut transaction);
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
        let initial = apply(vec![assignment("DP-1", Source::video("/wall/one.mp4", None))], false);
        let mut transaction = manager.begin_apply(&initial, &socket).await.unwrap();
        ready(&mut transaction);
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
                assignment("DP-1", Source::video("/wall/one.mp4", None)),
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
            outputs: vec!["DP-1".into(), "DP-2".into()],
            mute: Some(false),
            volume: Some(42),
        };
        let (updated, status) = fixture.manager.audio(&update).await.unwrap();
        assert_eq!(updated, 2);
        assert!(status.iter().all(|entry| !entry.assignment.mute));
        assert!(status.iter().all(|entry| entry.assignment.volume == 42));
        let input = wait_log(fixture.directory.path(), "stdin", dynamic_pid, "\"volume\":42");
        assert!(input.contains("\"mute\":false"));
    });
}

#[test]
fn policy_inheritance() {
    block_on(async {
        let mut fixture = Fixture::new();
        let policy = RendererPolicy {
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
        let mut initial =
            apply(vec![assignment("DP-1", Source::video("/wall/one.mp4", None))], false);
        initial.policy = Some(policy.clone());
        let status = fixture.commit(&initial).await;
        let environment =
            wait_log(fixture.directory.path(), "env", status[0].pid, "SKWD_PAPER_IDLE_SEC=30");
        assert!(environment.contains("SKWD_PAPER_SAND_QUALITY=low"));
        assert!(environment.contains("SKWD_PAPER_SAND_FPS=24"));
        assert!(environment.contains("SKWD_PAPER_WE_FPS=60"));
        assert!(environment.contains("SKWD_VK_SCENE_STRICT=1"));
        assert!(environment.contains("SKWD_PAPER_OUTPUT_FPS=DP-1=60"));
        assert_eq!(fixture.manager.policy(), Some(policy.clone()));

        let mut changed =
            apply(vec![assignment("DP-2", Source::video("/wall/two.mp4", None))], false);
        changed.policy = Some(RendererPolicy { idle_seconds: Some(60), ..Default::default() });
        assert!(fixture.manager.begin_apply(&changed, &fixture.socket).await.is_err());

        let inherited =
            apply(vec![assignment("DP-2", Source::video("/wall/two.mp4", None))], false);
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
            from: Some("/wall/old.png".into()),
            effect: Some("sand-donut".into()),
            duration_ms: Some(800),
        });
        let mut first = assignment("DP-1", Source::video("/wall/new.mp4", None));
        first.transition.clone_from(&transition);
        let mut second = assignment("DP-2", Source::video("/wall/new.mp4", None));
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
    let transition =
        Some(TransitionPolicy { effect: Some("sand-donut".into()), ..Default::default() });
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
    let mut first = assignment("DP-1", Source::video("/wall/new.mp4", None));
    first.transition = Some(TransitionPolicy { effect: Some("fade".into()), ..Default::default() });
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

        let mut next = assignment("DP-1", Source::video("/wall/two.mp4", None));
        next.transition = Some(TransitionPolicy {
            from: None,
            effect: Some("fade".into()),
            duration_ms: Some(800),
        });
        let mut transaction =
            fixture.manager.begin_apply(&apply(vec![next], false), &fixture.socket).await.unwrap();
        ready(&mut transaction);
        let status = fixture.manager.commit(transaction).await.unwrap();
        let args = wait_log(fixture.directory.path(), "args", status[0].pid, "--transition-from");
        assert!(args.contains("--persist\n"));
        assert!(args.contains("/wall/one.png"));
        assert!(args.contains("--duration-ms\n800"));
    });
}

#[test]
fn static_transition_waits_for_overlay_then_steady_presentation() {
    block_on(async {
        let mut fixture = Fixture::new();
        let initial = fixture
            .commit(&apply(vec![assignment("DP-1", Source::static_file("/wall/one.png"))], false))
            .await;
        let mut next = assignment("DP-1", Source::static_file("/wall/two.png"));
        next.transition = Some(TransitionPolicy::default());
        let mut transaction =
            fixture.manager.begin_apply(&apply(vec![next], false), &fixture.socket).await.unwrap();
        assert!(transaction.next.is_some());
        let overlay = transaction.candidates[0].pid();
        let args = wait_log(fixture.directory.path(), "args", overlay, "--transition-hold");
        assert!(args.contains("--transition-from\n/wall/one.png"));
        assert!(args.contains("--layer\nbottom"));
        assert!(!args.contains("--persist"));
        assert_eq!(fixture.manager.status()[0].pid, initial[0].pid);
        for notification in transaction.candidate_readiness() {
            transaction.note_ready(&notification);
        }
        assert!(transaction.prepare_next().unwrap());
        assert!(!transaction.all_ready());
        let steady = transaction.candidates[0].pid();
        let args = wait_log(fixture.directory.path(), "args", steady, "--persist");
        assert!(args.contains("/wall/two.png"));
        assert!(!fixture.directory.path().join(format!("stdin-{overlay}")).exists());
        ready(&mut transaction);
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
            .commit(&apply(vec![assignment("DP-1", Source::video("/wall/one.mp4", None))], false))
            .await;
        let mut next = assignment("DP-1", Source::static_file("/wall/two.png"));
        next.transition = Some(TransitionPolicy::default());
        let mut transaction =
            fixture.manager.begin_apply(&apply(vec![next], false), &fixture.socket).await.unwrap();
        for notification in transaction.candidate_readiness() {
            transaction.note_ready(&notification);
        }
        transaction.prepare_next().unwrap();
        transaction.rollback().await;
        assert_eq!(fixture.manager.status()[0].pid, initial[0].pid);
        assert!(fixture.manager.overlays.is_empty());
    });
}

#[test]
fn failed_steady_spawn_retires_the_held_overlay_on_rollback() {
    block_on(async {
        let mut fixture = Fixture::new();
        let initial = fixture
            .commit(&apply(vec![assignment("DP-1", Source::video("/wall/one.mp4", None))], false))
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
        assert!(transaction.prepare_next().is_err());
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
            Source::video("/wall/clip.mp4", None),
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
            let commands = wait_log(fixture.directory.path(), "stdin", initial[0].pid, "false");
            assert!(commands.contains("true"));
            assert!(!commands.contains("freeze"));
        }
    });
}
