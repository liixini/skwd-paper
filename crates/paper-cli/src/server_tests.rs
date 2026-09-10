use super::*;
use paper_control::{
    ApplyRequest, Assignment, AudioSetRequest, CapabilitiesRequest, PauseRequest, RendererPolicy,
    Request, RequestParams, Source, StatusRequest, StopRequest, decode_ndjson, encode_ndjson,
};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;

#[test]
fn parent_perms_unchanged() {
    let temp = tempfile::tempdir().unwrap();
    let parent = temp.path().join("custom");
    std::fs::create_dir(&parent).unwrap();
    std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o750)).unwrap();
    let socket = parent.join("paper.sock");
    assert!(prepare_parent(&socket).is_err());
    assert_eq!(std::fs::symlink_metadata(&parent).unwrap().mode() & 0o777, 0o750);
}

#[test]
fn parent_and_lock_private() {
    let temp = tempfile::tempdir().unwrap();
    let socket = temp.path().join("skwd-paper-v2/paper.sock");
    let parent = prepare_parent(&socket).unwrap();
    assert_eq!(std::fs::symlink_metadata(&parent).unwrap().mode() & 0o777, 0o700);
    let lock = acquire_lock(&socket).unwrap();
    assert_eq!(
        lock.metadata().unwrap().ino(),
        parent.join("manager.lock").metadata().unwrap().ino()
    );
    let contention = acquire_lock(&socket);
    assert!(contention.is_err());
    drop(contention);
    drop(lock);
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        match acquire_lock(&socket) {
            Ok(lock) => {
                drop(lock);
                break;
            }
            Err(_) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("Paper manager lock was not released: {error}"),
        }
    }
}

#[test]
fn custom_socket_locks_are_private_independent_and_reusable() {
    let temp = tempfile::tempdir().unwrap();
    let first = temp.path().join("overview.sock");
    let second = temp.path().join("overview.other");
    let first_lock = acquire_lock(&first).unwrap();
    let second_lock = acquire_lock(&second).unwrap();
    assert_eq!(first_lock.metadata().unwrap().mode() & 0o777, 0o600);
    assert_eq!(second_lock.metadata().unwrap().mode() & 0o777, 0o600);
    assert!(acquire_lock(&first).is_err());
    assert!(acquire_lock(&second).is_err());
    drop(first_lock);
    assert!(acquire_lock(&first).is_ok());
    assert!(acquire_lock(&second).is_err());
}

#[test]
fn custom_socket_lock_rejects_symlinks_and_shared_files() {
    let temp = tempfile::tempdir().unwrap();
    let socket = temp.path().join("overview-backdrop.sock");
    let lock = temp.path().join("overview-backdrop.sock.manager.lock");
    let target = temp.path().join("target");
    std::fs::write(&target, "preserve").unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::os::unix::fs::symlink(&target, &lock).unwrap();
    assert!(acquire_lock(&socket).is_err());
    std::fs::remove_file(&lock).unwrap();
    std::fs::hard_link(&target, &lock).unwrap();
    assert!(acquire_lock(&socket).is_err());
    std::fs::remove_file(&lock).unwrap();
    std::fs::write(&lock, "").unwrap();
    std::fs::set_permissions(&lock, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(acquire_lock(&socket).is_err());
    assert_eq!(std::fs::read_to_string(target).unwrap(), "preserve");
}

#[test]
fn overview_and_default_controllers_coexist_in_either_start_order() {
    for names in
        [["paper.sock", "overview-backdrop.sock"], ["overview-backdrop.sock", "paper.sock"]]
    {
        let temp = tempfile::tempdir().unwrap();
        let sockets = names.map(|name| temp.path().join("runtime").join(name));
        let mut servers = Vec::new();
        for socket in &sockets {
            let path = socket.clone();
            servers.push(std::thread::spawn(move || run_at(&path).unwrap()));
            wait_for_socket(socket);
            let inode = socket.metadata().unwrap().ino();
            assert!(
                run_at(socket).unwrap_err().to_string().contains("already starting or running")
            );
            assert_eq!(socket.metadata().unwrap().ino(), inode);
        }
        let pause = serde_json::to_value(request(
            &sockets[0],
            &Request::new(1, RequestParams::Pause(PauseRequest { paused: true })),
        ))
        .unwrap();
        assert_eq!(pause["result"]["paused"], true);
        let status = serde_json::to_value(request(
            &sockets[1],
            &Request::new(2, RequestParams::Status(StatusRequest {})),
        ))
        .unwrap();
        assert_eq!(status["result"]["paused"], false);
        for socket in &sockets {
            request(socket, &Request::new(3, RequestParams::Stop(StopRequest::default())));
        }
        for server in servers {
            server.join().unwrap();
        }
        for socket in &sockets {
            assert!(!socket.exists());
            assert!(acquire_lock(socket).is_ok());
        }
    }
}

#[test]
fn custom_controller_recovers_stale_socket_without_replacing_regular_files() {
    let temp = tempfile::tempdir().unwrap();
    let socket = temp.path().join("runtime/overview-backdrop.sock");
    prepare_parent(&socket).unwrap();
    drop(StdUnixListener::bind(&socket).unwrap());
    let path = socket.clone();
    let server = std::thread::spawn(move || run_at(&path).unwrap());
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if StdUnixStream::connect(&socket).is_ok() {
            break;
        }
        assert!(Instant::now() < deadline, "stale socket was not recovered");
        std::thread::sleep(Duration::from_millis(10));
    }
    request(&socket, &Request::new(1, RequestParams::Stop(StopRequest::default())));
    server.join().unwrap();
    std::fs::write(&socket, "preserve").unwrap();
    assert!(run_at(&socket).unwrap_err().to_string().contains("refusing to replace non-socket"));
    assert_eq!(std::fs::read_to_string(socket).unwrap(), "preserve");
}

