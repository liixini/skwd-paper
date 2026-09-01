use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StillCommand {
    #[serde(default)]
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slide: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preload: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<String>,
}

impl StillCommand {
    pub fn new(path: &str) -> Self {
        Self {
            path: path.to_string(),
            slide: None,
            duration_ms: None,
            preload: Vec::new(),
            fill: None,
        }
    }

    pub fn slide(path: &str, direction: &str, duration_ms: u64) -> Self {
        Self {
            path: path.to_string(),
            slide: Some(direction.to_string()),
            duration_ms: Some(duration_ms),
            preload: Vec::new(),
            fill: None,
        }
    }

    pub fn preload(paths: Vec<String>) -> Self {
        Self { path: String::new(), slide: None, duration_ms: None, preload: paths, fill: None }
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
