use super::{BAND_COUNTS, Bands, WINDOW, band_edges, fft};

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
