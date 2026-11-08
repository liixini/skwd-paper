use super::*;

fn assignment(outputs: &[&str], source: Source) -> Assignment {
    Assignment::new(outputs.iter().map(|output| (*output).to_string()).collect(), source)
}

#[test]
fn drawing_ignores_normal_policy() {
    for source in [
        Source::static_file("/wall/image.png"),
        Source::video("/wall/video.mp4"),
        Source::wallpaper_engine("/wall/project.json"),
    ] {
        let mut target = assignment(&["DP-1"], source);
        target.transition = Some(TransitionPolicy {
            from: Some("/wall/previous.png".into()),
            effect: Some(crate::DRAWING_EFFECT.into()),
            drawing: Some(crate::DrawingTransition {
                artifact: "/cache/drawing/manifest.json".into(),
                colours: crate::DrawingColours::default(),
                ink: crate::DrawingInk::Plain,
            }),
            drawing_source: Some("/cache/drawing/reference.png".into()),
            ..Default::default()
        });
        let request = ApplyRequest {
            assignments: vec![target],
            replace_all: false,
            policy: Some(RendererPolicy { transitions_enabled: Some(false), ..Default::default() }),
        };
        assert_eq!(request.validate(), Ok(()));
    }
}

#[test]
fn apply_golden_line() {
    let request = Request::new(
        7,
        RequestParams::Apply(ApplyRequest {
            assignments: vec![
                assignment(&["DP-1"], Source::static_file("/wall/a.png")),
                assignment(&["DP-2"], Source::video("/wall/b.mp4")),
                assignment(&["HDMI-A-1"], Source::wallpaper_engine("/scene/forest")),
            ],
            replace_all: true,
            policy: None,
        }),
    );
    let golden = concat!(
        r#"{"id":7,"method":"paper.apply","params":{"assignments":["#,
        r#"{"outputs":["DP-1"],"source":{"kind":"static","path":"/wall/a.png"}},"#,
        r#"{"outputs":["DP-2"],"source":{"kind":"video","path":"/wall/b.mp4"}},"#,
        r#"{"outputs":["HDMI-A-1"],"source":{"kind":"we","path":"/scene/forest"}}"#,
        r#"],"replace_all":true}}"#,
        "\n"
    );
    assert_eq!(encode_ndjson(&request).unwrap(), golden);
    assert_eq!(decode_ndjson::<Request>(golden).unwrap(), request);
    assert_eq!(request.validate(), Ok(()));
}

#[test]
fn apply_defaults() {
    let golden = concat!(
        r#"{"id":8,"method":"paper.apply","params":{"assignments":["#,
        r#"{"outputs":["DP-1"],"source":{"kind":"video","path":"/wall/a.mp4"}}"#,
        r#"]}}"#,
        "\n"
    );
    let request = decode_ndjson::<Request>(golden).unwrap();
    let RequestParams::Apply(apply) = &request.params else {
        panic!("expected apply request");
    };
    assert!(!apply.replace_all);
    assert_eq!(apply.assignments[0].fill_mode, FillMode::Fill);
    assert!(apply.assignments[0].mute);
    assert_eq!(apply.assignments[0].volume, 80);
    assert_eq!(apply.assignments[0].layer, Layer::Background);
    assert_eq!(encode_ndjson(&request).unwrap(), golden);
}

#[test]
fn retired_engine_fields_ignored() {
    let source: Source = serde_json::from_str(
        r#"{"kind":"video","path":"/wall/loop.ivf","engine":"tinier","frame_rate":"30000/1001"}"#,
    )
    .unwrap();
    assert_eq!(source, Source::video("/wall/loop.ivf"));
    assert_eq!(
        serde_json::to_string(&source).unwrap(),
        r#"{"kind":"video","path":"/wall/loop.ivf"}"#
    );
}

#[test]
fn assignment_options_golden() {
    let mut configured = assignment(&["DP-1"], Source::video("/wall/a.mp4"));
    configured.fill_mode = FillMode::Fit;
    configured.mute = false;
    configured.volume = 55;
    configured.layer = Layer::Top;
    let request = Request::new(
        14,
        RequestParams::Apply(ApplyRequest {
            assignments: vec![configured],
            replace_all: false,
            policy: None,
        }),
    );
    let golden = concat!(
        r#"{"id":14,"method":"paper.apply","params":{"assignments":["#,
        r#"{"outputs":["DP-1"],"source":{"kind":"video","path":"/wall/a.mp4"},"#,
        r#""fill_mode":"fit","mute":false,"volume":55,"layer":"top"}]}}"#,
        "\n"
    );
    assert_eq!(encode_ndjson(&request).unwrap(), golden);
    assert_eq!(decode_ndjson::<Request>(golden).unwrap(), request);
    assert_eq!(request.validate(), Ok(()));
}

