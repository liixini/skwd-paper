pub fn frame_ready() -> std::io::Result<()> {
    let Some(fd) = std::env::var("SKWD_PAPER_PLASMA_FD")
        .ok()
        .and_then(|value| value.parse::<i32>().ok())
        .filter(|fd| *fd >= 0)
    else {
        return Ok(());
    };
    let mut packet = [0u8; 32];
    packet[..4].copy_from_slice(b"SKDG");
    packet[4] = 6;
    let mut iov = libc::iovec { iov_base: packet.as_mut_ptr().cast(), iov_len: packet.len() };
    let mut message: libc::msghdr = unsafe { std::mem::zeroed() };
    message.msg_iov = &raw mut iov;
    message.msg_iovlen = 1;
    loop {
        let sent = unsafe { libc::sendmsg(fd, &raw const message, libc::MSG_NOSIGNAL) };
        if sent == packet.len() as isize {
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        if sent < 0 && error.kind() == std::io::ErrorKind::Interrupted {
            continue;
        }
        return Err(if sent < 0 { error } else { std::io::ErrorKind::WriteZero.into() });
    }
}
