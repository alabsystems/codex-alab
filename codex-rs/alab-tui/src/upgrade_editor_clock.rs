// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Editor deadlines within the authenticated host/boot's monotonic clock domain.
//! No Instant layout or wall clock crosses exec. Transport proves host identity.

use codex_alab_core::goal_clock;
use serde::Deserialize;
use serde::Serialize;
use std::time::Duration;
use std::time::Instant;

// Rust's unix Instant uses CLOCK_UPTIME_RAW on Apple and CLOCK_MONOTONIC
// elsewhere, matching goal_clock. Bracket that public API conversion and refuse
// a preempted sample instead of moving short paste deadlines by an unknown amount.
const MAX_SAMPLE: Duration = Duration::from_micros(100);

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    domain: u8,
    seconds: u64,
    nanos: u32,
    uncertainty_ns: u32,
}

struct Anchor {
    instant: Instant,
    ticks: Duration,
    domain: u8,
    uncertainty_ns: u32,
}

impl Anchor {
    fn sample() -> Result<Self, &'static str> {
        for _ in 0..3 {
            match Self::sample_once() {
                Err("editor clock sample was preempted") => continue,
                result => return result,
            }
        }
        Err("editor clock sample was preempted")
    }

    fn sample_once() -> Result<Self, &'static str> {
        let before = Instant::now();
        let stamp = goal_clock::checkpoint(&goal_clock::Instant::now())?;
        let after = Instant::now();
        let span = after
            .checked_duration_since(before)
            .ok_or("editor clock moved backwards")?;
        if span > MAX_SAMPLE {
            return Err("editor clock sample was preempted");
        }
        let (domain, seconds, nanos) = stamp.parts();
        let half = span / 2;
        Ok(Self {
            instant: before
                .checked_add(half)
                .ok_or("editor clock anchor overflow")?,
            ticks: Duration::new(seconds, nanos),
            domain,
            uncertainty_ns: u32::try_from(span.as_nanos().div_ceil(2))
                .map_err(|_| "editor clock uncertainty overflow")?,
        })
    }

    fn capture(&self, instant: Instant) -> Result<Snapshot, &'static str> {
        let ticks = if let Some(delta) = instant.checked_duration_since(self.instant) {
            self.ticks.checked_add(delta)
        } else {
            self.ticks.checked_sub(self.instant.duration_since(instant))
        }
        .ok_or("editor instant outside native clock range")?;
        goal_clock::Stamp::from_parts(self.domain, ticks.as_secs(), ticks.subsec_nanos())?;
        Ok(Snapshot {
            domain: self.domain,
            seconds: ticks.as_secs(),
            nanos: ticks.subsec_nanos(),
            uncertainty_ns: self.uncertainty_ns,
        })
    }

    fn restore(&self, snapshot: &Snapshot) -> Result<Instant, &'static str> {
        goal_clock::Stamp::from_parts(snapshot.domain, snapshot.seconds, snapshot.nanos)?;
        if snapshot.domain != self.domain
            || u128::from(snapshot.uncertainty_ns) > (MAX_SAMPLE / 2).as_nanos()
        {
            return Err("incompatible editor monotonic clock");
        }
        let ticks = Duration::new(snapshot.seconds, snapshot.nanos);
        if ticks >= self.ticks {
            self.instant.checked_add(ticks - self.ticks)
        } else {
            self.instant.checked_sub(self.ticks - ticks)
        }
        .ok_or("restored editor instant outside platform range")
    }
}

impl Snapshot {
    pub fn capture(instant: Instant) -> Result<Self, &'static str> {
        Anchor::sample()?.capture(instant)
    }
    pub fn restore(&self) -> Result<Instant, &'static str> {
        Anchor::sample()?.restore(self)
    }
}

pub mod instant {
    use super::*;

