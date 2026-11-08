use super::*;
use std::io::{Read, Seek};
use std::os::unix::net::UnixDatagram;

fn place() -> (Face, Place) {
    let scene = super::super::tests::scene();
    let widget = &scene.widgets[0];
    let place = paper_widgets::place(
        [640, 360],
        [320, 180],
        paper_widgets::frame(widget.face),
        widget.pin,
        widget.height,
    )
    .unwrap();
    (widget.face, place)
}

fn painted((face, place): (Face, Place), extent: [u32; 2]) -> (Snapshot, Vec<u8>) {
    let widget = super::super::tests::scene();
    let lettering = paper_widgets::Lettering::open(paper_widgets::FONT.to_vec()).unwrap();
    let scene = Scene {
        lettering: &lettering,
        tones: paper_widgets::Tones::over(&widget.palette, 0.0),
        moment: paper_widgets::Moment::civil(10, 24, 3, 18, 9),
        labels: &widget.labels,
    };
    let pixels = scene.paint(face, place, None).pixels.as_flattened().to_vec();
    (Snapshot::paint(&scene, &[(face, place)], extent).unwrap().unwrap(), pixels)
}

#[test]
fn preserves_canvas_bytes() {
    let (mut snapshot, pixels) = painted(place(), [320, 180]);
    let (_, place) = place();
    snapshot.file.rewind().unwrap();
    let mut bytes = Vec::new();
    snapshot.file.read_to_end(&mut bytes).unwrap();
    let mut expected = Vec::new();
    for value in place.origin.into_iter().chain(place.size.map(|side| side as u32)) {
        expected.extend_from_slice(&value.to_le_bytes());
    }
    expected.extend_from_slice(&24u64.to_le_bytes());
    expected.extend_from_slice(&pixels);
    assert_eq!(bytes, expected);
    assert_eq!(unsafe { libc::fcntl(snapshot.file.as_raw_fd(), libc::F_GET_SEALS) }, SEALS);
    assert!(snapshot.file.set_len(0).is_err());
    assert!(snapshot.file.write_all(&[0]).is_err());
}

#[test]
fn preflights_snapshot_budget() {
    let (face, mut place) = place();
    place.size = [1, (LIMIT - ENTRY_BYTES) / CHANNELS];
    assert_eq!(snapshot_size(&[(face, place)]), Some(LIMIT));
    place.size[1] += 1;
    assert_eq!(snapshot_size(&[(face, place)]), None);
    place.size = [usize::MAX, usize::MAX];
    assert_eq!(snapshot_size(&[(face, place)]), None);
    assert_eq!(snapshot_size(&[]), Some(0));
}

#[test]
fn gates_exact_descriptors() {
    for (fd, list, expected) in [
        (3, Some("3,5,7"), true),
        (4, Some("3,5,7"), false),
        (5, Some("3,5,7"), true),
        (7, Some("3,5,7"), true),
        (3, None, false),
        (-1, Some("-1"), false),
    ] {
        assert_eq!(enabled(fd, list), expected);
    }
}

#[test]
fn clears_every_slot() {
    let (sender, receiver) = socket_pair();
    receiver.set_nonblocking(true).unwrap();
    let target = StreamTarget::single(sender.as_raw_fd(), 320, 180, 30, false);
    let mut snapshots = Snapshots::default();
    snapshots.replace(None);
    for slot in 0..SLOT_COUNT {
        snapshots.publish(&target, slot).unwrap();
        let mut bytes = [0; 32];
        assert_eq!(receiver.recv(&mut bytes).unwrap(), bytes.len());
        assert_eq!(bytes[4], 9);
        assert_eq!(bytes[5], slot as u8);
        assert_eq!(&bytes[8..12], &1u32.to_le_bytes());
        assert_eq!(&bytes[12..16], &[0; 4]);
        assert_eq!(&bytes[16..20], &320u32.to_le_bytes());
        assert_eq!(&bytes[20..24], &180u32.to_le_bytes());
        assert_eq!(&bytes[24..32], &1u64.to_le_bytes());
        snapshots.publish(&target, slot).unwrap();
        assert_eq!(receiver.recv(&mut bytes).unwrap_err().kind(), io::ErrorKind::WouldBlock);
    }
    snapshots.replace(None);
    for slot in 0..SLOT_COUNT {
        snapshots.publish(&target, slot).unwrap();
        let mut bytes = [0; 32];
        receiver.recv(&mut bytes).unwrap();
        assert_eq!(&bytes[24..32], &2u64.to_le_bytes());
    }
}

