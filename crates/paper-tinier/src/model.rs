use std::time::Duration;

pub const BUFFER_COUNT: usize = 2;
pub const MAX_FRAME_EDGE: u32 = 8192;
pub const MAX_FRAME_PIXELS: u64 = 3840 * 2160;
const MAX_FPS: u32 = 240;
const NS_PER_SECOND: u64 = 1_000_000_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameRate {
    pub numerator: u32,
    pub denominator: u32,
}

impl FrameRate {
    pub fn new(numerator: u32, denominator: u32) -> Result<Self, String> {
        if numerator == 0 || denominator == 0 {
            return Err("frame rate values must be nonzero".into());
        }
        if u64::from(numerator) > u64::from(denominator) * u64::from(MAX_FPS) {
            return Err(format!("frame rate must not exceed {MAX_FPS} fps"));
        }
        Ok(Self { numerator, denominator })
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        let (numerator, denominator) = match value.split_once('/') {
            Some((left, right)) if !right.contains('/') => {
                (parse_positive(left)?, parse_positive(right)?)
            }
            Some(_) => return Err("frame rate must contain at most one slash".into()),
            None => (parse_positive(value)?, 1),
        };
        Self::new(numerator, denominator)
    }

    pub fn value(self) -> f64 {
        f64::from(self.numerator) / f64::from(self.denominator)
    }

    pub fn equivalent(self, other: Self) -> bool {
        u64::from(self.numerator) * u64::from(other.denominator)
            == u64::from(other.numerator) * u64::from(self.denominator)
    }

    pub fn label(self) -> String {
        if self.denominator == 1 {
            self.numerator.to_string()
        } else {
            format!("{}/{}", self.numerator, self.denominator)
        }
    }
}

fn parse_positive(value: &str) -> Result<u32, String> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("frame rate values must be positive decimal integers".into());
    }
    let parsed = value.parse::<u32>().map_err(|_| "frame rate value exceeds u32".to_string())?;
    if parsed == 0 {
        return Err("frame rate values must be nonzero".into());
    }
    Ok(parsed)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ColorMatrix {
    Bt601,
    Bt709,
}

impl ColorMatrix {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Bt601 => "BT.601",
            Self::Bt709 => "BT.709",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct VideoInfo {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub frame_bytes: usize,
    pub frame_rate: FrameRate,
    pub matrix: ColorMatrix,
}

impl VideoInfo {
    pub fn new(
        width: i32,
        height: i32,
        frame_rate: FrameRate,
        matrix: ColorMatrix,
    ) -> Result<Self, String> {
        let width = u32::try_from(width).map_err(|_| "decoded width is invalid")?;
        let height = u32::try_from(height).map_err(|_| "decoded height is invalid")?;
        let pixels = u64::from(width) * u64::from(height);
        if width == 0
            || height == 0
            || width > MAX_FRAME_EDGE
            || height > MAX_FRAME_EDGE
            || pixels > MAX_FRAME_PIXELS
        {
            return Err("decoded geometry exceeds the 8192-edge/3840x2160-pixel cap".into());
        }
        let stride = width
            .checked_mul(4)
            .ok_or_else(|| "decoded stride exceeds wl_shm limits".to_string())?;
        let frame_bytes_u32 = stride
            .checked_mul(height)
            .ok_or_else(|| "decoded frame exceeds wl_shm limits".to_string())?;
        let total = frame_bytes_u32
            .checked_mul(u32::try_from(BUFFER_COUNT).unwrap())
            .ok_or_else(|| "decoded buffers exceed wl_shm limits".to_string())?;
        if stride > i32::MAX as u32 || total > i32::MAX as u32 {
            return Err("decoded dimensions exceed wl_shm limits".into());
        }
        Ok(Self {
            width,
            height,
            stride,
            frame_bytes: frame_bytes_u32 as usize,
            frame_rate,
            matrix,
        })
    }
}

#[derive(Debug)]
pub struct FrameClock {
    pub deadline: u64,
    whole_ns: u64,
    remainder: u64,
    denominator: u64,
    carry: u64,
}

impl FrameClock {
    pub fn new(rate: FrameRate, start_ns: u64) -> Self {
        let duration_numerator = NS_PER_SECOND * u64::from(rate.denominator);
        Self {
            deadline: start_ns,
            whole_ns: duration_numerator / u64::from(rate.numerator),
            remainder: duration_numerator % u64::from(rate.numerator),
            denominator: u64::from(rate.numerator),
            carry: 0,
        }
    }

    pub const fn advance(&mut self) {
        self.deadline += self.whole_ns;
        self.carry += self.remainder;
        if self.carry >= self.denominator {
            self.deadline += self.carry / self.denominator;
            self.carry %= self.denominator;
        }
    }

    pub const fn recover_lag(&mut self, now: u64) -> bool {
        if now <= self.deadline {
            return false;
        }
        self.deadline = now;
        self.carry = 0;
        self.advance();
        true
    }
}

#[derive(Debug, Default)]
pub struct RuntimeStats {
    commits: u64,
    first_commit_ns: u64,
    last_commit_ns: u64,
    pub deadline_reanchors: u64,
    max_commit_lateness_ns: u64,
}

impl RuntimeStats {
    pub fn record(&mut self, committed_at: u64, target_deadline: u64, paced: bool) {
        if self.commits == 0 {
            self.first_commit_ns = committed_at;
        }
        self.last_commit_ns = committed_at;
        self.commits += 1;
        if paced && committed_at > target_deadline {
            self.max_commit_lateness_ns =
                self.max_commit_lateness_ns.max(committed_at - target_deadline);
        }
    }

    #[allow(clippy::cast_precision_loss)]
    pub fn print(&self) {
        if self.commits == 0 {
            return;
        }
        let intervals = self.commits - 1;
        let elapsed = self.last_commit_ns.saturating_sub(self.first_commit_ns);
        let seconds = Duration::from_nanos(elapsed).as_secs_f64();
        let achieved = if seconds > 0.0 { intervals as f64 / seconds } else { 0.0 };
        eprintln!(
            "skwd-paper-tinier: health commits={} paced_intervals={} paced_seconds={seconds:.6} achieved_fps={achieved:.3} deadline_reanchors={} max_commit_lateness_ms={:.3}",
            self.commits,
            intervals,
            self.deadline_reanchors,
            self.max_commit_lateness_ns as f64 / 1_000_000.0
        );
    }
}

pub fn monotonic_ns() -> Result<u64, String> {
    let mut now = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &raw mut now) } < 0 {
        return Err(format!("monotonic clock failed: {}", std::io::Error::last_os_error()));
    }
    let seconds = u64::try_from(now.tv_sec).map_err(|_| "monotonic clock was negative")?;
    let nanos = u64::try_from(now.tv_nsec).map_err(|_| "monotonic clock was invalid")?;
    seconds
        .checked_mul(NS_PER_SECOND)
        .and_then(|value| value.checked_add(nanos))
        .ok_or_else(|| "monotonic clock overflowed".to_string())
}

mod tests;
