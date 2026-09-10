fn main() {
    let hz: f32 = std::env::var("PROBE_FPS").ok().and_then(|t| t.parse().ok()).unwrap_or(30.0);
    let dt = 1.0 / hz;
    let mut analyser = paper_audio::Analyser::start().expect("analyser");
    let mut bands = paper_audio::Bands::default();
    let mut previous = vec![0.0f32; 16];
    let (mut jitter, mut level, mut frames) = (0.0f32, 0.0f32, 0u32);
    let start = std::time::Instant::now();
    while start.elapsed().as_secs_f32() < 6.0 {
        std::thread::sleep(std::time::Duration::from_secs_f32(dt));
        analyser.fill(&mut bands, dt);
        let now = bands.slice(16, false).unwrap();
        if start.elapsed().as_secs_f32() > 1.5 {
            jitter += now.iter().zip(&previous).map(|(a, b)| (a - b).abs()).sum::<f32>() / 16.0;
            level += now.iter().sum::<f32>() / 16.0;
            frames += 1;
        }
        previous.copy_from_slice(now);
    }
    let frames = frames.max(1) as f32;
    println!(
        "{hz:>5.0} fps  mean level {:.4}  frame-to-frame jitter {:.4}  relative {:.1}%",
        level / frames,
        jitter / frames,
        100.0 * (jitter / frames) / (level / frames).max(1e-6)
    );
}
