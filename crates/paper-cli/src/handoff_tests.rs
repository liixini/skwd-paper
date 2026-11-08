use super::*;

async fn drawing_overlay(fixture: &mut Fixture) -> u32 {
    let mut initial = assignment("DP-1", Source::static_file("/wall/one.png"));
    initial.transition =
        Some(TransitionPolicy { from: Some("/wall/old.png".into()), ..Default::default() });
    fixture
        .commit(&apply(vec![initial, assignment("DP-2", Source::video("/wall/video.mp4"))], false))
        .await;
    let overlay = fixture.manager.overlays.last_mut().unwrap();
    overlay.assignment.transition.as_mut().unwrap().drawing =
        Some(paper_control::DrawingTransition {
            artifact: "/unused/manifest.json".into(),
            colours: paper_control::DrawingColours::default(),
            ink: paper_control::DrawingInk::Plain,
        });
    overlay.pid()
}

fn replacement() -> ApplyRequest {
    let mut next = assignment("DP-1", Source::static_file("/wall/two.png"));
    next.transition = Some(TransitionPolicy::default());
    apply(vec![next], false)
}

#[test]
fn drawing_snapshot_lifetime() {
    block_on(async {
        for commit in [false, true] {
            let mut fixture = Fixture::new();
            let overlay = drawing_overlay(&mut fixture).await;
            let video = fixture
                .manager
                .status()
                .into_iter()
                .find(|worker| worker.assignment.outputs == ["DP-2"])
                .unwrap()
                .pid;
            let mut transaction =
                fixture.manager.begin_apply(&replacement(), &fixture.socket).await.unwrap();
            let snapshot = PathBuf::from(
                transaction.next.as_ref().unwrap().assignments[0]
                    .0
                    .transition
                    .as_ref()
                    .unwrap()
                    .from
                    .as_ref()
                    .unwrap(),
            );
            assert!(snapshot.exists());
            ready(&mut transaction).await;
            if commit {
                fixture.manager.commit(transaction).await.unwrap();
                assert!(snapshot.exists());
                fixture.manager.stop(&StopRequest { outputs: vec!["DP-1".into()] }).await.unwrap();
            } else {
                transaction.rollback().await;
                wait_log(fixture.directory.path(), "stdin", overlay, "\"pause\":false");
            }
            assert!(!snapshot.exists());
            assert!(fixture.manager.status().iter().any(|worker| worker.pid == video));
        }
    });
}

#[test]
fn drawing_capture_failure_resumes() {
    block_on(async {
        let mut fixture = Fixture::new();
        let error_worker = freeze_error_worker(fixture.directory.path());
        fixture.manager.backends =
            Some(BackendPaths::from_executables(error_worker.clone(), error_worker));
        let overlay = drawing_overlay(&mut fixture).await;
        assert!(fixture.manager.begin_apply(&replacement(), &fixture.socket).await.is_err());
        wait_log(fixture.directory.path(), "stdin", overlay, "\"pause\":false");
        assert_eq!(fixture.manager.overlays[0].pid(), overlay);
        assert!(
            !std::fs::read_dir(fixture.directory.path()).unwrap().any(|entry| entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("handoff-"))
        );
    });
}

#[test]
fn grouped_drawing_retained() {
    block_on(async {
        let mut fixture = Fixture::new();
        let overlay = drawing_overlay(&mut fixture).await;
        fixture.manager.overlays[0].assignment.outputs.push("DP-2".into());
        assert!(fixture.manager.begin_apply(&replacement(), &fixture.socket).await.is_err());
        assert_eq!(fixture.manager.overlays[0].pid(), overlay);
        let commands = wait_log(fixture.directory.path(), "stdin", overlay, "pause");
        assert!(!commands.contains("freeze"));
    });
}

