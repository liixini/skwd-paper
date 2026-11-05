use anyhow::{Context, Result};
use dbus::arg::{PropMap, RefArg, prop_cast};
use dbus::blocking::Connection;
use dbus::blocking::stdintf::org_freedesktop_dbus::Properties;
use paper_scene::tex::Pixels;
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

const PLAYER: &str = "org.mpris.MediaPlayer2.Player";
const PATH: &str = "/org/mpris/MediaPlayer2";
const CONTROL_TIMEOUT: Duration = Duration::from_millis(150);
const CONTROL_BATCH: usize = 32;
const RECONNECT_DELAY: Duration = Duration::from_secs(1);
const MEDIA_STATUS: &str = "mediaStatusChanged";
const ENABLED_FIELD: &str = "enabled";
const PEEK_FLAGS: libc::c_int = libc::MSG_PEEK | libc::MSG_DONTWAIT;

enum BusState {
    Starting,
    Online,
    Offline,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Shortcut {
    Play = 1,
    Pause,
    Toggle,
    Next,
    Previous,
    Stop,
}

impl Shortcut {
    pub(super) fn named(name: &str) -> Option<Self> {
        const NAMES: &[(&str, Shortcut)] = &[
            ("play", Shortcut::Play),
            ("bplay", Shortcut::Play),
            ("pause", Shortcut::Pause),
            ("bpause", Shortcut::Pause),
            ("playpause", Shortcut::Toggle),
            ("toggle", Shortcut::Toggle),
            ("next", Shortcut::Next),
            ("bnext", Shortcut::Next),
            ("previous", Shortcut::Previous),
            ("prev", Shortcut::Previous),
            ("bprev", Shortcut::Previous),
            ("stop", Shortcut::Stop),
        ];
        NAMES.iter().find(|(alias, _)| name.eq_ignore_ascii_case(alias)).map(|(_, action)| *action)
    }

    fn method(byte: u8) -> Option<&'static str> {
        const METHODS: &[(Shortcut, &str)] = &[
            (Shortcut::Play, "Play"),
            (Shortcut::Pause, "Pause"),
            (Shortcut::Toggle, "PlayPause"),
            (Shortcut::Next, "Next"),
            (Shortcut::Previous, "Previous"),
            (Shortcut::Stop, "Stop"),
        ];
        METHODS.iter().find(|(action, _)| *action as u8 == byte).map(|(_, method)| *method)
    }
}

const LIMIT: u64 = 8 * 1024 * 1024;

pub(super) struct Snapshot {
    pub events: Value,
    pub art: Option<Pixels>,
    pub art_changed: bool,
}

pub(super) struct Media {
    pub(super) controls: bool,
    wake: UnixStream,
    thread: Option<JoinHandle<()>>,
    pending: Arc<Mutex<Option<Snapshot>>>,
    state: Arc<Mutex<BusState>>,
}

impl Media {
    pub fn start() -> Result<Self> {
        let (wake, stop) = UnixStream::pair()?;
        wake.set_nonblocking(true)?;
        stop.set_nonblocking(true)?;
        let pending = Arc::new(Mutex::new(None));
        let output = pending.clone();
        let state = Arc::new(Mutex::new(BusState::Starting));
        let status = state.clone();
        let thread = std::thread::Builder::new().name("skwd-media".into()).spawn(move || {
            if let Err(error) = supervise(&stop, &output, &status) {
                tracing::warn!("media integration stopped: {error:#}");
            }
        })?;
        Ok(Self { controls: false, wake, thread: Some(thread), pending, state })
    }

    pub(super) fn control(&mut self, action: Shortcut) -> Result<()> {
        let state = self.state.lock().map_err(|_| anyhow::anyhow!("media state poisoned"))?;
        if matches!(*state, BusState::Offline) {
            return Ok(());
        }
        (&self.wake).write_all(&[action as u8]).context("queue media shortcut")?;
        self.controls = true;
        Ok(())
    }

    pub fn take(&self) -> Option<Snapshot> {
        let mut pending = self.pending.lock().ok()?;
        let mut byte = [0];
        let _ = (&self.wake).read(&mut byte);
        pending.take()
    }
    pub fn has_pending(&self) -> bool {
        self.pending.lock().is_ok_and(|pending| pending.is_some())
    }

