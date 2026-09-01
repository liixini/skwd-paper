#![cfg(test)]

use std::os::fd::AsRawFd;

#[test]
fn poke_drain() {
    let pipe = super::make_pipe().expect("pipe2");
    let mut pfd = libc::pollfd { fd: pipe.read_fd(), events: libc::POLLIN, revents: 0 };
    assert_eq!(unsafe { libc::poll(&mut pfd, 1, 0) }, 0);
    pipe.poke();
    pfd.revents = 0;
    assert_eq!(unsafe { libc::poll(&mut pfd, 1, 100) }, 1);
    pipe.drain();
    pfd.revents = 0;
    assert_eq!(unsafe { libc::poll(&mut pfd, 1, 0) }, 0);
}

#[test]
fn pipe_flags() {
    let pipe = super::make_pipe().expect("pipe2");
    for fd in [pipe.0.read.as_raw_fd(), pipe.0.write.as_raw_fd()] {
        let (status, descriptor) =
            unsafe { (libc::fcntl(fd, libc::F_GETFL), libc::fcntl(fd, libc::F_GETFD)) };
        assert_ne!(status, -1);
        assert_ne!(descriptor, -1);
        assert_ne!(status & libc::O_NONBLOCK, 0);
        assert_ne!(descriptor & libc::FD_CLOEXEC, 0);
    }
}

#[test]
fn clone_keeps_ends() {
    let pipe = super::make_pipe().expect("pipe2");
    let sender = pipe.clone();
    let read_fd = pipe.read_fd();
    drop(pipe);
    sender.poke();
    let mut pfd = libc::pollfd { fd: read_fd, events: libc::POLLIN, revents: 0 };
    assert_eq!(unsafe { libc::poll(&mut pfd, 1, 100) }, 1);
    sender.drain();
}
