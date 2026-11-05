use super::*;
use crate::health::{Outcome, error_outcome};
use std::io::Write;
use std::os::unix::process::ExitStatusExt;
use std::process::Command;
use std::time::Duration;

pub(super) const FAULT_ENV: &str = "SKWD_PROBE_TEST_FAULT";
const ENTRY_ENV: &str = "SKWD_PROBE_TEST_ENTRY";
const BROKER_ENTRY: &str = "broker";
const REEXEC_ENTRY: &str = "reexec";
const SOURCE: &str = "/dev/null";
const HELPER: &str = "sandbox::broker::tests::broker_child";
const WATCHDOG: &str = "/usr/bin/timeout";
const WATCHDOG_LIMIT: &str = "20s";
const PRIVATE_FD_MIN: i32 = 512;
const REPLACEMENT: &str = "sandbox::broker::tests::replacement_child";

fn launch(mode: &str, timeout: Duration) -> Ticket {
    submit(Probe {
        source: File::open(SOURCE).unwrap(),
        job: crate::health::Job::Fault(mode.into()),
        deadline: Instant::now() + timeout,
    })
    .unwrap()
}

#[test]
fn subprocess_failures_contained() {
    let status = Command::new(WATCHDOG)
        .args(["--signal=KILL", WATCHDOG_LIMIT])
        .arg(std::env::current_exe().unwrap())
        .args(["--exact", HELPER, "--nocapture"])
        .env(ENTRY_ENV, BROKER_ENTRY)
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
fn broker_child() {
    if std::env::var(ENTRY_ENV).as_deref() != Ok(BROKER_ENTRY) {
        return;
    }
    arm().unwrap();
    crate::sandbox::restrict_renderer().unwrap();
    let expired = submit(Probe {
        source: File::open(SOURCE).unwrap(),
        job: crate::health::Job::Fault("ok".into()),
        deadline: Instant::now(),
    })
    .err()
    .unwrap();
    assert_eq!(expired.kind(), io::ErrorKind::TimedOut);
    let crashed = launch("signal", Duration::from_secs(5)).wait().unwrap();
    assert_eq!(crashed.status.signal(), Some(libc::SIGSEGV));
    let rejected = launch("reject", Duration::from_secs(5)).wait().unwrap();
    assert_eq!(rejected.status.code(), Some(2));
    let teardown = launch("teardown", Duration::from_secs(5)).wait().unwrap();
    assert_eq!(teardown.status.signal(), Some(libc::SIGSEGV));
    assert!(launch("oversized", Duration::from_secs(5)).wait().is_err());
    let timeout = launch("hang", Duration::from_millis(100)).wait().err().unwrap();
    assert_eq!(timeout.kind(), io::ErrorKind::TimedOut);
    assert_eq!(error_outcome(Some(timeout.kind())), Outcome::TimedOut);
    let cancelled = launch("hang", Duration::from_secs(5));
    cancelled.started.recv_timeout(Duration::from_secs(2)).unwrap();
    let queued = launch("ok", Duration::from_millis(100));
    let expired = queued.wait().err().unwrap();
    assert_eq!(expired.kind(), io::ErrorKind::TimedOut);
    drop(cancelled);
    let active = launch("hang", Duration::from_secs(5));
    active.started.recv_timeout(Duration::from_secs(2)).unwrap();
    let first = launch("ok", Duration::from_secs(5));
    let second = launch("ok", Duration::from_secs(5));
    let full = submit(Probe {
        source: File::open(SOURCE).unwrap(),
        job: crate::health::Job::Fault("ok".into()),
        deadline: Instant::now() + Duration::from_secs(5),
    })
    .err()
    .unwrap();
    assert_eq!(full.kind(), io::ErrorKind::WouldBlock);
    assert_eq!(error_outcome(Some(full.kind())), Outcome::Saturated);
    drop(active);
    for ticket in [first, second] {
        assert!(ticket.wait().unwrap().status.success());
    }
    assert!(launch("ok", Duration::from_secs(5)).wait().unwrap().status.success());
    let source = File::open(SOURCE).unwrap();
    let descriptor = unsafe { libc::fcntl(source.as_raw_fd(), libc::F_DUPFD, PRIVATE_FD_MIN) };
    assert!(descriptor >= PRIVATE_FD_MIN);
    let inherited = unsafe { OwnedFd::from_raw_fd(descriptor) };
    let before = unsafe { libc::fcntl(inherited.as_raw_fd(), libc::F_GETFD) };
    assert_eq!(before & libc::FD_CLOEXEC, 0);
    assert!(
        launch(&format!("fd:{descriptor}"), Duration::from_secs(5))
            .wait()
            .unwrap()
            .status
            .success()
    );
    assert_eq!(unsafe { libc::fcntl(inherited.as_raw_fd(), libc::F_GETFD) }, before);
    assert!(launch("exec-denied", Duration::from_secs(5)).wait().unwrap().status.success());
    let active = launch("hang", Duration::from_secs(5));
    active.started.recv_timeout(Duration::from_secs(2)).unwrap();
    let queued = launch("ok", Duration::from_secs(5));
    shutdown();
    for ticket in [active, queued] {
        let error = ticket.wait().err().unwrap();
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
        assert_eq!(error_outcome(Some(error.kind())), Outcome::Cancelled);
    }
}

#[test]
fn reexec_cancels_probe() {
    let status = Command::new(WATCHDOG)
        .args(["--signal=KILL", WATCHDOG_LIMIT])
        .arg(std::env::current_exe().unwrap())
        .args(["--exact", "sandbox::broker::tests::reexec_child", "--nocapture"])
        .env(ENTRY_ENV, REEXEC_ENTRY)
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
fn reexec_child() {
    if std::env::var(ENTRY_ENV).as_deref() != Ok(REEXEC_ENTRY) {
        return;
    }
    arm().unwrap();
    crate::sandbox::restrict_renderer().unwrap();
    let ticket = launch("hang", Duration::from_secs(5));
    ticket.started.recv_timeout(Duration::from_secs(2)).unwrap();
    let arguments = [SELF, "--exact", REPLACEMENT, "--nocapture"]
        .map(|argument| CString::new(argument).unwrap())
        .to_vec();
    reexec(arguments);
}

#[test]
fn replacement_child() {
    if std::env::var(ENTRY_ENV).as_deref() != Ok(REEXEC_ENTRY) {
        return;
    }
    let mut status = 0;
    assert_eq!(unsafe { libc::waitpid(-1, &raw mut status, libc::WNOHANG) }, -1);
    assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::ECHILD));
}

