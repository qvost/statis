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
        let runnable: Vec<_> = self
            .threads
            .values()
            .filter(|thread| thread.state == ThreadState::Runnable)
            .map(|thread| thread.tid)
            .collect();
        let next = if runnable.is_empty() {
            None
        } else {
            Some(runnable[self.rng.gen_range(0..runnable.len())])
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
