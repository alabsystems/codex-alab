// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Same physical held key and monotonic deadline; no fresh activation is produced.
use super::*;
use crate::key_hint::alab_retained_keys::Key;
use codex_alab_tui::upgrade_editor_clock::Snapshot;
use serde::Deserialize;
use serde::Serialize;
use std::io;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Data {
    key: Key,
    until: Snapshot,
    legacy: bool,
    active: bool,
}
impl Data {
    pub(crate) fn capture(actual: ActivationKeyGuard) -> io::Result<Self> {
        Ok(Self {
            key: Key::capture(actual.key),
            until: Snapshot::capture(actual.block_until).map_err(io::Error::other)?,
            legacy: actual.legacy_repeat_fallback,
            active: actual.active,
        })
    }
    pub(crate) fn restore(self) -> io::Result<ActivationKeyGuard> {
        Ok(ActivationKeyGuard {
            key: self.key.restore()?,
            block_until: self.until.restore().map_err(io::Error::other)?,
            legacy_repeat_fallback: self.legacy,
            active: self.active,
        })
    }
}
#[cfg(test)]
mod controls {
    use super::*;
    use crossterm::event::KeyCode;
    use crossterm::event::KeyModifiers;
    #[test]
    fn actual_transition_repeat_and_release_continue_under_original_deadline() -> io::Result<()> {
        let now = Instant::now();
        let key = KeyBinding::new(KeyCode::BackTab, KeyModifiers::SHIFT);
        for legacy in [false, true] {
            let mut original = ActivationKeyGuard::new_at(key, now, !legacy);
            let mut restored = Data::capture(original)?.restore()?;
            for (kind, at) in [
                (KeyEventKind::Repeat, now + Duration::from_millis(100)),
                (KeyEventKind::Release, now + Duration::from_millis(200)),
                (KeyEventKind::Press, now + Duration::from_secs(2)),
            ] {
                assert_eq!(
                    original.consume_at(key, kind, at),
                    restored.consume_at(key, kind, at)
                );
                assert_eq!(original.is_active(), restored.is_active());
            }
        }
        Ok(())
    }
}