#[test]
fn drawing_completed_during_capture() {
    block_on(async {
        let mut fixture = Fixture::new();
        std::fs::write(fixture.directory.path().join("finish-on-freeze"), b"").unwrap();
        drawing_overlay(&mut fixture).await;
        let mut transaction =
            fixture.manager.begin_apply(&replacement(), &fixture.socket).await.unwrap();
        let from = transaction.next.as_ref().unwrap().assignments[0]
            .0
            .transition
            .as_ref()
            .unwrap()
            .from
            .as_deref();
        assert_eq!(from, Some("/wall/one.png"));
        assert!(transaction.handoff.is_none());
        ready(&mut transaction).await;
        fixture.manager.commit(transaction).await.unwrap();
    });
}

#[test]
fn drawing_handoff_eof() {
    block_on(async {
        let mut fixture = Fixture::new();
        let overlay = drawing_overlay(&mut fixture).await;
        let mut transaction =
            fixture.manager.begin_apply(&replacement(), &fixture.socket).await.unwrap();
        ready(&mut transaction).await;
        fixture.manager.commit(transaction).await.unwrap();
        assert!(fixture.directory.path().join(format!("eof-{overlay}")).exists());
        fixture.manager.stop(&StopRequest { outputs: vec![ALL_OUTPUT.into()] }).await.unwrap();
    });
}

pub(super) fn media_drawing(directory: &Path) -> Assignment {
    let source = directory.join("reference.png");
    std::fs::write(&source, b"").unwrap();
    let metadata = std::fs::metadata(&source).unwrap();
    let manifest = serde_json::json!({
        "version": 3,
        "source": {"path":source,"size":0,"mtime_ns":metadata.modified().unwrap().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),"sha256":"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"},
        "width":1,"height":1,"texture":"reference.png","strokes":"reference.png","timing_width":1,"timing_height":1,"focus":[0.5,0.5]
    });
    let artifact = directory.join("drawing.json");
    std::fs::write(&artifact, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let mut next = assignment("DP-1", Source::video("/wall/new.mp4"));
    next.mute = false;
    next.transition = Some(TransitionPolicy {
        drawing_source: Some(source),
        drawing: Some(paper_control::DrawingTransition {
            artifact,
            colours: paper_control::DrawingColours::default(),
            ink: paper_control::DrawingInk::Plain,
        }),
        from: Some("/wall/old.png".into()),
        effect: Some(paper_control::DRAWING_EFFECT.into()),
        ..Default::default()
    });
    next
}

#[test]
fn media_stage_holds_playback() {
    block_on(async {
        let mut fixture = Fixture::new();
        let next = media_drawing(fixture.directory.path());
        let mut transaction =
            fixture.manager.begin_apply(&apply(vec![next], true), &fixture.socket).await.unwrap();
        let pid = transaction.candidates[0].pid();
        let env = wait_log(fixture.directory.path(), "env", pid, "SKWD_PAPER_PREPARE_HIDDEN=1");
        assert!(!env.contains("SKWD_PAPER_DRAWING="));
        let args = wait_log(fixture.directory.path(), "args", pid, "--mute");
        assert!(args.contains("--mute\ntrue"));
        assert!(!args.contains("--transition-from"));
        ready(&mut transaction).await;
        let overlay = transaction.overlays[0].pid();
        assert_eq!(
            transaction.overlays[0].assignment.source.kind,
            paper_control::SourceKind::Video
        );
        let args = wait_log(fixture.directory.path(), "args", overlay, DRAWING_SNAPSHOT);
        let target = args.lines().nth(1).unwrap().to_owned();
        assert!(Path::new(&target).exists());
        fixture.manager.commit(transaction).await.unwrap();
        assert!(fixture.manager.workers[0].drawing_hold.is_some());
        wait_log(fixture.directory.path(), "stdin", pid, "\"reveal\":true");
        fixture
            .manager
            .audio(&AudioSetRequest { mute: Some(false), volume: Some(42), ..Default::default() })
            .await
            .unwrap();
        assert!(fixture.manager.begin_pause(&fixture.socket).await.unwrap().is_none());
        wait_log(fixture.directory.path(), "stdin", overlay, "\"pause\":true");
        assert!(fixture.manager.begin_resume(&fixture.socket).await.unwrap().is_none());
        assert!(fixture.manager.workers[0].drawing_hold.is_some());
        let commands =
            std::fs::read_to_string(fixture.directory.path().join(format!("stdin-{pid}"))).unwrap();
        assert!(!commands.contains("\"pause\":false"));
        assert!(!commands.contains("\"mute\":false"));
        fixture.manager.overlays[0].child.kill().await.unwrap();
        fixture.manager.refresh().unwrap();
        assert!(fixture.manager.workers[0].drawing_hold.is_none());
        let commands = wait_log(fixture.directory.path(), "stdin", pid, "\"pause\":false");
        assert!(commands.contains("\"mute\":false,\"volume\":42"));
        assert!(!Path::new(&target).exists());
    });
}