#[test]
fn policy_golden_line() {
    let mut configured = assignment(&["DP-1"], Source::video("/wall/b.mp4"));
    configured.transition = Some(TransitionPolicy {
        drawing_source: None,
        drawing: None,
        fps: None,
        from: Some("/wall/a.png".into()),
        effect: Some("sand-bloom".into()),
        duration_ms: Some(700),
    });
    let policy = RendererPolicy {
        transition_fps: None,
        shared_outputs: None,
        load_timeout_ms: None,
        surface: None,
        idle_seconds: Some(45),
        transitions_enabled: Some(true),
        sand: Some(SandPolicy {
            quality: Some(SandQuality::Low),
            scope: Some(SandScope::Primary),
            primary: Some("DP-1".into()),
            sharp: Some(true),
            fps: Some(30),
        }),
        scene: Some(ScenePolicy {
            fps: Some(60),
            disable_particles: Some(true),
            assets_dir: Some("/we/assets".into()),
            max_dimension: Some(2048),
            max_effect_chains: Some(4),
            max_effect_passes: Some(8),
            strict: Some(true),
            clamp: Some(SceneClamp::Border),
        }),
        output_fps: [("DP-1".into(), 60), ("DP-2".into(), 120)].into(),
    };
    let request = Request::new(
        15,
        RequestParams::Apply(ApplyRequest {
            assignments: vec![configured],
            replace_all: true,
            policy: Some(policy),
        }),
    );
    let golden = concat!(
        r#"{"id":15,"method":"paper.apply","params":{"assignments":["#,
        r#"{"outputs":["DP-1"],"source":{"kind":"video","path":"/wall/b.mp4"},"#,
        r#""transition":{"from":"/wall/a.png","effect":"sand-bloom","duration_ms":700}}],"#,
        r#""replace_all":true,"policy":{"idle_seconds":45,"transitions_enabled":true,"#,
        r#""sand":{"quality":"low","scope":"primary","primary":"DP-1","sharp":true,"fps":30},"#,
        r#""scene":{"fps":60,"disable_particles":true,"assets_dir":"/we/assets","#,
        r#""max_dimension":2048,"max_effect_chains":4,"max_effect_passes":8,"strict":true,"clamp":"border"},"#,
        r#""output_fps":{"DP-1":60,"DP-2":120}}}}"#,
        "\n"
    );
    assert_eq!(encode_ndjson(&request).unwrap(), golden);
    assert_eq!(decode_ndjson::<Request>(golden).unwrap(), request);
    assert_eq!(request.validate(), Ok(()));
}

#[test]
fn apply_rejects_overlaps() {
    let duplicate = ApplyRequest {
        assignments: vec![
            assignment(&["DP-1", "DP-2"], Source::static_file("/wall/a.png")),
            assignment(&["DP-2"], Source::video("/wall/b.mp4")),
        ],
        replace_all: false,
        policy: None,
    };
    assert_eq!(duplicate.validate(), Err(ValidationError::DuplicateOutput("DP-2".to_string())));

    let mixed_wildcard = ApplyRequest {
        assignments: vec![
            assignment(&["*"], Source::static_file("/wall/a.png")),
            assignment(&["DP-1"], Source::video("/wall/b.mp4")),
        ],
        replace_all: false,
        policy: None,
    };
    assert_eq!(mixed_wildcard.validate(), Err(ValidationError::WildcardMixed));

    let wildcard_with_name = ApplyRequest {
        assignments: vec![assignment(&["*", "DP-1"], Source::wallpaper_engine("/scene/forest"))],
        replace_all: false,
        policy: None,
    };
    assert_eq!(wildcard_with_name.validate(), Err(ValidationError::WildcardMixed));

    let wildcard = ApplyRequest {
        assignments: vec![assignment(&["*"], Source::static_file("/wall/a.png"))],
        replace_all: false,
        policy: None,
    };
    assert_eq!(wildcard.validate(), Ok(()));
}

