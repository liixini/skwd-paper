use crate::backend::RendererUnavailable;
use crate::client::socket_path;
use crate::manager::{ApplyTransaction, Manager, WorkerStatus};
use anyhow::{Context, Result, anyhow};
use paper_control::{
    ApplyResponse, ApplyResult, AssignmentStatus, AudioSetResponse, AudioSetResult,
    CapabilitiesResponse, CapabilitiesResult, PauseResponse, PauseResult, Request, RequestParams,
    Response, StatusResponse, StatusResult, StopResponse, StopResult, decode_ndjson, encode_ndjson,
};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener as StdUnixListener, UnixStream as StdUnixStream};
use std::path::Path;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};

const MAX_REQUEST: u64 = 1024 * 1024;
const START_IDLE: Duration = Duration::from_secs(5);
const EMPTY_IDLE: Duration = Duration::from_millis(750);
const DEFAULT_READY_TIMEOUT: Duration = Duration::from_secs(15);
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(1);

struct SocketGuard {
    path: std::path::PathBuf,
    device: u64,
    inode: u64,
}

impl SocketGuard {
    fn new(path: &Path) -> Result<Self> {
        let metadata = std::fs::symlink_metadata(path)?;
        Ok(Self { path: path.to_path_buf(), device: metadata.dev(), inode: metadata.ino() })
    }
}

