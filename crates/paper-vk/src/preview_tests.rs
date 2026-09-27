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
