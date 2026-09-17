#![cfg(test)]

use super::*;
#[test]
fn acknowledgements_cannot_cross_producer_handoffs() {
    let old = packet(3, 1, 1);
    assert_eq!(acknowledged_slot(&old, 1), Some(1));
    assert_eq!(acknowledged_slot(&old, 2), None);
    assert_eq!(acknowledged_slot(&packet(3, 2, 2), 2), Some(2));
    assert_eq!(acknowledged_slot(&packet(3, 3, 2), 2), None);
    assert_eq!(acknowledged_slot(&old[..7], 1), None);
    assert_eq!(acknowledged_slot(&packet(6, 1, 1), 1), None);
    assert_eq!(acknowledged_slot(&packet(3, 0, 0), 0), Some(0));
}

#[test]
fn ready_packets_advertise_late_acknowledgements() {
    let ready = ready_packet(2);
    assert_eq!(&ready[..8], &packet(6, 0, 2)[..8]);
    assert_eq!(ready[8] & LATE_ACKS, LATE_ACKS);
    assert!(ready[9..].iter().all(|byte| *byte == 0));
    assert_eq!(acknowledged_slot(&ready, 2), None);
}

#[test]
fn ready_fds_accept_one_or_many_sockets() {
    assert_eq!(ready_fds(Some("3")), vec![3]);
    assert_eq!(ready_fds(Some("3,4, 5")), vec![3, 4, 5]);
    assert_eq!(ready_fds(Some("-1,x,7")), vec![7]);
    assert!(ready_fds(None).is_empty());
    assert!(ready_fds(Some("")).is_empty());
}

#[test]
fn frame_pipe_rejects_invalid_descriptors() {
    assert_eq!(frame_pipe(-1).unwrap_err().raw_os_error(), Some(libc::EBADF));
    assert_eq!(frame_pipe(i32::MAX).unwrap_err().raw_os_error(), Some(libc::EBADF));
}

#[test]
fn frame_pipe_does_not_take_ownership_of_the_source() {
    use std::io::{Read, Write};
    use std::os::fd::AsRawFd;

    let (mut writer, mut reader) = std::os::unix::net::UnixStream::pair().unwrap();
    let fd = writer.as_raw_fd();
    let mut first = frame_pipe(fd).unwrap();
    let mut second = frame_pipe(fd).unwrap();
    first.write_all(b"one").unwrap();
    drop(first);
    second.write_all(b"two").unwrap();
    drop(second);
    writer.write_all(b"end").unwrap();
    let mut received = [0; 9];
    reader.read_exact(&mut received).unwrap();
    assert_eq!(&received, b"onetwoend");
}
