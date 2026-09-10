use paper_control::PaperCommand;
use std::os::fd::RawFd;

static STDIN_ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

pub fn set_stdin_enabled(enabled: bool) {
    STDIN_ENABLED.store(enabled, std::sync::atomic::Ordering::Relaxed);
}

pub fn stdin_enabled() -> bool {
    STDIN_ENABLED.load(std::sync::atomic::Ordering::Relaxed)
}

pub struct SwapReq {
    pub to: String,
    pub duration_ms: u64,
    pub shader: Option<String>,
    pub properties: Option<serde_json::Map<String, serde_json::Value>>,
}

pub enum AudioSink {
    Media(paper_audio::GatedAudio),
    Scene(paper_audio::GatedScene),
}

impl AudioSink {
    pub fn set_mute(&mut self, mute: bool) {
        match self {
            Self::Media(audio) => audio.set_mute(mute),
            Self::Scene(scene) => scene.set_mute(mute),
        }
    }

    pub fn set_volume(&mut self, volume: u32) {
        match self {
            Self::Media(audio) => audio.set_volume(volume),
            Self::Scene(scene) => scene.set_volume(volume),
        }
    }

    pub fn set_pause(&mut self, paused: bool) {
        match self {
            Self::Media(audio) => audio.set_pause(paused),
            Self::Scene(scene) => scene.set_pause(paused),
        }
    }
}

pub struct Ctl {
    rx: std::sync::mpsc::Receiver<PaperCommand>,
    pub audio: Option<AudioSink>,
    pub mute: bool,
    pub volume: u32,
    pub paused: bool,
    freeze: Option<String>,
    capture: Option<paper_control::SceneCapture>,
    wake: Option<paper_runtime::wake::Pipe>,
}

pub fn parse_audio_opts(args: &[String]) -> (bool, u32) {
    let mut mute = true;
    let mut volume = 80u32;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        if arg != "-o" {
            continue;
        }
        let Some(val) = it.next() else { break };
        for part in val.split(';') {
            if let Some(text) = part.strip_prefix("mute=") {
                mute = matches!(text, "yes" | "true" | "1");
            }
            if let Some(vol) =
                part.strip_prefix("volume=").and_then(|text| text.parse::<u32>().ok())
            {
                volume = vol.min(100);
            }
        }
    }
    (mute, volume)
}

impl Ctl {
    pub fn start(video: &str, mute: bool, volume: u32, with_audio: bool) -> Self {
        Self::start_opts(video, mute, volume, with_audio, true)
    }

