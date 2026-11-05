use crate::decode::Clip;
use std::fs::File;
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

const DESCRIPTORS: &str = "/proc/self/fd";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    device: u64,
    inode: u64,
    bytes: u64,
    modified: (i64, i64),
    changed: (i64, i64),
}

impl Identity {
    fn read(file: &File) -> io::Result<Self> {
        let metadata = file.metadata()?;
        Ok(Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            bytes: metadata.len(),
            modified: (metadata.mtime(), metadata.mtime_nsec()),
            changed: (metadata.ctime(), metadata.ctime_nsec()),
        })
    }
}

pub fn descriptor(file: &File) -> String {
    format!("{DESCRIPTORS}/{}", file.as_raw_fd())
}

pub struct Input {
    file: File,
    logical: PathBuf,
    identity: Identity,
    clip: Option<Clip>,
}

impl Input {
    pub fn open(logical: PathBuf) -> io::Result<Self> {
        Self::new(File::open(&logical)?, logical, None)
    }

    pub fn new(file: File, logical: PathBuf, clip: Option<Clip>) -> io::Result<Self> {
        let identity = Identity::read(&file)?;
        Ok(Self { file, logical, identity, clip })
    }

    pub fn identity(&self) -> &Identity {
        &self.identity
    }

    pub fn clip(&self) -> Option<Clip> {
        self.clip
    }

    pub fn logical(&self) -> &Path {
        &self.logical
    }

    pub fn duplicate(&self) -> io::Result<File> {
        self.file.try_clone()
    }

    pub fn unchanged(&self) -> io::Result<bool> {
        Ok(Identity::read(&self.file)? == self.identity)
    }

    pub fn decode_path(&self) -> String {
        let path = descriptor(&self.file);
        match self.clip {
            Some(clip) => clip.url(&path),
            None => path,
        }
    }
}

#[cfg(test)]
mod tests;
