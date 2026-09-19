use super::StartupReadiness;

#[test]
fn waits_for_every_initial_output_before_requesting_sync() {
    let mut readiness = StartupReadiness::default();
    readiness.finish_enumeration([(3, false), (7, false), (11, false)].into_iter());
    readiness.committed(3);
    assert!(!readiness.begin_sync().unwrap());
    readiness.committed(11);
    assert!(!readiness.begin_sync().unwrap());
    readiness.committed(7);
    assert!(readiness.begin_sync().unwrap());
    assert!(readiness.complete_sync());
}

#[test]
fn enumeration_must_finish_before_early_commits_can_signal_ready() {
    let mut readiness = StartupReadiness::default();
    readiness.committed(3);
    assert!(!readiness.begin_sync().unwrap());
    assert!(!readiness.complete_sync());
    readiness.finish_enumeration([(3, true), (7, false)].into_iter());
    assert!(!readiness.begin_sync().unwrap());
    readiness.committed(7);
    assert!(readiness.begin_sync().unwrap());
    assert!(readiness.complete_sync());
}

#[test]
fn fully_committed_enumeration_snapshot_still_waits_for_sync_acknowledgement() {
    let mut readiness = StartupReadiness::default();
    readiness.finish_enumeration([(3, true), (7, true)].into_iter());
    assert!(!readiness.complete_sync());
    assert!(readiness.begin_sync().unwrap());
    assert!(!readiness.begin_sync().unwrap());
    assert!(readiness.complete_sync());
}

#[test]
fn batched_commits_can_request_only_one_sync() {
    let mut readiness = StartupReadiness::default();
    readiness.finish_enumeration([(3, false), (7, false)].into_iter());
    for output in [3, 7, 3, 7] {
        readiness.committed(output);
    }
    assert!(readiness.begin_sync().unwrap());
    for _ in 0..3 {
        assert!(!readiness.begin_sync().unwrap());
    }
    assert!(readiness.complete_sync());
    assert!(!readiness.complete_sync());
}

#[test]
fn removing_unconfigured_output_unblocks_remaining_committed_output() {
    let mut readiness = StartupReadiness::default();
    readiness.finish_enumeration([(3, true), (7, false)].into_iter());
    assert!(!readiness.begin_sync().unwrap());
    readiness.removed(7);
    assert!(readiness.begin_sync().unwrap());
    assert!(readiness.complete_sync());
}

#[test]
fn removing_committed_output_does_not_skip_remaining_unconfigured_output() {
    let mut readiness = StartupReadiness::default();
    readiness.finish_enumeration([(3, true), (7, false)].into_iter());
    readiness.removed(3);
    assert!(!readiness.begin_sync().unwrap());
    readiness.committed(7);
    assert!(readiness.begin_sync().unwrap());
    assert!(readiness.complete_sync());
}

#[test]
fn initially_empty_selection_fails_instead_of_reporting_ready_or_waiting_forever() {
    let mut readiness = StartupReadiness::default();
    readiness.finish_enumeration(std::iter::empty());
    assert!(readiness.begin_sync().is_err());
    assert!(!readiness.complete_sync());
}

#[test]
fn losing_all_initial_outputs_before_sync_fails() {
    let mut readiness = StartupReadiness::default();
    readiness.finish_enumeration([(3, false), (7, true)].into_iter());
    readiness.removed(3);
    readiness.removed(7);
    assert!(readiness.begin_sync().is_err());
    assert!(!readiness.complete_sync());
}

#[test]
fn losing_last_output_while_sync_is_pending_never_reports_ready() {
    let mut readiness = StartupReadiness::default();
    readiness.finish_enumeration([(3, true)].into_iter());
    assert!(readiness.begin_sync().unwrap());
    readiness.removed(3);
    assert!(!readiness.complete_sync());
    assert!(readiness.begin_sync().is_err());
}

#[test]
fn removing_one_output_while_sync_is_pending_keeps_remaining_ready_output() {
    let mut readiness = StartupReadiness::default();
    readiness.finish_enumeration([(3, true), (7, true)].into_iter());
    assert!(readiness.begin_sync().unwrap());
    readiness.removed(7);
    assert!(readiness.complete_sync());
    assert!(!readiness.begin_sync().unwrap());
}

#[test]
fn later_output_events_do_not_change_the_initial_cohort() {
    let mut readiness = StartupReadiness::default();
    readiness.finish_enumeration([(3, false)].into_iter());
    readiness.committed(7);
    readiness.removed(11);
    assert!(!readiness.begin_sync().unwrap());
    readiness.committed(3);
    assert!(readiness.begin_sync().unwrap());
    readiness.committed(11);
    readiness.removed(7);
    assert!(readiness.complete_sync());
}

#[test]
fn post_ready_removal_and_hotplug_do_not_restart_readiness_or_fail() {
    let mut readiness = StartupReadiness::default();
    readiness.finish_enumeration([(3, true)].into_iter());
    assert!(readiness.begin_sync().unwrap());
    assert!(readiness.complete_sync());
    readiness.removed(3);
    assert!(!readiness.begin_sync().unwrap());
    readiness.committed(7);
    readiness.removed(7);
    assert!(!readiness.begin_sync().unwrap());
    assert!(!readiness.complete_sync());
}
