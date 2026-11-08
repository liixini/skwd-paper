use super::*;
use paper_control::{SourceKind, SurfaceOverridesRequest, SurfacePolicy};

fn surfaces() -> SurfaceOverridesRequest {
    SurfaceOverridesRequest {
        surfaces: [
            (SourceKind::Static, 11),
            (SourceKind::Video, 22),
            (SourceKind::WallpaperEngine, 33),
        ]
        .into_iter()
        .map(|(kind, blur)| (kind, SurfacePolicy { namespace: "stationary".into(), blur, dim: 44 }))
        .collect(),
    }
}

fn mixed(fixture: &Fixture) -> ApplyRequest {
    let project = fixture.directory.path().join("scene");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("project.json"), r#"{"type":"scene","file":"scene.pkg"}"#).unwrap();
    std::fs::write(project.join("scene.pkg"), b"scene").unwrap();
    let mut request = apply(
        vec![
            assignment("DP-1", Source::static_file("/one.png")),
            assignment("DP-2", Source::video("/two.mp4")),
            assignment("DP-3", Source::wallpaper_engine(project.display().to_string())),
        ],
        true,
    );
    request.policy = Some(RendererPolicy {
        surface: Some(Box::new(SurfacePolicy { namespace: "stationary".into(), blur: 0, dim: 0 })),
        ..Default::default()
    });
    request
}