#[test]
fn shared_drawing_holds() {
    block_on(async {
        let mut fixture = Fixture::new();
        let first = media_drawing(fixture.directory.path());
        let mut second = first.clone();
        second.outputs = vec!["DP-2".into()];
        second.mute = true;
        second.transition.as_mut().unwrap().from = Some("/wall/other.png".into());
        let mut request = apply(vec![first.clone(), second.clone()], true);
        request.policy = Some(RendererPolicy {
            shared_outputs: Some(true),
            surface: Some(Box::new(paper_control::SurfacePolicy {
                namespace: "stationary".into(),
                blur: 0,
                dim: 0,
            })),
            ..Default::default()
        });
        let status = fixture.commit(&request).await;
        assert_eq!(fixture.manager.workers.len(), 1);
        assert_eq!(fixture.manager.overlays.len(), 2);
        assert_eq!(status[0].pid, status[1].pid);
        assert_eq!((status[0].assignment.mute, status[1].assignment.mute), (false, true));
        let worker = &fixture.manager.workers[0];
        assert!(worker.drawing_hold.is_some());
        let snapshots = wait_log(fixture.directory.path(), "stdin", worker.pid(), "DP-2");
        assert!(snapshots.contains("DP-1"));
        assert!(snapshots.contains("DP-2"));
        for (overlay, expected) in fixture.manager.overlays.iter().zip([first, second]) {
            let args =
                wait_log(fixture.directory.path(), "args", overlay.pid(), "--transition-from");
            assert!(args.contains(expected.transition.unwrap().from.as_ref().unwrap()));
        }
        fixture
            .manager
            .set_surface_overrides(paper_control::SurfaceOverridesRequest {
                surfaces: [(
                    SourceKind::Video,
                    paper_control::SurfacePolicy {
                        namespace: "stationary".into(),
                        blur: 12,
                        dim: 20,
                    },
                )]
                .into(),
            })
            .await
            .unwrap();
        for overlay in &fixture.manager.overlays {
            let commands =
                wait_log(fixture.directory.path(), "stdin", overlay.pid(), "drawing_photo");
            assert!(commands.contains(&overlay.output));
        }
        fixture.manager.overlays.remove(0).stop().await;
        fixture.manager.refresh().unwrap();
        assert!(fixture.manager.workers[0].drawing_hold.is_some());
        fixture.manager.overlays.remove(0).stop().await;
        fixture.manager.refresh().unwrap();
        assert!(fixture.manager.workers[0].drawing_hold.is_none());
        assert_eq!(fixture.manager.workers[0].pid(), status[0].pid);
        fixture.manager.stop(&StopRequest::default()).await.unwrap();
    });
}

#[test]
fn drawing_to_shared_video() {
    block_on(async {
        let mut fixture = Fixture::new();
        let first = media_drawing(fixture.directory.path());
        let mut second = first.clone();
        second.outputs = vec!["DP-2".into()];
        let mut drawing = apply(vec![first, second], true);
        drawing.policy = Some(RendererPolicy { shared_outputs: Some(true), ..Default::default() });
        fixture.commit(&drawing).await;
        let next = ["DP-1", "DP-2"].map(|output| {
            let mut item = assignment(output, Source::video("/wall/next.mp4"));
            item.transition = Some(TransitionPolicy::default());
            item
        });
        let status = fixture.commit(&apply(next.to_vec(), true)).await;
        assert_eq!(status[0].pid, status[1].pid);
        assert!(fixture.manager.overlays.is_empty());
        let origins = transition_origins(&fixture, status[0].pid);
        assert_eq!(origins.len(), 2);
        assert_ne!(origins["DP-1"], origins["DP-2"]);
        let args = wait_log(fixture.directory.path(), "args", status[0].pid, "DP-1,DP-2");
        assert_eq!(args.lines().next(), Some("DP-1,DP-2"));
        fixture.manager.stop(&StopRequest::default()).await.unwrap();
    });
}

