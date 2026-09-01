use super::*;

fn entry(output: &str, video: &str, mute: bool, transition_from: Option<&str>) -> MultiVideoEntry {
    MultiVideoEntry {
        output: output.to_string(),
        video: video.to_string(),
        mute,
        volume: 42,
        transition_from: transition_from.map(String::from),
    }
}

#[test]
fn manifest_accepts_paths_with_shell_delimiters() {
    let entries = vec![entry("DP-1", "/v/a=b;c.mp4", false, None)];
    let wire = serde_json::to_string(&entries).unwrap();
    assert_eq!(parse_manifest(&wire), entries);
}

#[test]
fn same_source_decodes_once_unless_transition_sources_differ() {
    let entries = vec![
        entry("DP-2", "/v/new.mp4", true, Some("/v/old-b.mp4")),
        entry("DP-1", "/v/new.mp4", false, Some("/v/old-a.mp4")),
        entry("DP-3", "/v/other.mp4", true, None),
    ];
    let groups = group_entries(&entries);
    assert_eq!(groups.len(), 3);
    assert!(groups[0].with_audio);
    assert!(!groups[1].with_audio);
    assert_eq!((groups[0].mute, groups[0].volume), (false, 42));
}

#[test]
fn targeted_commands_only_reach_matching_outputs() {
    let outputs = vec!["DP-1".to_string(), "DP-2".to_string()];
    assert!(routed_to(&outputs, &PaperCommand::audio(None, Some(10))));
    assert!(routed_to(&outputs, &PaperCommand::audio_for(&["DP-2".to_string()], None, Some(10))));
    assert!(!routed_to(&outputs, &PaperCommand::audio_for(&["DP-3".to_string()], None, Some(10))));
}
