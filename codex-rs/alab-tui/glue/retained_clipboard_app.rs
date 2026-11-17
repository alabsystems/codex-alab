// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Operation owner for the original C1 clipboard UI path. It never creates Draw,
//! Paste or a completion. The actual actor supplies its original broker event.
use super::*;
use crate::clipboard_copy::worker::alab_retained_clipboard::ClipboardData;
use crate::clipboard_copy::worker::alab_retained_clipboard::JoinObservation;
use crate::clipboard_copy::worker::alab_retained_clipboard::JoinPreparation;
use crate::clipboard_copy::worker::alab_retained_clipboard::JoinedWorker;
use crate::clipboard_copy::worker::alab_retained_clipboard::ScopeCapture;
use crate::history_cell::alab_retained_cells::Capture as Cells;
use crate::terminal_hyperlinks::alab_retained_text as text;
use std::future::Future;
use std::io;
use std::pin::Pin;
/// Borrows the SAME real App and Tui for the complete original event operation.
/// This is not a separately cancellable observer or terminal-ownership grant.
pub(in crate::app) struct C1Clipboard<'a> {
    app: &'a mut App,
    tui: &'a mut tui::Tui,
    prepared: JoinPreparation,
    joined: Option<JoinedWorker>,
}
pub(in crate::app) struct EventRefused {
    pub(in crate::app) original: TuiEvent,
    pub(in crate::app) error: io::Error,
}
impl<'a> C1Clipboard<'a> {
    /// Store this owner before polling original UI work or waiting for the join.
    pub(in crate::app) fn begin(app: &'a mut App, tui: &'a mut tui::Tui) -> io::Result<Self> {
        let frame = tui.frame_requester();
        let prepared = tui
            .clipboard
            .alab_prepare_join(frame)
            .map_err(io::Error::other)?;
        Ok(Self {
            app,
            tui,
            prepared,
            joined: None,
        })
    }
    /// The actual C1 main actor must hold its original parent-TTY/input owner.
    /// In particular is_owned_screen alone includes editor yield and is not a
    /// substitute for real editor terminal continuation. Non-Draw input stays
    /// owned by EventRefused for the outer original FIFO, never fabricated/replayed.
    pub(in crate::app) fn drive_original_draw<'s>(
        &'s mut self,
        server: &'s mut AppServerSession,
        event: TuiEvent,
    ) -> std::result::Result<Pin<Box<dyn Future<Output = Result<AppRunControl>> + 's>>, EventRefused>
    {
        let guard = match self
            .tui
            .clipboard
            .alab_begin_original_draw(&self.prepared, &event)
        {
            Ok(guard) => guard,
            Err(error) => {
                return Err(EventRefused {
                    original: event,
                    error,
                });
            }
        };
        Ok(Box::pin(async move {
            // This FULL original future polls/delivers actual terminal_sequence,
            // all selection/message owners and actual right-click Paste before
            // it returns. Keep its maintenance/order/target checks unchanged.
            let result = Box::pin(self.app.handle_tui_event(self.tui, server, event)).await;
            if result.is_ok() {
                guard.complete().map_err(color_eyre::eyre::Report::new)?;
            }
            result
        }))
    }
    pub(in crate::app) fn observe(&mut self) -> io::Result<JoinObservation> {
        let state = self
            .tui
            .clipboard
            .alab_observe_join(&self.prepared)
            .map_err(io::Error::other)?;
        if let JoinObservation::Joined(actual) = &state {
            self.joined = Some(actual.clone());
        }
        Ok(state)
    }
    pub(in crate::app) fn close_if_original_results_delivered(&mut self) -> io::Result<bool> {
        self.tui
            .clipboard
            .alab_close_join(&self.prepared)
            .map_err(io::Error::other)
    }
    /// No event is needed when there were no native pending results. This can
    /// capture a joined worker without drawing over an active editor.
    pub(in crate::app) fn capture(
        &self,
        cells: &mut Cells,
        strings: &mut text::Capture,
        scopes: &ScopeCapture,
    ) -> io::Result<ClipboardData> {
        let joined = self
            .joined
            .as_ref()
            .ok_or_else(|| io::Error::other("actual original clipboard join not observed"))?;
        ClipboardData::capture(self.tui, joined, cells, strings, scopes)
    }
    pub(in crate::app) fn original(&self) -> (&App, &tui::Tui) {
        (self.app, self.tui)
    }
    pub(in crate::app) fn cancel_before_close(&mut self) -> io::Result<()> {
        self.tui
            .clipboard
            .alab_cancel_join(&self.prepared)
            .map_err(io::Error::other)
    }
    pub(in crate::app) fn resume_after_join(&mut self) -> io::Result<()> {
        let joined = self
            .joined
            .as_ref()
            .ok_or_else(|| io::Error::other("actual original clipboard join not observed"))?;
        self.tui
            .clipboard
            .alab_resume_after_join(joined)
            .map_err(io::Error::other)
    }
}

#[cfg(test)]
#[path = "retained_clipboard_app_tests.rs"]
mod tests;
