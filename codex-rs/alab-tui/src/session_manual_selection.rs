// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Serialize the picker's local resume custody with every background and final record write.

use super::*;
use crate::record::ManualAccountSelection;
use codex_alab_aterm::account_move::Selection;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ManualSelectionState {
    None,
    Sending,
    Reconcile(Selection),
}

/// A live caller owns the native dispatch. If its task disappears, a subsequent read of
/// /accounts can cancel the uncertain attempt; dropping this guard never restores the line.
pub struct ManualAttempt<'a> {
    writer: &'a Writer,
    selection: Selection,
}

impl Drop for ManualAttempt<'_> {
    fn drop(&mut self) {
        let mut state = self.writer.lock();
        if state
            .manual_selection
            .as_ref()
            .is_some_and(|held| held.request == self.selection.request)
        {
            state.manual_sending = false;
        }
    }
}

impl Writer {
    pub fn manual_selection_state(&self) -> ManualSelectionState {
        // A same-process exec may still be restoring its previous record.
        if !self.lock().prepared {
            self.prepare();
        }
        let state = self.lock();
        match &state.manual_selection {
            None => ManualSelectionState::None,
            Some(_) if state.manual_sending => ManualSelectionState::Sending,
            Some(held) => ManualSelectionState::Reconcile(held.selection()),
        }
    }

    /// Freeze this current, materialized thread's exact validated line at rest. Return
    /// success only after the ordinary resumeArgv is durably null.
    pub fn begin_manual_selection(
        &self,
        thread: &str,
        selection: &Selection,
    ) -> Option<ManualAttempt<'_>> {
        codex_alab_core::with_model_source_lock(|| {
            self.prepare_with_source_lock();
            let mut state = self.lock();
            if state.finalized
                || state.start_failed
                || state.held_status.is_some()
                || state.manual_selection.is_some()
            {
                return None;
            }
            let mut record = self.build(&mut state, now_ms());
            if record.status != Status::Idle
                || !record.draft_empty
                || record.exit.is_some()
                || record.identity.is_none()
                || record.proc_start.is_none()
                || record.aterm_session.is_none()
                || record.session_id.as_deref() != Some(thread)
                || record.displayed_thread_id.as_deref() != Some(thread)
            {
                return None;
            }
            let Some(argv) = record.resume_argv.take() else {
                return None;
            };
            if argv.last().map(String::as_str) != Some(thread) {
                return None;
            }
            let held = ManualAccountSelection {
                revision: selection.revision.clone(),
                selection_ref: selection.selection_ref.clone(),
                request: selection.request.clone(),
                resume_argv: argv,
            };
            if !held.valid() {
                return None;
            }
            // Invalidate a live upgrade that already inspected the ordinary resume line.
            // A later scan must read this writer after the frozen record has been committed.
            self.shared.invalidate_upgrade();
            if self.shared.exec_pending() {
                return None;
            }
            record.manual_account_selection = Some(held.clone());
            if record::write_atomic(&self.dir, &record).is_err() {
                // An error after rename can leave the new bytes visible. No restart was sent.
                state.last = None;
                self.shared.mark_changed();
                return None;
            }
            state.manual_selection = Some(held);
            state.manual_sending = true;
            state.last = Some(record);
            self.writes.fetch_add(1, Ordering::SeqCst);
            Some(ManualAttempt {
                writer: self,
                selection: selection.clone(),
            })
        })
    }

    /// Stop marking the call in flight. A carried restart or unread reply stays frozen
    /// until the native host cancels exactly this request. A subsequent refresh may retry.
    pub fn release_manual_selection(&self, selection: &Selection) -> bool {
        codex_alab_core::with_model_source_lock(|| {
            let mut state = self.lock();
            if state.finalized
                || state
                    .manual_selection
                    .as_ref()
                    .map(ManualAccountSelection::selection)
                    .as_ref()
                    != Some(selection)
            {
                return false;
            }
            state.manual_sending = false;
            let held = state.manual_selection.take();
            let current = self.build(&mut state, now_ms());
            if record::write_atomic(&self.dir, &current).is_err() {
                state.manual_selection = held;
                state.last = None;
                self.shared.mark_changed();
                return false;
            }
            state.last = Some(current);
            self.writes.fetch_add(1, Ordering::SeqCst);
            drop(state);
            self.shared.note_resume_line();
            true
        })
    }
}
