use crate::effects::Assets;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use swash::{FontRef, StringId};

#[derive(Default)]
pub(crate) struct Cache {
    prefix: Option<Vec<Entry>>,
    resolved: BTreeMap<String, Option<Vec<u8>>>,
}

struct Entry {
    path: PathBuf,
    names: BTreeSet<String>,
    regular: bool,
}

impl Cache {
    pub fn resolve(&mut self, assets: &Assets, family: &str) -> Option<Vec<u8>> {
        let key = normalized(family);
        if let Some(bytes) = self.resolved.get(&key) {
            return bytes.clone();
        }
        let prefix = self.prefix.get_or_insert_with(|| prefix_fonts(assets.root()));
        let matching = |name: &str| {
            prefix
                .iter()
                .find(|entry| entry.names.contains(name))
                .and_then(|entry| read(&entry.path))
        };
        let bytes = matching(&key)
            .or_else(|| host_font(family))
            .or_else(|| matching("arial"))
            .or_else(|| assets.read_bytes(super::FALLBACK_FONT));
        self.resolved.insert(key, bytes.clone());
        bytes
    }
}

fn normalized(name: &str) -> String {
    name.chars().filter(|ch| ch.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

fn read(path: &Path) -> Option<Vec<u8>> {
    const LIMIT: u64 = 32 * 1024 * 1024;
    let file = std::fs::File::open(path).ok()?;
    if file.metadata().ok()?.len() > LIMIT {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(LIMIT + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() <= LIMIT as usize).then_some(bytes)
}

fn names(bytes: &[u8]) -> Option<(BTreeSet<String>, bool)> {
    let font = FontRef::from_index(bytes, 0)?;
    let names = font
        .localized_strings()
        .filter(|name| {
            matches!(name.id(), StringId::Family | StringId::TypographicFamily | StringId::Full)
        })
        .map(|name| normalized(&name.to_string()))
        .filter(|name| !name.is_empty())
        .collect();
    let regular = font.attributes().style() == swash::Style::Normal
        && font.attributes().weight() == swash::Weight::NORMAL;
    Some((names, regular))
}

fn prefix_fonts(root: Option<&Path>) -> Vec<Entry> {
    let Some(steamapps) = root.and_then(|root| {
        root.ancestors().find(|path| path.file_name().is_some_and(|name| name == "steamapps"))
    }) else {
        return Vec::new();
    };
    let windows = steamapps.join("compatdata/431960/pfx/drive_c/windows");
    let directory =
        [windows.join("Fonts"), windows.join("fonts")].into_iter().find(|path| path.is_dir());
    let mut entries: Vec<_> = directory
        .and_then(|path| std::fs::read_dir(path).ok())
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().and_then(|name| name.to_str()).is_some_and(|name| {
                ["ttf", "otf", "ttc"].iter().any(|extension| name.eq_ignore_ascii_case(extension))
            })
        })
        .filter_map(|path| {
            let (names, regular) = names(&read(&path)?)?;
            Some(Entry { path, names, regular })
        })
        .collect();
    entries.sort_by(|a, b| b.regular.cmp(&a.regular).then_with(|| a.path.cmp(&b.path)));
    entries
}

fn host_font(family: &str) -> Option<Vec<u8>> {
    if family.trim().is_empty() {
        return None;
    }
    let output = std::process::Command::new("fc-match")
        .args(["--format=%{file}", "--", family])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8(output.stdout).ok()?;
    let bytes = read(Path::new(path.trim()))?;
    let (names, _) = names(&bytes)?;
    names.contains(&normalized(family)).then_some(bytes)
}

#[cfg(test)]
#[path = "system_font_tests.rs"]
mod tests;
