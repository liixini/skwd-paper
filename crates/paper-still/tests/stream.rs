#![cfg(test)]

use std::io::{Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use paper_control::{ClockStyle, Depth, Face, Labels, Palette, Pin, Widget, WidgetScene};

struct Stream {
    child: Child,
    frames: Receiver<Vec<u8>>,
    _directory: tempfile::TempDir,
}

impl Stream {
    fn open() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let image = directory.path().join("wall.png");
        image::RgbaImage::from_pixel(320, 200, image::Rgba([32, 32, 32, 255]))
            .save(&image)
            .unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_skwd-wall-still"))
            .args(["DP-1", image.to_str().unwrap(), "--frame-stream", "320x200"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut output = child.stdout.take().unwrap();
        let (sender, frames) = channel();
        std::thread::spawn(move || {
            let mut header = [0; 12];
            output.read_exact(&mut header).unwrap();
            assert_eq!(&header, b"SKWP\x40\x01\0\0\xc8\0\0\0");
            loop {
                let mut frame = vec![0; 320 * 200 * 4];
                if output.read_exact(&mut frame).is_err() || sender.send(frame).is_err() {
                    return;
                }
            }
        });
        Self { child, frames, _directory: directory }
    }

    fn frame(&self) -> Vec<u8> {
        self.frames.recv_timeout(Duration::from_secs(10)).unwrap()
    }

    fn send(&mut self, scene: &WidgetScene) {
        let mut command = paper_control::StillCommand::new("");
        command.widgets = Some(scene.clone());
        self.child.stdin.as_mut().unwrap().write_all(command.line().as_bytes()).unwrap();
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn clock() -> WidgetScene {
    WidgetScene {
        widgets: vec![Widget {
            face: Face::Clock(ClockStyle::Aperture),
            depth: Depth::Front,
            pin: Pin { anchor: [0.5, 0.5], offset: [0.0, 0.0] },
            height: Some(0.5),
            output: Some("DP-1".into()),
        }],
        outlines: Vec::new(),
        font: None,
        palette: Palette { text: [255; 3], accent: [255; 3], shade: [0; 3] },
        labels: Labels {
            days: std::array::from_fn(|_| "MON".into()),
            day_names: std::array::from_fn(|_| "MONDAY".into()),
            months: std::array::from_fn(|_| "JAN".into()),
            month_names: std::array::from_fn(|_| "JANUARY".into()),
        },
    }
}

#[test]
fn applies_fragmented_scene() {
    let mut stream = Stream::open();
    let bare = stream.frame();
    assert!(bare.chunks_exact(4).all(|pixel| pixel == [32, 32, 32, 255]));
    let scene = clock();
    let mut command = paper_control::StillCommand::new("");
    command.widgets = Some(scene.clone());
    let line = command.line();
    let mut wire = line.trim_end().as_bytes().to_vec();
    wire.extend(std::iter::repeat_n(b' ', 7000));
    wire.push(b'\n');
    let split = wire.len() / 2;
    stream.child.stdin.as_mut().unwrap().write_all(&wire[..split]).unwrap();
    assert!(stream.frames.recv_timeout(Duration::from_millis(100)).is_err());
    stream.child.stdin.as_mut().unwrap().write_all(&wire[split..]).unwrap();
    assert_ne!(stream.frame(), bare);
    let mut hidden = scene.clone();
    hidden.widgets.clear();
    stream.send(&hidden);
    assert_eq!(stream.frame(), bare);
    stream.send(&scene);
    assert_ne!(stream.frame(), bare);
}