fn receive(receiver: &UnixDatagram) -> ([u8; 32], File) {
    let mut bytes = [0; 32];
    let mut control = [0usize; 8];
    let mut iov = libc::iovec { iov_base: bytes.as_mut_ptr().cast(), iov_len: bytes.len() };
    let mut message: libc::msghdr = unsafe { std::mem::zeroed() };
    message.msg_iov = &raw mut iov;
    message.msg_iovlen = 1;
    message.msg_control = control.as_mut_ptr().cast();
    message.msg_controllen = std::mem::size_of_val(&control);
    let size =
        unsafe { libc::recvmsg(receiver.as_raw_fd(), &raw mut message, libc::MSG_CMSG_CLOEXEC) };
    assert_eq!(size, bytes.len() as isize);
    assert_eq!(message.msg_flags & (libc::MSG_TRUNC | libc::MSG_CTRUNC), 0);
    let header = unsafe { libc::CMSG_FIRSTHDR(&raw const message) };
    assert!(!header.is_null());
    unsafe {
        assert_eq!((*header).cmsg_level, libc::SOL_SOCKET);
        assert_eq!((*header).cmsg_type, libc::SCM_RIGHTS);
        assert_eq!(
            (*header).cmsg_len,
            libc::CMSG_LEN(std::mem::size_of::<RawFd>() as u32) as usize
        );
        let fd = std::ptr::read_unaligned(libc::CMSG_DATA(header).cast::<RawFd>());
        (bytes, File::from_raw_fd(fd))
    }
}

fn socket_pair() -> (UnixDatagram, UnixDatagram) {
    let mut pair = [-1; 2];
    assert_eq!(
        unsafe {
            libc::socketpair(
                libc::AF_UNIX,
                libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC,
                0,
                pair.as_mut_ptr(),
            )
        },
        0
    );
    unsafe { (UnixDatagram::from_raw_fd(pair[0]), UnixDatagram::from_raw_fd(pair[1])) }
}

#[test]
fn shares_sealed_backing() {
    use std::os::unix::fs::MetadataExt;

    let (sender, receiver) = socket_pair();
    let target = StreamTarget::single(sender.as_raw_fd(), 320, 180, 30, false);
    let (snapshot, pixels) = painted(place(), [320, 180]);
    let inode = snapshot.file.metadata().unwrap().ino();
    let mut snapshots = Snapshots::default();
    snapshots.replace(Some(snapshot));
    for slot in 0..SLOT_COUNT {
        snapshots.publish(&target, slot).unwrap();
        let (bytes, mut file) = receive(&receiver);
        assert_eq!(&bytes[12..16], &1u32.to_le_bytes());
        assert_eq!(file.metadata().unwrap().ino(), inode);
        assert_eq!(unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GET_SEALS) }, SEALS);
        file.seek(std::io::SeekFrom::Start(24)).unwrap();
        let mut received = Vec::new();
        file.read_to_end(&mut received).unwrap();
        assert_eq!(received, pixels);
    }
}

#[test]
fn clips_scaled_edges() {
    let extent = [1800, 1012];
    let face = place().0;
    let place = paper_widgets::place(
        [1920, 1080],
        extent,
        paper_widgets::frame(face),
        paper_control::Pin { anchor: [1.0; 2], offset: [0.0; 2] },
        Some(0.5),
    )
    .unwrap();
    assert!(place.origin[1] + place.size[1] as u32 > extent[1]);
    let (mut snapshot, original) = painted((face, place), extent);
    snapshot.file.rewind().unwrap();
    let mut bytes = Vec::new();
    snapshot.file.read_to_end(&mut bytes).unwrap();
    let height = extent[1] - place.origin[1];
    assert_eq!(&bytes[12..16], &height.to_le_bytes());
    let len = place.size[0] * height as usize * CHANNELS;
    assert_eq!(&bytes[ENTRY_BYTES..], &original[..len]);
}
