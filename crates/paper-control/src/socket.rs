use std::env;
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub fn socket_path() -> PathBuf {
    let runtime_dir = env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(runtime_dir).join("skwd-wall-v2").join("wall.sock")
}

pub fn signal_paper_ready() -> std::io::Result<()> {
    if let Some(path) = env::var_os("SKWD_PAPER_READY_SOCKET") {
        let generation = env::var("SKWD_PAPER_GENERATION")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(0);
        return signal_paper_ready_generation_to(&PathBuf::from(path), generation);
    }
    signal_paper_ready_to(&socket_path())
}

pub fn signal_paper_ready_to(path: &Path) -> std::io::Result<()> {
    let mut stream = UnixStream::connect(path)?;
    stream.set_write_timeout(Some(Duration::from_millis(500)))?;
    write_paper_ready(&mut stream)
}

fn write_paper_ready(mut writer: impl Write) -> std::io::Result<()> {
    let message = format!(
        "{{\"method\":\"paper.ready\",\"params\":{{\"pid\":{}}},\"id\":0}}\n",
        std::process::id()
    );
    writer.write_all(message.as_bytes())
}

pub fn signal_paper_ready_generation_to(path: &Path, generation: u64) -> std::io::Result<()> {
    let mut stream = UnixStream::connect(path)?;
    stream.set_write_timeout(Some(Duration::from_millis(500)))?;
    let message = crate::encode_ndjson(&crate::Request::new(
        0,
        crate::RequestParams::Ready(crate::RendererReady { pid: std::process::id(), generation }),
    ))
    .map_err(std::io::Error::other)?;
    stream.write_all(message.as_bytes())
}

pub fn signal_paper_failed(code: &str, message: &str) -> std::io::Result<()> {
    let Some(path) = env::var_os("SKWD_PAPER_READY_SOCKET") else {
        return Ok(());
    };
    let generation =
        env::var("SKWD_PAPER_GENERATION").ok().and_then(|value| value.parse().ok()).unwrap_or(0);
    signal_paper_failed_generation_to(&PathBuf::from(path), generation, code, message)
}

pub fn signal_paper_failed_generation_to(
    path: &Path,
    generation: u64,
    code: &str,
    message: &str,
) -> std::io::Result<()> {
    let mut stream = UnixStream::connect(path)?;
    stream.set_write_timeout(Some(Duration::from_millis(500)))?;
    let message = crate::encode_ndjson(&crate::Request::new(
        0,
        crate::RequestParams::Failed(crate::RendererFailed {
            pid: std::process::id(),
            generation,
            code: code.chars().take(64).collect(),
            message: message.chars().take(4096).collect(),
        }),
    ))
    .map_err(std::io::Error::other)?;
    stream.write_all(message.as_bytes())
}

#[cfg(test)]
#[path = "socket_tests.rs"]
mod tests;
