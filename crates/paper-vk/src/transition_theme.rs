use std::path::{Path, PathBuf};

const PALETTE_ENV: &str = "SKWD_PAPER_PALETTE";
const WALL_CACHE_ENV: &str = "SKWD_WALL_V2_CACHE";
const CACHE_ENV: &str = "XDG_CACHE_HOME";
const HOME_ENV: &str = "HOME";
const CACHE_DIR: &str = ".cache";
const PALETTE_FILE: &str = "colors.json";
const PALETTE_DIRS: [&str; 2] = ["skwd", "skwd-wall-v2"];
const ROLES: [&str; 4] = ["surface", "surfaceVariant", "primaryContainer", "secondaryContainer"];
const SAVED_THEME: [&str; 4] = ["#0c1015", "#1f252c", "#b5bfcb", "#46494e"];
const HEX_CHANNELS: usize = 6;
const HEX_RADIX: u32 = 16;
const HEX_BYTES: usize = 2;
const RGB_COMPONENTS: usize = 3;
const RGBA_COMPONENTS: usize = 4;

fn environment(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from)
}

fn rgb(hex: &str) -> Option<[f32; 4]> {
    let hex = hex.strip_prefix('#')?;
    if hex.len() != HEX_CHANNELS || !hex.is_ascii() {
        return None;
    }
    let mut color = [1.0; RGBA_COMPONENTS];
    for (index, channel) in color.iter_mut().take(RGB_COMPONENTS).enumerate() {
        let offset = index * HEX_BYTES;
        *channel = f32::from(u8::from_str_radix(&hex[offset..offset + HEX_BYTES], HEX_RADIX).ok()?)
            / f32::from(u8::MAX);
    }
    Some(color)
}

fn read(path: &Path) -> Option<[[f32; 4]; 4]> {
    let document: serde_json::Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    let mut palette = [[0.0; RGBA_COMPONENTS]; ROLES.len()];
    for (color, role) in palette.iter_mut().zip(ROLES) {
        *color = rgb(document.get(role)?.as_str()?)?;
    }
    Some(palette)
}

pub(crate) fn palette() -> [[f32; 4]; 4] {
    let cache =
        environment(CACHE_ENV).or_else(|| environment(HOME_ENV).map(|home| home.join(CACHE_DIR)));
    let mut paths = Vec::new();
    paths.extend(environment(PALETTE_ENV));
    paths.extend(environment(WALL_CACHE_ENV).map(|cache| cache.join(PALETTE_FILE)));
    if let Some(cache) = cache {
        paths.extend(PALETTE_DIRS.map(|dir| cache.join(dir).join(PALETTE_FILE)));
    }
    paths
        .iter()
        .find_map(|path| read(path))
        .unwrap_or_else(|| SAVED_THEME.map(|color| rgb(color).expect("saved theme")))
}

#[cfg(test)]
#[path = "transition_theme/tests.rs"]
mod tests;
