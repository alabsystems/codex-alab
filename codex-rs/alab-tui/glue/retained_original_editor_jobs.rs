// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Original App-job resource custody, independent of a borrowed actor future.
//! No owner here contains an App/sender/backend back-reference or constructs a
//! native resource from serialized IDs. Only the real callback retires a Job.
use super::Job;
use std::collections::BTreeMap;
use std::io;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
const MAX_JOBS: usize = 4096;
type Records = BTreeMap<u64, Vec<Arc<Job>>>;
fn records() -> &'static Mutex<Records> {
    static RECORDS: OnceLock<Mutex<Records>> = OnceLock::new();
    RECORDS.get_or_init(|| Mutex::new(BTreeMap::new()))
}
pub(super) struct Reservation {
    key: u64,
    installed: bool,
}
impl Reservation {
    /// Reserve BEFORE the original terminal handoff. No actual Child/input/TTY
    /// resource has moved when admission fails. Empty slots own no callback.
    pub(super) fn reserve() -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let mut records = records()
            .lock()
            .map_err(|_| io::Error::other("original editor job registry poisoned"))?;
        if records.len() >= MAX_JOBS {
            return Err(io::Error::other("original editor job admission is full"));
        }
        let key = NEXT
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .map_err(|_| io::Error::other("original editor job identity exhausted"))?;
        if records.contains_key(&key) {
            return Err(io::Error::other("original editor job identity conflict"));
        }
        records.insert(key, Vec::new());
        Ok(Self {
            key,
            installed: false,
        })
    }
    pub(super) fn key(&self) -> u64 {
        self.key
    }
    /// Infallible custody publication after the real handoff. Poison recovery
    /// and a quarantine vector preserve every actual owner even under an
    /// internal duplicate; they never reset the terminal or drop the old Job.
    pub(super) fn install(mut self, actual: Arc<Job>) {
        let mut records = records()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        records.entry(self.key).or_default().push(actual);
        self.installed = true;
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        if self.installed {
            return;
        }
        let mut records = records()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if records.get(&self.key).is_some_and(Vec::is_empty) {
            records.remove(&self.key);
        }
    }
}
/// This private call follows successful SAME Finished.apply_and_retire in the
/// original App callback. Uncertainty/error paths never reach it. Drop actual
/// resources outside metadata lock, after their exact original application.
pub(super) fn retire_after_original_callback(actual: &Arc<Job>) -> io::Result<()> {
    let retired = {
        let mut records = records()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !records
            .get(&actual.registration)
            .is_some_and(|jobs| jobs.len() == 1 && Arc::ptr_eq(&jobs[0], actual))
        {
            return Err(io::Error::other(
                "original editor callback changed exact registered Job",
            ));
        }
        records.remove(&actual.registration)
    };
    drop(retired);
    Ok(())
}

/// Exact local no-child cancellation plus successful original Tui/parser/mode
/// handback precedes this call; it creates no callback or Native receipt.
pub(super) fn retire_after_cancelled_handback(
    actual: &Arc<Job>,
    cancelled:&codex_alab_tui::frontend_control::retained_service::original_editor::CancelledBeforeStart,
) -> io::Result<()> {
    if !cancelled.is_for(&actual.operation) {
        return Err(io::Error::other("cancelled original Job changed operation"));
    }
    retire_after_original_callback(actual)
}
