use crate::AudioPlayer;

pub fn wants_pipeline(mute: bool, volume: u32) -> bool {
    !mute && volume > 0
}

pub struct GatedAudio {
    path: String,
    mute: bool,
    volume: u32,
    paused: bool,
    player: Option<AudioPlayer>,
}

impl GatedAudio {
    pub fn new(path: &str, mute: bool, volume: u32) -> Self {
        let mut gated = Self {
            path: path.to_string(),
            mute,
            volume: volume.min(100),
            paused: false,
            player: None,
        };
        gated.sync();
        gated
    }

    fn sync(&mut self) {
        let inaudible = !wants_pipeline(self.mute, self.volume);
        if let Some(player) = &self.player {
            player.set_mute(inaudible);
            player.set_volume(self.volume);
            return;
        }
        if inaudible {
            return;
        }
        match AudioPlayer::new(&self.path, self.mute, self.volume) {
            Ok(player) => {
                if let Some(player) = &player {
                    player.set_pause(self.paused);
                    tracing::info!("audio pipeline started for {}", self.path);
                }
                self.player = player;
            }
            Err(error) => {
                tracing::info!("audio unavailable for {}: {error:?}", self.path);
            }
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
        if let Some(player) = &self.player {
            player.set_pause(paused);
        }
    }

    pub fn swap(&mut self, path: &str, mute: bool, volume: u32) {
        self.path = path.to_string();
        self.mute = mute;
        self.volume = volume.min(100);
        self.player = None;
        self.sync();
    }

    pub fn pipeline_running(&self) -> bool {
        self.player.is_some()
    }
}

#[cfg(test)]
#[path = "gated_tests.rs"]
mod tests;