    pub fn wake_fd(&self) -> Option<i32> {
        self.thread.as_ref().filter(|thread| !thread.is_finished()).map(|_| self.wake.as_raw_fd())
    }
}

impl Drop for Media {
    fn drop(&mut self) {
        let _ = self.wake.shutdown(std::net::Shutdown::Both);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn text(map: &PropMap, name: &str) -> String {
    map.get(name).and_then(|v| v.0.as_str()).unwrap_or("").chars().take(4096).collect()
}

fn artist(map: &PropMap, name: &str) -> String {
    map.get(name)
        .and_then(|v| v.0.as_iter())
        .map(|v| {
            v.filter_map(|v| v.as_str())
                .take(16)
                .collect::<Vec<_>>()
                .join(", ")
                .chars()
                .take(4096)
                .collect()
        })
        .unwrap_or_default()
}

fn events(properties: &PropMap) -> Value {
    let empty = PropMap::new();
    let metadata = prop_cast::<PropMap>(properties, "Metadata").unwrap_or(&empty);
    let artist = artist(metadata, "xesam:artist");
    let album_artist = self::artist(metadata, "xesam:albumArtist");
    json!({
        "mediaStatusChanged":{"enabled":true},
        "mediaPlaybackChanged":{"state":match text(properties,"PlaybackStatus").as_str(){"Playing"=>1,"Paused"=>2,_=>0}},
        "mediaPropertiesChanged":{"title":text(metadata,"xesam:title"),"artist":artist,"albumTitle":text(metadata,"xesam:album"),"albumArtist":if album_artist.is_empty(){artist}else{album_artist},"contentType":"music"},
        "mediaTimelineChanged":{"position":properties.get("Position").and_then(|v|v.0.as_i64()).unwrap_or(0).max(0) as f64/1_000_000.0,"duration":metadata.get("mpris:length").and_then(|v|v.0.as_i64()).unwrap_or(0).max(0) as f64/1_000_000.0}
    })
}

fn current(
    connection: &Connection,
    previous: &str,
    stop: &UnixStream,
) -> Result<(String, PropMap)> {
    let proxy = connection.with_proxy(
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        Duration::from_millis(250),
    );
    let (mut names,): (Vec<String>,) =
        proxy.method_call("org.freedesktop.DBus", "ListNames", ())?;
    names.retain(|name| name.starts_with("org.mpris.MediaPlayer2."));
    names.sort_by_key(|name| (name != previous, name.clone()));
    let mut selected = (String::new(), PropMap::new());
    for name in names.into_iter().take(32) {
        if stopped(stop) {
            break;
        }
        let proxy = connection.with_proxy(name.as_str(), PATH, Duration::from_millis(150));
        let Ok(properties) = proxy.get_all(PLAYER) else { continue };
        if text(&properties, "PlaybackStatus") == "Playing" {
            return Ok((name, properties));
        }
        if selected.0.is_empty()
            || (text(&selected.1, "PlaybackStatus") != "Paused"
                && text(&properties, "PlaybackStatus") == "Paused")
        {
            selected = (name, properties);
        }
    }
    Ok(selected)
}

fn stopped(stop: &UnixStream) -> bool {
    let mut byte = 0_u8;
    unsafe { libc::recv(stop.as_raw_fd(), (&raw mut byte).cast(), 1, PEEK_FLAGS) == 0 }
}

fn publish(output: &Mutex<Option<Snapshot>>, mut wake: &UnixStream, mut snapshot: Snapshot) {
    if let Ok(mut pending) = output.lock() {
        let notify = pending.is_none();
        snapshot.art_changed |= pending.as_ref().is_some_and(|previous| previous.art_changed);
        *pending = Some(snapshot);
        if notify {
            let _ = wake.write(&[1]);
        }
    }
}

fn supervise(
    mut stop: &UnixStream,
    output: &Mutex<Option<Snapshot>>,
    state: &Mutex<BusState>,
) -> Result<()> {
    let mut offline = false;
    loop {
        if stopped(stop) {
            return Ok(());
        }
        let result = connect().and_then(|connection| {
            offline = false;
            *state.lock().map_err(|_| anyhow::anyhow!("media state poisoned"))? = BusState::Online;
            watch(&connection, stop, output)
        });
        let Err(error) = result else { return Ok(()) };
        {
            let mut status = state.lock().map_err(|_| anyhow::anyhow!("media state poisoned"))?;
            *status = BusState::Offline;
            let mut discarded = [0; CONTROL_BATCH];
            while stop.read(&mut discarded).is_ok_and(|count| count > 0) {}
        }
        if !offline {
            tracing::warn!(%error, "media bus unavailable");
            let mut events = events(&PropMap::new());
            events[MEDIA_STATUS][ENABLED_FIELD] = Value::Bool(false);
            publish(output, stop, Snapshot { events, art: None, art_changed: true });
            offline = true;
        }
        let mut poll = libc::pollfd { fd: stop.as_raw_fd(), events: 0, revents: 0 };
        let result = unsafe { libc::poll(&raw mut poll, 1, RECONNECT_DELAY.as_millis() as i32) };
        if result < 0 && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            return Err(std::io::Error::last_os_error().into());
        }
    }
}

fn connect() -> Result<Connection> {
    let mut channel = dbus::channel::Channel::get_private(dbus::channel::BusType::Session)?;
    channel.set_watch_enabled(true);
    let connection = Connection::from(channel);
    for rule in [
        "type='signal',interface='org.freedesktop.DBus.Properties',member='PropertiesChanged',path='/org/mpris/MediaPlayer2'",
        "type='signal',interface='org.freedesktop.DBus',member='NameOwnerChanged',arg0namespace='org.mpris.MediaPlayer2'",
        "type='signal',interface='org.mpris.MediaPlayer2.Player',member='Seeked'",
    ] {
        connection.add_match_no_cb(rule)?;
    }
    Ok(connection)
}

fn watch(
    connection: &Connection,
    stop: &UnixStream,
    output: &Mutex<Option<Snapshot>>,
) -> Result<()> {
    let mut previous = String::new();
    let mut art_url = String::new();
    let mut art = None;
    let mut thumbnail = thumbnail_event(None);
    loop {
        if stopped(stop) {
            break;
        }
        let (name, properties) = current(connection, &previous, stop)?;
        previous = name;
        let url = prop_cast::<PropMap>(&properties, "Metadata")
            .map(|m| text(m, "mpris:artUrl"))
            .unwrap_or_default();
        let art_changed = url != art_url;
        if art_changed {
            art = load_art(&url).ok();
            thumbnail = thumbnail_event(art.as_ref());
            art_url = url;
        }
        let mut events = events(&properties);
        events["mediaThumbnailChanged"] = thumbnail.clone();
        publish(output, stop, Snapshot { events, art: art.clone(), art_changed });
        loop {
            let mut changed = false;
            while let Some(message) = connection.channel().pop_message() {
                changed |= message.msg_type() == dbus::MessageType::Signal;
            }
            if changed || stopped(stop) {
                break;
            }
            let watch = connection.channel().watch();
            let mut fds = [
                libc::pollfd {
                    fd: watch.fd,
                    events: libc::POLLIN | if watch.write { libc::POLLOUT } else { 0 },
                    revents: 0,
                },
                libc::pollfd { fd: stop.as_raw_fd(), events: libc::POLLIN, revents: 0 },
            ];
            let result = unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) };
            if result < 0 {
                if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(std::io::Error::last_os_error().into());
            }
            if fds[1].revents != 0 {
                if stopped(stop) {
                    return Ok(());
                }
                dispatch(connection, &previous, stop)?;
                break;
            }
            connection
                .channel()
                .read_write(Some(Duration::ZERO))
                .map_err(|_| anyhow::anyhow!("media bus disconnected"))?;
        }
    }
    Ok(())
}

