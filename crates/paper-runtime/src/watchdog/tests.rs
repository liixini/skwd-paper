#![cfg(test)]

use super::{parse_vmrss_kb, read_self_rss_kb, should_restart};

#[test]
fn vmrss_parse() {
    let status = "Name:\tskwd-wall-vk\nVmPeak:\t  500000 kB\nVmHWM:\t  200000 kB\nVmRSS:\t  123456 kB\nThreads:\t4\n";
    assert_eq!(parse_vmrss_kb(status), Some(123456));
}

#[test]
fn vmrss_garbled_none() {
    assert_eq!(parse_vmrss_kb("Name:\tskwd-wall-vk\nThreads:\t4\n"), None);
    assert_eq!(parse_vmrss_kb("VmRSS:\tnot-a-number kB\n"), None);
    assert_eq!(parse_vmrss_kb("VmRSS:\n"), None);
    assert_eq!(parse_vmrss_kb(""), None);
}

#[test]
fn live_proc_parses() {
    let rss = read_self_rss_kb();
    assert!(rss.is_some_and(|kb| kb > 0));
}

#[test]
fn restart_at_limit() {
    assert!(should_restart(512 * 1024, 512));
    assert!(!should_restart(512 * 1024 - 1, 512));
    assert!(should_restart(513 * 1024, 512));
    assert!(!should_restart(0, 512));
}
