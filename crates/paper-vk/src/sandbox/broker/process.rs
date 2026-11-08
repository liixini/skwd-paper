use super::{Control, Finished, Reply, Request, SELF, drain_wake};
use crate::health::REPORT_BYTES;
use std::io::{self, Read};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::process::{Child, ChildStderr, ChildStdout, Command, Stdio};
use std::sync::atomic::Ordering;
use std::time::Instant;

const READ_BYTES: usize = 1024;
const DIAGNOSTIC_BYTES: usize = 32 * 1024;
const FIRST_PRIVATE_DESCRIPTOR: libc::c_uint = 3;

struct Running {
    child: Child,
    output: ChildStdout,
    errors: ChildStderr,
    process: OwnedFd,
    report: Vec<u8>,
    diagnostics: Vec<u8>,
    output_eof: bool,
    errors_eof: bool,
}

pub(super) fn run(request: Request, control: &Control) {
    let Request {
        probe,
        reply,
        cancelled,
        #[cfg(test)]
        started,
    } = request;
    if let Some(error) = interruption(&cancelled, control, probe.deadline) {
        let _ = reply.send(Err(error));
        return;
    }
    let child = spawn(probe.source, &probe.job);
    let mut running = match child {
        Ok(child) => child,
        Err(error) => {
            let _ = reply.send(Err(error));
            return;
        }
    };
    #[cfg(test)]
    let _ = started.send(());
    let mut reply = Some(reply);
    loop {
        if reply.is_some()
            && let Some(error) = interruption(&cancelled, control, probe.deadline)
        {
            stop(&mut running.child, &mut reply, error);
        }
        if let Err(error) = read_report(&mut running) {
            stop(&mut running.child, &mut reply, error);
        }
        match running.child.try_wait() {
            Ok(Some(status)) => {
                if let Some(reply) = reply.take() {
                    let result = read_report(&mut running).map(|()| Finished {
                        status,
                        report: running.report,
                        diagnostics: running.diagnostics,
                    });
                    let _ = reply.send(result);
                }
                return;
            }
            Ok(None) => {}
            Err(error) => {
                stop(&mut running.child, &mut reply, error);
            }
        }
        let timeout = if reply.is_some() {
            i32::try_from(probe.deadline.saturating_duration_since(Instant::now()).as_millis())
                .unwrap_or(i32::MAX)
        } else {
            -1
        };
        let mut descriptors = [
            libc::pollfd { fd: running.process.as_raw_fd(), events: libc::POLLIN, revents: 0 },
            libc::pollfd { fd: control.wake.as_raw_fd(), events: libc::POLLIN, revents: 0 },
            libc::pollfd {
                fd: if reply.is_some() && !running.output_eof {
                    running.output.as_raw_fd()
                } else {
                    -1
                },
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: if reply.is_some() && !running.errors_eof {
                    running.errors.as_raw_fd()
                } else {
                    -1
                },
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        unsafe {
            libc::poll(descriptors.as_mut_ptr(), descriptors.len() as libc::nfds_t, timeout);
        }
        if descriptors[1].revents != 0 {
            drain_wake(control);
        }
    }
}

fn interruption(
    cancelled: &std::sync::atomic::AtomicBool,
    control: &Control,
    deadline: Instant,
) -> Option<io::Error> {
    if cancelled.load(Ordering::Acquire) || control.terminal() {
        Some(io::Error::new(io::ErrorKind::Interrupted, "probe cancelled"))
    } else if Instant::now() >= deadline {
        Some(io::Error::new(io::ErrorKind::TimedOut, "probe expired"))
    } else {
        None
    }
}

fn stop(child: &mut Child, reply: &mut Option<Reply>, reason: io::Error) {
    let _ = child.kill();
    if let Some(reply) = reply.take() {
        let _ = reply.send(Err(reason));
    }
}

fn spawn(source: std::fs::File, job: &crate::health::Job) -> io::Result<Running> {
    let mut command = Command::new(SELF);
    command
        .args(job.arguments())
        .stdin(Stdio::from(source))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(test)]
    if let crate::health::Job::Fault(mode) = job {
        command.env(super::tests::FAULT_ENV, mode);
    }
    paper_runtime::die_with_parent(&mut command);
    unsafe {
        command.pre_exec(move || {
            let core = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
            if libc::setrlimit(libc::RLIMIT_CORE, &raw const core) != 0 {
                return Err(io::Error::last_os_error());
            }
            if libc::syscall(
                libc::SYS_close_range,
                FIRST_PRIVATE_DESCRIPTOR,
                libc::c_uint::MAX,
                libc::CLOSE_RANGE_CLOEXEC,
            ) != 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command.spawn()?;
    let descriptor = unsafe { libc::syscall(libc::SYS_pidfd_open, child.id(), 0) };
    if descriptor < 0 {
        let error = io::Error::last_os_error();
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }
    let process = unsafe { OwnedFd::from_raw_fd(descriptor as i32) };
    let output = child.stdout.take().expect("piped probe output");
    let errors = child.stderr.take().expect("piped probe diagnostics");
    for descriptor in [output.as_raw_fd(), errors.as_raw_fd()] {
        let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFL) };
        if flags < 0
            || unsafe { libc::fcntl(descriptor, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
        {
            let error = io::Error::last_os_error();
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    }
    Ok(Running {
        child,
        output,
        errors,
        process,
        report: Vec::new(),
        diagnostics: Vec::new(),
        output_eof: false,
        errors_eof: false,
    })
}

fn read_report(running: &mut Running) -> io::Result<()> {
    running.output_eof |= read_bounded(&mut running.output, &mut running.report, REPORT_BYTES)?;
    running.errors_eof |=
        read_bounded(&mut running.errors, &mut running.diagnostics, DIAGNOSTIC_BYTES)?;
    Ok(())
}

fn read_bounded(reader: &mut impl Read, retained: &mut Vec<u8>, limit: usize) -> io::Result<bool> {
    let mut bytes = [0_u8; READ_BYTES];
    loop {
        match reader.read(&mut bytes) {
            Ok(0) => return Ok(true),
            Ok(count) => {
                if retained.len() + count > limit {
                    return Err(io::Error::other("probe output exceeds limit"));
                }
                retained.extend_from_slice(&bytes[..count]);
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(false),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests;
