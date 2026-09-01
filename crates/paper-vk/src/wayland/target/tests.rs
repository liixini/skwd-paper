use super::{
    all_surfaces_presented_after, commit_due, desktop_is_kwin, input_passthrough_for,
    return_free_buffer, session_flag_enabled,
};

#[test]
fn presentation_barrier_requires_every_surface_to_advance() {
    assert!(!all_surfaces_presented_after([]));
    assert!(!all_surfaces_presented_after([(1, 1), (2, 1)]));
    assert!(!all_surfaces_presented_after([(2, 1), (1, 1)]));
    assert!(all_surfaces_presented_after([(2, 1), (3, 2)]));
}

#[test]
fn output_commit_cadence_respects_its_own_limit() {
    let mut next = 0;
    assert!(commit_due(&mut next, 60, 0));
    assert!(!commit_due(&mut next, 60, 6_060_606));
    assert!(!commit_due(&mut next, 60, 12_121_212));
    assert!(commit_due(&mut next, 60, 18_181_818));
    assert!(commit_due(&mut next, 0, 18_181_818));
}

#[test]
fn throttled_commit_returns_its_ring_slot_once() {
    let mut free = vec![0, 2];
    return_free_buffer(&mut free, 1);
    return_free_buffer(&mut free, 1);
    assert_eq!(free, vec![0, 2, 1]);
}

#[test]
fn kwin_desktop_detection_is_tokenized_and_case_insensitive() {
    assert!(desktop_is_kwin("KDE"));
    assert!(desktop_is_kwin("GNOME:KWin"));
    assert!(desktop_is_kwin("plasma"));
    assert!(!desktop_is_kwin("niri"));
    assert!(!desktop_is_kwin("ukdesktop"));
}

#[test]
fn kde_session_enables_automatic_passthrough() {
    assert!(input_passthrough_for(None, Some("KDE"), None, None, None));
    assert!(input_passthrough_for(None, None, None, Some("plasma"), None));
    assert!(input_passthrough_for(None, None, None, None, Some("true")));
    assert!(!input_passthrough_for(None, Some("niri"), None, None, None));
}

#[test]
fn explicit_input_mode_overrides_session_detection() {
    assert!(!input_passthrough_for(Some("interactive"), Some("KDE"), None, None, Some("true")));
    assert!(input_passthrough_for(Some("passthrough"), Some("niri"), None, None, None));
}

#[test]
fn disabled_kde_session_flag_does_not_enable_passthrough() {
    for value in ["", "0", "false", "FALSE", "no"] {
        assert!(!session_flag_enabled(value));
    }
}

fn argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).to_string()).collect()
}

#[test]
fn reexec_argv_tracks_the_swapped_video() {
    let args = argv(&["skwd-wall-vk", "DP-1", "/old.mp4", "--fill-mode", "fill"]);
    let out = super::rebuild_argv(&args, &super::ReexecSource::Video("/new.mp4"));
    assert_eq!(out, argv(&["skwd-wall-vk", "DP-1", "/new.mp4", "--fill-mode", "fill"]));
}

#[test]
fn reexec_argv_tracks_the_swapped_scene() {
    let args = argv(&["skwd-wall-vk", "DP-1", "/vid.mp4", "--scene", "/old-scene"]);
    let out = super::rebuild_argv(
        &args,
        &super::ReexecSource::Scene { dir: "/new-scene", properties: None },
    );
    assert_eq!(out, argv(&["skwd-wall-vk", "DP-1", "/vid.mp4", "--scene", "/new-scene"]));
}

#[test]
fn reexec_argv_replaces_stale_scene_properties() {
    let args = argv(&[
        "skwd-wall-vk",
        "DP-1",
        "/vid.mp4",
        "--scene",
        "/old-scene",
        "--scene-properties",
        r#"{"a":1}"#,
    ]);
    let out = super::rebuild_argv(
        &args,
        &super::ReexecSource::Scene { dir: "/new-scene", properties: Some(r#"{"b":2}"#) },
    );
    assert_eq!(
        out,
        argv(&[
            "skwd-wall-vk",
            "DP-1",
            "/vid.mp4",
            "--scene",
            "/new-scene",
            "--scene-properties",
            r#"{"b":2}"#,
        ])
    );

    let cleared = super::rebuild_argv(
        &args,
        &super::ReexecSource::Scene { dir: "/new-scene", properties: None },
    );
    assert_eq!(cleared, argv(&["skwd-wall-vk", "DP-1", "/vid.mp4", "--scene", "/new-scene"]));
}

#[test]
fn reexec_argv_never_replays_a_transition() {
    let args = argv(&[
        "skwd-wall-vk",
        "DP-1",
        "/b.mp4",
        "--transition-from",
        "/a.png",
        "--shader",
        "fade",
    ]);
    let out = super::rebuild_argv(&args, &super::ReexecSource::Video("/b.mp4"));
    assert_eq!(out, argv(&["skwd-wall-vk", "DP-1", "/b.mp4", "--shader", "fade"]));

    let tail = argv(&["skwd-wall-vk", "DP-1", "/b.mp4", "--transition-from"]);
    let out = super::rebuild_argv(&tail, &super::ReexecSource::Video("/b.mp4"));
    assert_eq!(out, argv(&["skwd-wall-vk", "DP-1", "/b.mp4"]));
}

#[test]
fn reexec_argv_leaves_multi_json_untouched() {
    let args = argv(&["skwd-wall-vk", "--multi-json", "/manifest.json"]);
    let out = super::rebuild_argv(&args, &super::ReexecSource::Video("/new.mp4"));
    assert_eq!(out, args);
}

#[test]
fn reexec_argv_skips_noncanonical_shapes() {
    let args = argv(&["skwd-wall-vk", "--layer", "top", "DP-1", "/vid.mp4"]);
    let out = super::rebuild_argv(&args, &super::ReexecSource::Video("/new.mp4"));
    assert_eq!(out, args, "a leading flag means argv[2] is not the media path");
}
