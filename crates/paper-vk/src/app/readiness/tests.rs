use super::PresentationReadiness;
use std::cell::RefCell;

#[test]
fn startup_and_swap_acks() {
    let mut readiness = PresentationReadiness::startup();

    assert!(readiness.take_if_committed(true));
    assert!(!readiness.take_if_committed(true));

    readiness.arm_swap();
    assert!(readiness.take_if_committed(true));
    assert!(!readiness.take_if_committed(true));

    readiness.arm_swap();
    assert!(readiness.take_if_committed(true));
    assert!(!readiness.take_if_committed(true));
}

#[test]
fn repeat_arm_coalesces() {
    let mut readiness = PresentationReadiness::startup();
    readiness.arm_swap();
    readiness.arm_swap();

    assert!(readiness.take_if_committed(true));
    assert!(!readiness.take_if_committed(true));
}

#[test]
fn cadence_skip_keeps_armed() {
    let mut readiness = PresentationReadiness::startup();

    assert!(!readiness.take_if_committed(false));
    assert!(!readiness.take_if_committed(false));
    assert!(readiness.take_if_committed(true));
    assert!(!readiness.take_if_committed(true));
}

#[test]
fn presentation_precedes_notification() {
    let mut readiness = PresentationReadiness::startup();
    let events = RefCell::new(Vec::new());

    let completed = readiness
        .complete_after_presentation(
            true,
            || {
                events.borrow_mut().push("presented");
                Ok::<(), ()>(())
            },
            || events.borrow_mut().push("ready"),
        )
        .unwrap();

    assert!(completed);
    assert_eq!(*events.borrow(), ["presented", "ready"]);
}

#[test]
fn failed_presentation_does_not_notify() {
    let mut readiness = PresentationReadiness::startup();
    let notified = RefCell::new(false);

    let completed = readiness.complete_after_presentation(
        true,
        || Err::<(), _>("discarded"),
        || *notified.borrow_mut() = true,
    );

    assert_eq!(completed, Err("discarded"));
    assert!(!*notified.borrow());
}
