mod app;
mod ctl;
mod decode;
mod dmabuf;
#[cfg(feature = "shared-device")]
mod ffmpeg_vulkan;
mod fill;
mod freeze;
mod memory;
#[cfg(feature = "shared-device")]
mod preview;
#[cfg(feature = "shared-device")]
mod probe;
mod sandbox;
#[cfg(feature = "shared-device")]
mod shared;
mod surface;
mod timing;
mod vk;
mod wayland;

pub(crate) use fill::fill_flag;

fn version_requested(arguments: &[String]) -> bool {
    arguments.get(1).is_some_and(|argument| argument == "--version" || argument == "-V")
}

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    if version_requested(&arguments) {
        println!("skwd-wall-vk {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    paper_log::init_tracing("skwd-wall-vk");
    sandbox::arm_reexec();
    if let Err(err) = sandbox::restrict_renderer() {
        app::signal_startup_failure(&format!("renderer sandbox unavailable: {err}"));
        tracing::error!("renderer sandbox unavailable: {err}");
        std::process::exit(1);
    }
    if let Err(err) = app::run() {
        app::signal_startup_failure(&format!("{err:#}"));
        tracing::error!("skwd-wall-vk exited with error: {err:?}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod main_tests {
    use super::version_requested;

    #[test]
    fn version_path_is_explicit_and_precedes_renderer_startup() {
        assert!(version_requested(&["skwd-wall-vk".into(), "--version".into()]));
        assert!(version_requested(&["skwd-wall-vk".into(), "-V".into()]));
        assert!(!version_requested(&["skwd-wall-vk".into(), "DP-1".into(), "--version".into(),]));
    }
}
