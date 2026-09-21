use super::*;
use std::io::Write;

fn sparse_package(entry_len: u32, actual_len: u32) -> tempfile::NamedTempFile {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    let mut header = Vec::new();
    header.extend_from_slice(&8u32.to_le_bytes());
    header.extend_from_slice(b"PKGV0007");
    header.extend_from_slice(&1u32.to_le_bytes());
    header.extend_from_slice(&5u32.to_le_bytes());
    header.extend_from_slice(b"large");
    header.extend_from_slice(&0u32.to_le_bytes());
    header.extend_from_slice(&entry_len.to_le_bytes());
    file.write_all(&header).unwrap();
    file.as_file().set_len(header.len() as u64 + u64::from(actual_len)).unwrap();
    file
}

#[test]
fn mapped_entry_above_old_limit() {
    let size = 594_971_164;
    let file = sparse_package(size, size);
    let package = Package::open(file.path()).unwrap();
    let entry = package.find("large").unwrap();
    assert_eq!(entry.len(), size as usize);
    assert_eq!(entry[0], 0);
    assert_eq!(entry[entry.len() - 1], 0);
}

#[test]
fn oversized_entry_must_fit_actual_file() {
    let file = sparse_package(594_971_164, 4);
    let error = Package::open(file.path()).err().unwrap();
    assert!(format!("{error:#}").contains("entry past end of file: large"));
}

#[test]
fn package_size_guard_remains() {
    let file = sparse_package(1, 1);
    file.as_file().set_len(MAX_PACKAGE_BYTES + 1).unwrap();
    let error = Package::open(file.path()).err().unwrap();
    assert!(format!("{error:#}").contains("package is"));
}

#[test]
fn scene_entry_uses_standard_names_in_order() {
    for files in [
        vec![("scene.json", &b"{\"selected\":1}"[..])],
        vec![("gifscene.json", &b"{\"selected\":1}"[..])],
        vec![("gifscene.json", &b"{}"[..]), ("scene.json", &b"{\"selected\":1}"[..])],
    ] {
        let package = Package::parse(crate::tests::build_pkg(&files)).unwrap();
        assert_eq!(package.scene_json().unwrap()["selected"], 1);
    }
}

#[test]
fn missing_scene_does_not_select_other_json() {
    let package = Package::parse(crate::tests::build_pkg(&[("materials/a.json", b"{}")])).unwrap();
    let error = package.scene_json().unwrap_err().to_string();
    assert!(error.contains("scene.json or gifscene.json"));
}

#[test]
fn malformed_standard_scene_does_not_fall_back() {
    let package =
        Package::parse(crate::tests::build_pkg(&[("scene.json", b"{"), ("gifscene.json", b"{}")]))
            .unwrap();
    assert!(package.scene_json().unwrap_err().to_string().contains("parse json entry scene.json"));
}

#[test]
fn declared_scene_errors_do_not_fall_back_or_read_external_files() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scene.pkg");
    std::fs::write(dir.path().join("external.json"), b"{}").unwrap();
    std::fs::write(&path, crate::tests::build_pkg(&[("scene.json", b"{}"), ("broken.json", b"{")]))
        .unwrap();
    for (file, expected) in [
        ("missing.json", "is missing from package"),
        ("external.json", "is missing from package"),
        ("broken.json", "parse json entry broken.json"),
    ] {
        std::fs::write(
            dir.path().join("project.json"),
            serde_json::json!({"file": file}).to_string(),
        )
        .unwrap();
        let package = Package::open(&path).unwrap();
        let error = package.scene_json().unwrap_err().to_string();
        assert!(error.contains(expected), "{error}");
        assert!(error.contains(file), "{error}");
    }
}
