use serde::Serialize;

pub const DEFAULT_DURATION_MS: u64 = 600;
pub const TRANSITION_SOURCES_ENV: &str = "SKWD_PAPER_TRANSITION_SOURCES";
pub const DEFAULT_EFFECT: &str = "fade";
pub const MIN_DURATION_MS: u64 = 50;
pub const MAX_DURATION_MS: u64 = 10_000;
pub const PHOTO_CUTOUT: &str = "photo-cutout";
pub const SLAT_TURN: &str = "slat-turn";
pub const CROP_RELAY: &str = "crop-relay";
pub const SUNLIT_ARCADE: &str = "sunlit-arcade";
pub const PAPER_HEART: &str = "paper-heart";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct TransitionSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub duration_ms: u64,
}

pub const STUDIO_TRANSITIONS: [TransitionSpec; 5] = [
    TransitionSpec { id: PHOTO_CUTOUT, label: "Photo cutout", duration_ms: 3400 },
    TransitionSpec { id: SLAT_TURN, label: "Slat turn", duration_ms: 3400 },
    TransitionSpec { id: CROP_RELAY, label: "Crop relay", duration_ms: 3000 },
    TransitionSpec { id: SUNLIT_ARCADE, label: "Sunlit arcade", duration_ms: 3100 },
    TransitionSpec { id: PAPER_HEART, label: "Paper heart", duration_ms: 3900 },
];

pub fn transition_duration(name: &str) -> u64 {
    STUDIO_TRANSITIONS
        .iter()
        .find(|spec| spec.id == name)
        .map_or(DEFAULT_DURATION_MS, |spec| spec.duration_ms)
}

#[cfg(test)]
#[path = "transitions/tests.rs"]
mod tests;
