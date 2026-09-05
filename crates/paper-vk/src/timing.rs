use std::time::{Duration, Instant};

pub(crate) struct VideoClock {
    clock: f64,
    last_pts: f64,
}

impl VideoClock {
    pub(crate) fn new(first_pts: f64) -> Self {
        Self { clock: first_pts, last_pts: first_pts }
    }

    pub(crate) fn tick(&mut self, refresh_period: f64, speed: f64) {
        self.clock += refresh_period * speed;
    }

    pub(crate) fn should_show(&mut self, pts: f64) -> bool {
        if pts < self.last_pts {
            self.clock = pts;
        }
        if pts <= self.clock {
            self.last_pts = pts;
            true
        } else {
            false
        }
    }
}

pub(crate) struct Pacer {
    anchor: Option<(Instant, f64)>,
    last_pts: f64,
    last_due: Option<Instant>,
    last_delta: f64,
}

impl Pacer {
    pub(crate) fn new() -> Self {
        Self { anchor: None, last_pts: f64::NEG_INFINITY, last_due: None, last_delta: 0.0 }
    }

    pub(crate) fn due(&mut self, now: Instant, pts: f64) -> Instant {
        let due = match self.anchor {
            Some((start, first_pts)) if pts >= self.last_pts => {
                start + Duration::from_secs_f64(pts - first_pts)
            }
            Some(_) => {
                let due = self.last_due.map_or(now, |previous| {
                    (previous + Duration::from_secs_f64(self.last_delta)).max(now)
                });
                self.anchor = Some((due, pts));
                due
            }
            None => {
                self.anchor = Some((now, pts));
                now
            }
        };
        if self.last_due.is_some() && pts > self.last_pts {
            self.last_delta = pts - self.last_pts;
        }
        self.last_due = Some(due);
        self.last_pts = pts;
        due
    }
}

pub(crate) fn due(elapsed_seconds: f64, speed: f64, pts: f64, anchor_pts: f64) -> bool {
    pts - anchor_pts <= elapsed_seconds * speed
}

pub(crate) fn frame_target(
    base_frame: u64,
    base_pts: f64,
    pts: f64,
    speed: f64,
    refresh: u64,
) -> u64 {
    let relative = (pts - base_pts) / speed;
    let slots = ((relative * 1e9 / refresh as f64).round() as i64).max(0) as u64;
    base_frame + slots * refresh
}

pub(crate) fn pace_stalled(target: i64, now: u64, refresh: u64) -> bool {
    now as i64 > target + (refresh * 8) as i64
}

pub(crate) fn loop_frame_target(previous_commit: u64, step: u64, now: u64, refresh: u64) -> u64 {
    (previous_commit + step).max(now) + refresh.saturating_sub(1_500_000)
}

mod tests;