fn dispatch(connection: &Connection, player: &str, mut control: &UnixStream) -> Result<()> {
    let mut batch = [0; CONTROL_BATCH];
    let count = control.read(&mut batch)?;
    if player.is_empty() {
        return Ok(());
    }
    let proxy = connection.with_proxy(player, PATH, CONTROL_TIMEOUT);
    for byte in &batch[..count] {
        if let Some(method) = Shortcut::method(*byte)
            && let Err(error) = proxy.method_call::<(), _, _, _>(PLAYER, method, ())
        {
            tracing::warn!(method, %error, "media shortcut failed");
        }
    }
    Ok(())
}

fn load_art(raw: &str) -> Result<Pixels> {
    let url = url::Url::parse(raw)?;
    let bytes = match url.scheme() {
        "file" => {
            let file = std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(url.to_file_path().map_err(|_| anyhow::anyhow!("invalid art path"))?)?;
            anyhow::ensure!(file.metadata()?.is_file(), "album art is not a file");
            let mut bytes = Vec::new();
            file.take(LIMIT + 1).read_to_end(&mut bytes)?;
            bytes
        }
        "http" | "https" => {
            let agent: ureq::Agent = ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(2)))
                .build()
                .into();
            agent.get(raw).call()?.body_mut().with_config().limit(LIMIT).read_to_vec()?
        }
        _ => anyhow::bail!("unsupported album art URL"),
    };
    anyhow::ensure!(bytes.len() <= LIMIT as usize, "album art exceeds 8 MiB");
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().context("decode media album art")?.thumbnail(1024, 1024).to_rgba8();
    Ok(Pixels::rgba(image.width(), image.height(), image.into_raw()))
}

