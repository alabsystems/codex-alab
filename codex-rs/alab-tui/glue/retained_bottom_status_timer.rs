// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Native elapsed/paused/display clock without restarting accounting on restore.
use super::*;
use codex_alab_tui::upgrade_editor_clock::Snapshot;
use serde::Deserialize;
use serde::Serialize;
use std::io;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Data {
    #[serde(deserialize_with = "required")]
    display: Option<Snapshot>,
    elapsed: Duration,
    resumed: Snapshot,
    paused: bool,
}
impl Data {
    pub(crate) fn capture(actual: &StatusTimer) -> io::Result<Self> {
        Ok(Self {
            display: actual
                .display_started_at
                .map(Snapshot::capture)
                .transpose()
                .map_err(io::Error::other)?,
            elapsed: actual.elapsed_running,
            resumed: Snapshot::capture(actual.last_resume_at).map_err(io::Error::other)?,
            paused: actual.is_paused,
        })
    }
    pub(crate) fn restore(self) -> io::Result<StatusTimer> {
        Ok(StatusTimer {
            display_started_at: self
                .display
                .map(|value| value.restore())
                .transpose()
                .map_err(io::Error::other)?,
            elapsed_running: self.elapsed,
            last_resume_at: self.resumed.restore().map_err(io::Error::other)?,
            is_paused: self.paused,
        })
    }
}
fn required<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Option<T>, D::Error> {
    Option::<T>::deserialize(d)
}
#[cfg(test)]
mod controls {
    use super::*;
    #[test]
    fn original_pause_resume_and_distinct_phase_clock_keep_accounted_elapsed() -> io::Result<()> {
        let now = Instant::now();
        let mut original = StatusTimer::default();
        original.last_resume_at = now - Duration::from_secs(4);
        original.display_started_at = Some(now - Duration::from_secs(1));
        original.pause_at(now);
        let mut restored = Data::capture(&original)?.restore()?;
        assert_eq!(restored.elapsed_at(now), original.elapsed_at(now));
        original.resume_at(now + Duration::from_secs(1));
        restored.resume_at(now + Duration::from_secs(1));
        assert_eq!(
            restored.elapsed_at(now + Duration::from_secs(3)),
            original.elapsed_at(now + Duration::from_secs(3))
        );
        assert!(restored.display_started_at.is_some());
        original.display_started_at = None;
        assert!(
            Data::capture(&original)?
                .restore()?
                .display_started_at
                .is_none()
        );
        Ok(())
    }
}
