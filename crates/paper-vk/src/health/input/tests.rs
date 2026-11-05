use super::*;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = std::env::var_os("TMPDIR").expect("disk scratch required");
        let index = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            PathBuf::from(root).join(format!("health-input-{}-{index}.mp4", std::process::id()));
        let mut file = File::options().create_new(true).write(true).open(&path).unwrap();
        file.write_all(b"original").unwrap();
        Self(path)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_file(&self.0).unwrap();
    }
}

#[test]
fn replacement_keeps_original() {
    let fixture = Fixture::new();
    let input = Input::open(fixture.0.clone()).unwrap();
    std::fs::remove_file(&fixture.0).unwrap();
    std::fs::write(&fixture.0, b"replacement").unwrap();
    let mut actual = String::new();
    File::open(input.decode_path()).unwrap().read_to_string(&mut actual).unwrap();
    assert_eq!(actual, "original");
    assert_eq!(input.logical().extension().unwrap(), "mp4");
}

#[test]
fn mutation_invalidates_admission() {
    let fixture = Fixture::new();
    let input = Input::open(fixture.0.clone()).unwrap();
    assert!(input.unchanged().unwrap());
    std::fs::write(&fixture.0, b"changed length").unwrap();
    assert!(!input.unchanged().unwrap());
}

#[test]
fn duplicate_keeps_file() {
    let fixture = Fixture::new();
    let input = Input::open(fixture.0.clone()).unwrap();
    let mut duplicate = input.duplicate().unwrap();
    drop(input);
    let mut actual = String::new();
    duplicate.read_to_string(&mut actual).unwrap();
    assert_eq!(actual, "original");
}

#[test]
fn clip_wraps_descriptor() {
    let fixture = Fixture::new();
    let clip = Clip { offset: 2, length: 3 };
    let file = File::open(&fixture.0).unwrap();
    let input = Input::new(file, fixture.0.clone(), Some(clip)).unwrap();
    assert_eq!(input.decode_path(), clip.url(&descriptor(&input.file)));
    assert_eq!(input.clip(), Some(clip));
}
