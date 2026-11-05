use std::ffi::CString;
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::time::Instant;

mod process;

const SELF: &str = "/proc/self/exe";
const THREAD_NAME: &str = "reexec";
const PENDING: usize = 2;
const WAKE: u64 = 1;
static BROKER: OnceLock<Broker> = OnceLock::new();

pub(crate) struct Finished {
    pub status: std::process::ExitStatus,
    pub report: Vec<u8>,
    pub diagnostics: Vec<u8>,
}

type Reply = mpsc::SyncSender<io::Result<Finished>>;

pub(crate) struct Probe {
    pub source: File,
    pub job: crate::health::Job,
    pub deadline: Instant,
}

struct Request {
    probe: Probe,
    reply: Reply,
    cancelled: Arc<AtomicBool>,
    #[cfg(test)]
    started: mpsc::SyncSender<()>,
}

enum Terminal {
    Reexec(Vec<CString>),
    Shutdown(mpsc::SyncSender<()>),
}

struct Control {
    terminal: Mutex<Option<Terminal>>,
    wake: OwnedFd,
}

struct Broker {
    pending: mpsc::SyncSender<Request>,
    control: Arc<Control>,
}

impl Control {
    fn wake(&self) {
        unsafe {
            libc::write(self.wake.as_raw_fd(), std::ptr::from_ref(&WAKE).cast(), size_of::<u64>());
        }
    }

    fn terminal(&self) -> bool {
        self.terminal.lock().expect("broker terminal").is_some()
    }
}

pub(crate) struct Ticket {
    response: mpsc::Receiver<io::Result<Finished>>,
    cancelled: Arc<AtomicBool>,
    control: Arc<Control>,
    deadline: Instant,
    #[cfg(test)]
    started: mpsc::Receiver<()>,
}

impl Ticket {
    pub(crate) fn wait(self) -> io::Result<Finished> {
        let left = self.deadline.saturating_duration_since(Instant::now());
        self.response.recv_timeout(left).map_err(|error| match error {
            mpsc::RecvTimeoutError::Timeout => io::Error::new(io::ErrorKind::TimedOut, error),
            mpsc::RecvTimeoutError::Disconnected => {
                io::Error::new(io::ErrorKind::Interrupted, error)
            }
        })?
    }
}

impl Drop for Ticket {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        self.control.wake();
    }
}

pub(crate) fn arm() -> io::Result<()> {
    if BROKER.get().is_some() {
        return Ok(());
    }
    let descriptor = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) };
    if descriptor < 0 {
        return Err(io::Error::last_os_error());
    }
    let control = Arc::new(Control {
        terminal: Mutex::new(None),
        wake: unsafe { OwnedFd::from_raw_fd(descriptor) },
    });
    let (pending, requests) = mpsc::sync_channel(PENDING);
    let thread_control = Arc::clone(&control);
    std::thread::Builder::new()
        .name(THREAD_NAME.into())
        .spawn(move || run(&requests, &thread_control))?;
    BROKER.set(Broker { pending, control }).map_err(|_| io::Error::other("broker already armed"))
}

pub(crate) fn submit(probe: Probe) -> io::Result<Ticket> {
    let broker = BROKER.get().ok_or_else(|| io::Error::other("broker not armed"))?;
    if broker.control.terminal() {
        return Err(io::Error::new(io::ErrorKind::Interrupted, "renderer stopping"));
    }
    if Instant::now() >= probe.deadline {
        return Err(io::Error::new(io::ErrorKind::TimedOut, "probe expired"));
    }
    let deadline = probe.deadline;
    let (reply, response) = mpsc::sync_channel(1);
    let cancelled = Arc::new(AtomicBool::new(false));
    #[cfg(test)]
    let (started, observed) = mpsc::sync_channel(1);
    let request = Request {
        probe,
        reply,
        cancelled: Arc::clone(&cancelled),
        #[cfg(test)]
        started,
    };
    broker.pending.try_send(request).map_err(|error| match error {
        mpsc::TrySendError::Full(_) => {
            io::Error::new(io::ErrorKind::WouldBlock, "probe queue full")
        }
        mpsc::TrySendError::Disconnected(_) => {
            io::Error::new(io::ErrorKind::Interrupted, "broker stopped")
        }
    })?;
    broker.control.wake();
    Ok(Ticket {
        response,
        cancelled,
        control: Arc::clone(&broker.control),
        deadline,
        #[cfg(test)]
        started: observed,
    })
}

pub(crate) fn reexec(arguments: Vec<CString>) -> String {
    let Some(broker) = BROKER.get() else { return "reexec thread not armed".into() };
    {
        let mut terminal = broker.control.terminal.lock().expect("broker terminal");
        if terminal.is_some() {
            return "renderer already stopping".into();
        }
        *terminal = Some(Terminal::Reexec(arguments));
    }
    broker.control.wake();
    loop {
        std::thread::park();
    }
}

pub(crate) fn shutdown() {
    if let Some(broker) = BROKER.get() {
        let (reply, received) = mpsc::sync_channel(1);
        {
            let mut terminal = broker.control.terminal.lock().expect("broker terminal");
            if terminal.is_some() {
                return;
            }
            *terminal = Some(Terminal::Shutdown(reply));
        }
        broker.control.wake();
        let _ = received.recv();
    }
}

fn run(requests: &mpsc::Receiver<Request>, control: &Control) {
    loop {
        if control.terminal() {
            for request in requests.try_iter() {
                let _ = request
                    .reply
                    .send(Err(io::Error::new(io::ErrorKind::Interrupted, "renderer stopping")));
            }
            let terminal = control.terminal.lock().expect("broker terminal").take();
            match terminal {
                Some(Terminal::Reexec(arguments)) => {
                    let mut pointers: Vec<_> =
                        arguments.iter().map(|argument| argument.as_ptr()).collect();
                    pointers.push(std::ptr::null());
                    let executable = CString::new(SELF).expect("executable path");
                    unsafe {
                        libc::execv(executable.as_ptr(), pointers.as_ptr());
                    }
                    std::process::exit(1);
                }
                Some(Terminal::Shutdown(reply)) => {
                    let _ = reply.send(());
                }
                None => {}
            }
            return;
        }
        match requests.try_recv() {
            Ok(request) => process::run(request, control),
            Err(mpsc::TryRecvError::Disconnected) => return,
            Err(mpsc::TryRecvError::Empty) => wait_wake(control),
        }
    }
}

fn wait_wake(control: &Control) {
    let mut descriptor =
        libc::pollfd { fd: control.wake.as_raw_fd(), events: libc::POLLIN, revents: 0 };
    unsafe {
        libc::poll(&raw mut descriptor, 1, -1);
    }
    drain_wake(control);
}

fn drain_wake(control: &Control) {
    let mut wake = 0_u64;
    unsafe {
        libc::read(
            control.wake.as_raw_fd(),
            std::ptr::from_mut(&mut wake).cast(),
            size_of::<u64>(),
        );
    }
}

#[cfg(test)]
mod tests;
