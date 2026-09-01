use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaperCommand {
    #[serde(default, alias = "path")]
    pub to: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mute: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pause: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freeze: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shader: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outputs: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub properties: Option<serde_json::Map<String, serde_json::Value>>,
}

impl PaperCommand {
    #[must_use]
    pub fn with_properties(
        mut self,
        properties: Option<serde_json::Map<String, serde_json::Value>>,
    ) -> Self {
        self.properties = properties;
        self
    }

    pub fn swap_video(to: &str, mute: bool, volume: u32) -> Self {
        Self {
            to: to.to_string(),
            mute: Some(mute),
            volume: Some(volume.min(100)),
            pause: None,
            freeze: None,
            shader: None,
            duration_ms: None,
            outputs: None,
            properties: None,
        }
    }

    pub fn swap_paper(to: &str, shader: &str, duration_ms: u64, mute: bool, volume: u32) -> Self {
        Self {
            to: to.to_string(),
            mute: Some(mute),
            volume: Some(volume.min(100)),
            pause: None,
            freeze: None,
            shader: Some(shader.to_string()),
            duration_ms: Some(duration_ms),
            outputs: None,
            properties: None,
        }
    }

    pub fn audio(mute: Option<bool>, volume: Option<u32>) -> Self {
        Self {
            to: String::new(),
            mute,
            volume: volume.map(|value| value.min(100)),
            pause: None,
            freeze: None,
            shader: None,
            duration_ms: None,
            outputs: None,
            properties: None,
        }
    }

    pub fn audio_for(outputs: &[String], mute: Option<bool>, volume: Option<u32>) -> Self {
        let mut command = Self::audio(mute, volume);
        command.outputs = Some(outputs.to_vec());
        command
    }

    pub fn pause(paused: bool) -> Self {
        Self {
            to: String::new(),
            mute: None,
            volume: None,
            pause: Some(paused),
            freeze: None,
            shader: None,
            duration_ms: None,
            outputs: None,
            properties: None,
        }
    }

    pub fn freeze(path: &str) -> Self {
        Self {
            to: String::new(),
            mute: None,
            volume: None,
            pause: None,
            freeze: Some(path.to_string()),
            shader: None,
            duration_ms: None,
            outputs: None,
            properties: None,
        }
    }

    pub fn retain_outputs(outputs: &[String]) -> Self {
        Self {
            to: String::new(),
            mute: None,
            volume: None,
            pause: None,
            freeze: None,
            shader: None,
            duration_ms: None,
            outputs: Some(outputs.to_vec()),
            properties: None,
        }
    }

    pub fn line(&self) -> String {
        crate::encode_ndjson(self).expect("PaperCommand serializes")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandClass {
    Freeze(String),
    Pause(bool),
    RetainOutputs(Vec<String>),
    Audio { mute: Option<bool>, volume: Option<u32> },
    Swap(PaperCommand),
}

pub fn classify_command(command: PaperCommand) -> CommandClass {
    if let Some(path) = command.freeze {
        return CommandClass::Freeze(path);
    }
    if let Some(paused) = command.pause {
        return CommandClass::Pause(paused);
    }
    if command.to.is_empty()
        && command.mute.is_none()
        && command.volume.is_none()
        && let Some(outputs) = command.outputs
    {
        return CommandClass::RetainOutputs(outputs);
    }
    if command.to.is_empty() {
        return CommandClass::Audio { mute: command.mute, volume: command.volume };
    }
    CommandClass::Swap(command)
}

#[cfg(test)]
#[path = "paper_command_tests.rs"]
mod tests;
