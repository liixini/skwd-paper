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
    pub drawing: Option<paper_control::DrawingTransition>,
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
    fn media(video: &str, mute: bool, volume: u32, playback: paper_control::PlaybackState) -> Self {
        let mut audio = paper_audio::GatedAudio::new(video, true, volume);
        audio.set_pause(playback.paused);
        audio.set_duck(playback.ducked);
        audio.set_mute(mute);
        Self::Media(audio)
    }

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

    pub fn set_duck(&mut self, ducked: bool) {
        match self {
            Self::Media(audio) => audio.set_duck(ducked),
            Self::Scene(scene) => scene.set_duck(ducked),
        }
    }
}

struct FreezeState {
    paused: bool,
    frozen_at: Option<std::time::Instant>,
}

pub struct Ctl {
    pub drawing_photo: Option<String>,
    pub reveal: bool,
    pub retained_outputs: Option<Vec<String>>,
    snapshot: Option<paper_control::FrameSnapshot>,
    pub output_pauses: Vec<(String, bool)>,
    route_output_pauses: bool,
    rx: std::sync::mpsc::Receiver<PaperCommand>,
    pub audio: Option<AudioSink>,
    pub mute: bool,
    pub volume: u32,
    pub paused: bool,
    pub frozen_at: Option<std::time::Instant>,
    pub ducked: bool,
    freeze: Option<String>,
    freeze_restore: Option<FreezeState>,
    capture: Option<paper_control::SceneCapture>,
    wake: Option<paper_runtime::wake::Pipe>,
    pointer: Option<paper_control::PointerState>,
    pointer_revision: u64,
    surface_changed: bool,
    system_audio: Option<bool>,
    widgets: Option<paper_control::WidgetScene>,
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
    pub fn render_paused(&self, transitioning: bool) -> bool {
        self.frozen_at.is_some() || (self.paused && !transitioning)
    }

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
        let wake = paper_runtime::wake::make_pipe();
        if with_stdin && stdin_enabled() {
            let wake_pipe = wake.clone();
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
        Self::from_channel(rx, wake, video, mute, volume, with_audio)
    }