#[test]
fn fault_child() {
    if std::env::var_os(FAULT_ENV).is_none() {
        return;
    }
    crate::sandbox::restrict_renderer().unwrap();
    match std::env::var(FAULT_ENV).unwrap().as_str() {
        "signal" => crash(),
        "teardown" => {
            std::io::stdout().write_all(b"{\"frames\":1}").unwrap();
            crash();
        }
        "reject" => std::process::exit(2),
        "ok" => std::process::exit(0),
        "exec-denied" => {
            let error = Command::new(SELF).arg("--help").status().unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        }
        mode if mode.starts_with("fd:") => {
            let descriptor: i32 = mode.strip_prefix("fd:").unwrap().parse().unwrap();
            assert_eq!(unsafe { libc::fcntl(descriptor, libc::F_GETFD) }, -1);
            assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::EBADF));
            assert!(unsafe { libc::fcntl(libc::STDIN_FILENO, libc::F_GETFD) } >= 0);
        }
        "oversized" => {
            std::io::stdout().write_all(&[b'x'; 8192]).unwrap();
        }
        "hang" => loop {
            std::thread::park();
        },
        _ => panic!("unknown child mode"),
    }
}

fn crash() -> ! {
    assert_ne!(unsafe { libc::signal(libc::SIGSEGV, libc::SIG_DFL) }, libc::SIG_ERR);
    assert_eq!(unsafe { libc::raise(libc::SIGSEGV) }, 0);
    panic!("SIGSEGV returned with its default disposition");
}
