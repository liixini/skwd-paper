use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::{ROTATE_BYTES, ROTATE_GENERATIONS};

pub struct RotatingWriter {
    path: PathBuf,
    lock: File,
}

impl RotatingWriter {
    pub fn new(path: impl Into<PathBuf>) -> io::Result<Self> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut lock_path = path.as_os_str().to_owned();
        lock_path.push(".lock");
        let lock = open_append(Path::new(&lock_path))?;
        Ok(Self { path, lock })
    }

    fn write_locked(&self, mut bytes: &[u8]) -> io::Result<()> {
        while !bytes.is_empty() {
            let mut file = open_append(&self.path)?;
            let length = file.metadata()?.len();
            if length >= ROTATE_BYTES {
                drop(file);
                match std::fs::remove_file(generation(&self.path, ROTATE_GENERATIONS)) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error),
                }
                for index in (1..=ROTATE_GENERATIONS).rev() {
                    let source = if index == 1 {
                        self.path.clone()
                    } else {
                        generation(&self.path, index - 1)
                    };
                    let destination = generation(&self.path, index);
                    match std::fs::rename(source, destination) {
                        Ok(()) => {}
                        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                        Err(error) => return Err(error),
                    }
                }
                continue;
            }
            let count = bytes.len().min((ROTATE_BYTES - length) as usize);
            file.write_all(&bytes[..count])?;
            bytes = &bytes[count..];
        }
        Ok(())
    }
}

impl Write for RotatingWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.lock.lock()?;
        let result = self.write_locked(bytes);
        let unlocked = self.lock.unlock();
        result.and(unlocked).map(|()| bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn generation(path: &Path, index: usize) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".{index}"));
    PathBuf::from(name)
}

fn open_append(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    crate::files::secure_mode(path, 0o600);
    Ok(file)
}

#[cfg(test)]
mod tests;
