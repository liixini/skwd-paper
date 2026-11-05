use ringbuf::traits::Split;
use std::ffi::OsStr;

use super::{
    Analyser, BAND_COUNTS, Bands, GAIN, WINDOW, band_edges, fft, monitor_source, spectrum_levels,
};

#[test]
fn audio_source_accepts_only_the_default_output_monitor() {
    assert_eq!(monitor_source(None).unwrap(), "@DEFAULT_MONITOR@");
    assert_eq!(monitor_source(Some(OsStr::new("@DEFAULT_MONITOR@"))).unwrap(), "@DEFAULT_MONITOR@");
    for source in [
        "",
        "@DEFAULT_SOURCE@",
        "alsa_input.pci.mic",
        "alsa_output.pci.monitor",
        "@DEFAULT_MONITOR@.monitor",
        " @DEFAULT_MONITOR@",
    ] {
        assert!(monitor_source(Some(OsStr::new(source))).is_err(), "accepted {source:?}");
    }
}

#[cfg(unix)]
#[test]
fn non_unicode_source_override_is_rejected() {
    use std::os::unix::ffi::OsStrExt;

    assert!(monitor_source(Some(OsStr::from_bytes(b"@DEFAULT_MONITOR@\xff"))).is_err());
}

#[test]
fn rejected_source_never_opens_capture() {
    let error = Analyser::start_with_capture(Some(OsStr::new("@DEFAULT_SOURCE@")), |_, _| {
        panic!("capture must not open for an input source")
    })
    .err()
    .expect("source override must fail");
    assert!(error.to_string().contains("SKWD_VK_AUDIO_SOURCE"));
}

#[test]
fn monitor_capture_failure_returns_without_a_fallback() {
    for source in [None, Some(OsStr::new("@DEFAULT_MONITOR@"))] {
        let error = Analyser::start_with_capture(source, |name, _| {
            assert_eq!(name, "@DEFAULT_MONITOR@");
            Err(anyhow::anyhow!("monitor capture unavailable"))
        })
        .err()
        .expect("capture failure must propagate");
        assert_eq!(error.to_string(), "monitor capture unavailable");
    }
}

fn tone_levels(hz: f32, amplitude: f32, rate: f32) -> [f32; 64] {
    let mut re: Vec<f32> = super::hann()
        .into_iter()
        .enumerate()
        .map(|(index, window)| {
            amplitude * (std::f32::consts::TAU * hz * index as f32 / rate).sin() * window
        })
        .collect();
    let mut im = vec![0.0; WINDOW];
    fft(&mut re, &mut im);
    spectrum_levels(&re, &im, &band_edges(64, rate), GAIN)
}

#[test]
fn a_pure_tone_peaks_in_the_bin_that_carries_its_frequency() {
    let rate = 48_000.0f32;
    let bin = 64usize;
    let hz = bin as f32 * rate / WINDOW as f32;
    let mut re: Vec<f32> =
        (0..WINDOW).map(|index| (std::f32::consts::TAU * hz * index as f32 / rate).cos()).collect();
    let mut im = vec![0.0f32; WINDOW];
    fft(&mut re, &mut im);
    let magnitude = |index: usize| (re[index] * re[index] + im[index] * im[index]).sqrt();
    let loudest = (1..WINDOW / 2).max_by(|a, b| magnitude(*a).total_cmp(&magnitude(*b))).unwrap();
    assert_eq!(loudest, bin);
    assert!(magnitude(bin) > magnitude(bin + 8) * 50.0, "the peak must be sharp");
}

#[test]
fn band_edges_rise_without_gaps_and_stay_inside_the_spectrum() {
    for count in BAND_COUNTS {
        let edges = band_edges(count, 48_000.0);
        assert_eq!(edges.len(), count);
        for (index, (from, to)) in edges.iter().enumerate() {
            assert!(from < to, "band {index} of {count} is empty");
            assert!(*to <= WINDOW / 2);
            assert!(*from >= 1, "the DC bin never belongs to a band");
            if index > 0 {
                assert!(*from >= edges[index - 1].0, "bands must ascend in frequency");
            }
        }
        assert!(edges[count - 1].1 > edges[0].1, "the top band must reach higher than the first");
    }
}

