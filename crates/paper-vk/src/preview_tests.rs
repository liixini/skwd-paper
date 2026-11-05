use super::*;

fn target(output: &str, fps: u32) -> StreamTarget {
    StreamTarget { socket: -1, width: 16, height: 16, fps, output: output.into(), paused: false }
}

#[test]
fn pacing_follows_the_fastest_stream() {
    assert_eq!(pacing_fps(&[target("DP-1", 60), target("DP-2", 144)]), 144);
    assert_eq!(pacing_fps(&[target("DP-1", 1000)]), 240);
    assert_eq!(pacing_fps(&[]), 30);
}

#[test]
fn output_pauses_only_touch_their_stream() {
    let mut targets = vec![target("DP-1", 60), target("DP-2", 60)];
    assert_eq!(apply_output_pauses(&mut targets, vec![("DP-1".into(), true)]), Some(false));
    assert!(targets[0].paused);
    assert!(!targets[1].paused);
    assert_eq!(apply_output_pauses(&mut targets, vec![("DP-2".into(), true)]), Some(true));
    assert_eq!(apply_output_pauses(&mut targets, vec![("DP-1".into(), false)]), Some(false));
    let mut unnamed = vec![target("", 60)];
    assert_eq!(apply_output_pauses(&mut unnamed, vec![("DP-9".into(), true)]), Some(true));
}

#[test]
fn free_slots_short_circuit_the_wait() {
    let mut free = [[false; 3], [true, false, false]];
    assert!(wait_any_free(&[-1, -1], &mut free, &[true, true], None, None).unwrap());
}

#[test]
fn late_acks_free_their_slots_without_blocking() {
    use std::io::Write;
    use std::os::fd::AsRawFd;
    let (mut plugin, paper) = std::os::unix::net::UnixStream::pair().unwrap();
    let mut free = [false; 3];
    drain_acks(paper.as_raw_fd(), &mut free).unwrap();
    assert_eq!(free, [false; 3]);
    let epoch = paper_runtime::plasma::stream_epoch();
    plugin.write_all(&paper_runtime::plasma::packet(3, 2, epoch)).unwrap();
    plugin.write_all(&paper_runtime::plasma::packet(3, 0, epoch.wrapping_add(1))).unwrap();
    plugin.write_all(&paper_runtime::plasma::packet(3, 0, epoch)).unwrap();
    drain_acks(paper.as_raw_fd(), &mut free).unwrap();
    assert_eq!(free, [true, false, true]);
    drop(plugin);
    assert!(drain_acks(paper.as_raw_fd(), &mut free).is_err());
}

fn check_output_resume(delayed: bool, all_paused: bool) {
    use std::os::fd::AsRawFd;
    let (send, receive) = std::sync::mpsc::channel();
    let wake = paper_runtime::wake::make_pipe().unwrap();
    let mut ctl = crate::ctl::Ctl::from_channel(receive, Some(wake.clone()), "", true, 0, false);
    ctl.route_output_pauses();
    ctl.set_paused(all_paused);
    let (plugin, paper) = std::os::unix::net::UnixStream::pair().unwrap();
    let rescue = plugin.try_clone().unwrap();
    let command: paper_control::PaperCommand =
        serde_json::from_value(serde_json::json!({"to": "stream-2", "pause": false})).unwrap();
    if !delayed {
        send.send(command.clone()).unwrap();
        wake.poke();
    }
    let (finished, completion) = std::sync::mpsc::channel();
    let sender = std::thread::spawn(move || {
        if delayed {
            std::thread::sleep(std::time::Duration::from_millis(80));
            send.send(command).unwrap();
            wake.poke();
        }
        let resumed = completion.recv_timeout(std::time::Duration::from_secs(2)).is_ok();
        if !resumed {
            rescue.shutdown(std::net::Shutdown::Both).unwrap();
        }
        resumed
    });
    let mut free = [[false; 3]];
    let result =
        wait_any_free(&[paper.as_raw_fd()], &mut free, &[!all_paused], ctl.wake_fd(), None);
    let _ = finished.send(());
    assert!(sender.join().unwrap(), "output resume stayed queued inside the slot wait");
    assert!(!result.unwrap());
    let _ = ctl.poll();
    let mut targets = vec![target("stream-1", 60), target("stream-2", 60)];
    for target in &mut targets {
        target.paused = true;
    }
    let all_paused = apply_output_pauses(&mut targets, ctl.take_output_pauses());
    ctl.set_paused(all_paused.unwrap());
    assert!(!ctl.paused);
    assert!(targets[0].paused);
    assert!(!targets[1].paused);
}

