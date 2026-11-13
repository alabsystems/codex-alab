// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Portable goal accounting baseline within one host's monotonic clock domain.
//! Transport must separately prove the original process/boot. These ticks carry
//! time data only, never native SDK or execution authority.
#[cfg(unix)]
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    domain: u8,
    seconds: u64,
    nanos: u32,
}
impl Stamp {
    pub fn parts(self) -> (u8, u64, u32) {
        (self.domain, self.seconds, self.nanos)
    }
    pub fn from_parts(domain: u8, seconds: u64, nanos: u32) -> Result<Self, &'static str> {
        if domain != DOMAIN || DOMAIN == 0 || nanos >= 1_000_000_000 || seconds > i64::MAX as u64 {
            return Err("unsupported or malformed native goal clock");
        }
        Ok(Self {
            domain,
            seconds,
            nanos,
        })
    }
}

#[cfg(all(unix, target_vendor = "apple"))]
const DOMAIN: u8 = 1;
#[cfg(all(unix, not(target_vendor = "apple")))]
const DOMAIN: u8 = 2;
#[cfg(not(unix))]
const DOMAIN: u8 = 0;

#[cfg(unix)]
#[derive(Debug, Clone, Copy)]
pub struct Instant {
    ticks: Duration,
    adopted: Option<Stamp>,
}
#[cfg(not(unix))]
pub use std::time::Instant;

#[cfg(unix)]
impl Instant {
    pub fn now() -> Self {
        let mut value = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        #[cfg(target_vendor = "apple")]
        let clock = libc::CLOCK_UPTIME_RAW;
        #[cfg(not(target_vendor = "apple"))]
        let clock = libc::CLOCK_MONOTONIC;
        // SAFETY: the platform clock ID is valid and value is writable.
        assert_eq!(
            unsafe { libc::clock_gettime(clock, &raw mut value) },
            0,
            "native goal clock unavailable"
        );
        let (Ok(seconds), Ok(nanos)) = (u64::try_from(value.tv_sec), u32::try_from(value.tv_nsec))
        else {
            panic!("invalid native goal clock reading");
        };
        assert!(nanos < 1_000_000_000, "invalid native goal clock fraction");
        Self {
            ticks: Duration::new(seconds, nanos),
            adopted: None,
        }
    }
    pub fn elapsed(self) -> Duration {
        Self::now().ticks.saturating_sub(self.ticks)
    }
    pub fn checked_add(self, duration: Duration) -> Option<Self> {
        let ticks = self.ticks.checked_add(duration)?;
        (ticks.as_secs() <= i64::MAX as u64).then_some(Self { ticks, ..self })
    }
    pub fn checked_sub(self, duration: Duration) -> Option<Self> {
        Some(Self {
            ticks: self.ticks.checked_sub(duration)?,
            ..self
        })
    }
}

pub fn checkpoint(instant: &Instant) -> Result<Stamp, &'static str> {
    #[cfg(unix)]
    {
        Stamp::from_parts(
            DOMAIN,
            instant.ticks.as_secs(),
            instant.ticks.subsec_nanos(),
        )
    }
    #[cfg(not(unix))]
    {
        let _ = instant;
        Err("native goal clock transfer is unsupported")
    }
}

