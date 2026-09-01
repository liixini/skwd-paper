use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::sync::Arc;

struct Ends {
    read: OwnedFd,
    write: OwnedFd,
}

#[derive(Clone)]
pub struct Pipe(Arc<Ends>);

pub fn make_pipe() -> Option<Pipe> {
    let mut fds = [-1i32; 2];
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_NONBLOCK | libc::O_CLOEXEC) } != 0 {
        return None;
    }
    Some(Pipe(Arc::new(unsafe {
        Ends { read: OwnedFd::from_raw_fd(fds[0]), write: OwnedFd::from_raw_fd(fds[1]) }
    })))
}

impl Pipe {
    pub fn read_fd(&self) -> RawFd {
        self.0.read.as_raw_fd()
    }

    pub fn poke(&self) {
        unsafe {
            libc::write(self.0.write.as_raw_fd(), [1u8].as_ptr().cast(), 1);
        }
    }

    pub fn drain(&self) {
        let mut buf = [0u8; 64];
        unsafe {
            libc::read(self.read_fd(), buf.as_mut_ptr().cast(), buf.len());
        }
    }
}

mod tests;