#[test]
fn live_handoff_rollback() {
    block_on(async {
        let mut fixture = Fixture::new();
        let old = fixture
            .commit(&apply(vec![assignment("DP-1", Source::video("/wall/old.mp4"))], true))
            .await[0]
            .pid;
        let next = media_drawing(fixture.directory.path());
        let transaction =
            fixture.manager.begin_apply(&apply(vec![next], true), &fixture.socket).await.unwrap();
        let from = transaction.next.as_ref().unwrap().assignments[0]
            .0
            .transition
            .as_ref()
            .unwrap()
            .from
            .clone()
            .unwrap();
        assert!(Path::new(&from).exists());
        transaction.rollback().await;
        wait_log(fixture.directory.path(), "stdin", old, "\"pause\":false");
        assert!(!Path::new(&from).exists());
    });
}

fn stationary(mut request: ApplyRequest) -> ApplyRequest {
    request.policy = Some(RendererPolicy {
        surface: Some(Box::new(paper_control::SurfacePolicy {
            namespace: "stationary".into(),
            blur: 0,
            dim: 0,
        })),
        ..Default::default()
    });
    request
}

#[test]
fn stationary_live_handoff() {
    block_on(async {
        let mut fixture = Fixture::new();
        let old = assignment("DP-1", Source::video("/wall/old.mp4"));
        let old = fixture.commit(&stationary(apply(vec![old], true))).await[0].pid;
        let mut next = assignment("DP-1", Source::video("/wall/new.mp4"));
        next.transition = Some(TransitionPolicy::default());
        let request = stationary(apply(vec![next], true));
        let transaction = fixture.manager.begin_apply(&request, &fixture.socket).await.unwrap();
        let from =
            transaction.candidates[0].assignment.transition.as_ref().unwrap().from.clone().unwrap();
        assert_ne!(from, "/wall/old.mp4");
        assert!(Path::new(&from).exists());
        transaction.rollback().await;
        wait_log(fixture.directory.path(), "stdin", old, "\"pause\":false");
        assert!(!Path::new(&from).exists());
    });
}

#[test]
fn stationary_pause_stays_live() {
    block_on(async {
        let mut fixture = Fixture::new();
        let old = assignment("DP-1", Source::video("/wall/old.mp4"));
        fixture.commit(&stationary(apply(vec![old], true))).await;
        let transaction = fixture.manager.begin_pause(&fixture.socket).await.unwrap();
        assert!(transaction.is_none());
    });
}

#[test]
fn held_exit_cleanup() {
    block_on(async {
        let mut fixture = Fixture::new();
        let next = media_drawing(fixture.directory.path());
        fixture.commit(&apply(vec![next], true)).await;
        fixture.manager.workers[0].child.kill().await.unwrap();
        fixture.manager.overlays[0].child.kill().await.unwrap();
        fixture.manager.refresh().unwrap();
        assert!(fixture.manager.is_empty());
    });
}

#[test]
fn media_stage_rollback() {
    block_on(async {
        for stages in 0..=2 {
            let mut fixture = Fixture::new();
            let old = fixture
                .commit(&apply(
                    vec![assignment("DP-1", Source::static_file("/wall/old.png"))],
                    true,
                ))
                .await[0]
                .pid;
            let next = media_drawing(fixture.directory.path());
            let mut transaction = fixture
                .manager
                .begin_apply(&apply(vec![next], true), &fixture.socket)
                .await
                .unwrap();
            for _ in 0..stages {
                for ready in transaction.candidate_readiness() {
                    assert!(transaction.note_ready(&ready));
                }
                assert!(transaction.prepare_next().await.unwrap());
            }
            let snapshots = std::fs::read_dir(fixture.directory.path())
                .unwrap()
                .map(|entry| entry.unwrap().path().join(DRAWING_SNAPSHOT))
                .filter(|path| path.exists())
                .collect::<Vec<_>>();
            transaction.rollback().await;
            assert_eq!(fixture.manager.status()[0].pid, old);
            assert!(snapshots.iter().all(|snapshot| !snapshot.exists()));
        }
    });
}

