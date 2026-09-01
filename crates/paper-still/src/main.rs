mod app;
mod cli;
mod fill_mode;
mod image_paper;
mod ipc;

fn main() {
    if let Err(err) = app::run() {
        tracing::error!("skwd-wall-still exited with error: {err:?}");
        std::process::exit(1);
    }
}
