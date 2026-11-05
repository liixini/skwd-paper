use super::*;
use dbus::arg::Variant;

#[test]
fn metadata_uses_album_artist_fallback_and_seconds() {
    let mut metadata = PropMap::new();
    metadata.insert("xesam:title".into(), Variant(Box::new("Test track".to_owned())));
    metadata.insert("xesam:artist".into(), Variant(Box::new(vec!["Artist".to_owned()])));
    metadata.insert("mpris:length".into(), Variant(Box::new(120_000_000i64)));
    let mut properties = PropMap::new();
    properties.insert("PlaybackStatus".into(), Variant(Box::new("Playing".to_owned())));
    properties.insert("Position".into(), Variant(Box::new(3_000_000i64)));
    properties.insert("Metadata".into(), Variant(Box::new(metadata)));
    let value = events(&properties);
    assert_eq!(value["mediaPropertiesChanged"]["albumArtist"], "Artist");
    assert_eq!(value["mediaPropertiesChanged"]["title"], "Test track");
    assert_eq!(value["mediaPlaybackChanged"]["state"], 1);
    assert_eq!(value["mediaTimelineChanged"]["position"], 3.0);
    assert_eq!(value["mediaTimelineChanged"]["duration"], 120.0);
    assert_eq!(events(&PropMap::new())["mediaPlaybackChanged"]["state"], 0);
}

#[test]
fn shortcut_names() {
    for (name, method) in [
        ("bplay", "Play"),
        ("PAUSE", "Pause"),
        ("playpause", "PlayPause"),
        ("next", "Next"),
        ("bprev", "Previous"),
        ("stop", "Stop"),
    ] {
        let action = Shortcut::named(name).unwrap();
        assert_eq!(Shortcut::method(action as u8), Some(method));
    }
    assert_eq!(Shortcut::named("unknown"), None);
    assert_eq!(Shortcut::method(0), None);
}

#[test]
fn control_is_not_shutdown() {
    let (writer, reader) = UnixStream::pair().unwrap();
    reader.set_nonblocking(true).unwrap();
    assert!(!stopped(&reader));
    (&writer).write_all(&[Shortcut::Next as u8]).unwrap();
    assert!(!stopped(&reader));
    let mut byte = [0];
    (&reader).read_exact(&mut byte).unwrap();
    assert_eq!(Shortcut::method(byte[0]), Some("Next"));
    drop(writer);
    assert!(stopped(&reader));
}

#[test]
fn offline_shortcuts_are_dropped() {
    let (wake, mut receiver) = UnixStream::pair().unwrap();
    receiver.set_nonblocking(true).unwrap();
    let state = Arc::new(Mutex::new(BusState::Offline));
    let mut media = Media {
        controls: false,
        wake,
        thread: None,
        pending: Arc::new(Mutex::new(None)),
        state: state.clone(),
    };
    media.control(Shortcut::Play).unwrap();
    *state.lock().unwrap() = BusState::Online;
    media.control(Shortcut::Pause).unwrap();
    let mut commands = [0; CONTROL_BATCH];
    let count = receiver.read(&mut commands).unwrap();
    assert_eq!(&commands[..count], &[Shortcut::Pause as u8]);
}

#[test]
#[ignore = "requires isolated session bus"]
fn mpris_shortcuts() {
    let expected = std::env::var("SKWD_TEST_MEDIA_BUS").expect("private test bus");
    assert_eq!(std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap(), expected);
    let connection = Connection::new_session().unwrap();
    connection.request_name("org.mpris.MediaPlayer2.SkwdTest", false, true, false).unwrap();
    let running = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let active = running.clone();
    let (sent, received) = std::sync::mpsc::channel();
    let server = std::thread::spawn(move || {
        while active.load(std::sync::atomic::Ordering::Relaxed) {
            connection.channel().read_write(Some(Duration::from_millis(20))).unwrap();
            while let Some(message) = connection.channel().pop_message() {
                if message.msg_type() != dbus::MessageType::MethodCall {
                    continue;
                }
                let method = message.member().unwrap().to_string();
                let reply = if method == "GetAll" {
                    let mut properties = PropMap::new();
                    properties
                        .insert("PlaybackStatus".into(), Variant(Box::new("Playing".to_owned())));
                    message.method_return().append1(properties)
                } else {
                    sent.send(method).unwrap();
                    message.method_return()
                };
                connection.channel().send(reply).unwrap();
            }
        }
    });
    let mut media = Media::start().unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !media.has_pending() {
        assert!(std::time::Instant::now() < deadline, "media startup");
        std::thread::sleep(Duration::from_millis(10));
    }
    for action in [
        Shortcut::Play,
        Shortcut::Pause,
        Shortcut::Toggle,
        Shortcut::Next,
        Shortcut::Previous,
        Shortcut::Stop,
    ] {
        media.control(action).unwrap();
        assert_eq!(
            received.recv_timeout(Duration::from_secs(5)).unwrap(),
            Shortcut::method(action as u8).unwrap()
        );
    }
    drop(media);
    running.store(false, std::sync::atomic::Ordering::Relaxed);
    server.join().unwrap();
}

#[test]
#[ignore = "requires controlled session bus restart"]
fn media_bus_reconnects() {
    let expected = std::env::var("SKWD_TEST_MEDIA_BUS").expect("private test bus");
    assert_eq!(std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap(), expected);
    let marker = std::env::var("SKWD_TEST_MEDIA_EVENTS").expect("test progress path");
    let media = Media::start().unwrap();
    for (index, online) in [false, true, false, true].into_iter().enumerate() {
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(snapshot) = media.take()
                && snapshot.events[MEDIA_STATUS][ENABLED_FIELD] == online
            {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "bus phase {index}");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(media.wake_fd().is_some());
        std::fs::write(&marker, index.to_string()).unwrap();
    }
}

#[test]
fn missing_art_keeps_colours() {
    let event = thumbnail_event(None);
    assert_eq!(event["hasThumbnail"], false);
    assert_eq!(event["primaryColor"], serde_json::json!([0, 0, 0]));
}
