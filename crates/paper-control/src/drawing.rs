use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

pub const DRAWING_EFFECT: &str = "drawing";
pub const DRAWING_ENV: &str = "SKWD_PAPER_DRAWING";
pub const PREPARE_HIDDEN_ENV: &str = "SKWD_PAPER_PREPARE_HIDDEN";
pub const DRAWING_SOURCE_ENV: &str = "SKWD_PAPER_DRAWING_SOURCE";
pub const DRAWING_VERSION: u32 = 3;
pub const DRAWING_PAPER: [u8; 3] = [241, 238, 229];
pub const DRAWING_INK: [u8; 3] = [0, 0, 0];
pub const DRAWING_DURATION_MAX: u64 = 20_000;
const MANIFEST_LIMIT: u64 = 64 * 1024;
const MAX_DIMENSION: u32 = 4096;
const HASH_LENGTH: usize = 64;
const HASH_BUFFER: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DrawingTransition {
    pub artifact: PathBuf,
    #[serde(default)]
    pub colours: DrawingColours,
    #[serde(default)]
    pub ink: DrawingInk,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DrawingInk {
    #[default]
    Plain,
    Rainbow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DrawingColours {
    pub paper: [u8; 3],
    pub ink: [u8; 3],
}

impl Default for DrawingColours {
    fn default() -> Self {
        Self { paper: DRAWING_PAPER, ink: DRAWING_INK }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DrawingSource {
    pub path: PathBuf,
    pub size: u64,
    pub mtime_ns: u128,
    pub sha256: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DrawingManifest {
    pub version: u32,
    pub source: DrawingSource,
    pub width: u32,
    pub height: u32,
    pub texture: PathBuf,
    pub strokes: PathBuf,
    pub timing_width: u32,
    pub timing_height: u32,
    pub focus: [f32; 2],
}

impl DrawingTransition {
    pub fn load(&self, source: &Path) -> io::Result<(DrawingManifest, PathBuf, PathBuf)> {
        DrawingManifest::open(&self.artifact, source)
    }
}

impl DrawingManifest {
    pub fn describes(artifact: &Path, source: &Path) -> bool {
        let mut bytes = Vec::new();
        let read = std::fs::File::open(artifact)
            .and_then(|file| file.take(MANIFEST_LIMIT + 1).read_to_end(&mut bytes));
        let Ok(manifest) = read
            .map_err(io::Error::other)
            .and_then(|_| serde_json::from_slice::<Self>(&bytes).map_err(io::Error::other))
        else {
            return false;
        };
        match (manifest.source.path.canonicalize(), source.canonicalize()) {
            (Ok(described), Ok(shown)) => described == shown,
            _ => false,
        }
    }

    pub fn open(artifact: &Path, source: &Path) -> io::Result<(Self, PathBuf, PathBuf)> {
        let invalid = |message| io::Error::new(io::ErrorKind::InvalidData, message);
        if !artifact.is_absolute() {
            return Err(invalid("drawing artifact must be absolute"));
        }
        let mut bytes = Vec::new();
        std::fs::File::open(artifact)?.take(MANIFEST_LIMIT + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MANIFEST_LIMIT {
            return Err(invalid("drawing manifest is too large"));
        }
        let manifest: Self = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
        if !manifest.source.path.is_absolute()
            || manifest.version != DRAWING_VERSION
            || [manifest.width, manifest.height, manifest.timing_width, manifest.timing_height]
                .iter()
                .any(|side| !(1..=MAX_DIMENSION).contains(side))
            || manifest.focus.iter().any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
            || manifest.source.sha256.len() != HASH_LENGTH
            || !manifest.source.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(invalid("invalid drawing manifest"));
        }
        let metadata = std::fs::metadata(source)?;
        let modified =
            metadata.modified()?.duration_since(UNIX_EPOCH).map_err(io::Error::other)?.as_nanos();
        if source.canonicalize()? != manifest.source.path.canonicalize()?
            || metadata.len() != manifest.source.size
            || modified != manifest.source.mtime_ns
            || !source_hash(source)?.eq_ignore_ascii_case(&manifest.source.sha256)
        {
            return Err(invalid("drawing artifact source changed"));
        }
        let directory = artifact
            .parent()
            .ok_or_else(|| invalid("drawing directory missing"))?
            .canonicalize()?;
        let texture = asset_path(&directory, &manifest.texture)?;
        let strokes = asset_path(&directory, &manifest.strokes)?;
        Ok((manifest, texture, strokes))
    }
}

fn source_hash(source: &Path) -> io::Result<String> {
    let mut file = std::fs::File::open(source)?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0; HASH_BUFFER];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hash.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn asset_path(directory: &Path, name: &Path) -> io::Result<PathBuf> {
    let invalid = |message| io::Error::new(io::ErrorKind::InvalidData, message);
    if name.components().count() != 1
        || !matches!(name.components().next(), Some(std::path::Component::Normal(_)))
    {
        return Err(invalid("drawing texture must be a filename"));
    }
    let path = directory.join(name).canonicalize()?;
    if path.parent() != Some(directory) {
        return Err(invalid("drawing texture escapes artifact directory"));
    }
    Ok(path)
}

#[cfg(test)]
#[path = "drawing_tests.rs"]
mod tests;
