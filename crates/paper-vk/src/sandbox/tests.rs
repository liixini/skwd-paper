#[test]
fn filter_length_bounds() {
    let filter = paper_runtime::seccomp::deny_filter(super::BLOCKED);
    assert!(filter.len() >= 8);
    assert!(filter.len() < u16::MAX as usize);
}

#[test]
fn blocked_deny_set() {
    assert_eq!(super::BLOCKED.len(), 22);
    assert!(super::BLOCKED.contains(&libc::SYS_execve));
    assert!(super::BLOCKED.contains(&libc::SYS_ptrace));
    assert!(super::BLOCKED.contains(&libc::SYS_bpf));
}

#[test]
fn reexec_unarmed() {
    assert_eq!(super::reexec(Vec::new()), "reexec thread not armed");
}
