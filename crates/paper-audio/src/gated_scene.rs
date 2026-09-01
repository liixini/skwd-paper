use crate::gated::wants_pipeline;
use crate::mixer::{SceneMixer, Voice};

pub struct GatedScene {
    voices: Vec<Voice>,
    mute: bool,
    volume: u32,
    paused: bool,
    mixer: Option<SceneMixer>,
}

impl GatedScene {
    #[must_use]
    pub fn new(voices: Vec<Voice>, mute: bool, volume: u32) -> Self {
        let mut gated = Self { voices, mute, volume: volume.min(100), paused: false, mixer: None };
        gated.sync();
        gated
    }

    fn sync(&mut self) {
        let inaudible = !wants_pipeline(self.mute, self.volume);
        if let Some(mixer) = &self.mixer {
            mixer.set_mute(inaudible);
            mixer.set_volume(self.volume);
            return;
        }
        if inaudible || self.voices.is_empty() {
            return;
        }
        match SceneMixer::new(&self.voices, self.mute, self.volume) {
            Ok(Some(mixer)) => {
                mixer.set_pause(self.paused);
                tracing::info!(voices = mixer.voice_count(), "scene mixer started");
                self.mixer = Some(mixer);
            }
            Ok(None) => tracing::info!("scene declares no decodable audio"),
            Err(error) => tracing::info!("scene audio unavailable: {error:?}"),
        }
    }

    pub fn set_mute(&mut self, mute: bool) {
        self.mute = mute;
        self.sync();
    }

    pub fn set_volume(&mut self, volume: u32) {
        self.volume = volume.min(100);
        self.sync();
    }

    pub fn set_pause(&mut self, paused: bool) {
        self.paused = paused;
        if let Some(mixer) = &self.mixer {
            mixer.set_pause(paused);
        }
    }

    pub fn swap(&mut self, voices: Vec<Voice>, mute: bool, volume: u32) {
        self.voices = voices;
        self.mute = mute;
        self.volume = volume.min(100);
        self.mixer = None;
        self.sync();
    }

    #[must_use]
    pub fn voice_count(&self) -> usize {
        self.mixer.as_ref().map_or(0, SceneMixer::voice_count)
    }

    #[must_use]
    pub fn pipeline_running(&self) -> bool {
        self.mixer.is_some()
    }
}

#[cfg(test)]
#[path = "gated_scene_tests.rs"]
mod tests;