impl Drop for SocketGuard {
    fn drop(&mut self) {
        let Ok(metadata) = std::fs::symlink_metadata(&self.path) else {
            return;
        };
        if metadata.file_type().is_socket()
            && metadata.dev() == self.device
            && metadata.ino() == self.inode
        {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

pub(crate) fn run() -> Result<()> {
    let path = socket_path();
    run_at(&path)
}

fn run_at(path: &Path) -> Result<()> {
    run_with_manager(path, Manager::new())
}

fn run_with_manager(path: &Path, manager: Manager) -> Result<()> {
    prepare_parent(path)?;
    let _lock = acquire_lock(path)?;
    let listener = bind(path)?;
    let _guard = SocketGuard::new(path)?;
    listener.set_nonblocking(true).context("set Paper listener nonblocking")?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("build Paper runtime")?;
    runtime.block_on(serve(listener, path, manager))
}

enum Event {
    Connection(UnixStream),
    WorkerExit,
    IdleDeadline,
}

async fn serve(listener: StdUnixListener, path: &Path, mut manager: Manager) -> Result<()> {
    let listener = UnixListener::from_std(listener).context("register Paper listener")?;
    let started = Instant::now();
    let mut handled = false;
    let mut last_handled = None;
    loop {
        let deadline = idle_deadline(started, handled, last_handled, manager.is_empty());
        match next_event(&listener, &mut manager, deadline).await? {
            Event::Connection(stream) => {
                handled = true;
                if let Err(error) = handle(stream, &listener, path, &mut manager).await {
                    tracing::warn!(error = %error, "Paper request failed");
                }
                last_handled = Some(Instant::now());
            }
            Event::WorkerExit => manager.refresh()?,
            Event::IdleDeadline => {
                if manager.is_empty()
                    && last_handled.is_some_and(|instant| instant.elapsed() >= EMPTY_IDLE)
                {
                    return Ok(());
                }
                if !handled && started.elapsed() >= START_IDLE {
                    return Ok(());
                }
            }
        }
    }
}

async fn next_event(
    listener: &UnixListener,
    manager: &mut Manager,
    deadline: Option<Instant>,
) -> Result<Event> {
    tokio::select! {
        biased;
        accepted = listener.accept() => {
            let (stream, _) = accepted?;
            Ok(Event::Connection(stream))
        }
        () = manager.worker_exit() => Ok(Event::WorkerExit),
        () = idle_sleep(deadline) => Ok(Event::IdleDeadline),
    }
}

async fn idle_sleep(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline.into()).await,
        None => std::future::pending().await,
    }
}

fn idle_deadline(
    started: Instant,
    handled: bool,
    last_handled: Option<Instant>,
    empty: bool,
) -> Option<Instant> {
    if !empty {
        return None;
    }
    if let Some(instant) = last_handled {
        return Some(instant + EMPTY_IDLE);
    }
    if handled { None } else { Some(started + START_IDLE) }
}

async fn handle(
    mut stream: UnixStream,
    listener: &UnixListener,
    socket: &Path,
    manager: &mut Manager,
) -> Result<()> {
    if peer_credentials(&stream)?.uid != effective_uid() {
        return Err(anyhow!("Paper request peer UID does not match"));
    }
    let request = read_request(&mut stream).await?;
    if let Err(error) = request.validate() {
        return write_failure(&mut stream, request.id, "invalid_request", &error.to_string()).await;
    }
    match request.params {
        RequestParams::Apply(apply) => {
            let mut transaction = match manager.begin_apply(&apply, socket).await {
                Ok(transaction) => transaction,
                Err(error) => {
                    return write_response(
                        &mut stream,
                        &ApplyResponse::failure(
                            request.id,
                            apply_rejection_code(&error),
                            error.to_string(),
                        ),
                    )
                    .await;
                }
            };
            let generation = transaction.generation();
            if let Err(error) = wait_ready(listener, &mut transaction).await {
                transaction.rollback().await;
                return write_response(
                    &mut stream,
                    &ApplyResponse::failure(request.id, "apply_failed", error.to_string()),
                )
                .await;
            }
            let assignments = match manager.commit(transaction).await {
                Ok(assignments) => statuses(assignments),
                Err(error) => {
                    return write_response(
                        &mut stream,
                        &ApplyResponse::failure(request.id, "apply_failed", error.to_string()),
                    )
                    .await;
                }
            };
            write_response(
                &mut stream,
                &ApplyResponse::success(
                    request.id,
                    ApplyResult {
                        generation,
                        paused: manager.paused(),
                        policy: manager.policy(),
                        assignments,
                    },
                ),
            )
            .await
        }
        RequestParams::Stop(stop) => match manager.stop(&stop).await {
            Ok(stopped) => {
                write_response(
                    &mut stream,
                    &StopResponse::success(request.id, StopResult { stopped }),
                )
                .await
            }
            Err(error) => {
                write_response(
                    &mut stream,
                    &StopResponse::failure(request.id, "stop_failed", error.to_string()),
                )
                .await
            }
        },
        RequestParams::Pause(pause) => {
            let transaction = if pause.paused {
                manager.begin_pause(socket).await
            } else {
                manager.begin_resume(socket).await
            };
            let mut transaction = match transaction {
                Ok(transaction) => transaction,
                Err(error) => {
                    return write_response(
                        &mut stream,
                        &PauseResponse::failure(request.id, "pause_failed", error.to_string()),
                    )
                    .await;
                }
            };
            if let Some(mut transaction) = transaction.take() {
                if let Err(error) = wait_ready(listener, &mut transaction).await {
                    transaction.rollback().await;
                    if pause.paused {
                        manager.abort_pause().await;
                    }
                    return write_response(
                        &mut stream,
                        &PauseResponse::failure(request.id, "pause_failed", error.to_string()),
                    )
                    .await;
                }
                let result = if pause.paused {
                    manager.commit_pause(transaction).await
                } else {
                    manager.commit_resume(transaction).await
                };
                if let Err(error) = result {
                    return write_response(
                        &mut stream,
                        &PauseResponse::failure(request.id, "pause_failed", error.to_string()),
                    )
                    .await;
                }
            }
            write_response(
                &mut stream,
                &PauseResponse::success(request.id, PauseResult { paused: pause.paused }),
            )
            .await
        }
        RequestParams::AudioSet(audio) => match manager.audio(&audio).await {
            Ok((updated, assignments)) => {
                write_response(
                    &mut stream,
                    &AudioSetResponse::success(
                        request.id,
                        AudioSetResult { updated, assignments: statuses(assignments) },
                    ),
                )
                .await
            }
            Err(error) => {
                write_response(
                    &mut stream,
                    &AudioSetResponse::failure(request.id, "audio_failed", error.to_string()),
                )
                .await
            }
        },
        RequestParams::Status(_) => {
            manager.refresh()?;
            let assignments = statuses(manager.status());
            write_response(
                &mut stream,
                &StatusResponse::success(
                    request.id,
                    StatusResult {
                        paused: manager.paused(),
                        policy: manager.policy(),
                        assignments,
                        renderers: manager.renderer_capabilities(),
                    },
                ),
            )
            .await
        }
        RequestParams::Capabilities(_) => {
            let capabilities =
                CapabilitiesResult::current().with_renderers(manager.renderer_capabilities());
            write_response(&mut stream, &CapabilitiesResponse::success(request.id, capabilities))
                .await
        }
        RequestParams::Ready(_) | RequestParams::Failed(_) => Ok(()),
    }
}

fn apply_rejection_code(error: &anyhow::Error) -> &'static str {
    if error.downcast_ref::<RendererUnavailable>().is_some() {
        "renderer_unavailable"
    } else {
        "apply_rejected"
    }
}

