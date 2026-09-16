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
    pub duck: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freeze: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture: Option<SceneCapture>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shader: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outputs: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub properties: Option<serde_json::Map<String, serde_json::Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pointer: Option<PointerState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PointerState {
    pub x: u16,
    pub y: u16,
    #[serde(default)]
    pub buttons: u8,
}

impl PointerState {
    #[must_use]
    pub fn from_normalized(x: f32, y: f32, buttons: [bool; 3]) -> Self {
        let quantize = |value: f32| (value.clamp(0.0, 1.0) * f32::from(u16::MAX)).round() as u16;
        Self {
            x: quantize(x),
            y: quantize(y),
            buttons: buttons
                .iter()
                .enumerate()
                .map(|(index, pressed)| u8::from(*pressed) << index)
                .sum(),
        }
    }

    #[must_use]
    pub fn position(&self) -> [f32; 2] {
        [f32::from(self.x) / f32::from(u16::MAX), f32::from(self.y) / f32::from(u16::MAX)]
    }

    #[must_use]
    pub fn pressed(&self) -> [bool; 3] {
        std::array::from_fn(|index| self.buttons & (1 << index) != 0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SceneCapture {
    pub source: String,
    pub path: String,
}

impl PaperCommand {
    pub fn capture_scene(source: &str, path: &str) -> Self {
        let mut command = Self::audio(None, None);
        command.capture = Some(SceneCapture { source: source.into(), path: path.into() });
        command
    }

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
            duck: None,
            capture: None,
            freeze: None,
            shader: None,
            duration_ms: None,
            outputs: None,
            properties: None,
            pointer: None,
        }
    }

    pub fn swap_paper(to: &str, shader: &str, duration_ms: u64, mute: bool, volume: u32) -> Self {
        Self {
            to: to.to_string(),
            mute: Some(mute),
            volume: Some(volume.min(100)),
            pause: None,
            duck: None,
            capture: None,
            freeze: None,
            shader: Some(shader.to_string()),
            duration_ms: Some(duration_ms),
            outputs: None,
            properties: None,
            pointer: None,
        }
    }

    pub fn audio(mute: Option<bool>, volume: Option<u32>) -> Self {
        Self {
            to: String::new(),
            mute,
            volume: volume.map(|value| value.min(100)),
            pause: None,
            duck: None,
            capture: None,
            freeze: None,
            shader: None,
            duration_ms: None,
            outputs: None,
            properties: None,
            pointer: None,
        }
    }

    pub fn audio_for(outputs: &[String], mute: Option<bool>, volume: Option<u32>) -> Self {
        let mut command = Self::audio(mute, volume);
        command.outputs = Some(outputs.to_vec());
        command
    }

    pub fn pointer(x: f32, y: f32, buttons: [bool; 3]) -> Self {
        let mut command = Self::audio(None, None);
        command.pointer = Some(PointerState::from_normalized(x, y, buttons));
        command
    }

    pub fn pause(paused: bool) -> Self {
        Self {
            to: String::new(),
            mute: None,
            volume: None,
            pause: Some(paused),
            duck: None,
            pointer: None,
            capture: None,
            freeze: None,
            shader: None,
            duration_ms: None,
            outputs: None,
            properties: None,
        }
    }

    pub fn duck(ducked: bool) -> Self {
        Self {
            to: String::new(),
            mute: None,
            volume: None,
            pause: None,
            duck: Some(ducked),
            capture: None,
            freeze: None,
            shader: None,
            duration_ms: None,
            outputs: None,
            properties: None,
            pointer: None,
        }
    }

    pub fn freeze(path: &str) -> Self {
        Self {
            to: String::new(),
            mute: None,
            volume: None,
            pause: None,
            duck: None,
            capture: None,
            freeze: Some(path.to_string()),
            shader: None,
            duration_ms: None,
            outputs: None,
            properties: None,
            pointer: None,
        }
    }

    pub fn retain_outputs(outputs: &[String]) -> Self {
        Self {
            to: String::new(),
            mute: None,
            volume: None,
            pause: None,
            duck: None,
            capture: None,
            freeze: None,
            shader: None,
            duration_ms: None,
            outputs: Some(outputs.to_vec()),
            properties: None,
            pointer: None,
        }
    }

    pub fn line(&self) -> String {
        crate::encode_ndjson(self).expect("PaperCommand serializes")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandClass {
    Pointer(PointerState),
    Freeze(String),
    Pause(bool),
    Duck(bool),
    RetainOutputs(Vec<String>),
    Audio { mute: Option<bool>, volume: Option<u32> },
    Swap(PaperCommand),
}

pub fn classify_command(command: PaperCommand) -> CommandClass {
    if let Some(pointer) = command.pointer {
        return CommandClass::Pointer(pointer);
    }
    if let Some(path) = command.freeze {
        return CommandClass::Freeze(path);
    }
    if let Some(paused) = command.pause {
        return CommandClass::Pause(paused);
    }
    if let Some(ducked) = command.duck {
        return CommandClass::Duck(ducked);
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
