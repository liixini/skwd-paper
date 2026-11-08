use super::*;
use paper_control::Source;

#[test]
fn effective_settings() {
    let mut left = Assignment::new(vec!["DP-1".into()], Source::video("/video.mp4"));
    let mut right = left.clone();
    right.outputs = vec!["DP-2".into()];
    left.mute = false;
    left.volume = 45;
    right.mute = true;
    right.volume = 100;
    right.background.blur = true;
    right.system_audio = Some(true);
    assert!(!compatible(&left, &right));
    right.volume = left.volume;
    assert!(compatible(&left, &right));
    right.system_audio = Some(false);
    assert!(!compatible(&left, &right));
    right.system_audio = None;
    right.mute = false;
    right.volume = 100;
    assert!(!compatible(&left, &right));
    right.mute = true;
    right.volume = left.volume;
    left.fill_mode = FillMode::Fit;
    right.fill_mode = FillMode::Fit;
    assert!(!compatible(&left, &right));
}

#[test]
fn opt_in_and_controls() {
    let left = Assignment::new(vec!["DP-1".into()], Source::video("/video.mp4"));
    let mut right = left.clone();
    right.outputs = vec!["DP-2".into()];
    let assignments = vec![(left, "DP-1".into()), (right, "DP-2".into())];
    let policy = RendererPolicy { shared_outputs: Some(true), ..Default::default() };
    let mut pause = Control::default();
    let mut duck = Control::default();
    assert_eq!(groups(assignments.clone(), None, &pause, &duck).len(), 2);
    assert_eq!(groups(assignments.clone(), Some(&policy), &pause, &duck).len(), 1);
    pause.set(&["DP-1".into()], true);
    assert_eq!(groups(assignments.clone(), Some(&policy), &pause, &duck).len(), 2);
    pause.set(&[], false);
    duck.set(&["DP-2".into()], true);
    assert_eq!(groups(assignments, Some(&policy), &pause, &duck).len(), 2);
}

#[test]
fn staged_member_prepares_group() {
    let mut left = Assignment::new(vec!["DP-1".into()], Source::static_file("/wall/new.png"));
    left.transition = Some(paper_control::TransitionPolicy {
        from: Some("/wall/new.png".into()),
        ..Default::default()
    });
    let mut right = left.clone();
    right.outputs = vec!["DP-2".into()];
    right.transition.as_mut().unwrap().from = Some("/wall/old.png".into());
    let policy = RendererPolicy { shared_outputs: Some(true), ..Default::default() };
    let groups = groups(
        vec![(left, "DP-1".into()), (right, "DP-2".into())],
        Some(&policy),
        &Control::default(),
        &Control::default(),
    );
    assert_eq!(groups.len(), 1);
    assert!(static_transition(&groups[0].0));
}
