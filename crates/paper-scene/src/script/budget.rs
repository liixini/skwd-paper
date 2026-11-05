use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

pub const ENGINE_WATCHDOG: Duration = Duration::from_secs(15);
pub const LOAD_BUDGET: Duration = Duration::from_secs(5);
const CLEANUP_BUDGET: Duration = Duration::from_millis(100);
const HOST_TRACE: &str = "paper_scene::script::host";
const HOST_EXHAUSTED: &str = "SceneScript host operation budget exhausted";

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limits {
    pub load: Duration,
    pub compose: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self { load: LOAD_BUDGET, compose: ENGINE_WATCHDOG }
    }
}

pub(super) struct Budget {
    deadline: Cell<Instant>,
    allowance: Cell<Duration>,
    spent: Cell<Duration>,
    depth: Cell<u32>,
}

impl Budget {
    pub(super) fn new(allowance: Duration) -> Rc<Self> {
        Rc::new(Self {
            deadline: Cell::new(Instant::now() + allowance),
            allowance: Cell::new(allowance),
            spent: Cell::new(Duration::ZERO),
            depth: Cell::new(0),
        })
    }

    pub(super) fn get(&self) -> Instant {
        self.deadline.get()
    }

    pub(super) fn exhausted(&self) -> bool {
        Instant::now() >= self.deadline.get()
    }

    pub(super) fn enter(self: &Rc<Self>) -> Entry {
        let started = Instant::now();
        if self.depth.replace(self.depth.get() + 1) == 0 {
            self.deadline.set(started + self.allowance.get().saturating_sub(self.spent.get()));
        }
        Entry { budget: self.clone(), started }
    }

    pub(super) fn renew(&self, allowance: Duration) {
        self.allowance.set(allowance);
        self.spent.set(Duration::ZERO);
    }

    pub(super) fn spent(&self) -> bool {
        self.spent.get() >= self.allowance.get()
    }

    pub(super) fn cleanup(&self) {
        self.deadline.set(Instant::now() + CLEANUP_BUDGET);
    }

    pub(super) fn restore(&self, deadline: Instant) {
        self.deadline.set(deadline);
    }

    pub(super) fn host(&self) -> Result<HostWork, &'static str> {
        let started = Instant::now();
        if started >= self.get() {
            return Err(HOST_EXHAUSTED);
        }
        Ok(HostWork { started })
    }
}

pub(super) struct Entry {
    budget: Rc<Budget>,
    started: Instant,
}

impl Drop for Entry {
    fn drop(&mut self) {
        let depth = self.budget.depth.get() - 1;
        self.budget.depth.set(depth);
        if depth == 0 {
            self.budget.spent.set(self.budget.spent.get() + self.started.elapsed());
        }
    }
}

pub(super) struct HostWork {
    started: Instant,
}

impl Drop for HostWork {
    fn drop(&mut self) {
        tracing::trace!(target: HOST_TRACE, elapsed_us = self.started.elapsed().as_micros());
    }
}
