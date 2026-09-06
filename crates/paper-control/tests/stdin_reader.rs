use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn stdin_reader_child() {
    if std::env::var_os("PAPER_STDIN_READER_CHILD").is_none() {
        return;
    }
    paper_control::spawn_stdin_line_reader("stdin-test", |_| println!("stdin-ready"));
    std::thread::sleep(Duration::from_secs(3));
    std::process::exit(99);
}

fn check_shutdown(close_stderr: bool, invalid_input: bool) {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "stdin_reader_child", "--nocapture"])
        .env("PAPER_STDIN_READER_CHILD", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    input.write_all(b"probe\n").unwrap();
    let output = BufReader::new(child.stdout.take().unwrap());
    assert!(output.lines().any(|line| line.unwrap().contains("stdin-ready")));
    if close_stderr {
        drop(child.stderr.take());
    }
    if invalid_input {
        input.write_all(b"\xff\n").unwrap();
    }
    drop(input);
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("stdin reader did not terminate the worker");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(status.code(), Some(0), "worker shutdown: {status}");
}

#[test]
fn stdin_eof_exits_cleanly() {
    check_shutdown(false, false);
}

#[test]
fn stdin_eof_exits_cleanly_after_stderr_closes() {
    check_shutdown(true, false);
}

#[test]
fn stdin_read_error_exits_cleanly_after_stderr_closes() {
    check_shutdown(true, true);
}