#[test]
fn exhausted_stream_yields_to_queued_output_resume() {
    check_output_resume(false, false);
}

#[test]
fn exhausted_stream_wakes_for_output_resume() {
    check_output_resume(true, false);
}

#[test]
fn all_paused_streams_yield_to_queued_output_resume() {
    check_output_resume(false, true);
}

#[test]
fn all_paused_streams_wake_for_output_resume() {
    check_output_resume(true, true);
}

#[test]
fn paused_stream_still_requires_its_first_frame() {
    let mut stream = target("activity-1-DP-2", 60);
    assert!(stream.active(false));
    assert!(stream.active(true));
    stream.paused = true;
    assert!(stream.active(false));
    assert!(!stream.active(true));
    stream.paused = false;
    assert!(stream.active(true));
}

#[test]
fn opaque_stream_labels_isolate_two_instances_on_one_monitor() {
    let mut targets = vec![target("activity-1-DP-2", 60), target("activity-2-DP-2", 60)];
    apply_output_pauses(&mut targets, vec![("activity-1-DP-2".into(), true)]);
    assert!(targets[0].paused);
    assert!(!targets[1].paused);
    apply_output_pauses(&mut targets, vec![("DP-2".into(), true)]);
    assert!(!targets[1].paused);
    apply_output_pauses(
        &mut targets,
        vec![("activity-1-DP-2".into(), false), ("activity-2-DP-2".into(), true)],
    );
    assert!(!targets[0].paused);
    assert!(targets[1].paused);
}

fn thread_cpu_time() -> std::time::Duration {
    let mut time = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    assert_eq!(unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut time) }, 0);
    std::time::Duration::new(time.tv_sec as u64, time.tv_nsec as u32)
}

#[test]
fn hidden_free_slots_do_not_spin_or_release_a_blocked_visible_stream() {
    use std::os::fd::AsRawFd;
    let (mut plugin, paper) = std::os::unix::net::UnixStream::pair().unwrap();
    let (hidden_plugin, hidden_paper) = std::os::unix::net::UnixStream::pair().unwrap();
    let wake = paper_runtime::wake::make_pipe().unwrap();
    let sender = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(120));
        plugin
            .write_all(&paper_runtime::plasma::packet(3, 1, paper_runtime::plasma::stream_epoch()))
            .unwrap();
        plugin
    });
    let mut free = [[false; 3], [true; 3]];
    let started = Instant::now();
    let cpu = thread_cpu_time();
    assert!(
        wait_any_free(
            &[paper.as_raw_fd(), hidden_paper.as_raw_fd()],
            &mut free,
            &[true, false],
            Some(wake.read_fd()),
            None,
        )
        .unwrap()
    );
    let cpu = thread_cpu_time() - cpu;
    assert!(started.elapsed() >= std::time::Duration::from_millis(80));
    assert!(cpu < std::time::Duration::from_millis(30), "slot wait used {cpu:?} CPU");
    assert_eq!(free, [[false, true, false], [true; 3]]);
    drop(sender.join().unwrap());
    drop(hidden_plugin);
}

#[test]
fn paused_stream_socket_closure_ends_the_wait() {
    use std::os::fd::AsRawFd;
    let (plugin, paper) = std::os::unix::net::UnixStream::pair().unwrap();
    let wake = paper_runtime::wake::make_pipe().unwrap();
    drop(plugin);
    let error =
        wait_any_free(&[paper.as_raw_fd()], &mut [[true; 3]], &[false], Some(wake.read_fd()), None)
            .unwrap_err();
    assert!(error.to_string().contains("frame socket closed"));
}

#[test]
fn control_wakeup_preserves_a_queued_scene_swap() {
    let (send, receive) = std::sync::mpsc::channel();
    let wake = paper_runtime::wake::make_pipe().unwrap();
    let mut ctl = crate::ctl::Ctl::from_channel(receive, Some(wake.clone()), "", true, 0, false);
    send.send(serde_json::from_value(serde_json::json!({"to": "/next/scene"})).unwrap()).unwrap();
    wake.poke();
    assert!(!wait_any_free(&[], &mut [], &[], ctl.wake_fd(), None).unwrap());
    assert_eq!(ctl.poll().unwrap().to, "/next/scene");
}

#[test]
fn unavailable_control_pipe_uses_a_bounded_sleep() {
    let started = Instant::now();
    assert!(!wait_any_free(&[], &mut [], &[], None, None).unwrap());
    assert!(started.elapsed() >= std::time::Duration::from_millis(40));
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
}

