use super::Group;
use paper_scene::model::Properties;
use std::os::unix::fs::MetadataExt;

pub(super) fn resolved(directory: &str, overrides: &Properties) -> Properties {
    let mut properties =
        paper_control::we_project::Project::resolve(std::path::Path::new(directory))
            .map(|project| paper_scene::effects::parse_properties(&project.document))
            .unwrap_or_default();
    properties.extend(overrides.clone());
    properties
}

#[derive(PartialEq, Eq)]
struct FileStamp {
    size: u64,
    modified: (i64, i64),
    changed: (i64, i64),
}

pub(super) struct PropertySource {
    directory: String,
    properties: Properties,
    files: Option<Vec<FileStamp>>,
}

impl PropertySource {
    pub fn new(directory: &str, properties: &Properties) -> Self {
        Self {
            directory: directory.into(),
            properties: properties.clone(),
            files: stamps(directory),
        }
    }

    fn accepts(&self, directory: &str, properties: &Properties) -> bool {
        self.directory == directory
            && self.properties != *properties
            && self.files.is_some()
            && self.files == stamps(directory)
    }
}

fn stamps(directory: &str) -> Option<Vec<FileStamp>> {
    let package = super::locate_pkg(directory).ok()?;
    let project =
        paper_control::we_project::Project::resolve(std::path::Path::new(directory)).ok()?;
    let mut files: Vec<_> =
        project.directories.iter().map(|dir| dir.join("project.json")).collect();
    files.push(package);
    files
        .iter()
        .map(|path| {
            let meta = path.metadata().ok()?;
            Some(FileStamp {
                size: meta.len(),
                modified: (meta.mtime(), meta.mtime_nsec()),
                changed: (meta.ctime(), meta.ctime_nsec()),
            })
        })
        .collect()
}

impl Group {
    pub(super) fn update_properties(
        &mut self,
        source: &mut PropertySource,
        directory: &str,
        properties: &Properties,
        mut scene_sound: impl FnMut(&str, paper_audio::VoiceOp),
    ) -> bool {
        if self.frozen
            || !self.baked_fx_outputs.is_empty()
            || !source.accepts(directory, properties)
        {
            return false;
        }
        let Some(scripts) = self.scripts.as_mut() else { return false };
        let started = std::time::Instant::now();
        let effective = resolved(directory, properties);
        match scripts.host.update_properties(&effective) {
            Ok(true) => {}
            Ok(false) => return false,
            Err(error) => {
                tracing::warn!("scene properties need a reload: {error:#}");
                return false;
            }
        }
        if let Err(error) = self.compose(self.composed_at, 0.0) {
            tracing::warn!("scene properties need a rebuild: {error:#}");
            return false;
        }
        for (id, op) in self.take_script_sounds() {
            scene_sound(&id, op);
        }
        source.properties.clone_from(properties);
        tracing::info!(
            update_us = started.elapsed().as_micros(),
            "skwd-wall-vk: scene properties updated"
        );
        true
    }
}

#[cfg(test)]
#[path = "properties_tests.rs"]
mod tests;
