use std::collections::VecDeque;
use std::num::NonZeroUsize;
use std::time::{Duration, Instant};

pub use crate::shared::DecodeDevice as Device;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Content {
    Digest { payload: [u8; 32], bytes: u64 },
    Package { identity: super::input::Identity, clip: crate::decode::Clip },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Key {
    pub device: Device,
    pub content: Content,
    pub configuration: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Healthy,
    Crashed,
    TimedOut,
    Cancelled,
    Saturated,
    Invalid,
}

impl Outcome {
    fn retained(self) -> bool {
        matches!(self, Self::Healthy | Self::Crashed)
    }
}

pub struct Cache {
    entries: VecDeque<(Key, Outcome)>,
    capacity: NonZeroUsize,
}

impl Cache {
    pub fn new(capacity: NonZeroUsize) -> Self {
        Self { entries: VecDeque::new(), capacity }
    }

    pub fn get(&mut self, key: &Key) -> Option<Outcome> {
        let index = self.entries.iter().position(|(entry, _)| entry == key)?;
        let entry = self.entries.remove(index)?;
        let outcome = entry.1;
        self.entries.push_back(entry);
        Some(outcome)
    }

    pub fn record(&mut self, key: Key, outcome: Outcome) {
        if !outcome.retained() {
            return;
        }
        self.entries.retain(|(entry, _)| entry != &key);
        if self.entries.len() == self.capacity.get() {
            self.entries.pop_front();
        }
        self.entries.push_back((key, outcome));
    }
}

pub struct Budget {
    deadline: Instant,
    child_limit: Duration,
}

impl Budget {
    pub fn new(started: Instant, total: Duration, child_limit: Duration) -> Option<Self> {
        Some(Self { deadline: started.checked_add(total)?, child_limit })
    }

    pub fn submit(&self, now: Instant) -> Option<Instant> {
        if now >= self.deadline {
            return None;
        }
        Some(now.checked_add(self.child_limit)?.min(self.deadline))
    }
}
