use super::{Ahead, STOPPED};
use crate::decode::{AnyDecoder, SwDecoder};
use std::path::{Path, PathBuf};

fn gif(directory: &Path) -> (PathBuf, AnyDecoder) {
    let path = crate::decode::tests::animated_gif(directory);
    let decoder = AnyDecoder::Sw(SwDecoder::open(path.to_str().unwrap()).unwrap());
    (path, decoder)
}

fn handles(path: &Path) -> usize {
    std::fs::read_dir("/proc/self/fd")
        .unwrap()
        .filter_map(|entry| std::fs::read_link(entry.ok()?.path()).ok())
        .filter(|target| target == path)
        .count()
}

#[test]
fn drop_releases_decoder() {
    let directory = tempfile::tempdir().unwrap();
    let (path, decoder) = gif(directory.path());
    let ahead = Ahead::start(decoder).unwrap();
    assert_eq!(handles(&path), 1);
    drop(ahead);
    assert_eq!(handles(&path), 0);
}

#[test]
fn stop_returns_decoder() {
    let directory = tempfile::tempdir().unwrap();
    let (path, decoder) = gif(directory.path());
    let mut ahead = Ahead::start(decoder).unwrap();
    ahead.next().unwrap();
    let decoder = ahead.stop().unwrap();
    drop(ahead);
    assert_eq!(handles(&path), 1);
    drop(decoder);
    assert_eq!(handles(&path), 0);
}

#[test]
fn seek_resumes_decoding() {
    let directory = tempfile::tempdir().unwrap();
    let (_, decoder) = gif(directory.path());
    let mut ahead = Ahead::start(decoder).unwrap();
    let (first, first_pts) = ahead.next().unwrap();
    let (second, _) = ahead.next().unwrap();
    assert_ne!(first.data(0)[..4], second.data(0)[..4]);
    let mut decoder = ahead.stop().unwrap();
    decoder.seek(first_pts).unwrap();
    let mut ahead = Ahead::start(decoder).unwrap();
    let (again, pts) = ahead.next().unwrap();
    assert_eq!((again.data(0)[..4].to_vec(), pts), (first.data(0)[..4].to_vec(), first_pts));
}

#[test]
fn errors_restart_worker() {
    let directory = tempfile::tempdir().unwrap();
    let path = crate::decode::tests::still_named_as_video(directory.path());
    let decoder = AnyDecoder::Sw(SwDecoder::open(path.to_str().unwrap()).unwrap());
    let mut ahead = Ahead::start(decoder).unwrap();
    ahead.next().unwrap();
    for _ in 0..2 {
        let Err(error) = ahead.next() else { panic!("decode error") };
        assert_ne!(error.to_string(), STOPPED);
    }
}
