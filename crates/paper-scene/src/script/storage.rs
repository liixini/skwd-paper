use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const LIMIT: u64 = 100 * 1024;

pub struct Storage {
    paths: [PathBuf; 2],
    pub(super) data: Value,
}

impl Storage {
    pub fn open(directory: &Path, scope: &str) -> Option<Self> {
        let root = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| Some(PathBuf::from(std::env::var_os("HOME")?).join(".local/state")))?;
        Self::at(&root, directory, scope)
    }

    pub fn at(root: &Path, directory: &Path, scope: &str) -> Option<Self> {
        let identity = std::fs::canonicalize(directory).ok()?;
        let digest = format!("{:x}", Sha256::digest(identity.as_os_str().as_encoded_bytes()));
        let root = root.join("skwd-paper/scene-storage").join(digest);
        let screen = format!("{:x}.json", Sha256::digest(scope.as_bytes()));
        let paths = [root.join(screen), root.join("global.json")];
        let mut data = json!({"screen":{},"global":{}});
        for (key, path) in ["screen", "global"].into_iter().zip(&paths) {
            if let Some(value) = read(path) {
                data[key] = value;
            }
        }
        Some(Self { paths, data })
    }

    pub(super) fn save(&mut self, json: &str) -> anyhow::Result<()> {
        anyhow::ensure!(json.len() <= LIMIT as usize, "Wallpaper storage exceeds 100 KB");
        let next: Value = serde_json::from_str(json)?;
        for (key, path) in ["screen", "global"].into_iter().zip(&self.paths) {
            if next[key] == self.data[key] {
                continue;
            }
            anyhow::ensure!(next[key].is_object(), "Invalid wallpaper storage");
            let parent = path.parent().unwrap();
            std::fs::create_dir_all(parent)?;
            let mut file = tempfile::NamedTempFile::new_in(parent)?;
            file.write_all(serde_json::to_string(&next[key])?.as_bytes())?;
            file.persist(path)?;
        }
        self.data = next;
        Ok(())
    }
}

fn read(path: &Path) -> Option<Value> {
    let file = std::fs::File::open(path).ok()?;
    if file.metadata().ok()?.len() > LIMIT {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(LIMIT + 1).read_to_end(&mut bytes).ok()?;
    let data: Value = serde_json::from_slice(&bytes).ok()?;
    data.is_object().then_some(data)
}
