use super::{control_stdin_enabled, parse_idle_secs, renderer_path, transition_hold_enabled};
#[cfg(feature = "shared-device")]
use super::{preview_drawing, stream_targets};

#[test]
fn standalone_mode_is_opt_in_and_disables_only_control_stdin() {
    assert!(control_stdin_enabled(&["skwd-wall-vk".into(), "DP-3".into(), "loop.mp4".into()]));
    assert!(!control_stdin_enabled(&[
        "skwd-wall-vk".into(),
        "DP-3".into(),
        "loop.mp4".into(),
        "--standalone".into(),
    ]));
}

#[test]
fn staged_transition_keeps_control_stdin() {
    let args =
        ["skwd-wall-vk".into(), "DP-3".into(), "next.png".into(), "--transition-hold".into()];
    assert!(transition_hold_enabled(&args));
    assert!(control_stdin_enabled(&args));
}

#[test]
fn idle_seconds_are_shared_by_single_and_multi_paths() {
    assert_eq!(parse_idle_secs(Some("45")), 45);
    assert_eq!(parse_idle_secs(Some("0")), 0);
    assert_eq!(parse_idle_secs(Some("-1")), 0);
    assert_eq!(parse_idle_secs(Some("not-a-number")), 0);
    assert_eq!(parse_idle_secs(None), 0);
}

#[test]
fn kwin_defaults_to_shared_vulkan_presentation() {
    assert_eq!(renderer_path(None, true, false), "shared");
    assert_eq!(renderer_path(None, false, false), "dmabuf-present");
}

#[test]
fn explicit_renderer_path_overrides_kwin_default() {
    assert_eq!(renderer_path(Some("dmabuf-present"), true, false), "dmabuf-present");
    assert_eq!(renderer_path(Some("shared"), false, false), "shared");
}

#[test]
fn transitions_always_use_the_transition_capable_path() {
    assert_eq!(renderer_path(None, true, true), "dmabuf-present");
    assert_eq!(renderer_path(Some("shared"), true, true), "dmabuf-present");
    assert_eq!(renderer_path(Some("nv12"), false, true), "dmabuf-present");
}

#[test]
fn retired_cpu_path_uses_the_vulkan_default() {
    assert_eq!(renderer_path(Some("cpu"), true, false), "shared");
    assert_eq!(renderer_path(Some("cpu"), false, false), "dmabuf-present");
}

#[cfg(feature = "shared-device")]
#[test]
fn stream_targets_pair_repeated_flags_by_position() {
    let args: Vec<String> = [
        "--stream-fd",
        "3",
        "--stream-size",
        "1920x1080",
        "--stream-fps",
        "144",
        "--stream-output",
        "DP-3",
        "--stream-fd",
        "4",
        "--stream-size",
        "1309x2327",
        "--stream-fps",
        "60",
        "--stream-output",
        "DP-2",
        "--stream-paused",
        "DP-2",
    ]
    .iter()
    .map(|arg| arg.to_string())
    .collect();
    let targets = stream_targets(&args, false);
    assert_eq!(targets.len(), 2);
    assert_eq!(
        (targets[0].socket, targets[0].width, targets[0].height, targets[0].fps),
        (3, 1920, 1080, 144)
    );
    assert_eq!(targets[0].output, "DP-3");
    assert!(!targets[0].paused);
    assert_eq!(
        (targets[1].socket, targets[1].width, targets[1].height, targets[1].fps),
        (4, 1309, 2327, 60)
    );
    assert!(targets[1].paused);
    let legacy: Vec<String> =
        ["--stream-fd", "3", "--stream-size", "1280x720", "--stream-fps", "30"]
            .iter()
            .map(|arg| arg.to_string())
            .collect();
    let single = stream_targets(&legacy, true);
    assert_eq!(single.len(), 1);
    assert!(single[0].paused);
    assert!(single[0].output.is_empty());
    assert!(stream_targets(&legacy[2..], false).is_empty());
}

#[cfg(feature = "shared-device")]
#[test]
fn preview_drawing_needs_shader() {
    let drawing = r#"{"artifact":"/cache/drawing/manifest.json","ink":"rainbow"}"#;
    assert!(preview_drawing("crossfade", Some("not json".into())).unwrap().is_none());
    assert!(preview_drawing("crossfade", Some(drawing.into())).unwrap().is_none());
    assert!(preview_drawing("drawing", Some("not json".into())).is_err());
    assert!(preview_drawing("drawing", None).unwrap().is_none());
    let parsed = preview_drawing("drawing", Some(drawing.into())).unwrap().unwrap();
    assert_eq!(parsed.ink, paper_control::DrawingInk::Rainbow);
}

#[cfg(feature = "shared-device")]
#[test]
fn separates_stream_geometry() {
    let args = [
        "--stream-fd",
        "3",
        "--stream-output",
        "stream-7",
        "--stream-connector",
        "DP-1",
        "--stream-size",
        "1920x1080",
        "--stream-logical",
        "2560x1440",
        "--stream-fd",
        "4",
        "--stream-output",
        "stream-8",
        "--stream-connector",
        "DP-2",
        "--stream-size",
        "1280x720",
        "--stream-logical",
        "3840x2160",
    ]
    .map(String::from);
    let targets = stream_targets(&args, false);
    assert_eq!(targets[0].output, "stream-7");
    assert_eq!(targets[0].connector, "DP-1");
    assert_eq!(targets[0].logical, [2560, 1440]);
    assert_eq!((targets[0].width, targets[0].height), (1920, 1080));
    assert_eq!(targets[1].output, "stream-8");
    assert_eq!(targets[1].connector, "DP-2");
    assert_eq!(targets[1].logical, [3840, 2160]);
}