#[test]
fn rejects_ambiguous_live_drawing() {
    block_on(async {
        let mut fixture = Fixture::new();
        let old = fixture
            .commit(&apply(
                vec![
                    assignment("DP-1", Source::video("/wall/one.mp4")),
                    assignment("DP-2", Source::video("/wall/two.mp4")),
                ],
                true,
            ))
            .await;
        let mut next = media_drawing(fixture.directory.path());
        next.outputs = vec!["*".into()];
        let error = fixture
            .manager
            .begin_apply(&apply(vec![next], true), &fixture.socket)
            .await
            .err()
            .unwrap();
        assert!(error.to_string().contains("one named output per renderer"));
        assert_eq!(
            fixture.manager.status().iter().map(|worker| worker.pid).collect::<Vec<_>>(),
            old.iter().map(|worker| worker.pid).collect::<Vec<_>>()
        );
        assert!(fixture.manager.overlays.is_empty());
    });
}

#[test]
fn drawing_overrides_disabled_policy() {
    block_on(async {
        for dynamic in [false, true] {
            let mut fixture = Fixture::new();
            let policy = RendererPolicy { transitions_enabled: Some(false), ..Default::default() };
            let mut initial = apply(
                vec![
                    assignment("DP-1", Source::static_file("/wall/one.png")),
                    assignment("DP-2", Source::static_file("/wall/two.png")),
                ],
                true,
            );
            initial.policy = Some(policy.clone());
            let old = fixture.commit(&initial).await;
            let mut next = media_drawing(fixture.directory.path());
            if !dynamic {
                next.source = Source::static_file(
                    next.transition
                        .as_ref()
                        .unwrap()
                        .drawing_source
                        .as_ref()
                        .unwrap()
                        .display()
                        .to_string(),
                );
            }
            let mut transaction = fixture
                .manager
                .begin_apply(&apply(vec![next], false), &fixture.socket)
                .await
                .unwrap();
            let pid = transaction.candidates[0].pid();
            let env = wait_log(fixture.directory.path(), "env", pid, "SKWD_PAPER_PREPARE_HIDDEN=1");
            assert!(env.contains("SKWD_PAPER_TRANSITIONS=0"));
            ready(&mut transaction).await;
            let overlay = transaction.overlays[0].pid();
            let env =
                wait_log(fixture.directory.path(), "env", overlay, "SKWD_PAPER_TRANSITIONS=1");
            assert!(env.contains("SKWD_PAPER_DRAWING="));
            fixture.manager.commit(transaction).await.unwrap();
            wait_log(fixture.directory.path(), "stdin", overlay, "\"pause\":false");
            assert_eq!(fixture.manager.policy(), Some(policy));
            assert!(fixture.manager.status().iter().any(|worker| worker.pid == old[1].pid));
            let mut regular = assignment("DP-1", Source::static_file("/wall/next.png"));
            regular.transition =
                Some(TransitionPolicy { from: Some("/wall/old.png".into()), ..Default::default() });
            assert!(
                fixture
                    .manager
                    .begin_apply(&apply(vec![regular], false), &fixture.socket)
                    .await
                    .is_err()
            );
            assert!(fixture.manager.status().iter().any(|worker| worker.pid == pid));
        }
    });
}

