use super::*;

#[test]
fn default_socket_suffix() {
    assert!(socket_path().ends_with("skwd-wall-v2/wall.sock"));
}

#[test]
fn missing_socket_errors() {
    let missing = std::env::temp_dir().join(format!(
        "paper-control-ready-missing-{}-{}/wall.sock",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    assert!(signal_paper_ready_to(&missing).is_err());
}

#[test]
fn ready_line_shape() {
    let mut line = Vec::new();
    write_paper_ready(&mut line).unwrap();
    let line = String::from_utf8(line).unwrap();
    assert!(line.ends_with('\n'));
    let value: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
    assert_eq!(value["method"], "paper.ready");
    assert_eq!(value["params"]["pid"], std::process::id());
    assert_eq!(value["id"], 0);
}

#[test]
fn generation_ready_wire() {
    use std::io::{BufRead, BufReader};
    use std::os::unix::net::UnixListener;

    let path = std::env::temp_dir().join(format!(
        "paper-control-generation-{}-{}.sock",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    let listener = UnixListener::bind(&path).unwrap();
    let receiver = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line).unwrap();
        crate::decode_ndjson::<crate::Request>(&line).unwrap()
    });
    signal_paper_ready_generation_to(&path, 72).unwrap();
    let request = receiver.join().unwrap();
    std::fs::remove_file(path).unwrap();
    assert_eq!(
        request.params,
        crate::RequestParams::Ready(crate::RendererReady {
            pid: std::process::id(),
            generation: 72,
        })
    );
}

#[test]
fn generation_failed_wire() {
    use std::io::{BufRead, BufReader};
    use std::os::unix::net::UnixListener;

    let path = std::env::temp_dir().join(format!(
        "paper-control-failed-{}-{}.sock",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    let listener = UnixListener::bind(&path).unwrap();
    let receiver = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line).unwrap();
        crate::decode_ndjson::<crate::Request>(&line).unwrap()
    });
    signal_paper_failed_generation_to(
        &path,
        73,
        "renderer_startup",
        "[native-scene-gap:light-objects] unsupported object",
    )
    .unwrap();
    let request = receiver.join().unwrap();
    std::fs::remove_file(path).unwrap();
    assert_eq!(
        request.params,
        crate::RequestParams::Failed(crate::RendererFailed {
            pid: std::process::id(),
            generation: 73,
            code: "renderer_startup".into(),
            message: "[native-scene-gap:light-objects] unsupported object".into(),
        })
    );
}