#[test]
fn transition_deadline_releases_an_unacknowledged_stream() {
    use std::os::fd::AsRawFd;
    let (plugin, paper) = std::os::unix::net::UnixStream::pair().unwrap();
    let started = Instant::now();
    let cpu = thread_cpu_time();
    assert!(
        !wait_any_free(
            &[paper.as_raw_fd()],
            &mut [[false; 3]],
            &[true],
            None,
            Some(started + std::time::Duration::from_millis(120)),
        )
        .unwrap()
    );
    let cpu = thread_cpu_time() - cpu;
    assert!(started.elapsed() >= std::time::Duration::from_millis(100));
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
    assert!(cpu < std::time::Duration::from_millis(30), "deadline wait used {cpu:?} CPU");
    drop(plugin);
}

#[test]
fn available_slot_can_present_the_final_frame_after_the_deadline() {
    assert!(wait_any_free(&[-1], &mut [[true; 3]], &[true], None, Some(Instant::now())).unwrap());
}

#[test]
fn stale_instance_tokens_preserve_global_and_target_pause_state() {
    let (send, receive) = std::sync::mpsc::channel();
    let mut ctl = crate::ctl::Ctl::from_channel(receive, None, "", true, 0, false);
    ctl.route_output_pauses();
    ctl.set_paused(true);
    let mut targets = vec![target("instance-2", 60), target("instance-3", 60)];
    targets[1].paused = true;
    let before = targets.clone();
    send.send(
        serde_json::from_value(serde_json::json!({"to": "instance-1", "pause": false})).unwrap(),
    )
    .unwrap();
    let _ = ctl.poll();
    let paused = apply_output_pauses(&mut targets, ctl.take_output_pauses());
    if let Some(paused) = paused {
        ctl.set_paused(paused);
    }
    assert_eq!(paused, None);
    assert!(ctl.paused);
    assert_eq!(targets, before);
    assert_eq!(apply_output_pauses(&mut targets, Vec::new()), None);
}

const CPU_STREAM_VIDEO: &str = "SKWD_PAPER_TEST_CPU_STREAM";
const CPU_STREAM_PAUSED: &str = "SKWD_PAPER_TEST_CPU_STREAM_PAUSED";
const CPU_STREAM_OUTPUT: &str = "DP-1";
const CPU_STREAM_OTHER_OUTPUT: &str = "DP-2";
const CPU_STREAM_SIZE: u32 = 64;
const CPU_STREAM_FPS: u32 = 30;
const CPU_STREAM_FIRST_FRAME: std::time::Duration = std::time::Duration::from_secs(10);
const CPU_STREAM_SETTLE: std::time::Duration = std::time::Duration::from_millis(200);
const CPU_STREAM_QUIET: std::time::Duration = std::time::Duration::from_millis(500);
const CPU_STREAM_HELD: std::time::Duration = std::time::Duration::from_millis(1500);
const CPU_STREAM_PACED_FRAMES: usize = 8;
const CPU_STREAM_PACING_FLOOR: f64 = 0.8;

fn cpu_stream_child() -> Option<()> {
    let video = std::env::var_os(CPU_STREAM_VIDEO)?;
    let paused = std::env::var_os(CPU_STREAM_PAUSED).is_some();
    let stream = StreamTarget {
        output: CPU_STREAM_OUTPUT.into(),
        ..StreamTarget::single(NO_SOCKET, CPU_STREAM_SIZE, CPU_STREAM_SIZE, CPU_STREAM_FPS, paused)
    };
    video_stream(video.to_str().unwrap(), &stream, true).unwrap();
    Some(())
}

struct CpuStream {
    child: std::process::Child,
    control: std::process::ChildStdin,
    frames: std::sync::mpsc::Receiver<Instant>,
    _directory: tempfile::TempDir,
}

