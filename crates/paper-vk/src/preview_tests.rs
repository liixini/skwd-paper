use super::*;

fn target(output: &str, fps: u32) -> StreamTarget {
    StreamTarget { socket: -1, width: 16, height: 16, fps, output: output.into(), paused: false }
}

#[test]
fn pacing_follows_the_fastest_stream() {
    assert_eq!(pacing_fps(&[target("DP-1", 60), target("DP-2", 144)]), 144);
    assert_eq!(pacing_fps(&[target("DP-1", 1000)]), 240);
    assert_eq!(pacing_fps(&[]), 30);
}

#[test]
fn output_pauses_only_touch_their_stream() {
    let mut targets = vec![target("DP-1", 60), target("DP-2", 60)];
    assert!(!apply_output_pauses(&mut targets, vec![("DP-1".into(), true)]));
    assert!(targets[0].paused);
    assert!(!targets[1].paused);
    assert!(apply_output_pauses(&mut targets, vec![("DP-2".into(), true)]));
    assert!(!apply_output_pauses(&mut targets, vec![("DP-1".into(), false)]));
    let mut unnamed = vec![target("", 60)];
    assert!(apply_output_pauses(&mut unnamed, vec![("DP-9".into(), true)]));
}

#[test]
fn free_slots_short_circuit_the_wait() {
    let mut free = [[false; 3], [true, false, false]];
    assert!(wait_any_free(&[-1, -1], &mut free, &[true, true]).is_ok());
}
