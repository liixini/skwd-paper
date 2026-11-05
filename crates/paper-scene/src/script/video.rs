use serde_json::Value;

const PLAY: &str = "play";
const PAUSE: &str = "pause";
const STOP: &str = "stop";
const SEEK: &str = "seek";
const RATE: &str = "rate";
const LOOP: &str = "loop";

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VideoOp {
    Play,
    Pause,
    Stop,
    Seek(f64),
    Rate(f64),
    Loop(bool),
}

impl VideoOp {
    pub(super) fn parse(name: &str, value: &Value) -> Option<Self> {
        let number = || value.as_f64().filter(|value| value.is_finite());
        Some(match name {
            PLAY => Self::Play,
            PAUSE => Self::Pause,
            STOP => Self::Stop,
            SEEK => Self::Seek(number()?),
            RATE => Self::Rate(number()?),
            LOOP => Self::Loop(value.as_bool()?),
            _ => return None,
        })
    }
}

#[derive(serde::Serialize)]
pub struct VideoState {
    pub object: usize,
    pub duration: f64,
    pub position: f64,
    pub rate: f64,
    pub looping: bool,
    pub playing: bool,
    pub ended: u64,
}