#[test]
fn measured_native_tones_land_in_the_same_sixteen_band_positions() {
    for rate in [44_100.0, 48_000.0, 96_000.0] {
        for (hz, expected) in
            [(125.0, 1), (250.0, 2), (500.0, 5), (1000.0, 8), (2000.0, 9), (4000.0, 11)]
        {
            let mut bands = Bands::default();
            bands.update(false, &tone_levels(hz, 0.001, rate), 1.0, 0.001, 0.001);
            let levels = bands.slice(16, false).unwrap();
            let peak = levels.iter().enumerate().max_by(|(_, a), (_, b)| a.total_cmp(b)).unwrap().0;
            assert_eq!(peak, expected, "{hz} Hz at {rate} Hz sample rate: {levels:?}");
        }
    }
}

#[test]
fn measured_loud_tones_stop_at_one_and_keep_the_same_peak_at_every_resolution() {
    for rate in [44_100.0, 48_000.0, 96_000.0] {
        let mut previous = 0.0;
        for amplitude in [0.01, 0.05, 0.1, 0.2, 0.35, 1.0] {
            let mut bands = Bands::default();
            bands.update(true, &tone_levels(250.0, amplitude, rate), 1.0, 0.001, 0.001);
            let peak =
                |count| bands.slice(count, true).unwrap().iter().copied().fold(0.0, f32::max);
            assert_eq!(peak(16), peak(64), "16-band peak at amplitude {amplitude}, rate {rate}");
            assert_eq!(peak(32), peak(64), "32-band peak at amplitude {amplitude}, rate {rate}");
            assert!(peak(64) >= previous && peak(64) <= 1.0);
            if amplitude >= 0.2 {
                assert_eq!(peak(64), 1.0, "loud native values saturate");
            }
            previous = peak(64);
            assert!(bands.slice(64, false).unwrap().iter().all(|value| *value == 0.0));
        }
    }
}

#[test]
fn reduced_bands_follow_the_strongest_current_peak_during_attack_and_release() {
    let mut bands = Bands::default();
    let mut target = [0.0; 64];
    target[8] = 1.0;
    for _ in 0..8 {
        bands.update(false, &target, 0.05, 0.045, 0.28);
    }
    target[8] = 0.0;
    target[11] = 0.8;
    for _ in 0..20 {
        bands.update(false, &target, 0.05, 0.045, 0.28);
        let full = bands.slice(64, false).unwrap();
        assert_eq!(bands.slice(16, false).unwrap()[2], full[8].max(full[11]));
        assert_eq!(bands.slice(32, false).unwrap()[4], full[8]);
        assert_eq!(bands.slice(32, false).unwrap()[5], full[11]);
    }
    assert!(bands.slice(64, false).unwrap()[11] > bands.slice(64, false).unwrap()[8]);
    bands.silence(0.1);
    assert_eq!(bands.slice(16, false).unwrap()[2], bands.slice(64, false).unwrap()[11]);
}

#[test]
fn invalid_capture_values_do_not_escape_as_nonfinite_audio_bands() {
    for sample in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let re = vec![sample; WINDOW];
        let im = vec![0.0; WINDOW];
        let levels = spectrum_levels(&re, &im, &band_edges(64, 48_000.0), GAIN);
        assert!(levels.iter().all(|value| value.is_finite() && (0.0..=1.0).contains(value)));
    }
    assert_eq!(tone_levels(250.0, 0.0, 48_000.0), [0.0; 64]);
}

#[test]
fn bands_expose_one_lane_per_supported_resolution_and_decay_towards_silence() {
    let mut bands = Bands::default();
    for count in BAND_COUNTS {
        assert_eq!(bands.slice(count, false).map(<[f32]>::len), Some(count));
        assert_eq!(bands.slice(count, true).map(<[f32]>::len), Some(count));
    }
    assert!(bands.slice(24, false).is_none());
    bands.left[0][0] = 1.0;
    bands.silence(0.05);
    let after = bands.slice(16, false).unwrap()[0];
    assert!(after > 0.0 && after < 1.0, "silence decays rather than cutting to zero: {after}");
    for _ in 0..80 {
        bands.silence(0.05);
    }
    assert!(bands.slice(16, false).unwrap()[0] < 1e-3);
}

#[test]
fn playback_source_selection() {
    let analyser = Analyser::select(None, Some(OsStr::new("wallpaper")), |_, _| {
        panic!("unexpected monitor capture")
    })
    .unwrap();
    assert!(analyser.playback_tap().producer.lock().unwrap().is_some());
    for mode in [None, Some(OsStr::new("system"))] {
        let analyser = Analyser::select(None, mode, |source, _| {
            assert_eq!(source, "@DEFAULT_MONITOR@");
            Err(anyhow::anyhow!("unavailable"))
        })
        .unwrap();
        assert!(analyser.playback_tap().producer.lock().unwrap().is_some());
    }
    for (source, mode) in
        [(Some(OsStr::new("@DEFAULT_SOURCE@")), None), (None, Some(OsStr::new("microphone")))]
    {
        assert!(Analyser::select(source, mode, |_, _| panic!("unexpected capture")).is_err());
    }
}