#[test]
fn empty_manager_grace() {
    let temp = tempfile::tempdir().unwrap();
    let socket = temp.path().join("skwd-paper-v2/paper.sock");
    let server_socket = socket.clone();
    let server = std::thread::spawn(move || run_at(&server_socket).unwrap());
    wait_for_socket(&socket);

    let status = request(&socket, &Request::new(1, RequestParams::Status(StatusRequest {})));
    assert_eq!(status.id, 1);
    let capabilities = request(
        &socket,
        &Request::new(2, RequestParams::Capabilities(CapabilitiesRequest::default())),
    );
    assert_eq!(capabilities.id, 2);
    server.join().unwrap();
    assert!(!socket.exists());
}

#[test]
fn status_and_capabilities_report_each_missing_renderer() {
    for missing_vk in [true, false] {
        let temp = tempfile::tempdir().unwrap();
        let available = temp.path().join("available-worker");
        std::fs::write(&available, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&available, std::fs::Permissions::from_mode(0o700)).unwrap();
        let missing = temp.path().join("missing-worker");
        let (vk, still) =
            if missing_vk { (missing.clone(), available) } else { (available, missing.clone()) };
        let manager = Manager::with_backends(
            crate::backend::BackendPaths::from_executables(vk, still),
            temp.path().to_path_buf(),
        );
        let socket = temp.path().join("runtime/paper.sock");
        let server_socket = socket.clone();
        let server = std::thread::spawn(move || run_with_manager(&server_socket, manager).unwrap());
        wait_for_socket(&socket);

        let status = serde_json::to_value(request(
            &socket,
            &Request::new(40, RequestParams::Status(StatusRequest {})),
        ))
        .unwrap();
        let capabilities = serde_json::to_value(request(
            &socket,
            &Request::new(41, RequestParams::Capabilities(CapabilitiesRequest::default())),
        ))
        .unwrap();
        let status_renderers = status["result"]["renderers"].as_array().unwrap();
        let capability_renderers = capabilities["result"]["renderers"].as_array().unwrap();
        assert_eq!(status_renderers, capability_renderers);
        let index = usize::from(!missing_vk);
        assert_eq!(status_renderers[index]["present"], false);
        assert_eq!(status_renderers[index]["executable_file"], false);
        assert_eq!(status_renderers[index]["path"], missing.display().to_string());
        assert_eq!(status_renderers[usize::from(missing_vk)]["executable_file"], true);
        let source = if missing_vk {
            Source::video("/wall/a.mp4", None)
        } else {
            Source::static_file("/wall/a.png")
        };
        let apply = Request::new(
            42,
            RequestParams::Apply(ApplyRequest {
                assignments: vec![Assignment::new(vec!["DP-1".into()], source)],
                replace_all: false,
                policy: None,
            }),
        );
        let apply = serde_json::to_value(request(&socket, &apply)).unwrap();
        assert_eq!(apply["error"]["code"], "renderer_unavailable");
        let message = apply["error"]["message"].as_str().unwrap();
        assert!(message.contains("media capability requires"));
        assert!(message.contains(if missing_vk { "skwd-wall-vk" } else { "skwd-wall-still" }));
        server.join().unwrap();
    }
}

