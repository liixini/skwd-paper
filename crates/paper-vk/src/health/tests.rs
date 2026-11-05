use super::policy::*;
use std::num::NonZeroUsize;
use std::time::{Duration, Instant};

fn key(payload: u8) -> Key {
    Key {
        device: Device {
            uuid: [1; 16],
            driver_uuid: [2; 16],
            render_node: Some((226, 128)),
            primary_node: None,
            fingerprint: "driver:kernel:ffmpeg".into(),
        },
        content: Content::Digest { payload: [payload; 32], bytes: 1024 },
        configuration: "scene-vulkan-v1".into(),
    }
}

#[test]
fn transient_results_expire() {
    let mut cache = Cache::new(NonZeroUsize::new(1).unwrap());
    for outcome in [Outcome::TimedOut, Outcome::Cancelled, Outcome::Saturated, Outcome::Invalid] {
        cache.record(key(1), outcome);
        assert_eq!(cache.get(&key(1)), None);
    }
}

#[test]
fn device_identity_matters() {
    let mut cache = Cache::new(NonZeroUsize::new(2).unwrap());
    cache.record(key(1), Outcome::Healthy);
    let mut other = key(1);
    other.device.uuid[0] = 2;
    assert_eq!(cache.get(&other), None);
    other = key(1);
    other.device.render_node = Some((226, 129));
    assert_eq!(cache.get(&other), None);
    assert_eq!(cache.get(&key(1)), Some(Outcome::Healthy));
}

#[test]
fn cache_evicts_unused() {
    let mut cache = Cache::new(NonZeroUsize::new(2).unwrap());
    cache.record(key(1), Outcome::Healthy);
    cache.record(key(2), Outcome::Crashed);
    assert_eq!(cache.get(&key(1)), Some(Outcome::Healthy));
    cache.record(key(3), Outcome::Crashed);
    assert_eq!(cache.get(&key(2)), None);
    assert_eq!(cache.get(&key(3)), Some(Outcome::Crashed));
}

#[test]
fn one_scene_deadline() {
    let start = Instant::now();
    let budget = Budget::new(start, Duration::from_secs(10), Duration::from_secs(8)).unwrap();
    assert_eq!(budget.submit(start), Some(start + Duration::from_secs(8)));
    assert_eq!(
        budget.submit(start + Duration::from_secs(6)),
        Some(start + Duration::from_secs(10))
    );
    assert_eq!(budget.submit(start + Duration::from_secs(10)), None);
}

#[test]
fn protocol_requires_identity() {
    use std::os::unix::process::ExitStatusExt;
    let device = key(1).device;
    let report =
        super::Report { contract: super::CONTRACT.into(), device: device.clone(), frames: 1 };
    let finished = crate::sandbox::broker::Finished {
        status: std::process::ExitStatus::from_raw(0),
        report: serde_json::to_vec(&report).unwrap(),
        diagnostics: Vec::new(),
    };
    let mut wrong = device.clone();
    wrong.uuid[0] = 9;
    assert!(super::validate(&finished, &wrong).is_err());
}

#[test]
fn teardown_signal_rejects() {
    use std::os::unix::process::ExitStatusExt;
    let device = key(1).device;
    let report =
        super::Report { contract: super::CONTRACT.into(), device: device.clone(), frames: 1 };
    let finished = crate::sandbox::broker::Finished {
        status: std::process::ExitStatus::from_raw(libc::SIGSEGV),
        report: serde_json::to_vec(&report).unwrap(),
        diagnostics: Vec::new(),
    };
    assert_eq!(super::validate(&finished, &device).unwrap(), Outcome::Crashed);
    assert_eq!(super::crash_diagnostics(&finished), Some((libc::SIGSEGV, String::new())));
}