    pub fn start_opts(
        video: &str,
        mute: bool,
        volume: u32,
        with_audio: bool,
        with_stdin: bool,
    ) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut wake = None;
        if with_stdin && stdin_enabled() {
            let wake_pipe = paper_runtime::wake::make_pipe();
            wake.clone_from(&wake_pipe);
            paper_control::spawn_stdin_line_reader("skwd-wall-vk", move |line| {
                match serde_json::from_str::<PaperCommand>(line) {
                    Ok(cmd) => {
                        if tx.send(cmd).is_ok()
                            && let Some(wake) = &wake_pipe
                        {
                            wake.poke();
                        }
                    }
                    Err(err) => tracing::info!("skwd-wall-vk: bad command ({err}): {line}"),
                }
            });
        }
        let audio =
            with_audio.then(|| AudioSink::Media(paper_audio::GatedAudio::new(video, mute, volume)));
        Self { rx, audio, mute, volume, paused: false, freeze: None, capture: None, wake }
    }

    #[cfg(test)]
    fn with_receiver(rx: std::sync::mpsc::Receiver<PaperCommand>) -> Self {
        Self {
            rx,
            audio: None,
            mute: true,
            volume: 80,
            paused: false,
            freeze: None,
            capture: None,
            wake: None,
        }
    }

    pub fn from_channel(
        rx: std::sync::mpsc::Receiver<PaperCommand>,
        wake: Option<paper_runtime::wake::Pipe>,
        video: &str,
        mute: bool,
        volume: u32,
        with_audio: bool,
    ) -> Self {
        let volume = volume.min(100);
        let audio =
            with_audio.then(|| AudioSink::Media(paper_audio::GatedAudio::new(video, mute, volume)));
        Self { rx, audio, mute, volume, paused: false, freeze: None, capture: None, wake }
    }

    pub fn set_scene_voices(&mut self, voices: Vec<paper_audio::Voice>) {
        if voices.is_empty() {
            self.audio = None;
            return;
        }
        match &mut self.audio {
            Some(AudioSink::Scene(scene)) => scene.swap(voices, self.mute, self.volume),
            _ => {
                let mut scene = paper_audio::GatedScene::new(voices, self.mute, self.volume);
                scene.set_pause(self.paused);
                self.audio = Some(AudioSink::Scene(scene));
            }
        }
    }

    pub fn scene_voice_count(&self) -> usize {
        match &self.audio {
            Some(AudioSink::Scene(scene)) => scene.voice_count(),
            _ => 0,
        }
    }

    pub fn wake_fd(&self) -> Option<RawFd> {
        self.wake.as_ref().map(paper_runtime::wake::Pipe::read_fd)
    }

    pub fn poll(&mut self) -> Option<SwapReq> {
        if let Some(wake) = &self.wake {
            wake.drain();
        }
        let mut swap = None;
        while let Ok(cmd) = self.rx.try_recv() {
            if let Some(req) = self.reduce(cmd) {
                swap = Some(req);
            }
        }
        swap
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
        if let Some(audio) = &mut self.audio {
            audio.set_pause(paused);
        }
    }

    pub fn freeze_pending(&self) -> bool {
        self.freeze.is_some()
    }

    pub fn take_freeze(&mut self) -> Option<String> {
        self.freeze.take()
    }

    pub fn cancel_freeze(&mut self) {
        self.freeze = None;
        self.paused = false;
        if let Some(audio) = &mut self.audio {
            audio.set_pause(false);
        }
    }

    pub fn take_capture(&mut self) -> Option<paper_control::SceneCapture> {
        self.capture.take()
    }

    fn reduce(&mut self, mut cmd: PaperCommand) -> Option<SwapReq> {
        if let Some(capture) = cmd.capture.take() {
            self.capture = Some(capture);
            return None;
        }
        match paper_control::classify_command(cmd) {
            paper_control::CommandClass::Freeze(path) => {
                tracing::info!(path, "skwd-wall-vk: freeze frame requested");
                self.paused = true;
                self.freeze = Some(path);
                if let Some(audio) = &mut self.audio {
                    audio.set_pause(true);
                }
                None
            }
            paper_control::CommandClass::Pause(paused) => {
                self.paused = paused;
                if let Some(audio) = &mut self.audio {
                    audio.set_pause(paused);
                }
                None
            }
            paper_control::CommandClass::RetainOutputs(_) => None,
            paper_control::CommandClass::Audio { mute, volume } => {
                if let Some(mute) = mute {
                    self.mute = mute;
                    if let Some(audio) = &mut self.audio {
                        audio.set_mute(mute);
                    }
                }
                if let Some(vol) = volume {
                    self.volume = vol.min(100);
                    if let Some(audio) = &mut self.audio {
                        audio.set_volume(self.volume);
                    }
                }
                None
            }
            paper_control::CommandClass::Swap(swap) => {
                self.mute = swap.mute.unwrap_or(true);
                self.volume = swap.volume.unwrap_or(80).min(100);
                Some(SwapReq {
                    to: swap.to,
                    duration_ms: swap.duration_ms.unwrap_or(0),
                    shader: swap.shader,
                    properties: swap.properties,
                })
            }
        }
    }

    pub fn swap_audio(&mut self, path: &str, with_audio: bool) {
        if with_audio {
            match &mut self.audio {
                Some(AudioSink::Media(audio)) => audio.swap(path, self.mute, self.volume),
                _ => {
                    self.audio = Some(AudioSink::Media(paper_audio::GatedAudio::new(
                        path,
                        self.mute,
                        self.volume,
                    )));
                }
            }
        } else {
            self.audio = None;
        }
    }
}

mod tests;