fn last_surface(fixture: &Fixture, pid: u32, expected: u32) -> serde_json::Value {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        let lines = wait_log(fixture.directory.path(), "stdin", pid, "surface");
        let surface = lines
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter_map(|value| value.get("surface").cloned())
            .next_back()
            .unwrap();
        if surface["blur"] == expected {
            return surface;
        }
        assert!(std::time::Instant::now() < deadline, "surface rollback timed out");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn kind_effects_in_place() {
    block_on(async {
        let mut fixture = Fixture::new();
        let request = mixed(&fixture);
        let initial = fixture.commit(&request).await;
        let overrides = surfaces();
        fixture.manager.set_surface_overrides(overrides.clone()).await.unwrap();
        for worker in initial {
            let expected = overrides.surfaces[&worker.assignment.source.kind].blur;
            assert_eq!(last_surface(&fixture, worker.pid, expected)["blur"], expected);
            assert!(fixture.manager.status().iter().any(|status| status.pid == worker.pid));
        }
        assert_eq!(fixture.manager.policy(), request.policy);
        assert!(fixture.manager.surface_override.is_none());
    });
}

#[test]
fn future_workers_and_clear() {
    block_on(async {
        let mut fixture = Fixture::new();
        let overrides = surfaces();
        fixture.manager.set_surface_overrides(overrides.clone()).await.unwrap();
        let request = mixed(&fixture);
        let initial = fixture.commit(&request).await;
        for worker in &initial {
            let blur = overrides.surfaces[&worker.assignment.source.kind].blur;
            wait_log(
                fixture.directory.path(),
                "env",
                worker.pid,
                &format!("SKWD_PAPER_BLUR={blur}"),
            );
        }
        fixture.manager.set_surface_overrides(SurfaceOverridesRequest::default()).await.unwrap();
        for worker in &initial {
            assert_eq!(last_surface(&fixture, worker.pid, 0)["dim"], 0);
        }
        for worker in fixture.commit(&request).await {
            wait_log(fixture.directory.path(), "env", worker.pid, "SKWD_PAPER_BLUR=0");
        }
        assert_eq!(fixture.manager.policy(), request.policy);
    });
}

#[test]
fn surface_rejection_preserves_effects() {
    block_on(async {
        let mut fixture = Fixture::new();
        let request = mixed(&fixture);
        let initial = fixture.commit(&request).await;
        let overrides = surfaces();
        fixture.manager.set_surface_overrides(overrides.clone()).await.unwrap();
        let mut invalid = overrides.clone();
        for surface in invalid.surfaces.values_mut() {
            surface.namespace = "other".into();
        }
        assert!(fixture.manager.set_surface_overrides(invalid).await.is_err());
        fixture.manager.workers[2].child.kill().await.unwrap();
        fixture.manager.workers[2].child.wait().await.unwrap();
        assert!(
            fixture
                .manager
                .set_surface_overrides(SurfaceOverridesRequest::default())
                .await
                .is_err()
        );
        assert_eq!(fixture.manager.surface_overrides, overrides);
        for worker in &initial[..2] {
            let expected = overrides.surfaces[&worker.assignment.source.kind].blur;
            wait_log(fixture.directory.path(), "stdin", worker.pid, "\"blur\":0");
            assert_eq!(last_surface(&fixture, worker.pid, expected)["blur"], expected);
        }
        assert_eq!(fixture.manager.policy(), request.policy);
    });
}

#[test]
fn ordinary_clear() {
    block_on(async {
        let mut fixture = Fixture::new();
        let initial =
            fixture.commit(&apply(vec![assignment("DP-1", Source::video("/one.mp4"))], true)).await;
        fixture.manager.set_surface_overrides(SurfaceOverridesRequest::default()).await.unwrap();
        assert!(fixture.manager.set_surface_overrides(surfaces()).await.is_err());
        assert_eq!(fixture.manager.status()[0].pid, initial[0].pid);
    });
}

#[test]
fn namespace_change_is_transactional() {
    block_on(async {
        let mut fixture = Fixture::new();
        let request = mixed(&fixture);
        let initial = fixture.commit(&request).await;
        let overrides = surfaces();
        fixture.manager.set_surface_overrides(overrides.clone()).await.unwrap();
        let mut ordinary = request.clone();
        ordinary.policy.as_mut().unwrap().surface = None;
        let mut transaction =
            fixture.manager.begin_apply(&ordinary, &fixture.socket).await.unwrap();
        ready(&mut transaction).await;
        transaction.rollback().await;
        assert_eq!(fixture.manager.surface_overrides, overrides);
        assert_eq!(fixture.manager.status()[0].pid, initial[0].pid);
        fixture.commit(&ordinary).await;
        assert!(fixture.manager.surface_overrides.surfaces.is_empty());
        assert_eq!(fixture.manager.policy(), ordinary.policy);
    });
}

#[test]
fn global_surface_replaces_overrides() {
    block_on(async {
        let mut fixture = Fixture::new();
        let request = mixed(&fixture);
        let initial = fixture.commit(&request).await;
        fixture.manager.set_surface_overrides(surfaces()).await.unwrap();
        let surface = SurfacePolicy { namespace: "stationary".into(), blur: 55, dim: 66 };
        fixture.manager.set_surface(surface.clone()).await.unwrap();
        for worker in initial {
            assert_eq!(last_surface(&fixture, worker.pid, surface.blur)["dim"], surface.dim);
        }
        assert!(fixture.manager.surface_overrides.surfaces.is_empty());
        assert_eq!(fixture.manager.policy().unwrap().surface.as_deref(), Some(&surface));
    });
}

async fn styled_drawing(fixture: &mut Fixture, kind: SourceKind) -> ApplyRequest {
    fixture.manager.set_surface_overrides(surfaces()).await.unwrap();
    let mut next = super::handoff::media_drawing(fixture.directory.path());
    if kind == SourceKind::Static {
        next.source = Source::static_file(
            fixture.directory.path().join("reference.png").display().to_string(),
        );
    }
    let mut request = mixed(fixture);
    if kind == SourceKind::WallpaperEngine {
        next.source = request.assignments[2].source.clone();
    }
    request.assignments = vec![next];
    fixture.commit(&request).await;
    request
}

#[test]
fn drawing_uses_styled_photos() {
    block_on(async {
        for kind in [SourceKind::Static, SourceKind::Video, SourceKind::WallpaperEngine] {
            let mut fixture = Fixture::new();
            let request = styled_drawing(&mut fixture, kind).await;
            let pid = fixture.manager.workers[0].pid();
            let overlay = fixture.manager.overlays[0].pid();
            let blur = surfaces().surfaces[&kind].blur;
            wait_log(fixture.directory.path(), "env", pid, &format!("SKWD_PAPER_BLUR={blur}"));
            wait_log(fixture.directory.path(), "env", overlay, "SKWD_PAPER_BLUR=0");
            wait_log(fixture.directory.path(), "stdin", pid, "snapshot");
            let old_photo =
                fixture.manager.workers[0].drawing_hold.as_ref().unwrap().path().to_owned();
            fixture
                .manager
                .set_surface_overrides(SurfaceOverridesRequest::default())
                .await
                .unwrap();
            let commands = wait_log(fixture.directory.path(), "stdin", overlay, "applied.ppm");
            assert!(commands.contains("drawing_photo"));
            assert!(!commands.contains("surface"));
            assert_eq!(fixture.manager.workers[0].pid(), pid);
            assert_eq!(fixture.manager.overlays[0].pid(), overlay);
            assert_ne!(fixture.manager.workers[0].drawing_hold.as_ref().unwrap().path(), old_photo);
            assert!(fixture.manager.workers[0].pending_photos.is_empty());
            assert_eq!(fixture.manager.policy(), request.policy);
        }
    });
}

#[test]
fn photo_retry() {
    block_on(async {
        let mut fixture = Fixture::new();
        let executable = fixture.directory.path().join("worker");
        let source = std::fs::read_to_string(&executable).unwrap().replace(
            "printf 'P6",
            "if [ -e \"$base/reject-photo\" ]; then case \"$path\" in */applied.ppm) printf 'rejected' > \"$path.error\"; continue;; esac; fi; printf 'P6",
        );
        std::fs::write(executable, source).unwrap();
        let request = styled_drawing(&mut fixture, SourceKind::Video).await;
        let pid = fixture.manager.workers[0].pid();
        let overlay = fixture.manager.overlays[0].pid();
        let rejection = fixture.directory.path().join("reject-photo");
        std::fs::write(&rejection, b"").unwrap();
        assert!(
            fixture
                .manager
                .set_surface_overrides(SurfaceOverridesRequest::default())
                .await
                .is_err()
        );
        assert_eq!(fixture.manager.surface_overrides, surfaces());
        assert_eq!(last_surface(&fixture, pid, 22)["dim"], 44);
        assert!(fixture.manager.surface_retry.contains(&pid));
        assert!(!fixture.manager.workers[0].pending_photos.is_empty());
        std::fs::remove_file(rejection).unwrap();
        fixture.manager.set_surface_overrides(surfaces()).await.unwrap();
        assert!(fixture.manager.surface_retry.is_empty());
        assert!(fixture.manager.workers[0].pending_photos.is_empty());
        assert_eq!(fixture.manager.workers[0].pid(), pid);
        assert_eq!(fixture.manager.overlays[0].pid(), overlay);
        assert_eq!(fixture.manager.policy(), request.policy);
    });
}

#[test]
fn drawing_rejects_ambiguous_views() {
    block_on(async {
        let mut fixture = Fixture::new();
        for outputs in [vec!["*".into()], vec!["DP-1".into(), "DP-2".into()]] {
            let mut next = super::handoff::media_drawing(fixture.directory.path());
            next.outputs = outputs;
            next.fill_mode = FillMode::Span;
            assert!(
                fixture
                    .manager
                    .begin_apply(&apply(vec![next], true), &fixture.socket)
                    .await
                    .is_err()
            );
            assert!(fixture.manager.is_empty());
        }
    });
}

#[test]
fn shared_photo_retry() {
    block_on(async {
        let mut fixture = Fixture::new();
        let executable = fixture.directory.path().join("worker");
        let source = std::fs::read_to_string(&executable).unwrap().replace(
            "printf 'P6",
            "if [ -e \"$base/reject-photo-$$\" ]; then case \"$path\" in */applied.ppm) printf 'rejected' > \"$path.error\"; continue;; esac; fi; printf 'P6",
        );
        std::fs::write(executable, source).unwrap();
        fixture.manager.set_surface_overrides(surfaces()).await.unwrap();
        let first = super::handoff::media_drawing(fixture.directory.path());
        let mut second = first.clone();
        second.outputs = vec!["DP-2".into()];
        let mut request = mixed(&fixture);
        request.assignments = vec![first, second];
        request.policy.as_mut().unwrap().shared_outputs = Some(true);
        fixture.commit(&request).await;
        let pid = fixture.manager.workers[0].pid();
        let overlay = fixture.manager.overlays[1].pid();
        let rejection = fixture.directory.path().join(format!("reject-photo-{overlay}"));
        std::fs::write(&rejection, b"").unwrap();
        assert!(
            fixture
                .manager
                .set_surface_overrides(SurfaceOverridesRequest::default())
                .await
                .is_err()
        );
        assert_eq!(fixture.manager.surface_overrides, surfaces());
        let pending = &fixture.manager.workers[0].pending_photos;
        assert!(pending.len() >= fixture.manager.overlays.len() * 2);
        assert!(pending.iter().all(|directory| directory.path().exists()));
        assert!(fixture.manager.surface_retry.contains(&pid));
        std::fs::remove_file(rejection).unwrap();
        fixture.manager.set_surface_overrides(surfaces()).await.unwrap();
        assert!(fixture.manager.surface_retry.is_empty());
        assert!(fixture.manager.workers[0].pending_photos.is_empty());
        assert_eq!(fixture.manager.workers[0].pid(), pid);
        assert_eq!(fixture.manager.overlays[1].pid(), overlay);
        fixture.manager.stop(&StopRequest::default()).await.unwrap();
    });
}

#[test]
fn drawing_captures_static_owner() {
    block_on(async {
        let mut fixture = Fixture::new();
        let mut initial = mixed(&fixture);
        initial.assignments.truncate(1);
        let pid = fixture.commit(&initial).await[0].pid;
        fixture.manager.set_surface_overrides(surfaces()).await.unwrap();
        initial.assignments = vec![super::handoff::media_drawing(fixture.directory.path())];
        let mut transaction = fixture.manager.begin_apply(&initial, &fixture.socket).await.unwrap();
        let source = transaction.next.as_ref().unwrap().assignments[0]
            .0
            .transition
            .as_ref()
            .unwrap()
            .from
            .clone()
            .unwrap();
        assert!(Path::new(&source).exists());
        let commands = wait_log(fixture.directory.path(), "stdin", pid, "snapshot");
        assert!(commands.contains("\"output\":\"DP-1\""));
        assert!(!commands.contains("\"pause\":true"));
        ready(&mut transaction).await;
        transaction.rollback().await;
        assert_eq!(fixture.manager.workers[0].pid(), pid);
        assert_eq!(last_surface(&fixture, pid, 11)["dim"], 44);
    });
}

fn scene() -> paper_control::WidgetScene {
    let names = |list: &str| list.split(' ').map(str::to_string).collect::<Vec<_>>();
    paper_control::WidgetScene {
        widgets: Vec::new(),
        outlines: Vec::new(),
        palette: paper_control::Palette { text: [236, 235, 231], accent: [90, 110, 200], shade: [18, 19, 22] },
        font: None,
        labels: paper_control::Labels {
            days: names("MON TUE WED THU FRI SAT SUN").try_into().unwrap(),
            day_names: names("MONDAY TUESDAY WEDNESDAY THURSDAY FRIDAY SATURDAY SUNDAY").try_into().unwrap(),
            months: names("JAN FEB MAR APR MAY JUN JUL AUG SEP OCT NOV DEC").try_into().unwrap(),
            month_names: names(
                "JANUARY FEBRUARY MARCH APRIL MAY JUNE JULY AUGUST SEPTEMBER OCTOBER NOVEMBER DECEMBER",
            )
            .try_into()
            .unwrap(),
        },
    }
}

#[test]
fn widgets_reach_still_workers() {
    block_on(async {
        let mut fixture = Fixture::new();
        let request = mixed(&fixture);
        let initial = fixture.commit(&request).await;
        fixture.manager.set_widgets(scene()).await.unwrap();
        let still = initial
            .iter()
            .find(|worker| worker.assignment.source.kind == SourceKind::Static)
            .unwrap();
        let lines = wait_log(fixture.directory.path(), "stdin", still.pid, "widgets");
        assert!(lines.contains("\"widgets\""));
        assert_eq!(fixture.manager.widgets, None);
    });
}

#[test]
fn widgets_reach_dynamic_workers() {
    block_on(async {
        let mut fixture = Fixture::new();
        let request = mixed(&fixture);
        let initial = fixture.commit(&request).await;
        fixture.manager.set_widgets(scene()).await.unwrap();
        for worker in
            initial.iter().filter(|worker| worker.assignment.source.kind != SourceKind::Static)
        {
            let lines = wait_log(fixture.directory.path(), "stdin", worker.pid, "widgets");
            assert!(lines.contains("\"widgets\""));
        }
    });
}

#[test]
fn keeps_placed_widgets() {
    block_on(async {
        let mut manager = Manager::new();
        let mut placed = scene();
        placed.widgets.push(paper_control::Widget {
            face: paper_control::Face::Clock(paper_control::ClockStyle::Aperture),
            depth: paper_control::Depth::Front,
            pin: paper_control::Pin { anchor: [0.5, 0.5], offset: [0.0, 0.0] },
            height: None,
            output: None,
        });
        manager.set_widgets(placed.clone()).await.unwrap();
        assert_eq!(manager.widgets, Some(placed));
    });
}

#[test]
fn rejects_bad_widgets() {
    block_on(async {
        let mut fixture = Fixture::new();
        let mut bad = scene();
        bad.font = Some(std::path::PathBuf::from("relative.ttf"));
        assert!(fixture.manager.set_widgets(bad).await.is_err());
        assert_eq!(fixture.manager.widgets, None);
    });
}