#[test]
fn playback_stereo_spectrum() {
    let mut analyser = Analyser::playback();
    let tap = analyser.playback_tap();
    let mut bands = Bands::default();
    let samples: Vec<f32> = (0..WINDOW)
        .flat_map(|index| [(std::f32::consts::TAU * index as f32 / 32.0).sin() * 0.5, 0.0])
        .collect();
    tap.write(&samples);
    analyser.fill(&mut bands, 0.1);
    assert!(bands.slice(64, false).unwrap().iter().any(|level| *level > 0.1));
    assert!(bands.slice(64, true).unwrap().iter().all(|level| *level == 0.0));
    let initial: f32 = bands.slice(64, false).unwrap().iter().sum();
    for _ in 0..20 {
        analyser.fill(&mut bands, 0.1);
    }
    let settled: f32 = bands.slice(64, false).unwrap().iter().sum();
    assert!(settled < initial * 0.01);
}

#[test]
fn tap_overflow_keeps_stereo() {
    let mut analyser = Analyser::playback();
    let tap = analyser.playback_tap();
    let samples = [0.75, 0.25].repeat(WINDOW * super::BUFFER_WINDOWS + 1);
    tap.write(&samples);
    assert_eq!(analyser.drain(), WINDOW * super::BUFFER_WINDOWS);
    assert!(analyser.left.iter().all(|value| *value == 0.75));
    assert!(analyser.right.iter().all(|value| *value == 0.25));
    assert_eq!(analyser.drain(), 0);
    let guard = tap.producer.lock().unwrap();
    tap.write(&samples);
    drop(guard);
    assert_eq!(analyser.drain(), 0);
    tap.write(&[0.5, 0.125]);
    assert_eq!(analyser.drain(), 1);
}

#[test]
#[ignore = "requires isolated playback output"]
fn live_playback_spectrum() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("tone.wav");
    std::fs::write(&path, crate::test_pcm::pcm_wav(2, None)).unwrap();
    let mut analyser = Analyser::playback();
    let voice = crate::Voice {
        id: "tone".into(),
        name: "tone".into(),
        clips: vec![path.to_string_lossy().into_owned()],
        gain: 1.0,
        mode: crate::VoiceMode::Loop,
        min_gap: 0.0,
        max_gap: 0.0,
        autostart: true,
    };
    let mixer = crate::SceneMixer::new(
        &[voice],
        crate::SceneConfig { tap: Some(analyser.playback_tap()), ..crate::SceneConfig::default() },
    )
    .unwrap()
    .unwrap();
    let mut bands = Bands::default();
    for audible in [true, false, true] {
        analyser.set_system_capture(false).unwrap();
        mixer.set_pause(!audible);
        let start = std::time::Instant::now();
        while start.elapsed() < std::time::Duration::from_secs(2) {
            std::thread::sleep(std::time::Duration::from_millis(20));
            analyser.fill(&mut bands, 0.02);
        }
        let peak = bands.slice(64, false).unwrap().iter().copied().fold(0.0, f32::max);
        if audible {
            assert!(peak > 0.2, "silent playback");
        } else {
            assert!(peak < 0.01, "paused spectrum");
        }
    }
    mixer.set_volume(0);
    for _ in 0..100 {
        std::thread::sleep(std::time::Duration::from_millis(20));
        analyser.fill(&mut bands, 0.02);
    }
    assert!(bands.slice(64, false).unwrap().iter().all(|level| *level < 0.01));
}

#[test]
fn source_change_keeps_tap() {
    let mut analyser = Analyser::playback();
    let tap = analyser.playback_tap();
    tap.write(&[0.75, 0.25]);
    analyser.replace(Analyser::playback());
    assert_eq!(analyser.drain(), 0);
    tap.write(&[0.5, 0.125]);
    assert_eq!(analyser.drain(), 1);
    assert_eq!((analyser.left[0], analyser.right[0]), (0.5, 0.125));
    let (_, consumer) = ringbuf::HeapRb::<f32>::new(8).split();
    analyser.replace(Analyser::with_capture(None, consumer, 48_000.0, 2));
    tap.write(&[1.0, 1.0]);
    assert_eq!(analyser.drain(), 0);
    analyser.replace(Analyser::playback());
    tap.write(&[0.25, 0.75]);
    assert_eq!(analyser.drain(), 1);
    assert_eq!((analyser.left[0], analyser.right[0]), (0.25, 0.75));
}
