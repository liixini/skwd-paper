use super::{read, rgb};

#[test]
fn theme_role_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("colors.json");
    std::fs::write(&path, r##"{"surface":"#001020","surfaceVariant":"#304050","primaryContainer":"#607080","secondaryContainer":"#90a0b0"}"##).unwrap();
    let colors = read(&path).unwrap();
    assert_eq!(colors[0], [0.0, 16.0 / 255.0, 32.0 / 255.0, 1.0]);
    assert_eq!(colors[3], [144.0 / 255.0, 160.0 / 255.0, 176.0 / 255.0, 1.0]);
    std::fs::write(&path, r##"{"surface":"#001020"}"##).unwrap();
    assert!(read(&path).is_none());
}

#[test]
fn rejects_invalid_colors() {
    for color in ["#abc", "#abcdef00", "abcdef", "#xxcdef", "#ébcde"] {
        assert!(rgb(color).is_none());
    }
    assert_eq!(rgb("#FFFFFF"), Some([1.0; 4]));
}
