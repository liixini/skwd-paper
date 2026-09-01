use super::*;

fn assignment(outputs: &[&str], source: Source) -> Assignment {
    Assignment::new(outputs.iter().map(|output| (*output).to_string()).collect(), source)
}

#[test]
fn apply_golden_line() {
    let request = Request::new(
        7,
        RequestParams::Apply(ApplyRequest {
            assignments: vec![
                assignment(&["DP-1"], Source::static_file("/wall/a.png")),
                assignment(&["DP-2"], Source::video("/wall/b.mp4", Some(VideoEngine::Default))),
                assignment(&["HDMI-A-1"], Source::wallpaper_engine("/scene/forest")),
            ],
            replace_all: true,
            policy: None,
        }),
    );
    let golden = concat!(
        r#"{"id":7,"method":"paper.apply","params":{"assignments":["#,
        r#"{"outputs":["DP-1"],"source":{"kind":"static","path":"/wall/a.png"}},"#,
        r#"{"outputs":["DP-2"],"source":{"kind":"video","path":"/wall/b.mp4","engine":"default"}},"#,
        r#"{"outputs":["HDMI-A-1"],"source":{"kind":"we","path":"/scene/forest"}}"#,
        r#"],"replace_all":true}}"#,
        "\n"
    );
    assert_eq!(encode_ndjson(&request).unwrap(), golden);
    assert_eq!(decode_ndjson::<Request>(golden).unwrap(), request);
    assert_eq!(request.validate(), Ok(()));
}

#[test]
fn apply_defaults_and_engine() {
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
    assert_eq!(apply.assignments[0].source.effective_video_engine(), Some(VideoEngine::Default));
    assert_eq!(encode_ndjson(&request).unwrap(), golden);

    for kind in [SourceKind::Static, SourceKind::WallpaperEngine] {
        let source = Source {
            kind,
            path: "/wall/source".into(),
            engine: Some(VideoEngine::Default),
            frame_rate: None,
            properties: None,
        };
        assert_eq!(source.validate(), Err(ValidationError::EngineNotAllowed(kind)));
    }
    let invalid_engine = concat!(
        r#"{"id":9,"method":"paper.apply","params":{"assignments":["#,
        r#"{"outputs":["DP-1"],"source":{"kind":"video","path":"/wall/a.mp4","engine":"fast"}}"#,
        r#"]}}"#,
        "\n"
    );
    assert!(matches!(decode_ndjson::<Request>(invalid_engine), Err(NdjsonError::Json(_))));
}

#[test]
fn tinier_frame_rate_contract() {
    let source = Source::tinier_video("/wall/loop.ivf", "30000/1001");
    let request = Request::new(
        16,
        RequestParams::Apply(ApplyRequest {
            assignments: vec![assignment(&["DP-1"], source)],
            replace_all: false,
            policy: None,
        }),
    );
    let golden = concat!(
        r#"{"id":16,"method":"paper.apply","params":{"assignments":["#,
        r#"{"outputs":["DP-1"],"source":{"kind":"video","path":"/wall/loop.ivf","engine":"tinier","frame_rate":"30000/1001"}}]}}"#,
        "\n"
    );
    assert_eq!(encode_ndjson(&request).unwrap(), golden);
    assert_eq!(decode_ndjson::<Request>(golden).unwrap(), request);
    assert_eq!(request.validate(), Ok(()));

    for frame_rate in ["", "0", "240/0", "241", "30/1/1", "29.97"] {
        let invalid = Source::tinier_video("/wall/loop.ivf", frame_rate);
        assert!(invalid.validate().is_err(), "{frame_rate}");
    }
    assert_eq!(
        Source::video("/wall/loop.ivf", Some(VideoEngine::Tinier)).validate(),
        Err(ValidationError::TinierFrameRateRequired)
    );
    let wildcard = ApplyRequest {
        assignments: vec![assignment(&["*"], Source::tinier_video("/wall/loop.ivf", "30"))],
        replace_all: false,
        policy: None,
    };
    assert_eq!(wildcard.validate(), Err(ValidationError::TinierWildcardNotSupported));
    let mut audible = assignment(&["DP-1"], Source::tinier_video("/wall/loop.ivf", "30"));
    audible.mute = false;
    assert_eq!(
        ApplyRequest { assignments: vec![audible], replace_all: false, policy: None }.validate(),
        Err(ValidationError::TinierAudioNotSupported)
    );
}