#[test]
fn held_controls_survive_release() {
    block_on(async {
        let mut fixture = Fixture::new();
        let next = media_drawing(fixture.directory.path());
        fixture
            .manager
            .playback_pause(&paper_control::PauseRequest {
                overrides: std::collections::BTreeMap::new(),
                paused: true,
                outputs: vec!["DP-1".into()],
            })
            .await
            .unwrap();
        fixture
            .manager
            .audio(&AudioSetRequest { duck: Some(true), ..Default::default() })
            .await
            .unwrap();
        fixture.commit(&apply(vec![next], true)).await;
        let pid = fixture.manager.workers[0].pid();
        let overlay = fixture.manager.overlays[0].pid();
        wait_log(fixture.directory.path(), "stdin", overlay, "\"pause\":true");
        fixture
            .manager
            .audio(&AudioSetRequest {
                outputs: vec!["DP-1".into()],
                mute: Some(false),
                volume: Some(42),
                duck: Some(false),
                system_audio: Some(false),
            })
            .await
            .unwrap();
        let input =
            std::fs::read_to_string(fixture.directory.path().join(format!("stdin-{pid}"))).unwrap();
        assert!(!input.contains("\"mute\":false"));
        assert!(!input.contains("\"pause\":false"));
        wait_log(fixture.directory.path(), "stdin", overlay, "\"duck\":false");
        fixture
            .manager
            .audio(&AudioSetRequest { duck: Some(true), ..Default::default() })
            .await
            .unwrap();
        let overlay_input = wait_log(
            fixture.directory.path(),
            "stdin",
            overlay,
            "\"duck\":false}\n{\"to\":\"\",\"duck\":true}",
        );
        assert!(!overlay_input.contains("\"mute\":false"));
        fixture.manager.overlays[0].child.kill().await.unwrap();
        fixture.manager.refresh().unwrap();
        let input = wait_log(fixture.directory.path(), "stdin", pid, "\"volume\":42");
        let commands: Vec<PaperCommand> =
            input.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
        let duck = commands.iter().position(|command| command.duck == Some(true)).unwrap();
        let unmute = commands.iter().position(|command| command.mute == Some(false)).unwrap();
        assert!(duck < unmute);
        assert_eq!(commands[unmute].system_audio, Some(false));
        assert!(!commands.iter().any(|command| command.pause == Some(false)));
        assert_eq!(fixture.manager.workers[0].pid(), pid);
    });
}

#[test]
fn rollback_keeps_scoped_pause() {
    block_on(async {
        let mut fixture = Fixture::new();
        let status =
            fixture.commit(&apply(vec![assignment("DP-1", Source::video("/one.mp4"))], true)).await;
        let pid = status[0].pid;
        fixture
            .manager
            .playback_pause(&paper_control::PauseRequest {
                paused: true,
                outputs: vec!["DP-1".into()],
                ..Default::default()
            })
            .await
            .unwrap();
        wait_log(fixture.directory.path(), "stdin", pid, "\"pause\":true");
        let path = fixture.directory.path().join(format!("stdin-{pid}"));
        std::fs::write(&path, "").unwrap();
        let next = media_drawing(fixture.directory.path());
        let transaction =
            fixture.manager.begin_apply(&apply(vec![next], true), &fixture.socket).await.unwrap();
        transaction.rollback().await;
        let input = wait_log(fixture.directory.path(), "stdin", pid, "\"pause\":true");
        assert!(!input.contains("\"pause\":false"));
        assert_eq!(fixture.manager.status()[0].pid, pid);
    });
}

fn scene_source(fixture: &Fixture) -> Source {
    let project = fixture.directory.path().join("scene");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("project.json"), r#"{"type":"scene","file":"scene.pkg"}"#).unwrap();
    std::fs::write(project.join("scene.pkg"), b"scene").unwrap();
    std::fs::write(project.join("preview.gif"), b"gif").unwrap();
    Source::wallpaper_engine(project.display().to_string())
}

fn shared_worker(outputs: &[&str], source: Source, fill_mode: FillMode) -> ApplyRequest {
    let mut old = Assignment::new(outputs.iter().map(ToString::to_string).collect(), source);
    old.fill_mode = fill_mode;
    apply(vec![old], true)
}

fn next_video(outputs: &[&str], fill_mode: FillMode) -> ApplyRequest {
    let mut request = shared_worker(outputs, Source::video("/wall/new.mp4"), fill_mode);
    request.assignments[0].transition = Some(TransitionPolicy::default());
    request
}

