//! Pending sleep notifications, ordered by their virtual deadlines.

use std::{cmp::Ordering, collections::BinaryHeap};

use nix::unistd::Pid;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimerEvent {
    pub wake_time_ns: u64,
    pub notif_id: u64,
    pub tid: Pid,
}

impl Ord for TimerEvent {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse every field so ties are deterministic and Ord agrees with Eq.
        (other.wake_time_ns, other.notif_id, other.tid.as_raw()).cmp(&(
            self.wake_time_ns,
            self.notif_id,
            self.tid.as_raw(),
        ))
    }
}

impl PartialOrd for TimerEvent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Default)]
pub struct TimeWheel {
    events: BinaryHeap<TimerEvent>,
}

impl TimeWheel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_timer(&mut self, event: TimerEvent) {
        self.events.push(event);
    }

    pub fn pop_expired(&mut self, current_time_ns: u64) -> Vec<TimerEvent> {
        let mut expired = Vec::new();
        while self
            .earliest_wake_time()
            .is_some_and(|time| time <= current_time_ns)
        {
            expired.push(self.events.pop().expect("heap was just checked"));
        }
        expired
    }

    pub fn earliest_wake_time(&self) -> Option<u64> {
        self.events.peek().map(|event| event.wake_time_ns)
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub(crate) fn retain(&mut self, keep: impl FnMut(&TimerEvent) -> bool) {
        self.events.retain(keep);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(time: u64, id: u64, tid: i32) -> TimerEvent {
        TimerEvent {
            wake_time_ns: time,
            notif_id: id,
            tid: Pid::from_raw(tid),
        }
    }

    #[test]
    fn empty_wheel() {
        let mut wheel = TimeWheel::new();
        assert!(wheel.is_empty());
        assert_eq!(wheel.len(), 0);
        assert_eq!(wheel.earliest_wake_time(), None);
        assert!(wheel.pop_expired(u64::MAX).is_empty());
    }

    #[test]
    fn deadlines_are_inclusive_and_sorted() {
        let mut wheel = TimeWheel::default();
        for event in [event(30, 3, 1), event(10, 1, 1), event(20, 2, 1)] {
            wheel.add_timer(event);
        }
        assert_eq!(wheel.len(), 3);
        assert_eq!(wheel.earliest_wake_time(), Some(10));
        assert!(wheel.pop_expired(9).is_empty());
        assert_eq!(wheel.pop_expired(20), [event(10, 1, 1), event(20, 2, 1)]);
        assert_eq!(wheel.earliest_wake_time(), Some(30));
        assert_eq!(wheel.pop_expired(u64::MAX), [event(30, 3, 1)]);
        assert!(wheel.is_empty());
    }

    #[test]
    fn heap_order_breaks_ties_consistently() {
        let events = [event(0, 2, 1), event(0, 1, 2), event(0, 1, 1)];
        let mut heap = BinaryHeap::from(events);
        for expected in events.into_iter().rev() {
            assert_eq!(heap.pop(), Some(expected));
            assert_eq!(expected.partial_cmp(&expected), Some(Ordering::Equal));
        }
    }

    #[test]
    fn canceled_timers_do_not_affect_next_deadline() {
        let mut wheel = TimeWheel::default();
        wheel.add_timer(event(1, 1, 1));
        wheel.add_timer(event(u64::MAX, 2, 2));
        wheel.retain(|event| event.notif_id != 1);
        assert_eq!(wheel.earliest_wake_time(), Some(u64::MAX));
        assert_eq!(wheel.pop_expired(u64::MAX), [event(u64::MAX, 2, 2)]);
    }
}
