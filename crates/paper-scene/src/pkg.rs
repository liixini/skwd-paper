use crate::read::Reader;
use anyhow::{Context, Result, anyhow};
use std::collections::HashMap;

pub const MAX_PACKAGE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
pub const MAX_PACKAGE_ENTRIES: usize = 32_768;
pub const MAX_PACKAGE_ENTRY_BYTES: usize = MAX_PACKAGE_BYTES as usize;

#[derive(Clone)]
pub struct Package {
    map: std::sync::Arc<Mapping>,
    version: String,
    entries: std::sync::Arc<[Entry]>,
    index: std::sync::Arc<HashMap<String, usize>>,
    folded: std::sync::Arc<std::sync::OnceLock<HashMap<String, usize>>>,
    data_start: usize,
    scene_file: Option<String>,
    path: Option<std::path::PathBuf>,
}

pub struct Entry {
    pub path: String,
    pub offset: usize,
    pub len: usize,
}

enum Mapping {
    Owned(Vec<u8>),
    Mapped { ptr: *mut libc::c_void, len: usize, file: std::sync::Arc<std::fs::File> },
}

unsafe impl Send for Mapping {}
unsafe impl Sync for Mapping {}

impl Mapping {
    fn bytes(&self) -> &[u8] {
        match self {
            Self::Owned(data) => data,
            Self::Mapped { ptr, len, .. } => unsafe {
                std::slice::from_raw_parts(ptr.cast::<u8>(), *len)
            },
        }
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        if let Self::Mapped { ptr, len, .. } = self {
            unsafe { libc::munmap(*ptr, *len) };
        }
    }
}

fn map_file(path: &std::path::Path) -> Result<Mapping> {
    use std::os::fd::AsRawFd;
    let file = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let len = file.metadata()?.len();
    if len == 0 {
        return Err(anyhow!("empty file"));
    }
    if len > MAX_PACKAGE_BYTES {
        return Err(anyhow!("package is {len} bytes; limit is {MAX_PACKAGE_BYTES}"));
    }
    let len = usize::try_from(len).map_err(|_| anyhow!("file too large"))?;
    let ptr = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            len,
            libc::PROT_READ,
            libc::MAP_PRIVATE,
            file.as_raw_fd(),
            0,
        )
    };
    if ptr == libc::MAP_FAILED {
        return Err(anyhow!("mmap failed: {}", std::io::Error::last_os_error()));
    }
    Ok(Mapping::Mapped { ptr, len, file: std::sync::Arc::new(file) })
}

impl Package {
    pub fn open(path: &std::path::Path) -> Result<Self> {
        let map = map_file(path)?;
        crate::source::file(path, map.bytes());
        let mut package = Self::build(map).with_context(|| format!("parse {}", path.display()))?;
        package.scene_file = path
            .parent()
            .and_then(|dir| crate::source::read(&dir.join("project.json")).ok())
            .and_then(|bytes| crate::json::parse(&bytes).ok())
            .and_then(|project| project.get("file")?.as_str().map(str::to_owned))
            .filter(|file| !file.is_empty());
        package.path = Some(path.to_path_buf());
        Ok(package)
    }

    pub fn parse(data: Vec<u8>) -> Result<Self> {
        if data.len() as u64 > MAX_PACKAGE_BYTES {
            return Err(anyhow!("package is {} bytes; limit is {MAX_PACKAGE_BYTES}", data.len()));
        }
        Self::build(Mapping::Owned(data))
    }

    fn build(map: Mapping) -> Result<Self> {
        let (version, entries, data_start) = {
            let data = map.bytes();
            let mut reader = Reader::new(data);
            let version = reader.len_string(32)?;
            if !version.starts_with("PKGV") {
                return Err(anyhow!("bad pkg magic {version:?}"));
            }
            let count = reader.u32()? as usize;
            if count > MAX_PACKAGE_ENTRIES {
                return Err(anyhow!("implausible entry count {count}"));
            }
            let mut entries = Vec::with_capacity(count.min(4096));
            for _ in 0..count {
                let path = reader.len_string(4096)?;
                let offset = reader.u32()? as usize;
                let len = reader.u32()? as usize;
                entries.push(Entry { path, offset, len });
            }
            let data_start = reader.pos();
            for entry in &entries {
                let end = entry
                    .offset
                    .checked_add(entry.len)
                    .and_then(|end| end.checked_add(data_start))
                    .ok_or_else(|| anyhow!("entry range overflow: {}", entry.path))?;
                if end > data.len() {
                    return Err(anyhow!("entry past end of file: {}", entry.path));
                }
            }
            (version, entries, data_start)
        };
        let index =
            entries.iter().enumerate().map(|(idx, entry)| (entry.path.clone(), idx)).collect();
        Ok(Self {
            map: std::sync::Arc::new(map),
            version,
            entries: entries.into(),
            index: std::sync::Arc::new(index),
            folded: std::sync::Arc::default(),
            data_start,
            scene_file: None,
            path: None,
        })
    }

    pub fn source_path(&self) -> Option<&std::path::Path> {
        self.path.as_deref()
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn read(&self, entry: &Entry) -> &[u8] {
        let start = self.data_start + entry.offset;
        &self.map.bytes()[start..start + entry.len]
    }

    fn position(&self, path: &str) -> Option<usize> {
        if let Some(idx) = self.index.get(path) {
            return Some(*idx);
        }
        let folded = self.folded.get_or_init(|| {
            let mut folded = HashMap::new();
            for (idx, entry) in self.entries.iter().enumerate() {
                folded.entry(entry.path.to_ascii_lowercase()).or_insert(idx);
            }
            folded
        });
        folded.get(&path.to_ascii_lowercase()).copied()
    }

    pub fn entry_path(&self, path: &str) -> Option<&str> {
        Some(self.entries[self.position(path)?].path.as_str())
    }

    pub fn origin(&self, path: &str) -> Option<(std::sync::Arc<std::fs::File>, u64)> {
        let Mapping::Mapped { file, .. } = &*self.map else { return None };
        let entry = &self.entries[self.position(path)?];
        Some((file.clone(), (self.data_start + entry.offset) as u64))
    }

    pub fn find(&self, path: &str) -> Option<&[u8]> {
        let idx = self.position(path)?;
        Some(self.read(&self.entries[idx]))
    }

    pub fn scene_entry(&self) -> Result<&Entry> {
        let path = match self.scene_file.as_deref() {
            Some(path) => path,
            None => ["scene.json", "gifscene.json"]
                .into_iter()
                .find(|path| self.position(path).is_some())
                .ok_or_else(|| anyhow!("no scene.json or gifscene.json in package"))?,
        };
        self.position(path).map(|index| &self.entries[index]).ok_or_else(|| {
            anyhow!("scene entry {path} declared in project.json is missing from package")
        })
    }

    pub fn scene_json(&self) -> Result<serde_json::Value> {
        let entry = self.scene_entry()?;
        crate::json::parse(self.read(entry))
            .with_context(|| format!("parse json entry {}", entry.path))
    }

    pub fn find_json(&self, path: &str) -> Result<Option<serde_json::Value>> {
        let Some(raw) = self.find(path) else {
            return Ok(None);
        };
        crate::json::parse(raw).map(Some).with_context(|| format!("parse json entry {path}"))
    }
}

#[cfg(test)]
mod tests;