#[test]
fn incomplete_client_timeout() {
    let temp = tempfile::tempdir().unwrap();
    let socket = temp.path().join("skwd-paper-v2/paper.sock");
    let server_socket = socket.clone();
    let server = std::thread::spawn(move || run_at(&server_socket).unwrap());
    wait_for_socket(&socket);

    let mut stalled = UnixStream::connect(&socket).unwrap();
    stalled.write_all(b"{").unwrap();
    let started = Instant::now();
    let status = request(&socket, &Request::new(3, RequestParams::Status(StatusRequest {})));
    assert_eq!(status.id, 3);
    assert!(started.elapsed() >= CONNECTION_TIMEOUT);
    drop(stalled);
    server.join().unwrap();
}

#[test]
fn mixed_apply_lifecycle() {
    let temp = tempfile::tempdir().unwrap();
    let worker = temp.path().join("worker");
    std::fs::write(
        &worker,
        concat!(
            "#!/usr/bin/python3\n",
            "import json, os, socket, sys\n",
            "peer = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)\n",
            "peer.connect(os.environ['SKWD_PAPER_READY_SOCKET'])\n",
            "message = {'id': 0, 'method': 'paper.ready', 'params': {'pid': os.getpid(), 'generation': int(os.environ['SKWD_PAPER_GENERATION'])}}\n",
            "peer.sendall((json.dumps(message, separators=(',', ':')) + '\\n').encode())\n",
            "peer.close()\n",
            "for line in sys.stdin:\n",
            "    command = json.loads(line)\n",
            "    if 'freeze' in command:\n",
            "        temporary = command['freeze'] + '.part'\n",
            "        fd = os.open(temporary, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)\n",
            "        with os.fdopen(fd, 'wb') as frame:\n",
            "            frame.write(b'P6\\n1 1\\n255\\n\\0\\0\\0')\n",
            "            frame.flush()\n",
            "            os.fsync(frame.fileno())\n",
            "        os.link(temporary, command['freeze'])\n",
            "        os.unlink(temporary)\n",
        ),
    )
    .unwrap();
    std::fs::set_permissions(&worker, std::fs::Permissions::from_mode(0o700)).unwrap();
    let manager = Manager::with_backends(
        crate::backend::BackendPaths::from_executables(worker.clone(), worker),
        temp.path().to_path_buf(),
    );
    let socket = temp.path().join("runtime/paper.sock");
    let server_socket = socket.clone();
    let server = std::thread::spawn(move || run_with_manager(&server_socket, manager).unwrap());
    wait_for_socket(&socket);

    let apply = Request::new(
        4,
        RequestParams::Apply(ApplyRequest {
            assignments: vec![
                Assignment::new(vec!["DP-1".into()], Source::static_file("/wall/a.png")),
                Assignment::new(vec!["DP-2".into()], Source::video("/wall/b.mp4", None)),
            ],
            replace_all: false,
            policy: Some(RendererPolicy { idle_seconds: Some(5), ..Default::default() }),
        }),
    );
    let apply = serde_json::to_value(request(&socket, &apply)).unwrap();
    assert_eq!(apply["result"]["assignments"].as_array().unwrap().len(), 2);
    assert_eq!(apply["result"]["paused"], false);
    assert_eq!(apply["result"]["policy"]["idle_seconds"], 5);
    let pause = serde_json::to_value(request(
        &socket,
        &Request::new(5, RequestParams::Pause(PauseRequest { paused: true })),
    ))
    .unwrap();
    assert_eq!(pause["result"]["paused"], true);
    let audio = serde_json::to_value(request(
        &socket,
        &Request::new(
            6,
            RequestParams::AudioSet(AudioSetRequest {
                outputs: vec!["DP-1".into(), "DP-2".into()],
                mute: Some(false),
                volume: Some(35),
            }),
        ),
    ))
    .unwrap();
    assert_eq!(audio["result"]["updated"], 2);
    let status = serde_json::to_value(request(
        &socket,
        &Request::new(7, RequestParams::Status(StatusRequest {})),
    ))
    .unwrap();
    assert_eq!(status["result"]["assignments"].as_array().unwrap().len(), 2);
    assert_eq!(status["result"]["paused"], true);
    assert_eq!(status["result"]["policy"]["idle_seconds"], 5);
    assert!(
        status["result"]["assignments"]
            .as_array()
            .unwrap()
            .iter()
            .all(|assignment| assignment["mute"] == false && assignment["volume"] == 35)
    );
    let stop = serde_json::to_value(request(
        &socket,
        &Request::new(8, RequestParams::Stop(StopRequest::default())),
    ))
    .unwrap();
    assert_eq!(stop["result"]["stopped"], 2);
    server.join().unwrap();
}

