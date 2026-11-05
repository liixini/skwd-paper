use anyhow::{Result, ensure};
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

thread_local! {
    static CAPTURE: RefCell<Option<Identity>> = const { RefCell::new(None) };
}

#[derive(Default)]
pub struct Identity {
    inputs: BTreeMap<Input, [u8; 32]>,
    changed: bool,
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum Input {
    File(PathBuf),
    Project(PathBuf),
    Declaration(PathBuf),
}

pub struct Capture;

impl Capture {
    pub fn start() -> Result<Self> {
        CAPTURE.with_borrow_mut(|capture| {
            ensure!(capture.is_none(), "scene source capture nested");
            *capture = Some(Identity::default());
            Ok(Self)
        })
    }

    pub fn finish(self) -> Result<Identity> {
        let identity = CAPTURE.with_borrow_mut(Option::take).expect("scene source capture");
        ensure!(!identity.changed, "scene source changed during load");
        for (input, hash) in &identity.inputs {
            if let Input::File(path) = input {
                ensure!(hash_file(path)? == *hash, "scene source changed during load");
            }
        }
        Ok(identity)
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        CAPTURE.with_borrow_mut(Option::take);
    }
}

impl Identity {
    pub fn matches(&self, other: &Self) -> bool {
        !self.changed && !other.changed && self.inputs == other.inputs
    }

    fn insert(&mut self, input: Input, hash: [u8; 32]) {
        if let Some(previous) = self.inputs.insert(input, hash) {
            self.changed |= previous != hash;
        }
    }
}

fn record(input: Input, bytes: &[u8]) {
    CAPTURE.with_borrow_mut(|capture| {
        if let Some(identity) = capture {
            identity.insert(input, Sha256::digest(bytes).into());
        }
    });
}

pub(crate) fn file(path: &Path, bytes: &[u8]) {
    if CAPTURE.with_borrow(Option::is_none) {
        return;
    }
    record(Input::File(path.to_path_buf()), bytes);
}

pub(crate) fn project(path: &Path, value: &serde_json::Value) {
    if CAPTURE.with_borrow(Option::is_none) {
        return;
    }
    record(Input::Project(path.to_path_buf()), &serde_json::to_vec(value).expect("project JSON"));
}

pub(crate) fn declaration(path: &Path, value: &serde_json::Value) {
    if CAPTURE.with_borrow(Option::is_none) {
        return;
    }
    record(
        Input::Declaration(path.to_path_buf()),
        &serde_json::to_vec(value).expect("project JSON"),
    );
}

pub(crate) fn read(path: &Path) -> std::io::Result<Vec<u8>> {
    let bytes = std::fs::read(path)?;
    file(path, &bytes);
    Ok(bytes)
}

#[cfg(test)]
#[path = "source_tests.rs"]
mod tests;

fn hash_file(path: &Path) -> std::io::Result<[u8; 32]> {
    const CHUNK_BYTES: usize = 64 * 1024;
    let mut file = std::fs::File::open(path)?;
    let mut buffer = vec![0; CHUNK_BYTES];
    let mut hash = Sha256::new();
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            return Ok(hash.finalize().into());
        }
        hash.update(&buffer[..read]);
    }
}

pub(crate) fn project_files(directories: &[PathBuf]) -> std::io::Result<()> {
    const PROJECT_FILE: &str = "project.json";
    if !CAPTURE.with_borrow(Option::is_some) {
        return Ok(());
    }
    for directory in directories {
        let path = directory.join(PROJECT_FILE);
        let hash = hash_file(&path)?;
        CAPTURE.with_borrow_mut(|capture| {
            capture.as_mut().expect("scene source capture").insert(Input::File(path), hash);
        });
    }
    Ok(())
}
