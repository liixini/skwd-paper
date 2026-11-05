use std::io::{self, Read};
use std::path::Path;

const MAGIC: [u8; 4] = *b"SKDS";
const VERSION: u32 = 1;
const WORD: usize = size_of::<u32>();
const STROKE_WORDS: usize = 5;
const POINT_WORDS: usize = 2;
const TRACE: u32 = 0;
const HATCH: u32 = 1;
const MIN_POINTS: usize = 2;
const MAX_WIDTH: f32 = 256.0;
const MAX_SEGMENTS: usize = 1 << 20;
const STROKES_LIMIT: u64 = 64 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub struct DrawingStroke {
    pub start: f32,
    pub end: f32,
    pub width: f32,
    pub hatch: bool,
    pub points: Vec<[f32; 2]>,
}

impl DrawingStroke {
    pub fn encode(strokes: &[Self]) -> Vec<u8> {
        let mut bytes = MAGIC.to_vec();
        let mut word = |value: u32| bytes.extend_from_slice(&value.to_le_bytes());
        word(VERSION);
        word(strokes.len() as u32);
        for stroke in strokes {
            word(stroke.start.to_bits());
            word(stroke.end.to_bits());
            word(stroke.width.to_bits());
            word(if stroke.hatch { HATCH } else { TRACE });
            word(stroke.points.len() as u32);
            for point in &stroke.points {
                word(point[0].to_bits());
                word(point[1].to_bits());
            }
        }
        bytes
    }

    pub fn decode(bytes: &[u8]) -> io::Result<Vec<Self>> {
        let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid drawing strokes");
        let body = bytes.strip_prefix(&MAGIC).ok_or_else(invalid)?;
        if body.len() % WORD != 0 {
            return Err(invalid());
        }
        let mut words = body
            .chunks_exact(WORD)
            .map(|chunk| u32::from_le_bytes(chunk.try_into().expect("word size")));
        let mut next = || words.next().ok_or_else(invalid);
        if next()? != VERSION {
            return Err(invalid());
        }
        let count = next()? as usize;
        let stroke_bytes = WORD * (STROKE_WORDS + MIN_POINTS * POINT_WORDS);
        if count > body.len() / stroke_bytes {
            return Err(invalid());
        }
        let mut segments = 0;
        let mut strokes = Vec::with_capacity(count);
        for _ in 0..count {
            let [start, end, width] = [next()?, next()?, next()?].map(f32::from_bits);
            let hatch = match next()? {
                TRACE => false,
                HATCH => true,
                _ => return Err(invalid()),
            };
            let points = next()? as usize;
            if points < MIN_POINTS || points > body.len() / (WORD * POINT_WORDS) {
                return Err(invalid());
            }
            segments += points - 1;
            if segments > MAX_SEGMENTS {
                return Err(invalid());
            }
            let points = (0..points)
                .map(|_| Ok([f32::from_bits(next()?), f32::from_bits(next()?)]))
                .collect::<io::Result<Vec<_>>>()?;
            let timed = (0.0..=1.0).contains(&start) && (start..=1.0).contains(&end);
            let sized = width > 0.0 && width <= MAX_WIDTH;
            if !timed || !sized || points.iter().flatten().any(|value| !value.is_finite()) {
                return Err(invalid());
            }
            strokes.push(Self { start, end, width, hatch, points });
        }
        if next().is_ok() {
            return Err(invalid());
        }
        Ok(strokes)
    }

    pub fn read(path: &Path) -> io::Result<Vec<Self>> {
        let mut bytes = Vec::new();
        std::fs::File::open(path)?.take(STROKES_LIMIT + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > STROKES_LIMIT {
            return Err(io::ErrorKind::FileTooLarge.into());
        }
        Self::decode(&bytes)
    }
}

#[cfg(test)]
#[path = "strokes_tests.rs"]
mod tests;
