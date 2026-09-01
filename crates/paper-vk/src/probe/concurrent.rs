use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, anyhow};

const FRAMES_PER_WORKER: usize = 24;

fn plane_digest(frame: &ffmpeg_the_third::frame::Video) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for plane in 0..frame.planes() {
        for byte in frame.data(plane) {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    hash
}

fn reference_digests(path: &str, render_node: Option<&std::path::Path>) -> Result<Vec<u64>> {
    let mut decoder = crate::decode::VaapiDecoder::open(path, render_node)
        .map_err(|error| anyhow!("reference decoder: {error:#}"))?;
    let mut digests = Vec::with_capacity(FRAMES_PER_WORKER);
    for _ in 0..FRAMES_PER_WORKER {
        let (frame, _) = decoder.next().map_err(|error| anyhow!("reference frame: {error:#}"))?;
        digests.push(plane_digest(&frame));
    }
    Ok(digests)
}

pub(super) fn run(path: &str, render_node: Option<&std::path::Path>, workers: usize) -> Result<()> {
    println!("\nconcurrent VAAPI check: {workers} workers x {FRAMES_PER_WORKER} frames");
    let reference = reference_digests(path, render_node)?;
    println!("  reference digests captured on a decoder opened alone");

    let mismatch = Arc::new(AtomicBool::new(false));
    let reference = Arc::new(reference);
    let mut handles = Vec::with_capacity(workers);
    for worker in 0..workers {
        let path = path.to_string();
        let node = render_node.map(std::path::Path::to_path_buf);
        let reference = Arc::clone(&reference);
        let mismatch = Arc::clone(&mismatch);
        handles.push(std::thread::spawn(move || {
            let mut decoder = match crate::decode::VaapiDecoder::open(&path, node.as_deref()) {
                Ok(decoder) => decoder,
                Err(error) => {
                    mismatch.store(true, Ordering::Relaxed);
                    return format!("worker {worker}: open failed: {error:#}");
                }
            };
            for index in 0..FRAMES_PER_WORKER {
                match decoder.next() {
                    Ok((frame, _)) => {
                        if plane_digest(&frame) != reference[index] {
                            mismatch.store(true, Ordering::Relaxed);
                            return format!(
                                "worker {worker}: frame {index} differs from reference"
                            );
                        }
                    }
                    Err(error) => {
                        mismatch.store(true, Ordering::Relaxed);
                        return format!("worker {worker}: frame {index} failed: {error:#}");
                    }
                }
            }
            format!("worker {worker}: {FRAMES_PER_WORKER} frames match")
        }));
    }

    for handle in handles {
        match handle.join() {
            Ok(line) => println!("  {line}"),
            Err(_) => {
                mismatch.store(true, Ordering::Relaxed);
                println!("  a worker panicked");
            }
        }
    }

    if mismatch.load(Ordering::Relaxed) {
        println!("  VERDICT: sharing one VAAPI device is NOT safe on this driver");
        return Err(anyhow!("concurrent VAAPI decode diverged from the reference"));
    }
    println!("  VERDICT: sharing one VAAPI device is safe on this driver");
    Ok(())
}
