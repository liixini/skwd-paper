#![deny(unsafe_code)]

mod decoder;
mod gated;
mod gated_scene;
mod mixer;
mod player;
pub mod spectrum;

pub use gated::{GatedAudio, wants_pipeline};
pub use gated_scene::GatedScene;
pub use mixer::{SceneMixer, Voice, VoiceMode, playable};
pub use player::AudioPlayer;
pub use spectrum::{Analyser, Bands};
