// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The original terminal handoff, shared by ordinary awaited programs and the
//! retained editor job. No child, SDK role, or Native authority is made here.
use super::*;
use std::sync::atomic::AtomicU8;

const HELD: u8 = 0;
const RESTORING: u8 = 1;
const RESTORED: u8 = 2;
const UNKNOWN: u8 = 3;

pub(super) struct State {
    events: Arc<EventBroker>,
    phase: AtomicU8,
}
/// Retain beside the actual editor producer before its first spawn/claim await.
/// Drop leaves both the original event source and Tui's restriction in place.
pub(crate) struct ExternalHandoff {
    actual: Arc<State>,
    was_alt_screen: bool,
    handoff: TerminalHandoff,
}
impl Tui {
    #[cfg(unix)]
    pub(crate) fn alab_capture_original_editor_stdio(
        &self,
        operation: &Arc<
            codex_alab_tui::frontend_control::retained_service::original_editor::Operation,
        >,
        modes: &codex_alab_tui::terminal_modes::EditorModes,
        fence: &codex_alab_tui::frontend_control::retained_service::original_editor::ExecFence,
        input: &codex_alab_tui::input::OriginalEditorInput,
    ) -> io::Result<Arc<super::OriginalEditorStdio>> {
        terminal_stderr::alab_original_editor::Captured::capture(
            &self._stderr_guard,
            operation,
            modes,
            fence,
            input,
        )
    }
    /// Availability is native Tui custody only. It is neither owned-screen
    /// policy nor a Native/SDK permission, and remains false on interrupted work.
    pub(crate) fn alab_parent_tty_available(&self) -> bool {
        self.alab_external_handoff.is_none()
    }
    pub(crate) fn alab_begin_external_handoff(
        &mut self,
        handoff: TerminalHandoff,
    ) -> io::Result<ExternalHandoff> {
        if self.alab_external_handoff.is_some() {
            return Err(io::Error::other("actual terminal handoff already owned"));
        }
        let actual = Arc::new(State {
            events: self.event_broker.clone(),
            phase: AtomicU8::new(HELD),
        });
        self.alab_external_handoff = Some(actual.clone());
        // Pause crossterm events to avoid stdin conflicts with external program `f`.
        self.pause_events();

        // Restore both screens' input modes before giving the terminal to an editor. Repaint the
        // last frame on the alternate screen so separate-window editors leave Codex visible.
        let was_alt_screen = self.is_alt_screen_active();
        if let Err(err) = crate::retained_tui_images::clear(self) {
            // ALAB(retained-ui): keep actual file owners through failed original clear.
            tracing::warn!(%err, "failed to clear images before terminal handoff");
        }
        let visible_frame = (was_alt_screen && matches!(handoff, TerminalHandoff::KeepScreen))
            .then(|| self.terminal.previous_buffer().clone());
        let restore_result = if let Some(visible_frame) = visible_frame {
            stdout()
                .sync_update(|_| {
                    let leave_result = self.leave_alt_screen_for_handoff();
                    let main_result = restore_keep_raw(TerminalHandoff::Restore);
                    let enter_result = self.enter_alt_screen();
                    if !self.is_alt_screen_active() {
                        return leave_result.and(main_result).and(enter_result);
                    }
                    let draw_result = if enter_result.is_ok() {
                        self.terminal.draw(|frame| {
                            let area = frame.area().intersection(visible_frame.area);
                            for y in area.top()..area.bottom() {
                                for x in area.left()..area.right() {
                                    frame.buffer_mut()[(x, y)] = visible_frame[(x, y)].clone();
                                }
                            }
                        })
                    } else {
                        Ok(())
                    };
                    let input_result = restore_keep_raw(TerminalHandoff::KeepScreen);
                    leave_result
                        .and(main_result)
                        .and(enter_result)
                        .and(draw_result)
                        .and(input_result)
                })
                .and_then(std::convert::identity)
        } else {
            if was_alt_screen {
                let _ = self.leave_alt_screen_for_handoff();
            }
            restore_keep_raw(TerminalHandoff::Restore)
        };
        if let Err(err) = restore_result {
            tracing::warn!("failed to restore terminal modes before external program: {err}");
        }
        if let Err(err) = terminal_stderr::pause() {
            tracing::warn!("failed to restore terminal stderr before external program: {err}");
        }
        if let Err(err) = crate::terminal_program_status::clear_terminal_program_status() {
            tracing::debug!(error = %err, "failed to clear terminal program status before external program");
        }

        Ok(ExternalHandoff {
            actual,
            was_alt_screen,
            handoff,
        })
    }
    fn alab_finish_external_handoff(
        &mut self,
        owner: &mut ExternalHandoff,
        before_resume: impl FnOnce() -> io::Result<()>,
    ) -> io::Result<()> {
        if !self
            .alab_external_handoff
            .as_ref()
            .is_some_and(|actual| Arc::ptr_eq(actual, &owner.actual))
            || !Arc::ptr_eq(&self.event_broker, &owner.actual.events)
        {
            return Err(io::Error::other(
                "terminal handoff belongs to another actual Tui",
            ));
        }
        owner
            .actual
            .phase
            .compare_exchange(HELD, RESTORING, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| io::Error::other("terminal handback already started or uncertain"))?;
        let mut attempt = Handback {
            actual: owner.actual.clone(),
            completed: false,
        };
        let was_alt_screen = owner.was_alt_screen;
        let handoff = owner.handoff;

        if was_alt_screen && matches!(handoff, TerminalHandoff::KeepScreen) {
            // The editor may already have left the alternate screen. Return to the main screen
            // before reapplying its keyboard mode and entering the alternate screen again.
            let _ = self.leave_alt_screen_for_handoff();
        }

        if let Err(err) = terminal_stderr::resume() {
            tracing::warn!("failed to suppress terminal stderr after external program: {err}");
        }
        if let Err(err) = set_modes() {
            tracing::warn!("failed to re-enable terminal modes after external program: {err}");
        }
        self.terminal.invalidate_cursor_state();
        // After the external program `f` finishes, reset terminal state and flush any buffered keypresses.
        flush_terminal_input_buffer();

        if was_alt_screen {
            let _ = self.enter_alt_screen();
        }

        crate::terminal_program_status::invalidate_terminal_program_status();
        before_resume()?;
        self.resume_events();
        self.schedule_screen_size_recheck(Duration::ZERO);
        owner.actual.phase.store(RESTORED, Ordering::Release);
        self.alab_external_handoff = None;
        attempt.completed = true;
        Ok(())
    }
}
impl ExternalHandoff {
    pub(crate) fn is_for(&self, tui: &Tui) -> io::Result<()> {
        if self.actual.phase.load(Ordering::Acquire) != HELD
            || !tui
                .alab_external_handoff
                .as_ref()
                .is_some_and(|actual| Arc::ptr_eq(actual, &self.actual))
            || !Arc::ptr_eq(&tui.event_broker, &self.actual.events)
        {
            return Err(io::Error::other(
                "original editor handoff changed actual Tui",
            ));
        }
        Ok(())
    }
    /// The actual retained caller invokes this only after retaining the SAME
    /// EditorFinished/handback resource. Its synchronous callback releases the
    /// original parser fence after original output/modes/TCIFLUSH and before the
    /// original event source restarts. Any failure retains the restriction.
    pub(crate) fn finish_with(
        &mut self,
        tui: &mut Tui,
        before_resume: impl FnOnce() -> io::Result<()>,
    ) -> io::Result<()> {
        tui.alab_finish_external_handoff(self, before_resume)
    }
}
struct Handback {
    actual: Arc<State>,
    completed: bool,
}
impl Drop for Handback {
    fn drop(&mut self) {
        if !self.completed {
            self.actual.phase.store(UNKNOWN, Ordering::Release);
        }
    }
}
impl Drop for ExternalHandoff {
    fn drop(&mut self) {
        let phase = self.actual.phase.load(Ordering::Acquire);
        if phase != RESTORED {
            self.actual.phase.store(UNKNOWN, Ordering::Release);
        }
    }
}

#[cfg(all(test, unix))]
#[path = "retained_tui_external_handoff_tests.rs"]
mod tests;