async fn wait_ready(listener: &UnixListener, transaction: &mut ApplyTransaction) -> Result<()> {
    let timeout = std::env::var("SKWD_PAPER_READY_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map_or(DEFAULT_READY_TIMEOUT, Duration::from_millis);
    let deadline = Instant::now() + timeout;
    loop {
        if transaction.all_ready() && !transaction.prepare_next()? {
            break;
        }
        if let Some(message) = transaction.failed()? {
            return Err(anyhow!(message));
        }
        if Instant::now() >= deadline {
            return Err(anyhow!("Paper composition readiness timed out"));
        }
        let Some(mut stream) = ready_event(listener, transaction, deadline).await? else {
            continue;
        };
        let credentials = peer_credentials(&stream)?;
        if credentials.uid != effective_uid() {
            continue;
        }
        let Ok(request) = read_request(&mut stream).await else { continue };
        match request.params {
            RequestParams::Ready(ready) => {
                if ready.pid == credentials.pid {
                    transaction.note_ready(&ready);
                }
            }
            RequestParams::Failed(failed) => {
                if failed.pid == credentials.pid {
                    transaction.note_failed(&failed);
                }
            }
            _ => {
                write_failure(&mut stream, request.id, "busy", "Paper apply is in progress")
                    .await?;
            }
        }
    }
    if let Some(message) = transaction.failed()? {
        return Err(anyhow!(message));
    }
    Ok(())
}

async fn ready_event(
    listener: &UnixListener,
    transaction: &mut ApplyTransaction,
    deadline: Instant,
) -> Result<Option<UnixStream>> {
    tokio::select! {
        biased;
        () = transaction.candidate_exit() => Ok(None),
        () = tokio::time::sleep_until(deadline.into()) => Ok(None),
        accepted = listener.accept() => Ok(Some(accepted?.0)),
    }
}

fn statuses(statuses: Vec<WorkerStatus>) -> Vec<AssignmentStatus> {
    statuses
        .into_iter()
        .map(|status| {
            AssignmentStatus::from_assignment(
                &status.assignment,
                status.generation,
                Some(status.pid),
                true,
            )
        })
        .collect()
}

fn bind(path: &Path) -> Result<StdUnixListener> {
    match StdUnixListener::bind(path) {
        Ok(listener) => finish_bind(listener, path),
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
            if StdUnixStream::connect(path).is_ok() {
                return Err(anyhow!("Paper manager is already running"));
            }
            let metadata = std::fs::symlink_metadata(path)
                .with_context(|| format!("inspect stale Paper socket {}", path.display()))?;
            if !metadata.file_type().is_socket() {
                return Err(anyhow!("refusing to replace non-socket {}", path.display()));
            }
            std::fs::remove_file(path)?;
            finish_bind(StdUnixListener::bind(path)?, path)
        }
        Err(error) => Err(error.into()),
    }
}

