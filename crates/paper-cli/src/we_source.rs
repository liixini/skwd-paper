use anyhow::{Context, Result, anyhow};
use serde_json::Value;
use std::path::{Component, Path, PathBuf};

pub(crate) enum WeTarget {
    Scene(PathBuf),
    Video(PathBuf),
}

pub(crate) fn resolve(path: &str) -> Result<WeTarget> {
    let root = std::fs::canonicalize(path)
        .with_context(|| format!("resolve Wallpaper Engine item {path}"))?;
    if !root.is_dir() {
        return Err(anyhow!("Wallpaper Engine item is not a directory: {}", root.display()));
    }
    let resolved = paper_control::we_project::Project::resolve(Path::new(path))?;
    let project = &resolved.document;
    let kind = project
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("Wallpaper Engine project type is missing"))?
        .to_ascii_lowercase();
    match kind.as_str() {
        "scene" => {
            resolved.scene_package()?;
            Ok(WeTarget::Scene(PathBuf::from(path)))
        }
        "video" => resolve_video(&resolved.source, project),
        unsupported => Err(anyhow!("unsupported Wallpaper Engine project type {unsupported:?}")),
    }
}

pub(crate) fn transition_media(path: &str) -> Result<PathBuf> {
    match resolve(path)? {
        WeTarget::Video(path) => Ok(path),
        WeTarget::Scene(root) => {
            let root = std::fs::canonicalize(root)?;
            let project: Value =
                serde_json::from_slice(&std::fs::read(root.join("project.json"))?)?;
            if let Some(relative) = project.get("preview").and_then(Value::as_str)
                && !Path::new(relative).components().any(|component| {
                    matches!(
                        component,
                        Component::ParentDir | Component::RootDir | Component::Prefix(_)
                    )
                })
                && let Ok(preview) = std::fs::canonicalize(root.join(relative))
                && preview.starts_with(&root)
                && preview.is_file()
                && preview_extension(&preview)
            {
                return Ok(preview);
            }
            let mut previews = std::fs::read_dir(&root)
                .with_context(|| format!("read Wallpaper Engine item {}", root.display()))?
                .filter_map(Result::ok)
                .filter(|entry| {
                    entry.file_name().to_string_lossy().to_ascii_lowercase().starts_with("preview.")
                        && preview_extension(&entry.path())
                })
                .filter_map(|entry| std::fs::canonicalize(entry.path()).ok())
                .filter(|preview| preview.starts_with(&root) && preview.is_file())
                .collect::<Vec<_>>();
            previews.sort();
            previews.into_iter().next().ok_or_else(|| {
                anyhow!(
                    "Wallpaper Engine scene has no safe preview for inferred transition: {}",
                    root.display()
                )
            })
        }
    }
}

fn preview_extension(path: &Path) -> bool {
    path.extension().and_then(|extension| extension.to_str()).is_some_and(|extension| {
        ["jpg", "jpeg", "png", "webp", "avif", "gif"]
            .iter()
            .chain(paper_control::VIDEO_EXTS)
            .any(|supported| extension.eq_ignore_ascii_case(supported))
    })
}

fn resolve_video(root: &Path, project: &Value) -> Result<WeTarget> {
    let relative = project
        .get("file")
        .and_then(Value::as_str)
        .filter(|path| !path.is_empty())
        .ok_or_else(|| anyhow!("Wallpaper Engine video file is missing"))?;
    let relative = Path::new(relative);
    if relative.is_absolute()
        || relative.components().any(|component| {
            matches!(component, Component::ParentDir | Component::RootDir | Component::Prefix(_))
        })
    {
        return Err(anyhow!("Wallpaper Engine video path must stay inside its item directory"));
    }
    let video = std::fs::canonicalize(root.join(relative))
        .context("resolve Wallpaper Engine video file")?;
    if !video.starts_with(root) || !video.is_file() {
        return Err(anyhow!("Wallpaper Engine video path escapes its item directory"));
    }
    Ok(WeTarget::Video(video))
}

#[cfg(test)]
#[path = "we_source_tests.rs"]
mod tests;
