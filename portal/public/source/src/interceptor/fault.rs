//! Seeded fault decisions for intercepted I/O operations.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultErrno {
    Econnreset,
    Etimedout,
    Eagain,
    Eintr,
    Eio,
    Epipe,
    Ebadf,
    Econnrefused,
}
impl FaultErrno {
    pub fn from_name(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "econnreset" | "connreset" => Some(Self::Econnreset),
            "etimedout" | "timedout" | "timeout" | "etimeout" => Some(Self::Etimedout),
            "eagain" | "again" | "wouldblock" => Some(Self::Eagain),
            "eintr" | "intr" => Some(Self::Eintr),
            "eio" | "io" => Some(Self::Eio),
            "epipe" | "pipe" => Some(Self::Epipe),
            "ebadf" | "badf" => Some(Self::Ebadf),
            "econnrefused" | "connrefused" => Some(Self::Econnrefused),
            _ => None,
        }
    }
    pub fn to_libc(self) -> i32 {
        match self {
            Self::Econnreset => libc::ECONNRESET,
            Self::Etimedout => libc::ETIMEDOUT,
            Self::Eagain => libc::EAGAIN,
            Self::Eintr => libc::EINTR,
            Self::Eio => libc::EIO,
            Self::Epipe => libc::EPIPE,
            Self::Ebadf => libc::EBADF,
            Self::Econnrefused => libc::ECONNREFUSED,
        }
    }
}
#[derive(Debug, Clone)]
pub struct FaultConfig {
    pub fault_rate: f64,
    pub fault_types: Vec<FaultErrno>,
}