impl CpuStream {
    fn start(test: &str, paused: bool) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let video = directory.path().join("motion.mp4");
        let size =
            format!("testsrc2=size={CPU_STREAM_SIZE}x{CPU_STREAM_SIZE}:rate={CPU_STREAM_FPS}");
        let encoded = std::process::Command::new("ffmpeg")
            .args(["-v", "error", "-f", "lavfi", "-i", &size, "-t", "10", "-c:v", "mpeg4", "-y"])
            .arg(&video)
            .status()
            .unwrap();
        assert!(encoded.success());
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", test, "--include-ignored"])
            .env(CPU_STREAM_VIDEO, &video)
            .env_remove(paper_control::PLAYBACK_ENV)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped());
        if paused {
            command.env(CPU_STREAM_PAUSED, "1");
        }
        let mut child = command.spawn().unwrap();
        let frames = cpu_frames(child.stdout.take().unwrap());
        let control = child.stdin.take().unwrap();
        Self { child, control, frames, _directory: directory }
    }

    fn send(&mut self, output: Option<&str>, paused: bool) {
        use std::io::Write;
        let command = match output {
            Some(output) => serde_json::json!({"to": output, "pause": paused}),
            None => serde_json::json!({"pause": paused}),
        };
        writeln!(self.control, "{command}").unwrap();
    }

    fn wait_frames(&self, count: usize) -> Vec<Instant> {
        (0..count)
            .map(|_| self.frames.recv_timeout(CPU_STREAM_FIRST_FRAME).expect("stream frame"))
            .collect()
    }

    fn drain(&self) {
        std::thread::sleep(CPU_STREAM_SETTLE);
        while self.frames.try_recv().is_ok() {}
    }

    fn holds(&self) -> bool {
        self.frames.recv_timeout(CPU_STREAM_QUIET).is_err()
    }
}

impl Drop for CpuStream {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn cpu_frames(stdout: std::process::ChildStdout) -> std::sync::mpsc::Receiver<Instant> {
    use std::io::Read;
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut stdout = std::io::BufReader::new(stdout);
        let mut window = [0u8; 4];
        while &window != b"SKWP" {
            window.rotate_left(1);
            if stdout.read_exact(&mut window[3..]).is_err() {
                return;
            }
        }
        let mut frame = vec![0u8; 8];
        stdout.read_exact(&mut frame).unwrap();
        frame.resize(64 * 64 * 4, 0);
        while stdout.read_exact(&mut frame).is_ok() && send.send(Instant::now()).is_ok() {}
    });
    receive
}

#[test]
#[ignore = "requires Vulkan and ffmpeg"]
fn cpu_stream_honours_pause() {
    const TEST: &str = "preview::tests::cpu_stream_honours_pause";
    if cpu_stream_child().is_some() {
        return;
    }
    let mut stream = CpuStream::start(TEST, false);
    stream.wait_frames(3);
    stream.send(None, true);
    stream.drain();
    assert!(stream.holds(), "frames while paused");
    stream.send(None, false);
    stream.wait_frames(1);
}

#[test]
#[ignore = "requires Vulkan and ffmpeg"]
fn cpu_stream_starts_paused() {
    const TEST: &str = "preview::tests::cpu_stream_starts_paused";
    if cpu_stream_child().is_some() {
        return;
    }
    let mut stream = CpuStream::start(TEST, true);
    stream.wait_frames(1);
    assert!(stream.holds(), "frames while paused");
    stream.send(None, false);
    stream.wait_frames(2);
}

#[test]
#[ignore = "requires Vulkan and ffmpeg"]
fn cpu_stream_resumes_paced() {
    const TEST: &str = "preview::tests::cpu_stream_resumes_paced";
    if cpu_stream_child().is_some() {
        return;
    }
    let mut stream = CpuStream::start(TEST, false);
    stream.wait_frames(3);
    stream.send(None, true);
    stream.drain();
    std::thread::sleep(CPU_STREAM_HELD);
    stream.send(None, false);
    let frames = stream.wait_frames(CPU_STREAM_PACED_FRAMES);
    let span = frames[CPU_STREAM_PACED_FRAMES - 1].duration_since(frames[0]);
    let intervals = (CPU_STREAM_PACED_FRAMES - 1) as f64;
    let floor = intervals / f64::from(CPU_STREAM_FPS) * CPU_STREAM_PACING_FLOOR;
    assert!(span.as_secs_f64() >= floor, "resume burst {span:?}");
}

#[test]
#[ignore = "requires Vulkan and ffmpeg"]
fn cpu_stream_output_pause() {
    const TEST: &str = "preview::tests::cpu_stream_output_pause";
    if cpu_stream_child().is_some() {
        return;
    }
    let mut stream = CpuStream::start(TEST, false);
    stream.wait_frames(3);
    stream.send(Some(CPU_STREAM_OTHER_OUTPUT), true);
    stream.drain();
    stream.wait_frames(3);
    stream.send(Some(CPU_STREAM_OUTPUT), true);
    stream.drain();
    assert!(stream.holds(), "frames while output paused");
    stream.send(Some(CPU_STREAM_OUTPUT), false);
    stream.wait_frames(1);
}
