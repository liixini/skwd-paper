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
