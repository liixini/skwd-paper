#![cfg(test)]

use super::*;

#[test]
fn log_path_order() {
    let xdg = Some(PathBuf::from("/xdg"));
    let home = Some(PathBuf::from("/home/u"));
    let lad = Some(PathBuf::from("/lad"));
    assert_eq!(
        log_path_from("skwd-wall", xdg, home.clone(), lad.clone()),
        Some(PathBuf::from("/xdg/skwd-wall-v2/skwd-wall.log"))
    );
    assert_eq!(
        log_path_from("skwd-walld", None, home, lad.clone()),
        Some(PathBuf::from("/home/u/.cache/skwd-wall-v2/skwd-walld.log"))
    );
    assert_eq!(
        log_path_from("skwd-paper", None, None, lad),
        Some(PathBuf::from("/lad/skwd-wall-v2/skwd-paper.log"))
    );
    assert_eq!(log_path_from("skwd-wall", None, None, None), None);
}

#[test]
fn rotate_threshold() {
    assert!(!should_rotate(0, ROTATE_BYTES));
    assert!(!should_rotate(ROTATE_BYTES - 1, ROTATE_BYTES));
    assert!(should_rotate(ROTATE_BYTES, ROTATE_BYTES));
}

#[test]
fn rotate_oversized() {
    let dir = std::env::temp_dir().join(format!("paper-log-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let big = dir.join("skwd-walld.log");
    std::fs::write(&big, vec![b'x'; 64]).unwrap();
    rotate_if_large(&big, 32);
    assert!(!big.exists());
    assert!(dir.join("skwd-walld.log.1").exists());
    let small = dir.join("small.log");
    std::fs::write(&small, vec![b'x'; 8]).unwrap();
    rotate_if_large(&small, 32);
    assert!(small.exists());
    rotate_if_large(&dir.join("nope.log"), 32);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rotate_prunes_generations() {
    let dir = std::env::temp_dir().join(format!("paper-log-gen-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let log = dir.join("app.log");
    let rotations = ROTATE_GENERATIONS + 2;
    for idx in 0..rotations {
        let mut data = format!("g{idx}").into_bytes();
        data.resize(64, b'x');
        std::fs::write(&log, &data).unwrap();
        rotate_if_large(&log, 32);
    }
    for index in 1..=ROTATE_GENERATIONS {
        assert!(dir.join(format!("app.log.{index}")).exists(), "gen {index} missing");
    }
    assert!(!dir.join(format!("app.log.{}", ROTATE_GENERATIONS + 1)).exists());
    let newest = std::fs::read(dir.join("app.log.1")).unwrap();
    assert!(newest.starts_with(format!("g{}", rotations - 1).as_bytes()));
    let _ = std::fs::remove_dir_all(&dir);
}
