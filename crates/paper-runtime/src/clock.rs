const NANOS_PER_MILLI: libc::c_long = 1_000_000;

unsafe extern "C" {
    fn tzset();
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalTime {
    pub hour: i32,
    pub minute: i32,
    pub second: i32,
    pub day: i32,
    pub month: i32,
    pub year: i32,
    pub weekday: i32,
    pub millisecond: i32,
}

pub fn reload_zone() {
    unsafe { tzset() };
}

#[must_use]
pub fn local_now() -> Option<LocalTime> {
    let mut spec = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    if unsafe { libc::clock_gettime(libc::CLOCK_REALTIME, &raw mut spec) } != 0 {
        return None;
    }
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&raw const spec.tv_sec, &raw mut tm) }.is_null() {
        return None;
    }
    Some(LocalTime {
        hour: tm.tm_hour,
        minute: tm.tm_min,
        second: tm.tm_sec,
        day: tm.tm_mday,
        month: tm.tm_mon + 1,
        year: tm.tm_year + 1900,
        weekday: tm.tm_wday,
        millisecond: (spec.tv_nsec / NANOS_PER_MILLI) as i32,
    })
}
