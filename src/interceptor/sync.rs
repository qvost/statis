//! Deterministic FIFO futex queues, keyed by target virtual address.

use std::collections::BTreeMap;

use nix::unistd::Pid;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FutexWaiter {
    pub tid: Pid,
    pub notif_id: u64,
    pub bitset: u32,
}

#[derive(Debug, Default)]
pub struct FutexManager {
    waiters: BTreeMap<usize, Vec<FutexWaiter>>,
}

impl FutexManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn wait(&mut self, uaddr: usize, waiter: FutexWaiter) {
        self.waiters.entry(uaddr).or_default().push(waiter);
    }

    /// Remove up to `count` matching waiters in registration order.
    pub fn wake(&mut self, uaddr: usize, count: usize, bitset: u32) -> Vec<FutexWaiter> {
        let mut woken = Vec::new();
        if let Some(queue) = self.waiters.get_mut(&uaddr) {
            queue.retain(|waiter| {
                if woken.len() < count && waiter.bitset & bitset != 0 {
                    woken.push(*waiter);
                    false
                } else {
                    true
                }
            });
            if queue.is_empty() {
                self.waiters.remove(&uaddr);
            }
        }
        woken
    }

    pub fn remove_by_notif_id(&mut self, notif_id: u64) {
        self.waiters.retain(|_, queue| {
            queue.retain(|waiter| waiter.notif_id != notif_id);
            !queue.is_empty()
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn waiter(id: u64, bitset: u32) -> FutexWaiter {
        FutexWaiter {
            tid: Pid::from_raw(id as i32),
            notif_id: id,
            bitset,
        }
    }

    #[test]
    fn wakes_fifo_and_keeps_addresses_separate() {
        let mut manager = FutexManager::new();
        for id in [3, 1, 2] {
            manager.wait(0x1000, waiter(id, u32::MAX));
        }
        manager.wait(0x2000, waiter(4, u32::MAX));
        assert!(manager.wake(0x1000, 0, u32::MAX).is_empty());
        assert_eq!(
            manager.wake(0x1000, 2, u32::MAX),
            [waiter(3, u32::MAX), waiter(1, u32::MAX)]
        );
        assert_eq!(
            manager.wake(0x1000, usize::MAX, u32::MAX),
            [waiter(2, u32::MAX)]
        );
        assert!(!manager.waiters.contains_key(&0x1000));
        assert_eq!(manager.wake(0x2000, 1, u32::MAX), [waiter(4, u32::MAX)]);
        assert!(manager.wake(0x3000, 1, u32::MAX).is_empty());
        assert!(manager.waiters.is_empty());
    }

    #[test]
    fn masks_match_any_common_bit_without_reordering_others() {
        let mut manager = FutexManager::default();
        for (id, mask) in [(1, 1), (2, 2), (3, 3), (4, 4)] {
            manager.wait(0x1000, waiter(id, mask));
        }
        assert!(manager.wake(0x1000, 10, 0).is_empty());
        assert_eq!(manager.wake(0x1000, 1, 2), [waiter(2, 2)]);
        assert_eq!(manager.wake(0x1000, 10, 2), [waiter(3, 3)]);
        assert_eq!(
            manager.wake(0x1000, 10, u32::MAX),
            [waiter(1, 1), waiter(4, 4)]
        );
    }

    #[test]
    fn cancellation_cleans_all_matching_queues() {
        let mut manager = FutexManager::default();
        manager.wait(1, waiter(1, 1));
        manager.wait(2, waiter(1, 2));
        manager.wait(2, waiter(2, 2));
        manager.remove_by_notif_id(99);
        manager.remove_by_notif_id(1);
        assert!(!manager.waiters.contains_key(&1));
        assert_eq!(manager.wake(2, 10, u32::MAX), [waiter(2, 2)]);
        assert!(manager.waiters.is_empty());
    }
}
