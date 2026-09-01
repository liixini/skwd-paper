#![cfg(test)]

use super::{signal_ready_to, write_ready};

#[test]
fn ready_token() {
    let mut out = Vec::new();
    write_ready(&mut out);
    assert_eq!(out, b"READY\n");
}

#[test]
fn ready_missing_socket() {
    let socket = std::env::temp_dir().join(format!(
        "paper-still-ready-missing-{}-{}/wall.sock",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    signal_ready_to(&socket);
}
