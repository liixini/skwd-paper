#[cfg(not(target_os = "linux"))]
pub fn restrict_renderer() -> Result<(), String> {
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn arm_reexec() {}

#[cfg(not(target_os = "linux"))]
pub fn reexec(_args: Vec<std::ffi::CString>) -> String {
    String::from("reexec unsupported on this platform")
}

#[cfg(target_os = "linux")]
static REEXEC: std::sync::OnceLock<std::sync::mpsc::SyncSender<Vec<std::ffi::CString>>> =
    std::sync::OnceLock::new();

#[cfg(target_os = "linux")]
pub fn arm_reexec() {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    if REEXEC.set(tx).is_err() {
        return;
    }
    let spawned = std::thread::Builder::new().name("reexec".into()).spawn(move || {
        let Ok(args) = rx.recv() else { return };
        let args: Vec<std::ffi::CString> = args;
        let mut ptrs: Vec<*const libc::c_char> = args.iter().map(|arg| arg.as_ptr()).collect();
        ptrs.push(std::ptr::null());
        unsafe {
            libc::execv(c"/proc/self/exe".as_ptr(), ptrs.as_ptr());
        }
        tracing::error!("skwd-wall-vk: reexec failed: {}", std::io::Error::last_os_error());
        std::process::exit(1);
    });
    if spawned.is_err() {
        tracing::error!("skwd-wall-vk: reexec thread unavailable");
    }
}

#[cfg(target_os = "linux")]
pub fn reexec(args: Vec<std::ffi::CString>) -> String {
    let Some(tx) = REEXEC.get() else {
        return String::from("reexec thread not armed");
    };
    if tx.send(args).is_err() {
        return String::from("reexec thread gone");
    }
    loop {
        std::thread::park();
    }
}

#[cfg(target_os = "linux")]
pub fn restrict_renderer() -> Result<(), String> {
    paper_runtime::seccomp::apply(&paper_runtime::seccomp::deny_filter(BLOCKED), false, None)
}

#[cfg(target_os = "linux")]
const BLOCKED: &[libc::c_long] = &[
    libc::SYS_execve,
    libc::SYS_execveat,
    libc::SYS_ptrace,
    libc::SYS_process_vm_readv,
    libc::SYS_process_vm_writev,
    libc::SYS_unshare,
    libc::SYS_setns,
    libc::SYS_mount,
    libc::SYS_umount2,
    libc::SYS_pivot_root,
    libc::SYS_chroot,
    libc::SYS_bpf,
    libc::SYS_add_key,
    libc::SYS_keyctl,
    libc::SYS_request_key,
    libc::SYS_kexec_load,
    libc::SYS_init_module,
    libc::SYS_finit_module,
    libc::SYS_delete_module,
    libc::SYS_open_by_handle_at,
    libc::SYS_userfaultfd,
    libc::SYS_perf_event_open,
];

#[cfg(all(test, target_os = "linux"))]
mod tests;
