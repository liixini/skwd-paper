use serde::{Deserialize, Serialize};

const ALL_OUTPUT: &str = "*";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StillCommand {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<crate::FrameSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface: Option<crate::SurfacePolicy>,
    #[serde(default)]
    pub path: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reveal: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slide: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preload: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mirror: Option<MirrorTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub widgets: Option<crate::WidgetScene>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MirrorTarget {
    pub output: String,
    #[serde(default)]
    pub socket: String,
}

impl MirrorTarget {
    pub fn validate(&self) -> Result<(), crate::ValidationError> {
        if self.output.is_empty()
            || self.output == ALL_OUTPUT
            || (!self.socket.is_empty() && !std::path::Path::new(&self.socket).is_absolute())
        {
            return Err(crate::ValidationError::InvalidMirrorTarget);
        }
        Ok(())
    }
}

impl StillCommand {
    pub fn new(path: &str) -> Self {
        Self {
            snapshot: None,
            surface: None,
            path: path.to_string(),
            reveal: false,
            slide: None,
            duration_ms: None,
            preload: Vec::new(),
            fill: None,
            mirror: None,
            widgets: None,
        }
    }

    pub fn mirror(output: &str, socket: &str) -> Self {
        Self {
            mirror: Some(MirrorTarget { output: output.into(), socket: socket.into() }),
            ..Self::new("")
        }
    }

    pub fn reveal() -> Self {
        Self { reveal: true, ..Self::new("") }
    }

    pub fn slide(path: &str, direction: &str, duration_ms: u64) -> Self {
        Self {
            snapshot: None,
            surface: None,
            path: path.to_string(),
            reveal: false,
            slide: Some(direction.to_string()),
            duration_ms: Some(duration_ms),
            preload: Vec::new(),
            fill: None,
            mirror: None,
            widgets: None,
        }
    }

    pub fn preload(paths: Vec<String>) -> Self {
        Self {
            snapshot: None,
            surface: None,
            path: String::new(),
            reveal: false,
            slide: None,
            duration_ms: None,
            preload: paths,
            fill: None,
            mirror: None,
            widgets: None,
        }
    }

    #[must_use]
    pub fn with_fill(mut self, fill: &str) -> Self {
        if !fill.is_empty() {
            self.fill = Some(fill.to_string());
        }
        self
    }

    pub fn line(&self) -> String {
        crate::encode_ndjson(self).expect("StillCommand serializes")
    }
}

#[cfg(test)]
#[path = "still_command_tests.rs"]
mod tests;
