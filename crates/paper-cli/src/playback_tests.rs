use super::*;
use paper_control::PauseRequest;

#[test]
fn scoped_pause_preserves_workers() {
    block_on(async {
        let mut fixture = Fixture::new();
        let initial = fixture
            .commit(&apply(
                vec![
                    assignment("DP-1", Source::video("/one.mp4")),
                    assignment("DP-2", Source::video("/two.mp4")),
                ],
                true,
            ))
            .await;
        for (outputs, paused) in [(vec!["*".into()], true), (vec!["DP-1".into()], false)] {
            fixture
                .manager
                .playback_pause(&PauseRequest {
                    overrides: std::collections::BTreeMap::new(),
                    outputs,
                    paused,
                })
                .await
                .unwrap();
        }
        for status in &initial {
            let expected = status.assignment.outputs != ["DP-1"];
            assert_eq!(
                fixture.manager.playback_pause.effective(
                    fixture
                        .manager
                        .workers
                        .iter()
                        .find(|worker| worker.pid() == status.pid)
                        .unwrap()
                ),
                expected
            );
        }
        let after = fixture.manager.status();
        assert_eq!(
            initial.iter().map(|status| status.pid).collect::<Vec<_>>(),
            after.iter().map(|status| status.pid).collect::<Vec<_>>()
        );
        let first = initial.iter().find(|status| status.assignment.outputs == ["DP-1"]).unwrap();
        wait_log(fixture.directory.path(), "stdin", first.pid, "\"pause\":false");
        assert!(fixture.manager.paused());
    });
}

#[test]
fn replacements_inherit_controls() {
    block_on(async {
        let mut fixture = Fixture::new();
        fixture
            .manager
            .playback_pause(&PauseRequest {
                overrides: std::collections::BTreeMap::new(),
                paused: true,
                outputs: vec!["DP-1".into()],
            })
            .await
            .unwrap();
        fixture
            .manager
            .audio(&AudioSetRequest { duck: Some(true), ..AudioSetRequest::default() })
            .await
            .unwrap();
        for source in ["/one.mp4", "/two.mp4"] {
            let mut next = assignment("DP-1", Source::video(source));
            next.mute = false;
            next.volume = 42;
            let status = fixture.commit(&apply(vec![next], true)).await;
            let input =
                wait_log(fixture.directory.path(), "stdin", status[0].pid, "\"pause\":true");
            assert!(input.contains("\"duck\":true"));
            wait_log(
                fixture.directory.path(),
                "env",
                status[0].pid,
                "SKWD_PAPER_PLAYBACK={\"paused\":true,\"ducked\":true}",
            );
            assert!(!status[0].assignment.mute);
            assert_eq!(status[0].assignment.volume, 42);
        }
    });
}

#[test]
fn rejects_partial_shared_controls() {
    block_on(async {
        for outputs in [vec!["*".into()], vec!["DP-1".into(), "DP-2".into()]] {
            let mut fixture = Fixture::new();
            let mut shared = assignment("DP-1", Source::video("/one.mp4"));
            shared.outputs = outputs;
            shared.fill_mode = FillMode::Span;
            fixture.commit(&apply(vec![shared], true)).await;
            let pause = PauseRequest {
                overrides: std::collections::BTreeMap::new(),
                paused: true,
                outputs: vec!["DP-1".into()],
            };
            assert!(fixture.manager.playback_pause(&pause).await.is_err());
            let audio =
                AudioSetRequest { outputs: pause.outputs, duck: Some(true), ..Default::default() };
            assert!(fixture.manager.audio(&audio).await.is_err());
            assert!(!fixture.manager.duck.effective(&fixture.manager.workers[0]));
        }
    });
}

#[test]
fn atomic_pause_preserves_overrides() {
    block_on(async {
        let mut fixture = Fixture::new();
        let status =
            fixture.commit(&apply(vec![assignment("DP-1", Source::video("/one.mp4"))], true)).await;
        let pid = status[0].pid;
        wait_log(fixture.directory.path(), "stdin", pid, "\"pause\":false");
        let path = fixture.directory.path().join(format!("stdin-{pid}"));
        std::fs::write(&path, "").unwrap();
        let request = PauseRequest {
            paused: false,
            outputs: vec!["*".into()],
            overrides: [("DP-1".into(), true)].into(),
        };
        for _ in 0..2 {
            fixture.manager.playback_pause(&request).await.unwrap();
        }
        let input = wait_log(fixture.directory.path(), "stdin", pid, "\"pause\":true");
        assert!(!input.contains("\"pause\":false"));
    });
}

#[test]
fn rejects_future_shared_controls() {
    block_on(async {
        for duck in [false, true] {
            let mut fixture = Fixture::new();
            if duck {
                fixture
                    .manager
                    .audio(&AudioSetRequest {
                        duck: Some(true),
                        outputs: vec!["DP-1".into()],
                        ..Default::default()
                    })
                    .await
                    .unwrap();
            } else {
                fixture
                    .manager
                    .playback_pause(&PauseRequest {
                        paused: true,
                        outputs: vec!["DP-1".into()],
                        ..Default::default()
                    })
                    .await
                    .unwrap();
            }
            for outputs in [vec!["*".into()], vec!["DP-1".into(), "DP-2".into()]] {
                let mut shared = assignment("DP-1", Source::video("/one.mp4"));
                shared.outputs = outputs;
                shared.fill_mode = FillMode::Span;
                assert!(
                    fixture
                        .manager
                        .begin_apply(&apply(vec![shared], true), &fixture.socket)
                        .await
                        .is_err()
                );
                assert!(fixture.manager.is_empty());
            }
        }
    });
}

#[test]
fn incompatible_apply_keeps_source() {
    block_on(async {
        let mut fixture = Fixture::new();
        let status =
            fixture.commit(&apply(vec![assignment("DP-1", Source::video("/one.mp4"))], true)).await;
        let pid = status[0].pid;
        fixture
            .manager
            .playback_pause(&PauseRequest {
                paused: true,
                outputs: vec!["DP-1".into()],
                ..Default::default()
            })
            .await
            .unwrap();
        wait_log(fixture.directory.path(), "stdin", pid, "\"pause\":true");
        let path = fixture.directory.path().join(format!("stdin-{pid}"));
        std::fs::write(&path, "").unwrap();
        let mut next = assignment("*", Source::video("/next.mp4"));
        next.transition =
            Some(TransitionPolicy { from: Some("/one.mp4".into()), ..Default::default() });
        assert!(
            fixture.manager.begin_apply(&apply(vec![next], true), &fixture.socket).await.is_err()
        );
        assert_eq!(fixture.manager.status()[0].pid, pid);
        fixture.manager.stop(&StopRequest::default()).await.unwrap();
        assert!(std::fs::read_to_string(path).unwrap().is_empty());
    });
}
