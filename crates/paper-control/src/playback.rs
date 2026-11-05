use serde::{Deserialize, Serialize};

pub const PLAYBACK_ENV: &str = "SKWD_PAPER_PLAYBACK";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaybackState {
    pub paused: bool,
    pub ducked: bool,
}

impl PlaybackState {
    pub fn from_env() -> Result<Self, serde_json::Error> {
        std::env::var_os(PLAYBACK_ENV)
            .map(|value| serde_json::from_str(&value.to_string_lossy()))
            .transpose()
            .map(Option::unwrap_or_default)
    }
}
