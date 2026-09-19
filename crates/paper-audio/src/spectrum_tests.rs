use super::{BAND_COUNTS, Bands, GAIN, WINDOW, band_edges, fft, spectrum_levels};

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
