use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};

const SECONDS_PER_MINUTE: libc::time_t = 60;

pub struct MinuteTimer {
    fd: OwnedFd,
}

impl MinuteTimer {
    pub fn new() -> io::Result<Self> {
        let raw = unsafe {
            libc::timerfd_create(libc::CLOCK_REALTIME, libc::TFD_NONBLOCK | libc::TFD_CLOEXEC)
        };
        if raw < 0 {
            return Err(io::Error::last_os_error());
        }
        let timer = Self { fd: unsafe { OwnedFd::from_raw_fd(raw) } };
        timer.arm()?;
        Ok(timer)
    }

    pub fn arm(&self) -> io::Result<()> {
        let mut now = libc::timespec { tv_sec: 0, tv_nsec: 0 };
        if unsafe { libc::clock_gettime(libc::CLOCK_REALTIME, &raw mut now) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let next = now.tv_sec - now.tv_sec.rem_euclid(SECONDS_PER_MINUTE) + SECONDS_PER_MINUTE;
        let spec = libc::itimerspec {
            it_interval: libc::timespec { tv_sec: SECONDS_PER_MINUTE, tv_nsec: 0 },
            it_value: libc::timespec { tv_sec: next, tv_nsec: 0 },
        };
        let flags = libc::TFD_TIMER_ABSTIME | libc::TFD_TIMER_CANCEL_ON_SET;
        if unsafe {
            libc::timerfd_settime(self.fd.as_raw_fd(), flags, &raw const spec, std::ptr::null_mut())
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub fn fire(&self) -> io::Result<bool> {
        let mut expirations = 0u64;
        let read = unsafe {
            libc::read(self.fd.as_raw_fd(), (&raw mut expirations).cast(), size_of::<u64>())
        };
        if read >= 0 {
            return Ok(true);
        }
        let error = io::Error::last_os_error();
        match error.raw_os_error() {
            Some(libc::ECANCELED) => {
                self.arm()?;
                Ok(true)
            }
            Some(libc::EAGAIN) => Ok(false),
            _ => Err(error),
        }
    }

    pub fn raw(&self) -> RawFd {
        self.fd.as_raw_fd()
    }
}

#[cfg(test)]
#[path = "minute_tests.rs"]
mod tests;
