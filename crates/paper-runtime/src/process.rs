#[cfg(target_env = "gnu")]
const MALLOC_ARENA_MAX: libc::c_int = 2;
#[cfg(target_env = "gnu")]
const MALLOC_MMAP_THRESHOLD: libc::c_int = 1024 * 1024;

pub fn init_process() {
    #[cfg(target_env = "gnu")]
    unsafe {
        libc::mallopt(libc::M_ARENA_MAX, MALLOC_ARENA_MAX)
    };
    #[cfg(target_env = "gnu")]
    unsafe {
        libc::mallopt(libc::M_MMAP_THRESHOLD, MALLOC_MMAP_THRESHOLD)
    };
    paper_log::init_tracing("skwd-paper");
    if let Some(limit_mb) =
        std::env::var("SKWD_PAPER_RSS_LIMIT_MB").ok().and_then(|text| text.parse::<u64>().ok())
    {
        crate::watchdog::start(limit_mb, 30);
        tracing::info!(limit_mb, "rss watchdog enabled");
    }
}