    #[cfg(test)]
    fn with_receiver(rx: std::sync::mpsc::Receiver<PaperCommand>) -> Self {
        Self {
            rx,
            audio: None,
            mute: true,
            volume: 80,
            paused: false,
            frozen_at: None,
            ducked: false,
            freeze: None,
            freeze_restore: None,
            snapshot: None,
            retained_outputs: None,
            capture: None,
            wake: paper_runtime::wake::make_pipe(),
            output_pauses: Vec::new(),
            route_output_pauses: false,
            pointer: None,
            pointer_revision: 0,
            surface_changed: false,
            system_audio: None,
            widgets: None,
            drawing_photo: None,
            reveal: false,
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
        let wake = wake.or_else(paper_runtime::wake::make_pipe);
        let volume = volume.min(100);
        let playback =
            paper_control::PlaybackState::from_env().expect("invalid playback environment");
        let audio = with_audio.then(|| AudioSink::media(video, mute, volume, playback));
        Self {
            rx,
            audio,
            mute,
            volume,
            paused: playback.paused,
            frozen_at: playback.paused.then(std::time::Instant::now),
            ducked: playback.ducked,
            freeze: None,
            freeze_restore: None,
            snapshot: None,
            retained_outputs: None,
            capture: None,
            wake,
            output_pauses: Vec::new(),
            route_output_pauses: false,
            pointer: None,
            pointer_revision: 0,
            surface_changed: false,
            system_audio: None,
            widgets: None,
            drawing_photo: None,
            reveal: false,
        }
    }

    pub fn take_surface_change(&mut self) -> bool {
        std::mem::take(&mut self.surface_changed)
    }

    pub fn take_widgets(&mut self) -> Option<paper_control::WidgetScene> {
        self.widgets.take()
    }

    pub fn pointer(&self) -> Option<(u64, [f32; 2], [bool; 3])> {
        self.pointer.map(|state| (self.pointer_revision, state.position(), state.pressed()))
    }

    pub fn scene_sound(&mut self, id: &str, op: paper_audio::VoiceOp) {
        if let Some(AudioSink::Scene(scene)) = &mut self.audio {
            scene.voice(id, op);
        }
    }

    pub fn scene_voice_states(&self) -> Vec<(String, bool)> {
        match &self.audio {
            Some(AudioSink::Scene(scene)) => scene.voice_states(),
            _ => Vec::new(),
        }
    }

    pub fn set_scene_voices(
        &mut self,
        voices: Vec<paper_audio::Voice>,
        tap: Option<paper_audio::PlaybackTap>,
    ) {
        if voices.is_empty() {
            self.audio = None;
            return;
        }
        let config = paper_audio::SceneConfig {
            mute: self.mute,
            volume: self.volume,
            wake: self.wake.clone(),
            tap,
        };
        match &mut self.audio {
            Some(AudioSink::Scene(scene)) => scene.swap(voices, config),
            _ => {
                let mut scene = paper_audio::GatedScene::new(
                    voices,
                    paper_audio::SceneConfig { mute: true, ..config },
                );
                scene.set_pause(self.paused);
                scene.set_duck(self.ducked);
                scene.set_mute(self.mute);
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
        if let Some(AudioSink::Scene(scene)) = &self.audio {
            scene.poll();
        }
        swap
    }

    fn hold(&mut self) {
        self.freeze_restore
            .get_or_insert(FreezeState { paused: self.paused, frozen_at: self.frozen_at });
        self.paused = true;
        self.frozen_at.get_or_insert_with(std::time::Instant::now);
        if let Some(audio) = &mut self.audio {
            audio.set_pause(true);
        }
    }

    pub fn set_paused(&mut self, paused: bool) {
        if let Some(previous) = self.freeze_restore.take() {
            self.frozen_at = previous.frozen_at;
        }
        self.paused = paused;
        if !paused {
            self.frozen_at = None;
        }
        if let Some(audio) = &mut self.audio {
            audio.set_pause(paused);
        }
    }

    pub fn freeze_pending(&self) -> bool {
        self.freeze.is_some() || self.snapshot.is_some()
    }

    pub fn snapshot_output(&self) -> Option<&str> {
        self.snapshot.as_ref().map(|snapshot| snapshot.output.as_str())
    }

    pub fn snapshots_surface(&self, name: &str) -> bool {
        self.snapshot.as_ref().is_none_or(|snapshot| {
            snapshot.output == paper_control::ALL_OUTPUT || snapshot.output == name
        })
    }

    pub fn take_freeze(&mut self) -> Option<(String, bool)> {
        self.snapshot
            .take()
            .map(|snapshot| (snapshot.path, false))
            .or_else(|| self.freeze.take().map(|path| (path, true)))
    }

    pub fn finish_freeze(&mut self) {
        self.freeze_restore = None;
    }

    pub fn cancel_freeze(&mut self) {
        self.freeze = None;
        if let Some(previous) = self.freeze_restore.take() {
            self.paused = previous.paused;
            self.frozen_at = previous.frozen_at;
            if let Some(audio) = &mut self.audio {
                audio.set_pause(self.paused);
            }
        }
    }

    pub fn take_capture(&mut self) -> Option<paper_control::SceneCapture> {
        self.capture.take()
    }

    pub fn route_output_pauses(&mut self) {
        self.route_output_pauses = true;
    }

    pub fn take_output_pauses(&mut self) -> Vec<(String, bool)> {
        std::mem::take(&mut self.output_pauses)
    }

    pub fn system_audio(&self) -> Option<bool> {
        self.system_audio
    }

    fn reduce(&mut self, mut cmd: PaperCommand) -> Option<SwapReq> {
        if let Some(enabled) = cmd.system_audio.take() {
            self.system_audio = Some(enabled);
        }
        if let Some(photo) = cmd.drawing_photo.take() {
            self.drawing_photo = Some(photo);
        }
        if let Some(snapshot) = cmd.snapshot.take() {
            if cmd.pause == Some(true) {
                self.hold();
            }
            self.snapshot = Some(*snapshot);
            return None;
        }
        if cmd.reveal == Some(true) {
            self.reveal = true;
            return None;
        }
        if let Some(capture) = cmd.capture.take() {
            self.capture = Some(capture);
            return None;
        }
        if self.route_output_pauses
            && let Some(paused) = cmd.pause
            && !cmd.to.is_empty()
            && cmd.to != "*"
        {
            self.output_pauses.push((cmd.to, paused));
            return None;
        }
        match paper_control::classify_command(cmd) {
            paper_control::CommandClass::Mirror(target) => {
                crate::wayland::request_mirror(target);
                None
            }
            paper_control::CommandClass::Surface(surface) => {
                if surface.validate().is_ok() {
                    crate::surface::set(&surface);
                    self.surface_changed = true;
                }
                None
            }
            paper_control::CommandClass::Widgets(scene) => {
                self.widgets = Some(*scene);
                None
            }
            paper_control::CommandClass::Pointer(state) => {
                if self.pointer != Some(state) {
                    self.pointer = Some(state);
                    self.pointer_revision = self.pointer_revision.wrapping_add(1);
                }
                None
            }
            paper_control::CommandClass::Freeze(path) => {
                tracing::info!(path, "skwd-wall-vk: freeze frame requested");
                self.hold();
                self.freeze = Some(path);
                None
            }
            paper_control::CommandClass::Pause(paused) => {
                self.set_paused(paused);
                None
            }
            paper_control::CommandClass::Duck(ducked) => {
                self.ducked = ducked;
                if let Some(audio) = &mut self.audio {
                    audio.set_duck(ducked);
                }
                None
            }
            paper_control::CommandClass::RetainOutputs(outputs) => {
                self.retained_outputs = Some(outputs);
                None
            }
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
                    drawing: swap.drawing.map(|drawing| *drawing),
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
                    self.audio = Some(AudioSink::media(
                        path,
                        self.mute,
                        self.volume,
                        paper_control::PlaybackState { paused: self.paused, ducked: self.ducked },
                    ));
                }
            }
        } else {
            self.audio = None;
        }
    }
}

mod tests;