    pub fn serialize<S: serde::Serializer>(
        value: &Instant,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        Snapshot::capture(*value)
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Instant, D::Error> {
        Snapshot::deserialize(deserializer)?
            .restore()
            .map_err(serde::de::Error::custom)
    }
}

pub mod optional {
    use super::*;
    pub fn serialize<S: serde::Serializer>(
        value: &Option<Instant>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        value
            .map(Snapshot::capture)
            .transpose()
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Instant>, D::Error> {
        Option::<Snapshot>::deserialize(deserializer)?
            .map(|value| value.restore())
            .transpose()
            .map_err(serde::de::Error::custom)
    }
}

pub mod pending_char {
    use super::*;
    pub fn serialize<S: serde::Serializer>(
        value: &Option<(char, Instant)>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        value
            .map(|(ch, at)| Snapshot::capture(at).map(|at| (ch, at)))
            .transpose()
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<(char, Instant)>, D::Error> {
        Option::<(char, Snapshot)>::deserialize(deserializer)?
            .map(|(ch, at)| at.restore().map(|at| (ch, at)))
            .transpose()
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn past_and_future_instants_keep_their_native_deadlines() {
        let anchor = Anchor {
            instant: Instant::now(),
            ticks: Duration::from_secs(100),
            domain: goal_clock::checkpoint(&goal_clock::Instant::now())
                .unwrap()
                .parts()
                .0,
            uncertainty_ns: 0,
        };
        for delta in [
            Duration::from_nanos(1),
            Duration::from_millis(8),
            Duration::from_secs(2),
        ] {
            for instant in [anchor.instant - delta, anchor.instant + delta] {
                assert_eq!(
                    anchor.restore(&anchor.capture(instant).unwrap()).unwrap(),
                    instant
                );
            }
        }
        let mut invalid = anchor.capture(anchor.instant).unwrap();
        invalid.nanos = 1_000_000_000;
        assert!(anchor.restore(&invalid).is_err());
        invalid.nanos = 0;
        invalid.uncertainty_ns = 100_000;
        assert!(anchor.restore(&invalid).is_err());
        invalid.uncertainty_ns = 0;
        invalid.domain = 0;
        assert!(anchor.restore(&invalid).is_err());
        assert!(
            anchor
                .capture(anchor.instant - Duration::from_secs(101))
                .is_err()
        );
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn editor_deadlines_survive_an_actual_same_pid_exec() {
        use std::os::unix::process::CommandExt;
        const STAGE: &str = "ALAB_EDITOR_CLOCK_EXEC_STAGE";
        const DATA: &str = "ALAB_EDITOR_CLOCK_EXEC_DATA";
        const PID: &str = "ALAB_EDITOR_CLOCK_EXEC_PID";
        const TEST: &str =
            "upgrade_editor_clock::tests::editor_deadlines_survive_an_actual_same_pid_exec";
        const SUCCESS: &str = "ALAB_EDITOR_CLOCK_EXEC_OK";
        match std::env::var(STAGE).ok().as_deref() {
            None => {
                let output = tokio::time::timeout(
                    Duration::from_secs(30),
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
                let now = Instant::now();
                let past = Snapshot::capture(now - Duration::from_secs(2)).unwrap();
                let future = Snapshot::capture(now + Duration::from_secs(120)).unwrap();
                let error = std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", TEST, "--nocapture"])
                    .env(STAGE, "destination")
                    .env(PID, std::process::id().to_string())
                    .env(DATA, serde_json::to_string(&(past, future)).unwrap())
                    .exec();
                panic!("editor clock test exec failed: {error}");
            }
            Some("destination") => {
                assert_eq!(std::env::var(PID).unwrap(), std::process::id().to_string());
                let (past, future): (Snapshot, Snapshot) =
                    serde_json::from_str(&std::env::var(DATA).unwrap()).unwrap();
                let past_instant = past.restore().unwrap();
                let future_instant = future.restore().unwrap();
                assert!(past_instant.elapsed() >= Duration::from_secs(2));
                assert!(future_instant.duration_since(Instant::now()) < Duration::from_secs(120));
                let restored = Snapshot::capture(past_instant).unwrap();
                let before = Duration::new(past.seconds, past.nanos);
                let after = Duration::new(restored.seconds, restored.nanos);
                assert!(before.abs_diff(after) <= MAX_SAMPLE);
                println!("{SUCCESS}");
            }
            _ => panic!("unexpected editor clock test stage"),
        }
    }
}
