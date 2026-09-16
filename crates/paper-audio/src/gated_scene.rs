use crate::gated::wants_pipeline;
use crate::mixer::{SceneMixer, Voice, VoiceOp};

pub struct GatedScene {
    voices: Vec<Voice>,
    mute: bool,
    volume: u32,
    paused: bool,
    ducked: bool,
    mixer: Option<SceneMixer>,
}

impl GatedScene {
    #[must_use]
    pub fn new(voices: Vec<Voice>, mute: bool, volume: u32) -> Self {
        let mut gated = Self {
            voices,
            mute,
            volume: volume.min(100),
            paused: false,
            ducked: false,
            mixer: None,
        };
        gated.sync();
        gated
    }

    fn sync(&mut self) {
        let inaudible = self.ducked || !wants_pipeline(self.mute, self.volume);
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

    pub fn set_duck(&mut self, ducked: bool) {
        self.ducked = ducked;
        self.sync();
    }

    #[must_use]
    pub fn ducked(&self) -> bool {
        self.ducked
    }

    pub fn set_pause(&mut self, paused: bool) {
        self.paused = paused;
        if let Some(mixer) = &self.mixer {
            mixer.set_pause(paused);
        }
    }

    pub fn voice(&mut self, id: &str, op: VoiceOp) -> bool {
        let Some(voice) = self.voices.iter_mut().find(|voice| voice.id == id) else {
            return false;
        };
        match op {
            VoiceOp::Play => voice.autostart = true,
            VoiceOp::Stop | VoiceOp::Pause => voice.autostart = false,
            VoiceOp::Gain(gain) => voice.gain = gain.max(0.0),
        }
        match &self.mixer {
            Some(mixer) => mixer.voice(id, op),
            None => {
                self.sync();
                true
            }
        }
    }

    pub fn voice_playing(&self, id: &str) -> bool {
        match &self.mixer {
            Some(mixer) => mixer.voice_playing(id),
            None => self.voices.iter().any(|voice| voice.id == id && voice.autostart),
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