#[test]
fn apply_rejects_unsafe_args() {
    for output in ["DP-1,DP-2", "--output"] {
        let request = ApplyRequest {
            assignments: vec![assignment(&[output], Source::static_file("/wall/a.png"))],
            replace_all: false,
            policy: None,
        };
        assert!(request.validate().is_err(), "{output}");
    }

    let unsafe_path = ApplyRequest {
        assignments: vec![assignment(&["DP-1"], Source::video("--scene"))],
        replace_all: false,
        policy: None,
    };
    assert_eq!(unsafe_path.validate(), Err(ValidationError::SourcePathStartsWithDash));

    let mut loud = assignment(&["DP-1"], Source::video("/wall/a.mp4"));
    loud.volume = 101;
    assert_eq!(
        ApplyRequest { assignments: vec![loud.clone()], replace_all: false, policy: None }
            .validate(),
        Err(ValidationError::VolumeOutOfRange(101))
    );
    assert_eq!(loud.normalized().volume, 100);

    let mut elevated = assignment(&["DP-1"], Source::static_file("/wall/a.png"));
    elevated.layer = Layer::Top;
    assert_eq!(
        ApplyRequest { assignments: vec![elevated], replace_all: false, policy: None }.validate(),
        Err(ValidationError::LayerNotAllowed(Layer::Top))
    );

    for outputs in [vec!["DP-1,DP-2".into()], vec!["--output".into()]] {
        assert!(StopRequest { outputs }.validate().is_err());
    }
}

#[test]
fn rejects_unsafe_controls() {
    let mut static_assignment = assignment(&["DP-1"], Source::static_file("/wall/a.png"));
    static_assignment.transition = Some(TransitionPolicy::default());
    assert_eq!(
        ApplyRequest { assignments: vec![static_assignment], replace_all: false, policy: None }
            .validate(),
        Ok(())
    );

    for transition in [
        TransitionPolicy {
            drawing_source: None,
            drawing: None,
            fps: None,
            from: Some(String::new()),
            effect: None,
            duration_ms: None,
        },
        TransitionPolicy {
            drawing_source: None,
            drawing: None,
            fps: None,
            from: Some("--old".into()),
            effect: None,
            duration_ms: None,
        },
        TransitionPolicy {
            drawing_source: None,
            drawing: None,
            fps: None,
            from: None,
            effect: Some("../fade".into()),
            duration_ms: None,
        },
        TransitionPolicy {
            drawing_source: None,
            drawing: None,
            fps: None,
            from: None,
            effect: None,
            duration_ms: Some(49),
        },
    ] {
        assert!(transition.validate().is_err());
    }

    let mut video = assignment(&["DP-1"], Source::video("/wall/a.mp4"));
    video.transition = Some(TransitionPolicy::default());
    assert_eq!(
        ApplyRequest {
            assignments: vec![video],
            replace_all: false,
            policy: Some(RendererPolicy {
                transition_fps: None,
                transitions_enabled: Some(false),
                ..RendererPolicy::default()
            }),
        }
        .validate(),
        Err(ValidationError::TransitionsDisabled)
    );

    assert_eq!(AudioSetRequest::default().validate(), Err(ValidationError::MissingAudioChange));
    assert_eq!(
        AudioSetRequest { volume: Some(101), ..Default::default() }.validate(),
        Err(ValidationError::VolumeOutOfRange(101))
    );
    for policy in [
        RendererPolicy {
            sand: Some(SandPolicy { fps: Some(0), ..Default::default() }),
            ..Default::default()
        },
        RendererPolicy {
            scene: Some(ScenePolicy { max_dimension: Some(16_385), ..Default::default() }),
            ..Default::default()
        },
        RendererPolicy {
            scene: Some(ScenePolicy { max_effect_chains: Some(65), ..Default::default() }),
            ..Default::default()
        },
        RendererPolicy {
            scene: Some(ScenePolicy { max_effect_passes: Some(65), ..Default::default() }),
            ..Default::default()
        },
    ] {
        assert!(policy.validate().is_err());
    }
}

