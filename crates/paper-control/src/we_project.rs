use serde_json::{Map, Value};
use std::collections::HashSet;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

pub const MAX_PRESET_DEPTH: usize = 32;

pub struct Project {
    pub source: PathBuf,
    pub directories: Vec<PathBuf>,
    pub document: Value,
}

pub fn read(directory: &Path) -> io::Result<Value> {
    let path = directory.join("project.json");
    let mut bytes = Vec::new();
    std::fs::File::open(&path)
        .and_then(|file| file.take(16 * 1024 * 1024 + 1).read_to_end(&mut bytes))
        .map_err(|error| io::Error::new(error.kind(), format!("{}: {error}", path.display())))?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err(io::Error::other("Wallpaper Engine project exceeds 16 MiB"));
    }
    let value: Value =
        serde_json::from_slice(bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&bytes))
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if !value.is_object() {
        return Err(io::Error::other("Wallpaper Engine project must be an object"));
    }
    Ok(value)
}

pub fn dependency(document: &Value) -> io::Result<Option<String>> {
    let Some(value) = document.get("dependency") else {
        if document.get("preset").is_some() {
            return Err(io::Error::other("Wallpaper Engine preset has no dependency"));
        }
        return Ok(None);
    };
    let id = match value {
        Value::String(id) => id.clone(),
        Value::Number(id) if id.as_u64().is_some() => id.to_string(),
        _ => return Err(io::Error::other("invalid Wallpaper Engine dependency ID")),
    };
    if id.is_empty()
        || !id.bytes().all(|byte| byte.is_ascii_digit())
        || id.parse::<u64>().unwrap_or(0) == 0
    {
        return Err(io::Error::other("invalid Wallpaper Engine dependency ID"));
    }
    if !document.get("preset").is_some_and(Value::is_object) {
        return Err(io::Error::other("Wallpaper Engine preset settings must be an object"));
    }
    Ok(Some(id))
}

impl Project {
    pub fn scene_package_at(directory: &Path) -> io::Result<PathBuf> {
        if directory.join("project.json").is_file() {
            return Self::resolve(directory)?.scene_package();
        }
        package_in(directory)
    }

    pub fn resolve(directory: &Path) -> io::Result<Self> {
        let library = directory
            .parent()
            .ok_or_else(|| io::Error::other("Wallpaper Engine item has no parent directory"))?;
        let mut source = directory.to_path_buf();
        let mut directories = Vec::new();
        let mut visited = HashSet::new();
        let mut presets = Vec::new();
        let mut document = loop {
            source = source.canonicalize()?;
            if !visited.insert(source.clone()) {
                return Err(io::Error::other("Wallpaper Engine preset dependency cycle"));
            }
            if directories.len() >= MAX_PRESET_DEPTH {
                return Err(io::Error::other(
                    "Wallpaper Engine preset dependency chain is too deep",
                ));
            }
            directories.push(source.clone());
            let document = read(&source)?;
            let Some(id) = dependency(&document)? else { break document };
            presets.push(document["preset"].as_object().unwrap().clone());
            let linked = library.join(&id);
            let sibling = source.parent().unwrap().join(&id);
            source = if linked.join("project.json").is_file() { linked } else { sibling };
            if !source.join("project.json").is_file() {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!(
                        "Wallpaper Engine preset requires Workshop item {id}; download it first"
                    ),
                ));
            }
        };
        if let Some(properties) = document
            .get_mut("general")
            .and_then(|general| general.get_mut("properties"))
            .and_then(Value::as_object_mut)
        {
            for preset in presets.iter().rev() {
                for (name, value) in preset {
                    if let Some(entry) = properties.get_mut(name).and_then(Value::as_object_mut) {
                        entry.insert("value".into(), value.clone());
                    }
                }
            }
        }
        Ok(Self { source, directories, document })
    }

    pub fn declarations(&self) -> Map<String, Value> {
        self.document
            .get("general")
            .and_then(|general| general.get("properties"))
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default()
    }

    pub fn scene_package(&self) -> io::Result<PathBuf> {
        package_in(&self.source)
    }
}

fn package_in(directory: &Path) -> io::Result<PathBuf> {
    ["scene.pkg", "gifscene.pkg"]
        .into_iter()
        .map(|name| directory.join(name))
        .find(|path| path.is_file())
        .ok_or_else(|| {
            io::Error::other(format!(
                "Wallpaper Engine scene package is missing in {}",
                directory.display()
            ))
        })
}

#[cfg(test)]
#[path = "we_project_tests.rs"]
mod tests;
