#![cfg(test)]

use super::deny_filter;

const X32_GUARD: usize = if cfg!(target_arch = "x86_64") { 2 } else { 0 };

#[test]
fn filter_shape() {
    let program = deny_filter(&[libc::SYS_execve, libc::SYS_ptrace]);
    assert_eq!(program.len(), 4 + X32_GUARD + 2 * 2 + 6);
    assert_eq!(program[0].k, 4);
    assert_eq!(program[3].k, 0);
    assert_eq!(program[4 + X32_GUARD].k, libc::SYS_execve as u32);
    assert_eq!(program[6 + X32_GUARD].k, libc::SYS_ptrace as u32);
    assert_eq!(program[program.len() - 2].k, libc::SECCOMP_RET_ALLOW);
}

#[test]
fn socket_gate_inet() {
    let program = deny_filter(&[]);
    let socket_jump = &program[4 + X32_GUARD];
    assert_eq!(socket_jump.k, libc::SYS_socket as u32);
    assert_eq!(program[6 + X32_GUARD].k, libc::AF_INET as u32);
    assert_eq!(program[7 + X32_GUARD].k, libc::AF_INET6 as u32);
}

#[cfg(target_arch = "x86_64")]
#[test]
fn x32_abi_denied() {
    let program = deny_filter(&[libc::SYS_execve]);
    let guard = &program[4];
    assert_eq!(guard.code, 0x05 | 0x30);
    assert_eq!(guard.k, 0x4000_0000);
    assert_eq!((guard.jt, guard.jf), (0, 1));
    assert_eq!(
        program[5].k,
        libc::SECCOMP_RET_ERRNO | (libc::EPERM as u32 & libc::SECCOMP_RET_DATA)
    );
}
