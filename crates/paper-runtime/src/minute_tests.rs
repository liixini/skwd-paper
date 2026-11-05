use super::*;

#[test]
fn arms_next_boundary() {
    let timer = MinuteTimer::new().unwrap();
    let mut spec = libc::itimerspec {
        it_interval: libc::timespec { tv_sec: 0, tv_nsec: 0 },
        it_value: libc::timespec { tv_sec: 0, tv_nsec: 0 },
    };
    unsafe { libc::timerfd_gettime(timer.raw(), &raw mut spec) };
    assert!(spec.it_value.tv_sec <= SECONDS_PER_MINUTE);
    assert_eq!(spec.it_interval.tv_sec, SECONDS_PER_MINUTE);
}
