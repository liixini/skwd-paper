use std::os::unix::process::CommandExt;

pub fn start(limit_mb: u64, check_interval_secs: u64) {
    let argv: Vec<String> = std::env::args().collect();
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(check_interval_secs));
            let Some(rss_kb) = read_self_rss_kb() else {
                continue;
            };
            if !should_restart(rss_kb, limit_mb) {
                continue;
            }
            tracing::warn!(rss_mb = rss_kb / 1024, limit_mb, "RSS exceeded threshold - re-execing");
            let mut cmd = std::process::Command::new(&exe);
            if argv.len() > 1 {
                cmd.args(&argv[1..]);
            }
            let err = cmd.exec();
            tracing::error!(?err, "exec failed");
            std::process::exit(1);
        }
    });
}

fn should_restart(rss_kb: u64, limit_mb: u64) -> bool {
    rss_kb / 1024 >= limit_mb
}

fn read_self_rss_kb() -> Option<u64> {
    parse_vmrss_kb(&std::fs::read_to_string("/proc/self/status").ok()?)
}

fn parse_vmrss_kb(status: &str) -> Option<u64> {
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            let val = rest.split_whitespace().next()?;
            return val.parse().ok();
        }
    }
    None
}

mod tests;
