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
const LIMIT: u64 = 8 * 1024 * 1024;

pub(super) struct Snapshot {
    pub events: Value,
    pub art: Option<Pixels>,
    pub art_changed: bool,
}

pub(super) struct Media {
    wake: UnixStream,
    thread: Option<JoinHandle<()>>,
    pending: Arc<Mutex<Option<Snapshot>>>,
}

impl Media {
    pub fn start() -> Result<Self> {
        let (wake, stop) = UnixStream::pair()?;
        wake.set_nonblocking(true)?;
        stop.set_nonblocking(true)?;
        let pending = Arc::new(Mutex::new(None));
        let output = pending.clone();
        let thread = std::thread::Builder::new().name("skwd-media".into()).spawn(move || {
            if let Err(error) = watch(stop, output) {
                tracing::warn!("Wallpaper media integration stopped: {error:#}");
            }
        })?;
        Ok(Self { wake, thread: Some(thread), pending })
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
    let mut fd = libc::pollfd { fd: stop.as_raw_fd(), events: libc::POLLIN, revents: 0 };
    unsafe { libc::poll(&mut fd, 1, 0) > 0 }
}

fn watch(stop: UnixStream, output: Arc<Mutex<Option<Snapshot>>>) -> Result<()> {
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
    let mut previous = String::new();
    let mut art_url = String::new();
    let mut art = None;
    let mut thumbnail = json!({"hasThumbnail":false});
    loop {
        if stopped(&stop) {
            break;
        }
        let (name, properties) = current(&connection, &previous, &stop)?;
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
        if let Ok(mut pending) = output.lock() {
            let notify = pending.is_none();
            let changed = art_changed || pending.as_ref().is_some_and(|p| p.art_changed);
            *pending = Some(Snapshot { events, art: art.clone(), art_changed: changed });
            if notify {
                let _ = (&stop).write(&[1]);
            }
        }
        loop {
            let mut changed = false;
            while let Some(message) = connection.channel().pop_message() {
                changed |= message.msg_type() == dbus::MessageType::Signal;
            }
            if changed || stopped(&stop) {
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
                return Ok(());
            }
            connection
                .channel()
                .read_write(Some(Duration::ZERO))
                .map_err(|_| anyhow::anyhow!("media bus disconnected"))?;
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
                    false,
                )?;
                let old = std::mem::replace(&mut self.textures[*slot], next);
                self.renderer.destroy_scene_texture(old);
            }
            self.media_art = snapshot.art;
        }
        if let Some(scripts) = &mut self.scripts {
            scripts.host.media(&snapshot.events)?;
        }
        Ok(())
    }
}
