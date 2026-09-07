use std::io::IsTerminal;
use tracing_subscriber::{EnvFilter, Layer, fmt, layer::SubscriberExt, util::SubscriberInitExt};

fn env_filter() -> EnvFilter {
    EnvFilter::try_from_env("SKWD_WALL_LOG")
        .or_else(|_| EnvFilter::try_from_default_env())
        .unwrap_or_else(|_| EnvFilter::new("info"))
}

fn init_stderr_only() {
    tracing_subscriber::registry()
        .with(
            fmt::layer()
                .with_writer(std::io::stderr)
                .with_ansi(std::io::stderr().is_terminal())
                .with_filter(env_filter()),
        )
        .init();
}

pub fn init_tracing(app: &str) {
    let Some(path) = super::log_path(app) else {
        init_stderr_only();
        return;
    };
    let Ok(file_appender) = crate::RotatingWriter::new(path) else {
        init_stderr_only();
        return;
    };
    let (file_writer, file_guard) = tracing_appender::non_blocking::NonBlockingBuilder::default()
        .buffered_lines_limit(256)
        .lossy(true)
        .finish(file_appender);
    Box::leak(Box::new(file_guard));

    tracing_subscriber::registry()
        .with(
            fmt::layer()
                .with_writer(std::io::stderr)
                .with_ansi(std::io::stderr().is_terminal())
                .with_filter(env_filter()),
        )
        .with(fmt::layer().with_writer(file_writer).with_ansi(false).with_filter(env_filter()))
        .init();
}