#[test]
fn request_method_goldens() {
    let stop = Request::new(
        10,
        RequestParams::Stop(StopRequest { outputs: vec!["DP-1".into(), "DP-2".into()] }),
    );
    assert_eq!(
        encode_ndjson(&stop).unwrap(),
        "{\"id\":10,\"method\":\"paper.stop\",\"params\":{\"outputs\":[\"DP-1\",\"DP-2\"]}}\n"
    );
    let status = Request::new(11, RequestParams::Status(StatusRequest {}));
    assert_eq!(
        encode_ndjson(&status).unwrap(),
        "{\"id\":11,\"method\":\"paper.status\",\"params\":{}}\n"
    );
    let capabilities =
        Request::new(12, RequestParams::Capabilities(CapabilitiesRequest::default()));
    assert_eq!(
        encode_ndjson(&capabilities).unwrap(),
        "{\"id\":12,\"method\":\"paper.capabilities\",\"params\":{}}\n"
    );
    let reset = Request::new(
        13,
        RequestParams::Capabilities(CapabilitiesRequest { reset_decode_cache: true }),
    );
    assert_eq!(
        encode_ndjson(&reset).unwrap(),
        "{\"id\":13,\"method\":\"paper.capabilities\",\"params\":{\"reset_decode_cache\":true}}\n"
    );
    let ready = Request::new(0, RequestParams::Ready(RendererReady { pid: 321, generation: 44 }));
    assert_eq!(
        encode_ndjson(&ready).unwrap(),
        "{\"id\":0,\"method\":\"paper.ready\",\"params\":{\"pid\":321,\"generation\":44}}\n"
    );
    let ready_without_id =
        "{\"method\":\"paper.ready\",\"params\":{\"pid\":321,\"generation\":44}}\n";
    assert_eq!(decode_ndjson::<Request>(ready_without_id).unwrap(), ready);
    let failed = Request::new(
        0,
        RequestParams::Failed(RendererFailed {
            pid: 321,
            generation: 44,
            code: "renderer_startup".into(),
            message: "[native-scene-gap:light-objects] unsupported object".into(),
        }),
    );
    assert_eq!(
        encode_ndjson(&failed).unwrap(),
        concat!(
            r#"{"id":0,"method":"paper.failed","params":{"pid":321,"generation":44,"#,
            r#""code":"renderer_startup","message":"[native-scene-gap:light-objects] unsupported object"}}"#,
            "\n"
        )
    );
    let pause = Request::new(
        13,
        RequestParams::Pause(PauseRequest {
            overrides: std::collections::BTreeMap::new(),
            paused: true,
            outputs: Vec::new(),
        }),
    );
    assert_eq!(
        encode_ndjson(&pause).unwrap(),
        "{\"id\":13,\"method\":\"paper.pause\",\"params\":{\"paused\":true}}\n"
    );
    let audio = Request::new(
        14,
        RequestParams::AudioSet(AudioSetRequest {
            duck: None,
            outputs: vec!["DP-1".into()],
            mute: Some(false),
            volume: Some(55),
            system_audio: None,
        }),
    );
    assert_eq!(
        encode_ndjson(&audio).unwrap(),
        concat!(
            r#"{"id":14,"method":"paper.audio.set","params":{"outputs":["DP-1"],"#,
            r#""mute":false,"volume":55}}"#,
            "\n"
        )
    );
}

