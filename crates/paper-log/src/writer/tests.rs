use super::*;

struct Directory(PathBuf);
impl Directory {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!("skwd-log-{label}-{}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn live_rotation_bounds_large_writes_and_prunes_old_generations() {
    let dir = Directory::new("rotation");
    let path = dir.0.join("app.log");
    File::create(&path).unwrap().set_len(256 * 1024 * 1024).unwrap();
    let mut writer = RotatingWriter::new(&path).unwrap();
    for byte in b'0'..=b'5' {
        writer.write_all(&vec![byte; ROTATE_BYTES as usize]).unwrap();
    }
    writer.write_all(b"final diagnostic").unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"final diagnostic");
    for index in 1..=ROTATE_GENERATIONS {
        let bytes = std::fs::read(generation(&path, index)).unwrap();
        assert_eq!(bytes.len(), ROTATE_BYTES as usize);
        assert!(bytes.iter().all(|byte| *byte == b'6' - index as u8));
    }
    assert!(!generation(&path, ROTATE_GENERATIONS + 1).exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for entry in std::fs::read_dir(&dir.0).unwrap() {
            assert_eq!(entry.unwrap().metadata().unwrap().permissions().mode() & 0o777, 0o600);
        }
    }
}

#[test]
fn independent_writers_follow_rotations_without_losing_records() {
    let dir = Directory::new("concurrent");
    let path = dir.0.join("shared.log");
    std::thread::scope(|scope| {
        for lane in 0..4 {
            let path = &path;
            scope.spawn(move || {
                let mut writer = RotatingWriter::new(path).unwrap();
                let record = vec![b'a' + lane; 4096];
                for _ in 0..512 {
                    writer.write_all(&record).unwrap();
                }
            });
        }
    });
    let mut counts = [0usize; 4];
    for file in [&path, &generation(&path, 1)] {
        let bytes = std::fs::read(file).unwrap();
        assert!(bytes.len() <= ROTATE_BYTES as usize);
        for byte in bytes {
            counts[(byte - b'a') as usize] += 1;
        }
    }
    assert_eq!(counts, [512 * 4096; 4]);
}

#[test]
fn rotation_failure_does_not_append_past_limit() {
    let dir = Directory::new("failure");
    let path = dir.0.join("app.log");
    File::create(&path).unwrap().set_len(ROTATE_BYTES).unwrap();
    std::fs::create_dir(generation(&path, ROTATE_GENERATIONS)).unwrap();
    let mut writer = RotatingWriter::new(&path).unwrap();
    assert!(writer.write_all(b"cannot rotate").is_err());
    assert_eq!(std::fs::metadata(path).unwrap().len(), ROTATE_BYTES);
}
