use serde::{Deserialize, Serialize};

fn default_mute() -> bool {
    true
}

fn default_volume() -> u32 {
    80
}

/// Passed as a JSON argv value so file names containing `=` or `;` stay unambiguous.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MultiVideoEntry {
    pub output: String,
    pub video: String,
    #[serde(default = "default_mute")]
    pub mute: bool,
    #[serde(default = "default_volume")]
    pub volume: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition_from: Option<String>,
}

impl MultiVideoEntry {
    #[must_use]
    pub fn normalized(mut self) -> Self {
        self.volume = self.volume.min(100);
        self
    }
}