/// Idempotent adoption never rewinds an already-accounted restored baseline.
pub fn adopt(instant: &mut Instant, stamp: Stamp) -> Result<(), &'static str> {
    #[cfg(unix)]
    {
        Stamp::from_parts(stamp.domain, stamp.seconds, stamp.nanos)?;
        if let Some(original) = instant.adopted {
            return if original == stamp {
                Ok(())
            } else {
                Err("another native goal clock was already restored")
            };
        }
        let ticks = Duration::new(stamp.seconds, stamp.nanos);
        if ticks > Instant::now().ticks {
            return Err("native goal clock baseline is in the future");
        }
        *instant = Instant {
            ticks,
            adopted: Some(stamp),
        };
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = (instant, stamp);
        Err("native goal clock transfer is unsupported")
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[tokio::test]
    async fn clock_baseline_survives_an_actual_same_pid_exec() {
        const STAGE: &str = "ALAB_GOAL_CLOCK_EXEC_STAGE";
        const DATA: &str = "ALAB_GOAL_CLOCK_EXEC_DATA";
        const PID: &str = "ALAB_GOAL_CLOCK_EXEC_PID";
        const TEST: &str = "goal_clock::tests::clock_baseline_survives_an_actual_same_pid_exec";
        const SUCCESS: &str = "ALAB_GOAL_CLOCK_EXEC_OK";
        match std::env::var(STAGE).ok().as_deref() {
            None => {
                let output = tokio::time::timeout(
                    Duration::from_secs(15),
                    tokio::process::Command::new(std::env::current_exe().unwrap())
                        .args(["--exact", TEST, "--nocapture"])
                        .env(STAGE, "source")
                        .kill_on_drop(true)
                        .output(),
                )
                .await
                .unwrap()
                .unwrap();
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                assert!(String::from_utf8_lossy(&output.stdout).contains(SUCCESS));
            }
            Some("source") => {
                use std::os::unix::process::CommandExt;
                let baseline = Instant::now()
                    .checked_sub(Duration::new(2, 999_999_999))
                    .unwrap();
                let (domain, seconds, nanos) = checkpoint(&baseline).unwrap().parts();
                let error = std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", TEST, "--nocapture"])
                    .env(STAGE, "destination")
                    .env(DATA, format!("{domain}:{seconds}:{nanos}"))
                    .env(PID, std::process::id().to_string())
                    .exec();
                panic!("test exec failed: {error}");
            }
            Some("destination") => {
                assert_eq!(std::env::var(PID).unwrap(), std::process::id().to_string());
                let data = std::env::var(DATA).unwrap();
                let parts = data.split(':').collect::<Vec<_>>();
                let original = Stamp::from_parts(
                    parts[0].parse().unwrap(),
                    parts[1].parse().unwrap(),
                    parts[2].parse().unwrap(),
                )
                .unwrap();
                let mut current = Instant::now();
                adopt(&mut current, original).unwrap();
                assert_eq!(checkpoint(&current).unwrap(), original);
                assert!(current.elapsed() >= Duration::new(2, 999_999_999));
                println!("{SUCCESS}");
            }
            _ => panic!("unknown clock test stage"),
        }
    }
    #[test]
    fn repeated_handoffs_preserve_every_baseline_nanosecond() {
        let baseline = Instant::now()
            .checked_sub(Duration::from_nanos(999_999_999))
            .unwrap();
        let original = checkpoint(&baseline).unwrap();
        let mut stamp = original;
        for _ in 0..1000 {
            let (domain, seconds, nanos) = stamp.parts();
            let decoded = Stamp::from_parts(domain, seconds, nanos).unwrap();
            let mut fresh = Instant::now();
            adopt(&mut fresh, decoded).unwrap();
            stamp = checkpoint(&fresh).unwrap();
            assert_eq!(stamp, original);
        }
    }
    #[test]
    fn retry_after_accounting_cannot_rewind_or_replace_the_clock() {
        let mut current = Instant::now();
        let old = checkpoint(&current.checked_sub(Duration::from_secs(2)).unwrap()).unwrap();
        adopt(&mut current, old).unwrap();
        current = current.checked_add(Duration::from_secs(1)).unwrap();
        let advanced = checkpoint(&current).unwrap();
        adopt(&mut current, old).unwrap();
        assert_eq!(checkpoint(&current).unwrap(), advanced);
        assert!(adopt(&mut current, advanced).is_err());
        assert!(Stamp::from_parts(0, 0, 0).is_err());
        assert!(Stamp::from_parts(DOMAIN, 1, 1_000_000_000).is_err());
        let future = Stamp::from_parts(DOMAIN, i64::MAX as u64, 0).unwrap();
        assert!(adopt(&mut Instant::now(), future).is_err());
    }
}
