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
    assert!(!apply_output_pauses(&mut targets, vec![("DP-1".into(), true)]));
    assert!(targets[0].paused);
    assert!(!targets[1].paused);
    assert!(apply_output_pauses(&mut targets, vec![("DP-2".into(), true)]));
    assert!(!apply_output_pauses(&mut targets, vec![("DP-1".into(), false)]));
    let mut unnamed = vec![target("", 60)];
    assert!(apply_output_pauses(&mut unnamed, vec![("DP-9".into(), true)]));
}

#[test]
fn free_slots_short_circuit_the_wait() {
    let mut free = [[false; 3], [true, false, false]];
    assert!(wait_any_free(&[-1, -1], &mut free, &[true, true]).is_ok());
}

fn check_output_resume(delayed: bool, with_wake: bool) {
    let (send, receive) = std::sync::mpsc::channel();
    let wake = with_wake.then(paper_runtime::wake::make_pipe).flatten();
    let mut ctl = crate::ctl::Ctl::from_channel(receive, wake.clone(), "", true, 0, false);
    ctl.route_output_pauses();
    ctl.set_paused(true);
    let (finished, completion) = std::sync::mpsc::channel();
    if !delayed {
        send.send(
            serde_json::from_value(serde_json::json!({"to": "DP-1", "pause": false})).unwrap(),
        )
        .unwrap();
    }
    let sender = std::thread::spawn(move || {
        if delayed {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        if delayed {
            send.send(
                serde_json::from_value(serde_json::json!({"to": "DP-1", "pause": false})).unwrap(),
            )
            .unwrap();
        }
        if let Some(wake) = &wake {
            wake.poke();
        }
        let resumed = completion.recv_timeout(std::time::Duration::from_secs(2)).is_ok();
        if !resumed {
            send.send(serde_json::from_value(serde_json::json!({"pause": false})).unwrap())
                .unwrap();
            if let Some(wake) = &wake {
                wake.poke();
            }
        }
        resumed
    });
    wait_stream_control(&mut ctl).unwrap();
    let _ = finished.send(());
    assert!(sender.join().unwrap(), "output resume stayed queued inside the paused wait");
    let mut targets = vec![target("DP-1", 60), target("DP-2", 60)];
    for target in &mut targets {
        target.paused = true;
    }
    let all_paused = apply_output_pauses(&mut targets, ctl.take_output_pauses());
    ctl.set_paused(all_paused);
    assert!(!ctl.paused);
    assert!(!targets[0].paused);
    assert!(targets[1].paused);
}

#[test]
fn paused_stream_yields_to_queued_output_resume() {
    check_output_resume(false, true);
}

#[test]
fn paused_stream_wakes_for_output_resume() {
    check_output_resume(true, true);
}

#[test]
fn paused_stream_without_wake_pipe_handles_output_resume() {
    check_output_resume(true, false);
}