#[test]
fn response_goldens() {
    let assignment = assignment(&["DP-1"], Source::video("/wall/a.mp4"));
    let status = AssignmentStatus::from_assignment(&assignment, 44, Some(321), true);
    let apply = ApplyResponse::success(
        7,
        ApplyResult {
            generation: 44,
            paused: false,
            policy: None,
            assignments: vec![status.clone()],
        },
    );
    let apply_golden = concat!(
        r#"{"id":7,"result":{"generation":44,"paused":false,"assignments":["#,
        r#"{"outputs":["DP-1"],"source":{"kind":"video","path":"/wall/a.mp4"},"#,
        r#""fill_mode":"fill","mute":true,"volume":80,"layer":"background","#,
        r#""generation":44,"pid":321,"ready":true}]}}"#,
        "\n"
    );
    assert_eq!(encode_ndjson(&apply).unwrap(), apply_golden);
    assert_eq!(decode_ndjson::<ApplyResponse>(apply_golden).unwrap(), apply);

    let stop = StopResponse::success(8, StopResult { stopped: 2 });
    assert_eq!(encode_ndjson(&stop).unwrap(), "{\"id\":8,\"result\":{\"stopped\":2}}\n");
    let current = StatusResponse::success(
        9,
        StatusResult { paused: false, policy: None, assignments: vec![status], renderers: vec![] },
    );
    assert!(encode_ndjson(&current).unwrap().contains("\"assignments\""));
    let capabilities = CapabilitiesResponse::success(10, CapabilitiesResult::current());
    assert_eq!(
        encode_ndjson(&capabilities).unwrap(),
        concat!(
            r#"{"id":10,"result":{"protocol":"skwd-paper","version":1,"#,
            r#""source_kinds":["static","video","we"],"#,
            r#""fill_modes":["fill","fit","stretch","center","tile","span"],"#,
            r#""layers":["background","bottom","top","overlay"],"#,
            r#""controls":{"surface_kinds":["static","video","we"],"playback":["scoped_pause","duck"],"mirror":true,"pause":true,"audio":true,"surface_effects":true},"#,
            r#""transitions":{"drawing":true,"drawing_max_duration_ms":20000,"startup_source_kinds":["video","we"],"#,
            r#""static_overlay":true,"default_effect":"fade","default_duration_ms":600,"#,
            r#""min_duration_ms":50,"max_duration_ms":10000},"#,
            r#""renderer_policy":{"shared_outputs":true,"surface":true,"idle":true,"sand":true,"scene":true,"output_fps":true},"#,
            r#""wallpaper_engine":{"project_types":["scene","video"],"#,
            r#""rejected_project_types":["web","application"],"scene":{"#,
            r#""renderer":"native-vulkan","supported":["image-layers","effect-chains","cross-layer-render-targets","particles","puppet-skeletons","scene-audio-mixing","user-properties"],"#,
            r#""partial":["shader-dialect","particle-timing-density-trails","particle-refraction"],"#,
            r#""unsupported":["embedded-video-textures","animated-image-textures","audio-reactivity","event-driven-sounds","light-objects","text-objects"],"strict_gap_rejection":true}},"#,
            r#""decode":{"reset_supported":false}}}"#,
            "\n"
        )
    );
    assert_eq!(CapabilitiesResult::current().protocol, PROTOCOL_NAME);
    assert_eq!(CapabilitiesResult::current().version, PROTOCOL_VERSION);
    let phase_one_apply = concat!(r#"{"id":7,"result":{"generation":44,"assignments":[]}}"#, "\n");
    let decoded = decode_ndjson::<ApplyResponse>(phase_one_apply).unwrap();
    let ResponseBody::Success { result } = decoded.body else { panic!("expected success") };
    assert!(!result.paused);
    assert!(result.policy.is_none());
    let phase_one_capabilities = concat!(
        r#"{"id":10,"result":{"protocol":"skwd-paper","version":1,"#,
        r#""source_kinds":["static","video","we"],"#,
        r#""fill_modes":["fill"],"layers":["background"]}}"#,
        "\n"
    );
    let decoded = decode_ndjson::<CapabilitiesResponse>(phase_one_capabilities).unwrap();
    let ResponseBody::Success { result } = decoded.body else { panic!("expected success") };
    assert_eq!(result.controls, ControlCapabilities::default());
    assert_eq!(result.wallpaper_engine, WallpaperEngineCapabilities::default());
    assert!(result.renderers.is_empty());
    let failure = StopResponse::failure(11, "conflict", "renderer generation changed");
    assert_eq!(
        encode_ndjson(&failure).unwrap(),
        concat!(
            r#"{"id":11,"error":{"code":"conflict","#,
            r#""message":"renderer generation changed"}}"#,
            "\n"
        )
    );
}

#[test]
fn renderer_capability_wire_shape() {
    let renderer = RendererCapability {
        executable: "skwd-wall-vk".into(),
        source_kinds: vec![SourceKind::Video, SourceKind::WallpaperEngine],
        path: Some("/usr/bin/skwd-wall-vk".into()),
        discovery: RendererDiscovery::Path,
        present: true,
        executable_file: true,
        dependencies: vec![RuntimeDependencyStatus {
            name: "vulkan_loader".into(),
            available: true,
            detail: "libvulkan.so.1 is loadable".into(),
        }],
        diagnostic: None,
    };
    let encoded = serde_json::to_string(&renderer).unwrap();
    assert_eq!(
        encoded,
        concat!(
            r#"{"executable":"skwd-wall-vk","source_kinds":["video","we"],"#,
            r#""path":"/usr/bin/skwd-wall-vk","#,
            r#""discovery":"path","present":true,"executable_file":true,"dependencies":[{"#,
            r#""name":"vulkan_loader","available":true,"detail":"libvulkan.so.1 is loadable"}]}"#
        )
    );
    assert!(renderer.available());
    assert_eq!(serde_json::from_str::<RendererCapability>(&encoded).unwrap(), renderer);
}

#[test]
fn decode_cache_wire_shape() {
    assert!(!CapabilitiesResult::current().decode.reset_supported);
    let golden =
        "{\"id\":13,\"method\":\"paper.capabilities\",\"params\":{\"reset_decode_cache\":true}}\n";
    let decoded = decode_ndjson::<Request>(golden).unwrap();
    let RequestParams::Capabilities(request) = decoded.params else {
        panic!("expected a capabilities request");
    };
    assert!(request.reset_decode_cache);
    assert_eq!(encode_ndjson(&decoded).unwrap(), golden);
}

#[test]
fn ndjson_one_record() {
    assert!(matches!(decode_ndjson::<Request>("{}"), Err(NdjsonError::MissingTerminator)));
    assert!(matches!(decode_ndjson::<Request>("\n"), Err(NdjsonError::EmptyRecord)));
    assert!(matches!(decode_ndjson::<Request>("{}\n{}\n"), Err(NdjsonError::MultipleRecords)));
    assert!(matches!(decode_ndjson::<Request>("{\n}\n"), Err(NdjsonError::MultipleRecords)));
}

#[test]
fn scene_properties_bounded() {
    let mut properties = serde_json::Map::new();
    properties.insert("tint".into(), serde_json::json!("1 0 0"));
    assert_eq!(
        Source::wallpaper_engine("/wall/item").with_properties(properties.clone()).validate(),
        Ok(())
    );
    for kind in [SourceKind::Static, SourceKind::Video] {
        let source =
            Source { kind, path: "/wall/source".into(), properties: Some(properties.clone()) };
        assert_eq!(source.validate(), Err(ValidationError::PropertiesNotAllowed(kind)));
    }

    let mut oversized = serde_json::Map::new();
    for index in 0..=MAX_SCENE_PROPERTIES {
        oversized.insert(format!("p{index}"), serde_json::json!(1));
    }
    let count = oversized.len();
    assert_eq!(
        Source::wallpaper_engine("/wall/item").with_properties(oversized).validate(),
        Err(ValidationError::TooManySceneProperties(count))
    );

    let mut unnamed = serde_json::Map::new();
    unnamed.insert("  ".into(), serde_json::json!(1));
    assert_eq!(
        Source::wallpaper_engine("/wall/item").with_properties(unnamed).validate(),
        Err(ValidationError::InvalidScenePropertyName("  ".into()))
    );
}

#[test]
fn scene_properties_round_trip() {
    let raw = concat!(
        r#"{"id":21,"method":"paper.apply","params":{"assignments":[{"outputs":["DP-1"],"#,
        r#""source":{"kind":"we","path":"/wall/item","properties":{"tint":"1 0 0","fade":0.5}}}]}}"#
    );
    let request: Request = serde_json::from_str(raw).unwrap();
    let RequestParams::Apply(apply) = request.params else {
        panic!("expected an apply request");
    };
    apply.validate().unwrap();
    let properties = apply.assignments[0].source.properties.as_ref().unwrap();
    assert_eq!(properties.get("tint").unwrap(), &serde_json::json!("1 0 0"));
    assert_eq!(properties.get("fade").unwrap(), &serde_json::json!(0.5));
    let encoded = serde_json::to_string(&apply.assignments[0].source).unwrap();
    assert!(encoded.contains(r#""properties":{"fade":0.5,"tint":"1 0 0"}"#), "{encoded}");
}

#[test]
fn surface_policy_validates_effects_and_supported_backends() {
    let mut request = ApplyRequest {
        assignments: vec![Assignment::new(vec!["DP-1".into()], Source::video("/wall/clip.mp4"))],
        replace_all: true,
        policy: Some(RendererPolicy {
            surface: Some(Box::new(SurfacePolicy {
                namespace: "skwd-paper-backdrop".into(),
                blur: 12,
                dim: 30,
            })),
            ..Default::default()
        }),
    };
    assert!(request.validate().is_ok());
    let encoded = serde_json::to_string(&request).unwrap();
    assert_eq!(serde_json::from_str::<ApplyRequest>(&encoded).unwrap(), request);
    request.policy.as_mut().unwrap().surface.as_mut().unwrap().dim = 101;
    assert_eq!(request.validate(), Err(ValidationError::InvalidSurfacePolicy));
    request.policy.as_mut().unwrap().surface.as_mut().unwrap().dim = 30;
    request.assignments[0].source = Source::wallpaper_engine("/wall/scene");
    assert!(request.validate().is_ok());
    request.policy.as_mut().unwrap().surface.as_mut().unwrap().namespace = "bad\nnamespace".into();
    assert_eq!(request.validate(), Err(ValidationError::InvalidSurfacePolicy));
}

#[test]
fn load_timeout_policy_is_optional_and_bounded() {
    let legacy: RendererPolicy = serde_json::from_str("{}").unwrap();
    assert_eq!(legacy.load_timeout_ms, None);
    assert!(!serde_json::to_string(&legacy).unwrap().contains("load_timeout_ms"));
    for ms in [3_000, 10_000, 60_000] {
        let policy = RendererPolicy { load_timeout_ms: Some(ms), ..Default::default() };
        assert!(policy.validate().is_ok());
        assert_eq!(
            serde_json::from_str::<RendererPolicy>(&serde_json::to_string(&policy).unwrap())
                .unwrap(),
            policy
        );
    }
    for ms in [0, 2_999, 60_001, u64::MAX] {
        let policy = RendererPolicy { load_timeout_ms: Some(ms), ..Default::default() };
        assert!(matches!(policy.validate(), Err(ValidationError::LoadTimeoutOutOfRange(_))));
    }
}

#[test]
fn transition_fps_decodes_additively_and_validates() {
    let legacy: TransitionPolicy = serde_json::from_str(r#"{"duration_ms":700}"#).unwrap();
    assert_eq!(legacy.fps, None);
    for fps in [1, 30, 120, 1000] {
        let policy: TransitionPolicy =
            serde_json::from_value(serde_json::json!({"fps":fps})).unwrap();
        assert!(policy.validate().is_ok());
        assert_eq!(serde_json::to_value(&policy).unwrap()["fps"], fps);
    }
    for fps in [0, 1001] {
        assert!(
            TransitionPolicy { drawing: None, fps: Some(fps), ..Default::default() }
                .validate()
                .is_err()
        );
    }
    assert!(RendererPolicy { transition_fps: Some(0), ..Default::default() }.validate().is_ok());
    assert!(
        RendererPolicy { transition_fps: Some(1001), ..Default::default() }.validate().is_err()
    );
}

#[test]
fn background_round_trip_and_legacy_default() {
    let legacy = r#"{"outputs":["DP-1"],"source":{"kind":"static","path":"/a.png"}}"#;
    let mut assignment: Assignment = serde_json::from_str(legacy).unwrap();
    assert_eq!(assignment.background, crate::Background::default());
    assignment.background = crate::Background { color: [1, 2, 3], blur: true };
    let value = serde_json::to_value(&assignment).unwrap();
    assert_eq!(value["background"], serde_json::json!({"color":[1,2,3],"blur":true}));
    assert_eq!(serde_json::from_value::<Assignment>(value).unwrap(), assignment);
    assert_eq!(
        AssignmentStatus::from_assignment(&assignment, 1, None, true).background,
        assignment.background
    );
}

#[test]
fn surface_control_is_validated_and_round_trips() {
    let request = Request::new(
        77,
        RequestParams::SurfaceSet(SurfacePolicy {
            namespace: "skwd-paper-stationary".into(),
            blur: 20,
            dim: 30,
        }),
    );
    assert!(request.validate().is_ok());
    let line = encode_ndjson(&request).unwrap();
    assert!(line.contains("paper.surface.set"));
    assert_eq!(decode_ndjson::<Request>(&line).unwrap(), request);
    for surface in [
        SurfacePolicy { namespace: String::new(), blur: 0, dim: 0 },
        SurfacePolicy { namespace: "stationary".into(), blur: 101, dim: 0 },
        SurfacePolicy { namespace: "stationary".into(), blur: 0, dim: 101 },
    ] {
        assert_eq!(
            Request::new(1, RequestParams::SurfaceSet(surface)).validate(),
            Err(ValidationError::InvalidSurfacePolicy)
        );
    }
    assert!(CapabilitiesResult::current().controls.surface_effects);
}

#[test]
fn mirror_request_contract() {
    let request = Request::new(
        8,
        RequestParams::Mirror(crate::MirrorTarget {
            output: "DP-1".into(),
            socket: "/run/lock.sock".into(),
        }),
    );
    let wire = encode_ndjson(&request).unwrap();
    assert_eq!(
        wire,
        "{\"id\":8,\"method\":\"paper.mirror\",\"params\":{\"output\":\"DP-1\",\"socket\":\"/run/lock.sock\"}}\n"
    );
    assert_eq!(decode_ndjson::<Request>(&wire).unwrap(), request);
    assert!(request.validate().is_ok());
    for (output, socket) in [("", "/run/lock.sock"), ("*", "/run/lock.sock"), ("DP-1", "relative")]
    {
        assert!(
            crate::MirrorTarget { output: output.into(), socket: socket.into() }
                .validate()
                .is_err()
        );
    }
    assert!(
        crate::MirrorTarget { output: "DP-1".into(), socket: String::new() }.validate().is_ok()
    );
}

#[test]
fn spectrum_source_request() {
    let request: AudioSetRequest = serde_json::from_str(r#"{"system_audio":false}"#).unwrap();
    assert_eq!(request.validate(), Ok(()));
    assert_eq!(request.system_audio, Some(false));
    assert_eq!(serde_json::to_value(&request).unwrap(), serde_json::json!({"system_audio":false}));
    let mut assignment = Assignment::new(vec!["DP-1".into()], Source::wallpaper_engine("/scene"));
    assignment.system_audio = Some(false);
    let status = AssignmentStatus::from_assignment(&assignment, 1, None, false);
    assert_eq!(status.system_audio, Some(false));
}

#[test]
fn drawing_media_reference_contract() {
    let mut transition: TransitionPolicy = serde_json::from_str(r#"{"effect":"drawing","drawing":{"artifact":"/cache/drawing.json","finish":"lens"},"drawing_source":"/cache/reference.png"}"#).unwrap();
    for source in [Source::video("/wall/video.mp4"), Source::wallpaper_engine("/wall/scene")] {
        let mut item = assignment(&["DP-1"], source);
        item.transition = Some(transition.clone());
        assert!(
            ApplyRequest { assignments: vec![item], replace_all: false, policy: None }
                .validate()
                .is_ok()
        );
    }
    assert_eq!(
        serde_json::to_value(&transition).unwrap()["drawing_source"],
        "/cache/reference.png"
    );
    transition.drawing_source = Some("relative.png".into());
    assert!(transition.validate().is_err());
    transition.drawing_source = Some("/cache/reference.png".into());
    transition.drawing = None;
    assert!(transition.validate().is_err());
}

#[test]
fn playback_control_shapes() {
    let pause: PauseRequest = serde_json::from_str(r#"{"paused":true}"#).unwrap();
    assert!(pause.outputs.is_empty());
    assert_eq!(serde_json::to_string(&pause).unwrap(), r#"{"paused":true}"#);
    for value in [
        serde_json::json!({"paused":false,"outputs":["DP-1"]}),
        serde_json::json!({"paused":true,"outputs":["*"]}),
    ] {
        let pause: PauseRequest = serde_json::from_value(value.clone()).unwrap();
        pause.validate().unwrap();
        assert_eq!(serde_json::to_value(pause).unwrap(), value);
    }
    let duck = AudioSetRequest { duck: Some(true), ..Default::default() };
    duck.validate().unwrap();
    assert_eq!(serde_json::to_value(duck).unwrap(), serde_json::json!({"duck":true}));
    assert!(
        PauseRequest {
            overrides: std::collections::BTreeMap::new(),
            paused: true,
            outputs: vec!["*".into(), "DP-1".into()]
        }
        .validate()
        .is_err()
    );
}

#[test]
fn playback_methods_are_distinct() {
    let pause = Request::new(
        1,
        RequestParams::PlaybackPause(PauseRequest {
            paused: true,
            outputs: vec!["*".into()],
            overrides: [("DP-1".into(), false)].into(),
        }),
    );
    let encoded = serde_json::to_value(&pause).unwrap();
    assert_eq!(encoded["method"], "paper.playback.pause");
    assert_eq!(encoded["params"]["overrides"], serde_json::json!({"DP-1":false}));
    assert_eq!(serde_json::from_value::<Request>(encoded).unwrap(), pause);
    let audio = Request::new(
        2,
        RequestParams::PlaybackAudio(AudioSetRequest { duck: Some(true), ..Default::default() }),
    );
    let encoded = serde_json::to_value(&audio).unwrap();
    assert_eq!(encoded["method"], "paper.playback.audio");
    assert_eq!(serde_json::from_value::<Request>(encoded).unwrap(), audio);
    assert_eq!(
        serde_json::to_value(crate::PlaybackState { paused: true, ducked: true }).unwrap(),
        serde_json::json!({"paused":true,"ducked":true})
    );
}

#[test]
fn surface_overrides_wire() {
    let mut overrides = SurfaceOverridesRequest {
        surfaces: [(
            SourceKind::Video,
            SurfacePolicy { namespace: "stationary".into(), blur: 22, dim: 44 },
        )]
        .into_iter()
        .collect(),
    };
    let request = Request::new(4, RequestParams::SurfaceOverrides(overrides.clone()));
    request.validate().unwrap();
    let wire = encode_ndjson(&request).unwrap();
    assert!(wire.contains("paper.surface.overrides"));
    assert_eq!(decode_ndjson::<Request>(&wire).unwrap(), request);
    overrides
        .surfaces
        .insert(SourceKind::Static, SurfacePolicy { namespace: "other".into(), blur: 0, dim: 0 });
    assert!(overrides.validate().is_err());
    overrides.surfaces.remove(&SourceKind::Static);
    overrides.surfaces.get_mut(&SourceKind::Video).unwrap().dim = 101;
    assert!(overrides.validate().is_err());
    assert!(SurfaceOverridesRequest::default().validate().is_ok());
}

#[test]
fn widgets_request_round_trips() {
    let scene: crate::WidgetScene = serde_json::from_str(
        r#"{"widgets":[{"face":{"kind":"clock","style":"aperture"},"pin":{"anchor":[0.5,0.5],"offset":[0.0,0.0]},"output":"DP-1"}],"palette":{"text":[236,235,231],"accent":[90,110,200],"shade":[18,19,22]},"labels":{"days":["MON","TUE","WED","THU","FRI","SAT","SUN"],"day_names":["MONDAY","TUESDAY","WEDNESDAY","THURSDAY","FRIDAY","SATURDAY","SUNDAY"],"months":["JAN","FEB","MAR","APR","MAY","JUN","JUL","AUG","SEP","OCT","NOV","DEC"],"month_names":["JANUARY","FEBRUARY","MARCH","APRIL","MAY","JUNE","JULY","AUGUST","SEPTEMBER","OCTOBER","NOVEMBER","DECEMBER"]}}"#,
    )
    .unwrap();
    let request = Request::new(5, RequestParams::Widgets(Box::new(scene)));
    assert!(request.validate().is_ok());
    let line = encode_ndjson(&request).unwrap();
    assert!(line.contains("paper.widgets"));
    assert_eq!(decode_ndjson::<Request>(&line).unwrap(), request);
}
