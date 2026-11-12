//! Deterministic runnable-thread selection and instruction accounting.

pub mod pmu;

use std::{collections::BTreeMap, os::fd::OwnedFd};

use nix::unistd::Pid;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha20Rng;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadState {
    Runnable,
    Waiting,
}

#[derive(Debug)]
pub struct ThreadDescriptor {
    pub tid: Pid,
    pub pmu_fd: Option<OwnedFd>,
    pub instructions_executed: u64,
    pub state: ThreadState,
}

pub struct DeterministicScheduler {
    threads: BTreeMap<Pid, ThreadDescriptor>,
    active_tid: Option<Pid>,
    quantum: u64,
    rng: ChaCha20Rng,
}

impl DeterministicScheduler {
    pub fn new(seed: u64, quantum: u64) -> Self {
        Self {
            threads: BTreeMap::new(),
            active_tid: None,
            quantum,
            rng: ChaCha20Rng::seed_from_u64(seed),
        }
    }

    pub fn pick_next_runnable(&mut self) -> Option<Pid> {
        let runnable = self
            .threads
            .values()
            .filter(|thread| thread.state == ThreadState::Runnable)
            .count();
        let next = if runnable == 0 {
            None
        } else {
            let index = self.rng.gen_range(0..runnable);
            self.threads
                .values()
                .filter(|thread| thread.state == ThreadState::Runnable)
                .nth(index)
                .map(|thread| thread.tid)
        };
        self.active_tid = next;
        next
    }

    pub fn register_thread(&mut self, tid: Pid) {
        self.threads.entry(tid).or_insert(ThreadDescriptor {
            tid,
            pmu_fd: None,
            instructions_executed: 0,
            state: ThreadState::Runnable,
        });
    }

    pub fn remove_thread(&mut self, tid: Pid) {
        self.threads.remove(&tid);
        if self.active_tid == Some(tid) {
            self.active_tid = None;
        }
    }

    pub fn thread_mut(&mut self, tid: Pid) -> Option<&mut ThreadDescriptor> {
        self.threads.get_mut(&tid)
    }
    pub fn active_tid(&self) -> Option<Pid> {
        self.active_tid
    }
    pub fn quantum(&self) -> u64 {
        self.quantum
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_preserves_seeded_order_without_a_runnable_vector() {
        let mut scheduler = DeterministicScheduler::new(42, 10_000);
        for tid in [4, 2, 3, 1] {
            scheduler.register_thread(Pid::from_raw(tid));
        }
        scheduler.thread_mut(Pid::from_raw(2)).unwrap().state = ThreadState::Waiting;
        let eligible = [1, 3, 4]; // the original BTreeMap/Vec selection order
        let mut reference = ChaCha20Rng::seed_from_u64(42);
        for _ in 0..100 {
            let expected = Pid::from_raw(eligible[reference.gen_range(0..eligible.len())]);
            assert_eq!(scheduler.pick_next_runnable(), Some(expected));
            assert_eq!(scheduler.active_tid(), Some(expected));
        }
    }

    #[test]
    fn empty_and_blocked_sets_do_not_consume_the_rng() {
        let mut scheduler = DeterministicScheduler::new(42, 10_000);
        assert_eq!(scheduler.pick_next_runnable(), None);
        let one = Pid::from_raw(1);
        let two = Pid::from_raw(2);
        for tid in [one, two] {
            scheduler.register_thread(tid);
            scheduler.thread_mut(tid).unwrap().state = ThreadState::Waiting;
        }
        assert_eq!(scheduler.pick_next_runnable(), None);
        assert_eq!(scheduler.active_tid(), None);
        scheduler.thread_mut(one).unwrap().state = ThreadState::Runnable;
        scheduler.thread_mut(two).unwrap().state = ThreadState::Runnable;
        let mut reference = ChaCha20Rng::seed_from_u64(42);
        let expected = [one, two][reference.gen_range(0..2)];
        assert_eq!(scheduler.pick_next_runnable(), Some(expected));
        scheduler.remove_thread(expected);
        assert_eq!(scheduler.active_tid(), None);
        assert_eq!(
            scheduler.pick_next_runnable(),
            Some(if expected == one { two } else { one })
        );
    }
}