#[test]
fn startup_failure_code() {
    let temp = tempfile::tempdir().unwrap();
    let worker = temp.path().join("worker");
    std::fs::write(
        &worker,
        concat!(
            "#!/usr/bin/python3\n",
            "import json, os, socket\n",
            "peer = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)\n",
            "peer.connect(os.environ['SKWD_PAPER_READY_SOCKET'])\n",
            "message = {'id': 0, 'method': 'paper.failed', 'params': {",
            "'pid': os.getpid(), 'generation': int(os.environ['SKWD_PAPER_GENERATION']), ",
            "'code': 'renderer_startup', 'message': '[native-scene-gap:light-objects] unsupported object'}}\n",
            "peer.sendall((json.dumps(message, separators=(',', ':')) + '\\n').encode())\n",
            "peer.close()\n",
        ),
    )
    .unwrap();
    std::fs::set_permissions(&worker, std::fs::Permissions::from_mode(0o700)).unwrap();
    let manager = Manager::with_backends(
        crate::backend::BackendPaths::from_executables(worker.clone(), worker),
        temp.path().to_path_buf(),
    );
    let socket = temp.path().join("runtime/paper.sock");
    let server_socket = socket.clone();
    let server = std::thread::spawn(move || run_with_manager(&server_socket, manager).unwrap());
    wait_for_socket(&socket);

    let apply = Request::new(
        20,
        RequestParams::Apply(ApplyRequest {
            assignments: vec![Assignment::new(
                vec!["DP-1".into()],
                Source::video("/wall/b.mp4", None),
            )],
            replace_all: false,
            policy: None,
        }),
    );
    let apply = serde_json::to_value(request(&socket, &apply)).unwrap();
    assert_eq!(apply["error"]["code"], "apply_failed");
    assert!(apply["error"]["message"].as_str().unwrap().contains("native-scene-gap:light-objects"));
    server.join().unwrap();
}

#[test]
fn idle_loop_no_sweeps() {
    let temp = tempfile::tempdir().unwrap();
    let worker = temp.path().join("worker");
    std::fs::write(
        &worker,
        concat!(
            "#!/usr/bin/python3\n",
            "import json, os, socket, sys\n",
            "peer = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)\n",
            "peer.connect(os.environ['SKWD_PAPER_READY_SOCKET'])\n",
            "message = {'id': 0, 'method': 'paper.ready', 'params': {'pid': os.getpid(), 'generation': int(os.environ['SKWD_PAPER_GENERATION'])}}\n",
            "peer.sendall((json.dumps(message, separators=(',', ':')) + '\\n').encode())\n",
            "peer.close()\n",
            "for line in sys.stdin:\n",
            "    pass\n",
        ),
    )
    .unwrap();
    std::fs::set_permissions(&worker, std::fs::Permissions::from_mode(0o700)).unwrap();
    let manager = Manager::with_backends(
        crate::backend::BackendPaths::from_executables(worker.clone(), worker),
        temp.path().to_path_buf(),
    );
    let sweeps = manager.refresh_sweeps();
    let socket = temp.path().join("runtime/paper.sock");
    let server_socket = socket.clone();
    let server = std::thread::spawn(move || run_with_manager(&server_socket, manager).unwrap());
    wait_for_socket(&socket);

    let apply = Request::new(
        30,
        RequestParams::Apply(ApplyRequest {
            assignments: vec![Assignment::new(
                vec!["DP-1".into()],
                Source::static_file("/wall/a.png"),
            )],
            replace_all: false,
            policy: None,
        }),
    );
    let apply = serde_json::to_value(request(&socket, &apply)).unwrap();
    assert_eq!(apply["result"]["assignments"].as_array().unwrap().len(), 1);
    let baseline = sweeps.load(std::sync::atomic::Ordering::Relaxed);
    std::thread::sleep(Duration::from_secs(1));
    assert_eq!(sweeps.load(std::sync::atomic::Ordering::Relaxed), baseline);
    let stop = serde_json::to_value(request(
        &socket,
        &Request::new(31, RequestParams::Stop(StopRequest::default())),
    ))
    .unwrap();
    assert_eq!(stop["result"]["stopped"], 1);
    server.join().unwrap();
}

fn wait_for_socket(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while !path.exists() {
        assert!(Instant::now() < deadline, "server did not bind");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn request(path: &Path, request: &Request) -> paper_control::Response<serde_json::Value> {
    let mut stream = UnixStream::connect(path).unwrap();
    stream.write_all(encode_ndjson(&request).unwrap().as_bytes()).unwrap();
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).unwrap();
    decode_ndjson(&line).unwrap()
}
