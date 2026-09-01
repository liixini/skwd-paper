use std::io::Write;

pub fn signal_ready() {
    write_ready(std::io::stdout().lock());
    if let Err(err) = paper_control::signal_paper_ready() {
        tracing::debug!(error = %err, "ipc: paper.ready send failed");
    }
}

fn write_ready(mut out: impl Write) {
    let _ = out.write_all(b"READY\n");
    let _ = out.flush();
}

#[cfg(test)]
pub(crate) fn signal_ready_to(path: &std::path::Path) {
    if let Err(err) = paper_control::signal_paper_ready_to(path) {
        tracing::debug!(error = %err, path = %path.display(), "ipc: paper.ready send failed");
    }
}

mod tests;
