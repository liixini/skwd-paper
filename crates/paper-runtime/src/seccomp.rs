#![cfg(target_os = "linux")]

use libc::{c_uint, c_ulong, sock_filter, sock_fprog};

const BPF_LD: u16 = 0x00;
const BPF_W: u16 = 0x00;
const BPF_ABS: u16 = 0x20;
const BPF_JMP: u16 = 0x05;
const BPF_JEQ: u16 = 0x10;
const BPF_JGE: u16 = 0x30;
const BPF_K: u16 = 0x00;
const BPF_RET: u16 = 0x06;

const OFF_NR: u32 = 0;
const OFF_ARCH: u32 = 4;
const OFF_ARG0: u32 = 16;

const SECCOMP_FILTER_FLAG_TSYNC: c_ulong = 1;

#[cfg(target_arch = "x86_64")]
const AUDIT_ARCH_NATIVE: u32 = 0xC000_003E;
#[cfg(target_arch = "aarch64")]
const AUDIT_ARCH_NATIVE: u32 = 0xC000_00B7;

fn errno_ret() -> u32 {
    libc::SECCOMP_RET_ERRNO | (libc::EPERM as u32 & libc::SECCOMP_RET_DATA)
}

fn stmt(code: u16, k: u32) -> sock_filter {
    sock_filter { code, jt: 0, jf: 0, k }
}

fn jeq(k: u32, jt: u8, jf: u8) -> sock_filter {
    sock_filter { code: BPF_JMP | BPF_JEQ | BPF_K, jt, jf, k }
}

#[cfg(target_arch = "x86_64")]
fn jge(k: u32, jt: u8, jf: u8) -> sock_filter {
    sock_filter { code: BPF_JMP | BPF_JGE | BPF_K, jt, jf, k }
}

#[cfg(target_arch = "x86_64")]
const X32_SYSCALL_BIT: u32 = 0x4000_0000;

pub fn deny_filter(blocked: &[libc::c_long]) -> Vec<sock_filter> {
    let deny = errno_ret();
    let mut program = vec![
        stmt(BPF_LD | BPF_W | BPF_ABS, OFF_ARCH),
        jeq(AUDIT_ARCH_NATIVE, 1, 0),
        stmt(BPF_RET | BPF_K, deny),
        stmt(BPF_LD | BPF_W | BPF_ABS, OFF_NR),
    ];
    #[cfg(target_arch = "x86_64")]
    {
        program.push(jge(X32_SYSCALL_BIT, 0, 1));
        program.push(stmt(BPF_RET | BPF_K, deny));
    }
    for &number in blocked {
        program.push(jeq(number as u32, 0, 1));
        program.push(stmt(BPF_RET | BPF_K, deny));
    }
    program.push(jeq(libc::SYS_socket as u32, 0, 3));
    program.push(stmt(BPF_LD | BPF_W | BPF_ABS, OFF_ARG0));
    program.push(jeq(libc::AF_INET as u32, 2, 0));
    program.push(jeq(libc::AF_INET6 as u32, 1, 0));
    program.push(stmt(BPF_RET | BPF_K, libc::SECCOMP_RET_ALLOW));
    program.push(stmt(BPF_RET | BPF_K, deny));
    program
}

pub fn apply(filter: &[sock_filter], tsync: bool, nofile_cap: Option<u64>) -> Result<(), String> {
    if unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } != 0 {
        return Err(format!("PR_SET_NO_NEW_PRIVS: {}", std::io::Error::last_os_error()));
    }
    let no_core = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
    if unsafe { libc::setrlimit(libc::RLIMIT_CORE, &no_core) } != 0 {
        return Err(format!("RLIMIT_CORE: {}", std::io::Error::last_os_error()));
    }
    if let Some(cap) = nofile_cap {
        let mut nofile = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
        if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut nofile) } != 0 {
            return Err(format!("get RLIMIT_NOFILE: {}", std::io::Error::last_os_error()));
        }
        let cap = nofile.rlim_max.min(cap);
        nofile.rlim_cur = cap;
        nofile.rlim_max = cap;
        if unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &nofile) } != 0 {
            return Err(format!("RLIMIT_NOFILE: {}", std::io::Error::last_os_error()));
        }
    }
    let program = sock_fprog { len: filter.len() as u16, filter: filter.as_ptr().cast_mut() };
    let flags: c_ulong = if tsync { SECCOMP_FILTER_FLAG_TSYNC } else { 0 };
    let result = unsafe {
        libc::syscall(
            libc::SYS_seccomp,
            libc::SECCOMP_SET_MODE_FILTER as c_uint,
            flags,
            std::ptr::from_ref(&program),
        )
    };
    if result == 0 { Ok(()) } else { Err(format!("seccomp: {}", std::io::Error::last_os_error())) }
}

mod tests;
