mod app;
mod backend;
mod cli;
mod client;
mod manager;
mod server;
mod we_source;

fn main() {
    paper_log::init_tracing("skwd-paper");
    if let Err(error) = app::run() {
        eprintln!("skwd-paper: {error:#}");
        std::process::exit(1);
    }
}