#[test]
fn assignment_options_golden() {
    let mut configured = assignment(&["DP-1"], Source::video("/wall/a.mp4", None));
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
    let mut configured = assignment(&["DP-1"], Source::video("/wall/b.mp4", None));
    configured.transition = Some(TransitionPolicy {
        from: Some("/wall/a.png".into()),
        effect: Some("sand-bloom".into()),
        duration_ms: Some(700),
    });
    let policy = RendererPolicy {
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
        r#""max_dimension":2048,"max_effect_chains":4,"max_effect_passes":8,"strict":true},"#,
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
            assignment(&["DP-2"], Source::video("/wall/b.mp4", None)),
        ],
        replace_all: false,
        policy: None,
    };
    assert_eq!(duplicate.validate(), Err(ValidationError::DuplicateOutput("DP-2".to_string())));

    let mixed_wildcard = ApplyRequest {
        assignments: vec![
            assignment(&["*"], Source::static_file("/wall/a.png")),
            assignment(&["DP-1"], Source::video("/wall/b.mp4", None)),
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
        assignments: vec![assignment(&["DP-1"], Source::video("--scene", None))],
        replace_all: false,
        policy: None,
    };
    assert_eq!(unsafe_path.validate(), Err(ValidationError::SourcePathStartsWithDash));

    let mut loud = assignment(&["DP-1"], Source::video("/wall/a.mp4", None));
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
        Err(ValidationError::TransitionNotAllowed(SourceKind::Static))
    );

    for transition in [
        TransitionPolicy { from: Some(String::new()), effect: None, duration_ms: None },
        TransitionPolicy { from: Some("--old".into()), effect: None, duration_ms: None },
        TransitionPolicy { from: None, effect: Some("../fade".into()), duration_ms: None },
        TransitionPolicy { from: None, effect: None, duration_ms: Some(49) },
    ] {
        assert!(transition.validate().is_err());
    }

    let mut video = assignment(&["DP-1"], Source::video("/wall/a.mp4", None));
    video.transition = Some(TransitionPolicy::default());
    assert_eq!(
        ApplyRequest {
            assignments: vec![video],
            replace_all: false,
            policy: Some(RendererPolicy {
                transitions_enabled: Some(false),
                ..RendererPolicy::default()
            }),
        }
        .validate(),
        Err(ValidationError::TransitionsDisabled)
    );

    assert_eq!(AudioSetRequest::default().validate(), Err(ValidationError::MissingAudioChange));
    assert_eq!(
        AudioSetRequest { outputs: vec![], mute: None, volume: Some(101) }.validate(),
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
    let pause = Request::new(13, RequestParams::Pause(PauseRequest { paused: true }));
    assert_eq!(
        encode_ndjson(&pause).unwrap(),
        "{\"id\":13,\"method\":\"paper.pause\",\"params\":{\"paused\":true}}\n"
    );
    let audio = Request::new(
        14,
        RequestParams::AudioSet(AudioSetRequest {
            outputs: vec!["DP-1".into()],
            mute: Some(false),
            volume: Some(55),
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
    let assignment = assignment(&["DP-1"], Source::video("/wall/a.mp4", None));
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
            r#""video_engines":["default","tinier"],"#,
            r#""fill_modes":["fill","fit","stretch","center","tile","span"],"#,
            r#""layers":["background","bottom","top"],"#,
            r#""controls":{"pause":true,"audio":true},"#,
            r#""transitions":{"startup_source_kinds":["video","we"],"#,
            r#""static_overlay":false,"default_effect":"fade","default_duration_ms":600,"#,
            r#""min_duration_ms":50,"max_duration_ms":10000},"#,
            r#""renderer_policy":{"idle":true,"sand":true,"scene":true,"output_fps":true},"#,
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
        r#""source_kinds":["static","video","we"],"video_engines":["regular","tiny"],"#,
        r#""fill_modes":["fill"],"layers":["background"]}}"#,
        "\n"
    );
    let decoded = decode_ndjson::<CapabilitiesResponse>(phase_one_capabilities).unwrap();
    let ResponseBody::Success { result } = decoded.body else { panic!("expected success") };
    assert_eq!(result.video_engines, vec![VideoEngine::Default, VideoEngine::Default]);
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
        video_engines: vec![VideoEngine::Default],
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
            r#""video_engines":["default"],"path":"/usr/bin/skwd-wall-vk","#,
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
        let source = Source {
            kind,
            path: "/wall/source".into(),
            engine: None,
            frame_rate: None,
            properties: Some(properties.clone()),
        };
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
