pub fn stream_epoch() -> u16 {
    static EPOCH: std::sync::OnceLock<u16> = std::sync::OnceLock::new();
    *EPOCH.get_or_init(|| {
        std::env::var("SKWD_PAPER_STREAM_EPOCH")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(0)
    })
}

pub fn packet(kind: u8, slot: u8, epoch: u16) -> [u8; 32] {
    let mut packet = [0u8; 32];
    packet[..4].copy_from_slice(b"SKDG");
    packet[4] = kind;
    packet[5] = slot;
    packet[6..8].copy_from_slice(&epoch.to_le_bytes());
    packet
}

pub fn acknowledged_slot(bytes: &[u8], epoch: u16) -> Option<usize> {
    (bytes.len() == 32
        && bytes[..4] == *b"SKDG"
        && bytes[4] == 3
        && bytes[5] < 3
        && bytes[6..8] == epoch.to_le_bytes())
    .then(|| bytes[5] as usize)
}

fn send(fd: i32, mut packet: [u8; 32]) -> std::io::Result<()> {
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

pub fn begin_stream(fd: i32, epoch: u16) -> std::io::Result<()> {
    send(fd, packet(7, 0, epoch))?;
    let started = std::time::Instant::now();
    loop {
        if started.elapsed() >= std::time::Duration::from_secs(10) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "Plasma stream handoff was not acknowledged",
            ));
        }
        let mut event = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
        let ready = unsafe { libc::poll(&raw mut event, 1, 100) };
        if ready < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if ready == 0 {
            continue;
        }
        let mut response = [0u8; 32];
        let len = unsafe { libc::recv(fd, response.as_mut_ptr().cast(), response.len(), 0) };
        if len <= 0 {
            return Err(std::io::ErrorKind::BrokenPipe.into());
        }
        if len == 32 && response == packet(8, 0, epoch) {
            return Ok(());
        }
    }
}

pub fn frame_ready() -> std::io::Result<()> {
    for fd in ready_fds(std::env::var("SKWD_PAPER_PLASMA_FD").ok().as_deref()) {
        frame_ready_on(fd)?;
    }
    Ok(())
}

pub const LATE_ACKS: u8 = 1;

pub fn ready_packet(epoch: u16) -> [u8; 32] {
    let mut ready = packet(6, 0, epoch);
    ready[8] = LATE_ACKS;
    ready
}

pub fn frame_ready_on(fd: i32) -> std::io::Result<()> {
    send(fd, ready_packet(stream_epoch()))
}

pub fn ready_fds(value: Option<&str>) -> Vec<i32> {
    value
        .unwrap_or_default()
        .split(',')
        .filter_map(|item| item.trim().parse::<i32>().ok())
        .filter(|fd| *fd >= 0)
        .collect()
}

pub fn frame_pipe(fd: i32) -> std::io::Result<std::fs::File> {
    use std::os::fd::FromRawFd;

    let duplicate = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 0) };
    if duplicate < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(unsafe { std::fs::File::from_raw_fd(duplicate) })
}

mod tests;
