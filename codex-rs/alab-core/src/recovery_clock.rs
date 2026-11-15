// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A boot-bound time axis for persisted retry pacing. Wall time labels the axis;
//! only the kernel's sleep-inclusive clock measures elapsed time on it.

use crate::session::ProcessIdentity;
use std::time::Duration;
use std::time::Instant;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sample {
    pub boot_id: String,
    pub continuous_ns: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Anchor {
    pub boot_id: String,
    pub continuous_ns: u64,
    pub epoch_ms: u64,
}

fn valid_boot(boot_id: &str) -> bool {
    !boot_id.is_empty()
        && boot_id.len() <= 128
        && boot_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

impl Sample {
    /// Missing native identity or clock support supplies no pacing authority.
    #[must_use]
    pub fn read(identity: Option<&ProcessIdentity>) -> Option<Self> {
        let boot_id = match identity? {
            ProcessIdentity::Linux { boot_id, .. } | ProcessIdentity::MacOs { boot_id, .. } => {
                boot_id
            }
        };
        valid_boot(boot_id).then_some(())?;
        Some(Self {
            boot_id: boot_id.clone(),
            continuous_ns: crate::wake::continuous_ns()?,
        })
    }

    #[must_use]
    pub fn anchor(&self, epoch_ms: u64) -> Option<Anchor> {
        valid_boot(&self.boot_id).then(|| Anchor {
            boot_id: self.boot_id.clone(),
            continuous_ns: self.continuous_ns,
            epoch_ms,
        })
    }
}

impl Anchor {
    #[must_use]
    pub fn valid(&self) -> bool {
        valid_boot(&self.boot_id)
    }

    fn elapsed_ns(&self, sample: &Sample) -> Option<u64> {
        (self.valid() && self.boot_id == sample.boot_id).then_some(())?;
        sample.continuous_ns.checked_sub(self.continuous_ns)
    }

    /// Capture Instant before taking `sample`. Rounding elapsed time upwards
    /// keeps even a past point's local backoff from moving earlier on this axis.
    #[must_use]
    pub fn epoch_at(&self, sample: &Sample) -> Option<u64> {
        let elapsed = self.elapsed_ns(sample)?;
        self.epoch_ms.checked_add(elapsed.div_ceil(1_000_000))
    }

    /// Take `sample` before reading `now`. A pause between them extends a bound.
    /// Exact nanoseconds matter here: rounding elapsed time up could spend the
    /// final fractional millisecond of a provider's wait before it actually passes.
    #[must_use]
    pub fn instant_of(&self, ms: u64, sample: &Sample, now: Instant) -> Option<Instant> {
        let elapsed = i128::from(self.elapsed_ns(sample)?);
        let encoded = i128::from(ms) - i128::from(self.epoch_ms);
        let remaining = encoded.checked_mul(1_000_000)?.checked_sub(elapsed)?;
        let magnitude = remaining.unsigned_abs();
        let duration = Duration::new(
            u64::try_from(magnitude / 1_000_000_000).ok()?,
            u32::try_from(magnitude % 1_000_000_000).ok()?,
        );
        if remaining >= 0 {
            let deadline = now.checked_add(duration)?;
            // Tokio's timers round up to the next millisecond.
            deadline.checked_add(Duration::from_millis(1))?;
            Some(deadline)
        } else {
            // An unrepresentably old bound is already due. Using now instead
            // keeps a reconstructed local backoff conservative.
            Some(now.checked_sub(duration).unwrap_or(now))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(ns: u64) -> Sample {
        Sample {
            boot_id: "same-boot".into(),
            continuous_ns: ns,
        }
    }

    #[test]
    fn elapsed_boot_time_preserves_a_relative_wait_without_wall_clock_input() {
        let origin = sample(5_000_000_000);
        let anchor = origin.anchor(1_800_000_000_000).unwrap();
        let now = Instant::now();
        let later = sample(origin.continuous_ns + 60_000_000_000);
        assert_eq!(anchor.epoch_at(&later), Some(anchor.epoch_ms + 60_000));
        assert_eq!(
            anchor.instant_of(anchor.epoch_ms + 600_000, &later, now),
            Some(now + Duration::from_secs(540))
        );
        let slept = sample(origin.continuous_ns + 720_000_000_000);
        assert_eq!(
            anchor.instant_of(anchor.epoch_ms + 600_000, &slept, now),
            Some(now - Duration::from_secs(120))
        );
    }

    #[test]
    fn capture_rounds_up_but_restore_does_not_spend_fractional_time() {
        let anchor = sample(1_000_000).anchor(100).unwrap();
        let later = sample(1_000_001);
        let now = Instant::now();
        assert_eq!(anchor.epoch_at(&later), Some(101));
        assert_eq!(
            anchor.instant_of(101, &later, now),
            Some(now + Duration::from_nanos(999_999))
        );
    }

    #[test]
    fn different_boot_rewound_counter_and_epoch_overflow_refuse() {
        let anchor = sample(100).anchor(u64::MAX).unwrap();
        let now = Instant::now();
        let other = Sample {
            boot_id: "another-boot".into(),
            continuous_ns: 101,
        };
        for invalid in [other, sample(99)] {
            assert_eq!(anchor.epoch_at(&invalid), None);
            assert_eq!(anchor.instant_of(u64::MAX, &invalid, now), None);
        }
        assert_eq!(anchor.epoch_at(&sample(101)), None);
        assert!(sample(100).anchor(0).unwrap().valid());
        for boot_id in [String::new(), "bad boot".into(), "a".repeat(129)] {
            assert!(
                Sample {
                    boot_id,
                    continuous_ns: 100
                }
                .anchor(1)
                .is_none()
            );
        }
        assert!(Sample::read(None).is_none());
    }

    #[test]
    fn platform_counter_is_a_bounded_read_without_global_session_mutation() {
        let first = crate::wake::continuous_ns();
        let second = crate::wake::continuous_ns();
        if crate::wake::supported() {
            assert!(
                second.expect("second native boot-clock sample")
                    >= first.expect("first native boot-clock sample")
            );
        } else {
            assert_eq!(first, None);
            assert_eq!(second, None);
        }
    }
}