#[test]
fn crash_keeps_stage() {
    use std::os::unix::process::ExitStatusExt;
    let finished = crate::sandbox::broker::Finished {
        status: std::process::ExitStatus::from_raw(libc::SIGBUS),
        report: Vec::new(),
        diagnostics: b"health probe stage Device\nhealth probe stage Decoder\n".to_vec(),
    };
    let (signal, diagnostics) = super::crash_diagnostics(&finished).unwrap();
    assert_eq!(signal, libc::SIGBUS);
    assert_eq!(diagnostics.lines().last(), Some("health probe stage Decoder"));
}

#[test]
fn child_dispatch_exact() {
    assert!(super::child_requested(&["renderer".into(), super::MODE.into()]));
    assert!(!super::child_requested(&["renderer".into(), "scene-dump".into(), super::MODE.into()]));
}

#[test]
fn killed_probe_not_cached() {
    use std::os::unix::process::ExitStatusExt;
    let device = key(1).device;
    let finished = crate::sandbox::broker::Finished {
        status: std::process::ExitStatus::from_raw(libc::SIGKILL),
        report: Vec::new(),
        diagnostics: Vec::new(),
    };
    assert_eq!(super::validate(&finished, &device).unwrap(), Outcome::Invalid);
    assert_eq!(super::crash_diagnostics(&finished), None);
}

#[test]
fn changed_source_not_cached() {
    use std::io::Write;
    for outcome in [Outcome::Healthy, Outcome::Crashed] {
        let scratch = std::env::var_os("TMPDIR").expect("disk scratch required");
        let mut file = tempfile::NamedTempFile::new_in(scratch).unwrap();
        file.write_all(b"original").unwrap();
        let input = super::Input::open(file.path().into()).unwrap();
        let cache = std::sync::Mutex::new(Cache::new(NonZeroUsize::new(1).unwrap()));
        file.write_all(b"mutation").unwrap();
        assert!(super::retain(&cache, &input, key(1), outcome).is_err());
        assert_eq!(cache.lock().unwrap().get(&key(1)), None);
        let unchanged = super::Input::open(file.path().into()).unwrap();
        super::retain(&cache, &unchanged, key(2), outcome).unwrap();
        assert_eq!(cache.lock().unwrap().get(&key(2)), Some(outcome));
    }
}

#[test]
fn failed_submission_skips_wait() {
    let result = super::finish_submission(Err(anyhow::anyhow!("submission failed")), || {
        panic!("wait after failed submission")
    });
    assert_eq!(result.unwrap_err().to_string(), "submission failed");
}

#[test]
fn submitted_frame_waits() {
    let mut waited = false;
    let result = super::finish_submission(Ok(()), || {
        waited = true;
        Err(anyhow::anyhow!("wait failed"))
    });
    assert!(waited);
    assert_eq!(result.unwrap_err().to_string(), "wait failed");
}

#[test]
fn request_size_bounded() {
    let request = super::Request { device: key(1).device, clip: None };
    let mut encoded = serde_json::to_string(&request).unwrap();
    encoded.extend(std::iter::repeat_n(' ', super::REPORT_BYTES - encoded.len()));
    let mut arguments = vec!["renderer".into(), super::MODE.into(), encoded];
    assert_eq!(super::child_request(&arguments).unwrap(), request);
    arguments[2].push(' ');
    assert!(super::child_request(&arguments).is_err());
}

#[test]
fn clip_request_round_trips() {
    let clip = crate::decode::Clip { offset: 9, length: 4 };
    let request = super::Request { device: key(1).device, clip: Some(clip) };
    let encoded = serde_json::to_string(&request).unwrap();
    let arguments = vec!["renderer".into(), super::MODE.into(), encoded];
    assert_eq!(super::child_request(&arguments).unwrap(), request);
}

#[test]
fn package_keys_match() {
    let file = tempfile::tempfile().unwrap();
    let input = super::Input::new(file, "clip.mp4".into(), None).unwrap();
    let content = |offset| Content::Package {
        identity: input.identity().clone(),
        clip: crate::decode::Clip { offset, length: 4 },
    };
    assert_eq!(content(2), content(2));
    assert_ne!(content(2), content(3));
}