fn prepare_parent(path: &Path) -> Result<std::path::PathBuf> {
    let parent = path.parent().ok_or_else(|| anyhow!("Paper socket has no parent"))?;
    if !parent.exists() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create Paper runtime directory {}", parent.display()))?;
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
    }
    let metadata = std::fs::symlink_metadata(parent)
        .with_context(|| format!("inspect Paper runtime directory {}", parent.display()))?;
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return Err(anyhow!("Paper runtime parent must be a directory, not a symlink"));
    }
    if metadata.uid() != effective_uid() {
        return Err(anyhow!("Paper runtime parent must be owned by the current user"));
    }
    if metadata.mode() & 0o077 != 0 {
        return Err(anyhow!(
            "Paper runtime parent permissions must not allow group or other access"
        ));
    }
    Ok(parent.to_path_buf())
}

fn acquire_lock(socket: &Path) -> Result<std::fs::File> {
    let name = socket.file_name().context("Paper socket has no file name")?;
    let path = if name == "paper.sock" {
        socket.with_file_name("manager.lock")
    } else {
        let mut name = name.to_os_string();
        name.push(".manager.lock");
        socket.with_file_name(name)
    };
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&path)
        .with_context(|| format!("open Paper manager lock {}", path.display()))?;
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.uid() != effective_uid()
        || metadata.nlink() != 1
        || metadata.mode() & 0o777 != 0o600
    {
        return Err(anyhow!("Paper manager lock must be a private regular file"));
    }
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if result != 0 {
        return Err(anyhow!("Paper manager is already starting or running"));
    }
    Ok(file)
}

fn finish_bind(listener: StdUnixListener, path: &Path) -> Result<StdUnixListener> {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

async fn read_request(stream: &mut UnixStream) -> Result<Request> {
    let mut line = String::new();
    let mut reader = BufReader::new(&mut *stream).take(MAX_REQUEST + 1);
    tokio::time::timeout(CONNECTION_TIMEOUT, reader.read_line(&mut line))
        .await
        .context("read Paper request")??;
    if line.len() as u64 > MAX_REQUEST {
        return Err(anyhow!("Paper request exceeds {MAX_REQUEST} bytes"));
    }
    decode_ndjson(&line).map_err(|error| anyhow!(error.to_string()))
}

async fn write_response<T: serde::Serialize>(stream: &mut UnixStream, response: &T) -> Result<()> {
    let line = encode_ndjson(response)?;
    tokio::time::timeout(CONNECTION_TIMEOUT, stream.write_all(line.as_bytes()))
        .await
        .context("write Paper response")??;
    Ok(())
}

async fn write_failure(stream: &mut UnixStream, id: u64, code: &str, message: &str) -> Result<()> {
    write_response(stream, &Response::<serde_json::Value>::failure(id, code, message)).await
}

struct PeerCredentials {
    uid: libc::uid_t,
    pid: u32,
}

fn peer_credentials(stream: &UnixStream) -> Result<PeerCredentials> {
    let mut credentials = libc::ucred { pid: 0, uid: 0, gid: 0 };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            std::ptr::from_mut(&mut credentials).cast(),
            &mut length,
        )
    };
    if result != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let pid = u32::try_from(credentials.pid).map_err(|_| anyhow!("Paper peer PID is invalid"))?;
    Ok(PeerCredentials { uid: credentials.uid, pid })
}

fn effective_uid() -> libc::uid_t {
    unsafe { libc::geteuid() }
}

#[cfg(test)]
#[path = "server_tests.rs"]
mod tests;
