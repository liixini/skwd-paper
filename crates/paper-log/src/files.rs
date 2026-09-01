use std::path::{Path, PathBuf};

pub const ROTATE_BYTES: u64 = 4 * 1024 * 1024;
pub const ROTATE_GENERATIONS: usize = 3;

pub fn log_path_from(
    app: &str,
    xdg_cache: Option<PathBuf>,
    home: Option<PathBuf>,
    localappdata: Option<PathBuf>,
) -> Option<PathBuf> {
    let base = xdg_cache.or_else(|| home.map(|dir| dir.join(".cache"))).or(localappdata)?;
    Some(base.join("skwd-wall-v2").join(format!("{app}.log")))
}

pub fn log_path(app: &str) -> Option<PathBuf> {
    log_path_from(
        app,
        std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from),
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
    )
}

fn should_rotate(len: u64, max: u64) -> bool {
    len >= max
}

fn generation(path: &Path, index: usize) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".{index}"));
    PathBuf::from(name)
}

pub fn rotate_if_large(path: &Path, max: u64) {
    let Ok(meta) = std::fs::metadata(path) else {
        return;
    };
    if !should_rotate(meta.len(), max) {
        return;
    }
    let _ = std::fs::remove_file(generation(path, ROTATE_GENERATIONS));
    for index in (1..ROTATE_GENERATIONS).rev() {
        let _ = std::fs::rename(generation(path, index), generation(path, index + 1));
    }
    let _ = std::fs::rename(path, generation(path, 1));
}

pub fn prepare(app: &str) -> Option<PathBuf> {
    let path = log_path(app)?;
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
        secure_mode(dir, 0o700);
    }
    secure_mode(&path, 0o600);
    rotate_if_large(&path, ROTATE_BYTES);
    for generation_number in 1..=ROTATE_GENERATIONS {
        secure_mode(&generation(&path, generation_number), 0o600);
    }
    Some(path)
}

#[cfg(unix)]
pub fn secure_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    if std::fs::symlink_metadata(path).is_ok_and(|metadata| !metadata.file_type().is_symlink()) {
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
    }
}

#[cfg(not(unix))]
pub fn secure_mode(_path: &Path, _mode: u32) {}

mod tests;
