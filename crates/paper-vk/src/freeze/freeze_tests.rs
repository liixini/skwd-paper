use super::*;

#[test]
fn rgba_freeze_preserves_pixels_across_buffer_boundaries() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("frame.ppm");
    let pixels: Vec<u8> = (0..257 * 129).flat_map(|i| [i as u8, (i >> 8) as u8, 73, 19]).collect();
    write_rgba_ppm(&path, 257, 129, &pixels).unwrap();
    let frame = image::open(&path).unwrap().into_rgb8();
    assert_eq!(frame.dimensions(), (257, 129));
    let expected: Vec<u8> =
        pixels.chunks_exact(4).flat_map(|pixel| pixel[..3].iter().copied()).collect();
    assert_eq!(frame.into_raw(), expected);
    assert!(write_rgba_ppm(&path, 257, 129, &vec![0; pixels.len()]).is_err());
    assert_eq!(image::open(&path).unwrap().into_rgb8().into_raw(), expected);
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn truncated_rgba_freeze_publishes_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("frame.ppm");
    assert!(write_rgba_ppm(&path, 2, 2, &[0; 15]).is_err());
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
}

#[test]
#[ignore = "manual freeze-frame latency measurement"]
fn measure_rgba_freeze_latency() {
    let directory = tempfile::tempdir().unwrap();
    for (width, height) in [(1366, 768), (1920, 1080), (2560, 1440)] {
        let pixels = [30, 80, 120, 255].repeat(width * height);
        let mut elapsed = Vec::new();
        for sample in 0..5 {
            let path = directory.path().join(format!("{width}-{height}-{sample}.ppm"));
            let start = std::time::Instant::now();
            write_rgba_ppm(&path, width as u32, height as u32, &pixels).unwrap();
            elapsed.push(start.elapsed().as_secs_f64() * 1000.0);
            std::fs::remove_file(path).unwrap();
        }
        elapsed.sort_by(f64::total_cmp);
        eprintln!(
            "RGBA freeze {width}x{height}: median={:.2} ms max={:.2} ms samples={elapsed:?}",
            elapsed[2], elapsed[4]
        );
    }
}