fn captured_from(transaction: &ApplyTransaction) -> String {
    let from =
        transaction.candidates[0].assignment.transition.as_ref().unwrap().from.clone().unwrap();
    assert!(from.ends_with(".ppm"), "{from}");
    assert!(Path::new(&from).exists());
    assert!(transaction.handoff.is_some());
    from
}

#[test]
fn wildcard_same_source() {
    block_on(async {
        for scene in [true, false] {
            let mut fixture = Fixture::new();
            let source =
                if scene { scene_source(&fixture) } else { Source::video("/wall/old.mp4") };
            let outputs =
                vec![assignment("DP-1", source.clone()), assignment("DP-2", source.clone())];
            fixture.commit(&apply(outputs, true)).await;
            let pid = fixture.manager.workers[0].pid();
            let request = next_video(&["*"], FillMode::Fill);
            let transaction = fixture.manager.begin_apply(&request, &fixture.socket).await.unwrap();
            assert_eq!(transaction.candidates.len(), 1);
            let from = captured_from(&transaction);
            transaction.rollback().await;
            wait_log(fixture.directory.path(), "stdin", pid, "\"pause\":false");
            assert!(!Path::new(&from).exists());
        }
    });
}

#[test]
fn span_captures_composition() {
    block_on(async {
        for (scene, outputs) in [(true, ["DP-1", "DP-2"]), (false, ["DP-2", "DP-1"])] {
            let mut fixture = Fixture::new();
            let source =
                if scene { scene_source(&fixture) } else { Source::video("/wall/old.mp4") };
            let old = shared_worker(&["DP-1", "DP-2"], source, FillMode::Span);
            let pid = fixture.commit(&old).await[0].pid;
            let request = next_video(&outputs, FillMode::Span);
            let transaction = fixture.manager.begin_apply(&request, &fixture.socket).await.unwrap();
            assert_eq!(transaction.candidates.len(), 1);
            let from = captured_from(&transaction);
            let commands = wait_log(fixture.directory.path(), "stdin", pid, "\"freeze\"");
            assert!(!commands.contains("snapshot"));
            transaction.rollback().await;
            wait_log(fixture.directory.path(), "stdin", pid, "\"pause\":false");
            assert!(!Path::new(&from).exists());
        }
    });
}

#[test]
fn named_from_shared_worker() {
    block_on(async {
        for (outputs, fill_mode) in [
            (&["DP-1", "DP-2"][..], FillMode::Span),
            (&["DP-2"][..], FillMode::Span),
            (&["*"][..], FillMode::Fill),
        ] {
            let mut fixture = Fixture::new();
            let old = shared_worker(outputs, scene_source(&fixture), fill_mode);
            let pid = fixture.commit(&old).await[0].pid;
            let request = next_video(&["DP-2"], FillMode::Fill);
            let transaction = fixture.manager.begin_apply(&request, &fixture.socket).await.unwrap();
            let from = captured_from(&transaction);
            let commands = wait_log(fixture.directory.path(), "stdin", pid, "\"output\":\"DP-2\"");
            assert!(!commands.contains("\"freeze\""));
            transaction.rollback().await;
            wait_log(fixture.directory.path(), "stdin", pid, "\"pause\":false");
            assert!(!Path::new(&from).exists());
        }
    });
}

#[test]
fn wildcard_leaves_span() {
    block_on(async {
        for outputs in [&["DP-1", "DP-2"][..], &["*"][..]] {
            let mut fixture = Fixture::new();
            let old = shared_worker(outputs, scene_source(&fixture), FillMode::Span);
            let pid = fixture.commit(&old).await[0].pid;
            let request = next_video(&["*"], FillMode::Fill);
            let transaction = fixture.manager.begin_apply(&request, &fixture.socket).await.unwrap();
            let from = captured_from(&transaction);
            let commands = wait_log(fixture.directory.path(), "stdin", pid, "\"output\":\"*\"");
            assert!(!commands.contains("\"freeze\""));
            transaction.rollback().await;
            wait_log(fixture.directory.path(), "stdin", pid, "\"pause\":false");
            assert!(!Path::new(&from).exists());
        }
    });
}
