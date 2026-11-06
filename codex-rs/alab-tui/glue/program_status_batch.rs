// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The status cache owns admission to the writer, including a paired turn edge. Compiled
//! inside `terminal_program_status` so normal reports, clears and edges share the same lock.

use std::io;
use std::io::Write;

use crossterm::Command;
use ratatui::crossterm::execute;

use super::CachedStatus;
use super::SessionStatusState;

pub(super) fn emit<C: Command, W: Write>(
    state: &SessionStatusState,
    status: CachedStatus,
    detail: &str,
    prefix: &str,
    command: C,
    acquire_writer: impl FnOnce() -> W,
) -> io::Result<()> {
    let write = || {
        // Acquiring stdout before the status cache would invert the panic hook's order.
        let mut writer = acquire_writer();
        writer.write_all(prefix.as_bytes())?;
        execute!(writer, command)
    };
    if prefix.is_empty() {
        state.update(status, detail, write)
    } else {
        state.update_with_force(status, detail, true, write)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::MutexGuard;
    use std::sync::TryLockError;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;
    use crate::terminal_program_status::ClearTerminalProgramStatus;
    use crate::terminal_program_status::ProgramStatus;
    use crate::terminal_program_status::SetTerminalProgramStatus;

    const WAIT: Duration = Duration::from_secs(60);
    const IDLE: &str = "\x1b]7501;state=idle:app=codex\x1b\\";
    const WORKING: &str = "\x1b]7501;state=working:app=codex\x1b\\";
    const CLEAR: &str = "\x1b]7501;state=clear\x1b\\";

    #[derive(Default)]
    struct Output {
        bytes: Mutex<Vec<u8>>,
        flushes: AtomicUsize,
    }

    impl Output {
        fn acquire(&self, state: &SessionStatusState) -> MemoryWriter<'_> {
            assert!(matches!(
                state.last_emitted.try_lock(),
                Err(TryLockError::WouldBlock)
            ));
            MemoryWriter {
                bytes: self
                    .bytes
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
                flushes: &self.flushes,
            }
        }
    }

    struct MemoryWriter<'a> {
        bytes: MutexGuard<'a, Vec<u8>>,
        flushes: &'a AtomicUsize,
    }

    impl Write for MemoryWriter<'_> {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            self.flushes.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    fn working() -> SetTerminalProgramStatus<'static> {
        SetTerminalProgramStatus {
            status: ProgramStatus::Working,
            iterm_detail: "",
            emit_iterm_status: false,
        }
    }

    /// The shipping emitter, with two owned memory writers in place of stdout: the edge
    /// pauses holding its writer while a concurrent clear arrives. Both factories require
    /// the actual status cache to be held, and each batch reaches one flush without a clear
    /// between its words. Partial-write and final-flush failures invalidate that same cache
    /// before an ordinary same-word retry.
    #[test]
    fn paired_edges_serialize_with_clears_and_retry_partial_writes() {
        let state = Arc::new(SessionStatusState::new());
        let output = Arc::new(Output::default());
        let (entered, paired_has_writer) = mpsc::sync_channel(1);
        let (release, resume_pair) = mpsc::sync_channel(1);
        let (finished, completed) = mpsc::channel();
        let pair_state = Arc::clone(&state);
        let pair_output = Arc::clone(&output);
        let pair_finished = finished.clone();
        let pair = std::thread::spawn(move || {
            let result = emit(
                &pair_state,
                CachedStatus::Reported(ProgramStatus::Working),
                "",
                IDLE,
                working(),
                || {
                    let writer = pair_output.acquire(&pair_state);
                    entered.send(()).unwrap_or_else(|error| panic!("{error}"));
                    resume_pair
                        .recv_timeout(WAIT)
                        .unwrap_or_else(|error| panic!("{error}"));
                    writer
                },
            );
            pair_finished
                .send(result)
                .unwrap_or_else(|error| panic!("{error}"));
        });
        paired_has_writer
            .recv_timeout(WAIT)
            .unwrap_or_else(|error| panic!("{error}"));
        let (attempting, clear_attempted) = mpsc::sync_channel(1);
        let clear_state = Arc::clone(&state);
        let clear_output = Arc::clone(&output);
        let clear = std::thread::spawn(move || {
            attempting
                .send(())
                .unwrap_or_else(|error| panic!("{error}"));
            let result = emit(
                &clear_state,
                CachedStatus::Cleared,
                "",
                "",
                ClearTerminalProgramStatus {
                    emit_iterm_status: false,
                },
                || clear_output.acquire(&clear_state),
            );
            finished
                .send(result)
                .unwrap_or_else(|error| panic!("{error}"));
        });
        clear_attempted
            .recv_timeout(WAIT)
            .unwrap_or_else(|error| panic!("{error}"));
        release.send(()).unwrap_or_else(|error| panic!("{error}"));
        for _ in 0..2 {
            completed
                .recv_timeout(WAIT)
                .unwrap_or_else(|error| panic!("{error}"))
                .unwrap_or_else(|error| panic!("{error}"));
        }
        pair.join().unwrap_or_else(|error| panic!("{error:?}"));
        clear.join().unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(
            *output
                .bytes
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            format!("{IDLE}{WORKING}{CLEAR}").as_bytes()
        );
        assert_eq!(output.flushes.load(Ordering::SeqCst), 2);

        // A cached final word cannot suppress an edge. Either a partial command write or
        // a failed final flush leaves that cache unknown, so an ordinary report retries it.
        for fail_on_flush in [false, true] {
            let state = SessionStatusState::new();
            let output = Output::default();
            let reported = CachedStatus::Reported(ProgramStatus::Working);
            emit(&state, reported, "", "", working(), || {
                output.acquire(&state)
            })
            .unwrap_or_else(|error| panic!("{error}"));
            let mut failed = FailedWriter {
                bytes: Vec::new(),
                fail_on_flush,
            };
            let error = emit(&state, reported, "", IDLE, working(), || &mut failed).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
            let written = if fail_on_flush {
                format!("{IDLE}{WORKING}")
            } else {
                IDLE.to_owned()
            };
            assert_eq!(failed.bytes, written.as_bytes());
            assert_eq!(
                state
                    .last_emitted
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .status,
                CachedStatus::Invalidated
            );
            emit(&state, reported, "", "", working(), || {
                output.acquire(&state)
            })
            .unwrap_or_else(|error| panic!("{error}"));
            emit(
                &state,
                reported,
                "",
                "",
                working(),
                || -> MemoryWriter<'_> { panic!("an unchanged ordinary report takes no writer") },
            )
            .unwrap_or_else(|error| panic!("{error}"));
            assert_eq!(
                *output
                    .bytes
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
                format!("{WORKING}{WORKING}").as_bytes()
            );
            assert_eq!(output.flushes.load(Ordering::SeqCst), 2);
        }
    }

    struct FailedWriter {
        bytes: Vec<u8>,
        fail_on_flush: bool,
    }

    impl Write for FailedWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.bytes.is_empty() || self.fail_on_flush {
                self.bytes.extend_from_slice(bytes);
                Ok(bytes.len())
            } else {
                Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "owned partial write",
                ))
            }
        }

        fn flush(&mut self) -> io::Result<()> {
            if self.fail_on_flush {
                Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "owned flush failure",
                ))
            } else {
                Ok(())
            }
        }
    }
}
