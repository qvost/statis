//! Monotonic virtual time and scheduled thread wakeups.

pub mod wheel;

pub use wheel::{TimeWheel, TimerEvent};

#[derive(Clone, Debug)]
pub struct VirtualClock {
    virtual_time_ns: u64,
    tick_step_ns: u64,
}

impl Default for VirtualClock {
    fn default() -> Self {
        Self::new(0, 100)
    }
}

impl VirtualClock {
    pub fn new(initial_ns: u64, tick_step_ns: u64) -> Self {
        Self {
            virtual_time_ns: initial_ns,
            tick_step_ns,
        }
    }

    #[inline]
    pub fn now_ns(&self) -> u64 {
        self.virtual_time_ns
    }

    /// Advance one tick, saturating at the end of the representable timeline.
    #[inline]
    pub fn advance_tick(&mut self) -> u64 {
        self.virtual_time_ns = self.virtual_time_ns.saturating_add(self.tick_step_ns);
        self.virtual_time_ns
    }

    #[inline]
    pub fn advance_to(&mut self, target_ns: u64) {
        self.virtual_time_ns = self.virtual_time_ns.max(target_ns);
    }

    #[inline]
    pub fn now_timespec(&self) -> libc::timespec {
        libc::timespec {
            tv_sec: (self.virtual_time_ns / 1_000_000_000) as _,
            tv_nsec: (self.virtual_time_ns % 1_000_000_000) as _,
        }
    }

    #[inline]
    pub fn now_timeval(&self) -> libc::timeval {
        libc::timeval {
            tv_sec: (self.virtual_time_ns / 1_000_000_000) as _,
            tv_usec: ((self.virtual_time_ns % 1_000_000_000) / 1_000) as _,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_and_warps_never_go_backwards() {
        let mut clock = VirtualClock::default();
        assert_eq!(clock.now_ns(), 0);
        assert_eq!(clock.advance_tick(), 100);
        clock.advance_to(90);
        assert_eq!(clock.now_ns(), 100);
        clock.advance_to(200);
        assert_eq!(clock.advance_tick(), 300);
        let mut custom = VirtualClock::new(5, 7);
        assert_eq!(custom.advance_tick(), 12);
    }

    #[test]
    fn conversions_split_seconds_and_truncate_microseconds() {
        for ns in [
            0,
            999,
            1_000,
            999_999_999,
            1_000_000_000,
            2_345_678_901,
            u64::MAX,
        ] {
            let clock = VirtualClock::new(ns, 0);
            let ts = clock.now_timespec();
            let tv = clock.now_timeval();
            assert_eq!(ts.tv_sec as u64, ns / 1_000_000_000);
            assert_eq!(ts.tv_nsec as u64, ns % 1_000_000_000);
            assert_eq!(tv.tv_sec, ts.tv_sec);
            assert_eq!(tv.tv_usec as u64, (ns % 1_000_000_000) / 1_000);
        }
    }

    #[test]
    fn zero_ticks_and_overflow_preserve_monotonicity() {
        assert_eq!(VirtualClock::new(17, 0).advance_tick(), 17);
        let mut clock = VirtualClock::new(u64::MAX - 1, 100);
        assert_eq!(clock.advance_tick(), u64::MAX);
        assert_eq!(clock.advance_tick(), u64::MAX);
    }
}
