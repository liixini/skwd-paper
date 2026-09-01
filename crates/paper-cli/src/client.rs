use anyhow::{Context, Result, anyhow};
use paper_control::{Request, encode_ndjson};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const START_TIMEOUT: Duration = Duration::from_secs(3);
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_RESPONSE: u64 = 1024 * 1024;

pub(crate) fn socket_path() -> PathBuf {
    if let Some(path) = std::env::var_os("SKWD_PAPER_V2_SOCKET") {
        return PathBuf::from(path);
    }
    std::env::var_os("XDG_RUNTIME_DIR").map_or_else(
        || {
            PathBuf::from("/tmp")
                .join(format!("skwd-paper-v2-{}", unsafe { libc::geteuid() }))
                .join("paper.sock")
        },
        |runtime| PathBuf::from(runtime).join("skwd-paper-v2").join("paper.sock"),
    )
}

pub(crate) fn request(request: &Request) -> Result<serde_json::Value> {
    let socket = socket_path();
    let mut stream = if let Ok(stream) = UnixStream::connect(&socket) {
        stream
    } else {
        start_server()?;
        wait_for_server(&socket)?
    };
    stream.set_write_timeout(Some(START_TIMEOUT)).context("set Paper request timeout")?;
    stream.set_read_timeout(Some(RESPONSE_TIMEOUT)).context("set Paper response timeout")?;
    let line = encode_ndjson(request).context("encode Paper request")?;
    stream.write_all(line.as_bytes()).context("write Paper request")?;
    let mut response = String::new();
    BufReader::new(stream)
        .take(MAX_RESPONSE + 1)
        .read_line(&mut response)
        .context("read Paper response")?;
    if response.is_empty() {
        return Err(anyhow!("Paper manager closed without a response"));
    }
    if response.len() as u64 > MAX_RESPONSE {
        return Err(anyhow!("Paper response exceeds {MAX_RESPONSE} bytes"));
    }
    serde_json::from_str(response.trim()).context("decode Paper response")
}

fn start_server() -> Result<()> {
    let executable = std::env::current_exe().context("resolve skwd-paper-v2 executable")?;
    Command::new(executable)
        .arg("serve")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .context("start Paper manager")?;
    Ok(())
}

fn wait_for_server(socket: &std::path::Path) -> Result<UnixStream> {
    let deadline = Instant::now() + START_TIMEOUT;
    loop {
        match UnixStream::connect(socket) {
            Ok(stream) => return Ok(stream),
            Err(error) if Instant::now() < deadline => {
                let _ = error;
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("Paper manager did not bind {}", socket.display()));
            }
        }
    }
}
