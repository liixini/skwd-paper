#![cfg(test)]

use super::*;

#[test]
fn loop_preserves_slow_frame_duration_with_pending_feedback() {
    let refresh = 10_000_000;
    let previous_commit = 1_000_000_000;
    let now = previous_commit + 2_000_000;
    let anchor = loop_frame_target(previous_commit, 350_000_000, now, refresh);
    let first_commit = frame_target(anchor, 0.0, 0.0, 1.0, refresh) - refresh + 1_500_000;
    let second_commit = frame_target(anchor, 0.0, 0.35, 1.0, refresh) - refresh + 1_500_000;
    assert_eq!(first_commit - previous_commit, 350_000_000);
    assert_eq!(second_commit - first_commit, 350_000_000);
}

#[test]
fn stalled_loop_reanchors_to_now() {
    let anchor = loop_frame_target(1_000_000_000, 350_000_000, 2_000_000_000, 10_000_000);
    assert_eq!(anchor - 10_000_000 + 1_500_000, 2_000_000_000);
}

#[test]
fn clock_video_cadence() {
    let mut clock = VideoClock::new(0.0);
    let period = 1.0 / 144.0;
    let step = 1.0 / 24.0;
    let mut pending = step;
    let mut shows_at = Vec::new();
    for tick in 1..=30 {
        clock.tick(period, 1.0);
        if clock.should_show(pending) {
            shows_at.push(tick);
            pending += step;
        }
    }
    assert_eq!(shows_at, vec![6, 12, 18, 24, 30]);
}

#[test]
fn clock_reanchors_on_wrap() {
    let step = 1.0 / 24.0;
    let mut clock = VideoClock::new(17.375);
    clock.tick(1.0 / 144.0, 1.0);
    assert!(clock.should_show(0.0));
    clock.tick(1.0 / 144.0, 1.0);
    assert!(!clock.should_show(step));
    for _ in 0..5 {
        clock.tick(1.0 / 144.0, 1.0);
    }
    assert!(clock.should_show(step));
}

#[test]
fn clock_honors_speed() {
    let mut clock = VideoClock::new(0.0);
    let step = 1.0 / 24.0;
    clock.tick(1.0 / 144.0, 6.0);
    assert!(clock.should_show(step));
    clock.tick(1.0 / 144.0, 6.0);
    assert!(clock.should_show(2.0 * step));
}

#[test]
fn pacer_tracks_anchor() {
    let mut pacer = Pacer::new();
    let start = Instant::now();
    assert_eq!(pacer.due(start, 0.0), start);
    assert_eq!(pacer.due(start, 1.0 / 24.0), start + Duration::from_secs_f64(1.0 / 24.0));
    assert_eq!(pacer.due(start, 2.0 / 24.0), start + Duration::from_secs_f64(2.0 / 24.0));
}

#[test]
fn pacer_cadence_across_wrap() {
    let mut pacer = Pacer::new();
    let start = Instant::now();
    pacer.due(start, 0.0);
    let step = 1.0 / 24.0;
    pacer.due(start, 17.0);
    let last = pacer.due(start, 17.0 + step);
    let now = last + Duration::from_millis(5);
    let wrapped = pacer.due(now, 0.0);
    assert_eq!(wrapped, last + Duration::from_secs_f64(step));
    assert_eq!(pacer.due(now, step), wrapped + Duration::from_secs_f64(step));
}

#[test]
fn pacer_wrap_not_past() {
    let mut pacer = Pacer::new();
    let start = Instant::now();
    pacer.due(start, 0.0);
    pacer.due(start, 1.0 / 24.0);
    let late = start + Duration::from_secs(2);
    assert!(pacer.due(late, 0.0) >= late);
}

#[test]
fn pacer_backward_jump() {
    let mut pacer = Pacer::new();
    let start = Instant::now();
    pacer.due(start, 5.0);
    let last = pacer.due(start, 6.0);
    let later = start + Duration::from_secs(2);
    assert!(pacer.due(later, 5.0) >= later.max(last));
}

#[test]
fn fade_due_speed_wrap() {
    let advance_count = |speed| {
        let mut advanced = 0u32;
        for tick in 1..=144 {
            let elapsed = f64::from(tick) / 144.0;
            while due(elapsed, speed, f64::from(advanced + 1) / 30.0, 0.0) {
                advanced += 1;
            }
        }
        advanced
    };
    assert_eq!(advance_count(1.0), 30);
    assert_eq!(advance_count(2.0), 60);

    let elapsed = 0.8;
    let wrapped_pts = 0.033;
    let anchor_pts = wrapped_pts - elapsed;
    assert!(due(elapsed, 1.0, wrapped_pts, anchor_pts));
    assert!(!due(elapsed, 1.0, wrapped_pts + 0.04, anchor_pts));
}

#[test]
fn frame_target_slots() {
    let refresh = 6_944_444;
    let base = 1_000_000_000;
    assert_eq!(frame_target(base, 0.0, 0.0, 1.0, refresh), base);
    assert_eq!(frame_target(base, 0.0, 1.0 / 144.0, 1.0, refresh), base + refresh);
    assert_eq!(frame_target(base, 0.0, 10.0 / 144.0, 1.0, refresh), base + refresh * 10);
}

#[test]
fn stalled_anchor_rebase() {
    let refresh = 6_944_444;
    let now = 100 * refresh;
    assert!(!pace_stalled((now + refresh) as i64, now, refresh));
    assert!(!pace_stalled((now - refresh * 2) as i64, now, refresh));
    assert!(pace_stalled((now - refresh * 40) as i64, now, refresh));

    let mut base = 1_000_000_000;
    let base_pts = 5.0;
    let late_now = base + refresh * 500;
    assert!(pace_stalled(
        frame_target(base, base_pts, base_pts, 1.0, refresh) as i64,
        late_now,
        refresh
    ));
    base = late_now;
    let next = frame_target(base, base_pts, base_pts + 1.0 / 30.0, 1.0, refresh);
    let ahead = next.saturating_sub(late_now);
    assert!(ahead >= refresh && ahead <= refresh * 8);
}

#[test]
fn plasma_resume_does_not_decode_through_hidden_time() {
    let start = Instant::now();
    let interval = Duration::from_millis(33);
    let deadline = start + interval;
    let resumed = start + Duration::from_secs(60);
    let shift = stream_resume_shift(deadline, resumed, Duration::from_secs(60), interval);
    assert_eq!(deadline + shift, resumed);
    assert_eq!(deadline + interval + shift, resumed + interval);
}

#[test]
fn ordinary_plasma_feedback_keeps_the_playback_clock() {
    let start = Instant::now();
    let interval = Duration::from_millis(33);
    assert_eq!(stream_resume_shift(start, start + interval, interval, interval), Duration::ZERO);
}

#[test]
fn plasma_resume_preserves_a_future_frame_deadline() {
    let resumed = Instant::now();
    assert_eq!(
        stream_resume_shift(
            resumed + Duration::from_millis(10),
            resumed,
            Duration::from_secs(60),
            Duration::from_millis(33)
        ),
        Duration::ZERO
    );
}