impl Default for FaultConfig {
    fn default() -> Self {
        Self {
            fault_rate: 0.0,
            fault_types: vec![
                FaultErrno::Econnreset,
                FaultErrno::Etimedout,
                FaultErrno::Eagain,
                FaultErrno::Eintr,
                FaultErrno::Eio,
                FaultErrno::Epipe,
            ],
        }
    }
}
impl FaultConfig {
    pub fn is_enabled(&self) -> bool {
        self.fault_rate > 0.0 && !self.fault_types.is_empty()
    }
    /// Return a negative errno for an injected failure. Disabled configurations
    /// and protected standard streams consume no RNG draws.
    pub fn should_fault<R: rand::RngCore>(
        &self,
        nr: i64,
        args: &[u64; 6],
        rng: &mut R,
    ) -> Option<i32> {
        if !self.is_enabled() {
            return None;
        }
        if matches!(nr, libc::SYS_read | libc::SYS_write) && args[0] <= 2 {
            return None;
        }
        let sample = (rng.next_u64() as f64) / (u64::MAX as f64);
        if sample < self.fault_rate {
            let fault = self.fault_types[(rng.next_u64() as usize) % self.fault_types.len()];
            Some(-fault.to_libc())
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{RngCore, SeedableRng, rngs::mock::StepRng};
    use rand_chacha::ChaCha20Rng;

    const ERRORS: [(FaultErrno, i32); 8] = [
        (FaultErrno::Econnreset, libc::ECONNRESET),
        (FaultErrno::Etimedout, libc::ETIMEDOUT),
        (FaultErrno::Eagain, libc::EAGAIN),
        (FaultErrno::Eintr, libc::EINTR),
        (FaultErrno::Eio, libc::EIO),
        (FaultErrno::Epipe, libc::EPIPE),
        (FaultErrno::Ebadf, libc::EBADF),
        (FaultErrno::Econnrefused, libc::ECONNREFUSED),
    ];

    #[test]
    fn names_and_errno_values_cover_every_variant() {
        let aliases: &[(FaultErrno, &[&str])] = &[
            (FaultErrno::Econnreset, &["econnreset", "connreset"]),
            (
                FaultErrno::Etimedout,
                &["etimedout", "timedout", "timeout", "etimeout"],
            ),
            (FaultErrno::Eagain, &["eagain", "again", "wouldblock"]),
            (FaultErrno::Eintr, &["eintr", "intr"]),
            (FaultErrno::Eio, &["eio", "io"]),
            (FaultErrno::Epipe, &["epipe", "pipe"]),
            (FaultErrno::Ebadf, &["ebadf", "badf"]),
            (FaultErrno::Econnrefused, &["econnrefused", "connrefused"]),
        ];
        for (fault, names) in aliases {
            for name in *names {
                assert_eq!(FaultErrno::from_name(name), Some(*fault));
                assert_eq!(
                    FaultErrno::from_name(&name.to_ascii_uppercase()),
                    Some(*fault)
                );
            }
        }
        assert_eq!(
            FaultErrno::from_name("WoUlDbLoCk"),
            Some(FaultErrno::Eagain)
        );
        for name in ["", "unknown", "ECONNRESET ", "EWOULDBLOCK"] {
            assert_eq!(FaultErrno::from_name(name), None);
        }
        for (fault, errno) in ERRORS {
            assert_eq!(fault.to_libc(), errno);
        }
    }

    #[test]
    fn defaults_and_enablement_require_a_positive_rate_and_fault_types() {
        let default = FaultConfig::default();
        assert_eq!(default.fault_rate, 0.0);
        assert_eq!(
            default.fault_types,
            ERRORS[..6]
                .iter()
                .map(|(fault, _)| *fault)
                .collect::<Vec<_>>()
        );
        let disabled = [
            FaultConfig {
                fault_rate: 0.0,
                ..default.clone()
            },
            FaultConfig {
                fault_rate: -1.0,
                ..default.clone()
            },
            FaultConfig {
                fault_rate: f64::NAN,
                ..default.clone()
            },
            FaultConfig {
                fault_rate: 1.0,
                fault_types: vec![],
            },
        ];
        for config in disabled {
            assert!(!config.is_enabled());
            let mut rng = StepRng::new(7, 1);
            assert_eq!(
                config.should_fault(libc::SYS_connect, &[0; 6], &mut rng),
                None
            );
            assert_eq!(rng.next_u64(), 7);
        }
        assert!(
            FaultConfig {
                fault_rate: 0.01,
                ..default
            }
            .is_enabled()
        );
    }

    #[test]
    fn seeded_decisions_are_repeatable_and_have_the_configured_distribution() {
        let config = FaultConfig {
            fault_rate: 0.35,
            fault_types: ERRORS.iter().map(|(fault, _)| *fault).collect(),
        };
        let draws = |seed| {
            let mut rng = ChaCha20Rng::seed_from_u64(seed);
            (0..10_000)
                .map(|_| config.should_fault(libc::SYS_connect, &[3, 0, 0, 0, 0, 0], &mut rng))
                .collect::<Vec<_>>()
        };
        let first = draws(0xDEADBEEF);
        assert_eq!(first, draws(0xDEADBEEF));
        assert_ne!(first, draws(7));
        let fault_count = first.iter().filter(|fault| fault.is_some()).count();
        assert!(
            (3200..3800).contains(&fault_count),
            "fault count {fault_count}"
        );
        for (_, errno) in ERRORS {
            let count = first.iter().filter(|fault| **fault == Some(-errno)).count();
            assert!((300..600).contains(&count), "errno {errno}: count {count}");
        }
    }

    #[test]
    fn protects_all_standard_streams_without_consuming_rng() {
        let config = FaultConfig {
            fault_rate: 1.0,
            fault_types: vec![FaultErrno::Eio],
        };
        let mut rng = StepRng::new(0, 1);
        for nr in [libc::SYS_read, libc::SYS_write] {
            for fd in 0..=2 {
                assert_eq!(
                    config.should_fault(nr, &[fd, 0, 0, 0, 0, 0], &mut rng),
                    None
                );
            }
        }
        assert_eq!(rng.next_u64(), 0);
        for nr in [libc::SYS_read, libc::SYS_write] {
            assert_eq!(
                config.should_fault(nr, &[3, 0, 0, 0, 0, 0], &mut rng),
                Some(-libc::EIO)
            );
        }
        assert_eq!(
            config.should_fault(libc::SYS_connect, &[0; 6], &mut rng),
            Some(-libc::EIO)
        );
    }

    #[test]
    fn sampling_is_strict_and_uses_a_second_draw_only_for_faults() {
        let config = FaultConfig {
            fault_rate: 1.0,
            fault_types: ERRORS.iter().map(|(fault, _)| *fault).collect(),
        };
        let mut rng = StepRng::new(u64::MAX, 1);
        assert_eq!(
            config.should_fault(libc::SYS_connect, &[0; 6], &mut rng),
            None
        );
        assert_eq!(rng.next_u64(), 0);
        for (index, (_, errno)) in ERRORS.into_iter().enumerate() {
            let mut rng = StepRng::new(0, index as u64);
            assert_eq!(
                config.should_fault(libc::SYS_connect, &[0; 6], &mut rng),
                Some(-errno)
            );
            assert_eq!(rng.next_u64(), 2 * index as u64);
        }
    }
}
