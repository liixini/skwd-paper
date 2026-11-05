use super::{MAX_DURATION_MS, STUDIO_TRANSITIONS};
use crate::TransitionPolicy;

#[test]
fn authored_durations() {
    for spec in STUDIO_TRANSITIONS {
        let policy =
            TransitionPolicy { effect: Some(spec.id.into()), ..TransitionPolicy::default() };
        assert_eq!(policy.duration_ms(), spec.duration_ms);
        assert!(spec.duration_ms <= MAX_DURATION_MS);
        assert_eq!(policy.validate(), Ok(()));
        assert_eq!(TransitionPolicy { duration_ms: Some(900), ..policy }.duration_ms(), 900);
    }
    assert_eq!(TransitionPolicy::default().duration_ms(), 600);
}

#[test]
fn studio_wire_ids() {
    let ids = STUDIO_TRANSITIONS.map(|spec| spec.id);
    assert_eq!(ids, ["photo-cutout", "slat-turn", "crop-relay", "sunlit-arcade", "paper-heart"]);
}
