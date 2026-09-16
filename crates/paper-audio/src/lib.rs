#![deny(unsafe_code)]

mod decoder;
mod gated;
mod gated_scene;
mod mixer;
mod player;
mod pulse;
pub mod spectrum;

pub use gated::{GatedAudio, wants_pipeline};
pub use gated_scene::GatedScene;
pub use mixer::{SceneMixer, Voice, VoiceMode, VoiceOp, playable};
pub use player::AudioPlayer;
pub use spectrum::{Analyser, Bands};