fn thumbnail_event(pixels: Option<&Pixels>) -> Value {
    let Some(pixels) = pixels else {
        return json!({"hasThumbnail":false,"primaryColor":[0,0,0],"secondaryColor":[0,0,0],"tertiaryColor":[0,0,0],"textColor":[1,1,1],"highContrastColor":[1,1,1]});
    };
    let data = &pixels.levels[0].data;
    let mut color = [0.0f64; 3];
    for pixel in data.chunks_exact(4) {
        for i in 0..3 {
            color[i] += f64::from(pixel[i]);
        }
    }
    for channel in &mut color {
        *channel /= (data.len() / 4).max(1) as f64 * 255.0;
    }
    let contrast = if color[0] * 0.2126 + color[1] * 0.7152 + color[2] * 0.0722 > 0.5 {
        [0.0; 3]
    } else {
        [1.0; 3]
    };
    json!({"hasThumbnail":true,"primaryColor":color,"secondaryColor":color,"tertiaryColor":color,"textColor":contrast,"highContrastColor":contrast})
}

#[cfg(test)]
mod tests;

impl super::Group {
    pub(super) fn advance_media(&mut self) -> Result<()> {
        let wanted = !self.media_slots.is_empty()
            || self
                .scripts
                .as_ref()
                .and_then(|scripts| scripts.host.as_ref())
                .is_some_and(paper_scene::script::SceneScripts::needs_media)
            || self.media.as_ref().is_some_and(|media| media.controls);
        if wanted && self.media.is_none() {
            self.media = Some(Media::start()?);
        } else if !wanted {
            self.media.take();
            self.media_art.take();
        }
        let Some(snapshot) = self.media.as_ref().and_then(Media::take) else {
            return Ok(());
        };
        if snapshot.art_changed {
            for (kind, slot) in &self.media_slots {
                let image = match kind {
                    paper_scene::model::MediaTexture::Current => snapshot.art.as_ref(),
                    paper_scene::model::MediaTexture::Previous => self.media_art.as_ref(),
                };
                let empty = Pixels::rgba(1, 1, vec![0; 4]);
                let next = self.renderer.create_scene_texture_pixels(
                    image.unwrap_or(&empty),
                    true,
                    false,
                    crate::vk::MipSampling::Disabled,
                )?;
                let old = std::mem::replace(&mut self.textures[*slot], next);
                self.renderer.destroy_scene_texture(old);
            }
            self.media_art = snapshot.art;
        }
        if let Some(host) = self.scripts.as_mut().and_then(|s| s.host.as_mut()) {
            host.media(&snapshot.events)?;
        }
        Ok(())
    }
}
