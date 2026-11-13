// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The ALab binary uses one input owner through mode changes and TUI pauses.
//! Other library consumers keep their existing crossterm input implementation.
use codex_alab_input::Broker;
pub use codex_alab_input::Fence as UpgradeInput;
use codex_alab_input::InternalEvent;
use codex_alab_input::Subscription;
use crossterm::event::Event;
use std::io::Write;
use std::io::{self};
use std::sync::Arc;
use std::sync::OnceLock;
use std::task::Context;
use std::task::Poll;
use std::task::Wake;
use std::task::Waker;
use std::time::Duration;
use std::time::Instant;
static INPUT: OnceLock<Arc<Broker>> = OnceLock::new();
fn broker() -> &'static Arc<Broker> {
    INPUT.get_or_init(Broker::terminal)
}
pub fn owned() -> bool {
    codex_alab_core::is_alab_binary()
}
/// Same-process state transport must be authenticated independently. Restore
/// before any startup probe or input consumer; snapshot data grants no SDK role.
pub fn restore_after_exec(modes: &crate::terminal_modes::Snapshot, bytes: &[u8]) -> io::Result<()> {
    if !owned() {
        return Err(io::Error::other(
            "input restoration requires the ALab binary",
        ));
    }
    broker().restore_with(bytes, || crate::terminal_modes::adopt(modes))
}
pub struct InputStream {
    source: io::Result<Subscription>,
}
impl Default for InputStream {
    fn default() -> Self {
        Self {
            source: crossterm::terminal::is_raw_mode_enabled()
                .and_then(|raw| broker().subscribe(raw)),
        }
    }
}
impl InputStream {
    /// Fence the exact subscription retained by the TUI's current event source.
    /// Its caller must first check higher-level filter state under the TUI's
    /// owner lock; this does not inspect or serialize that separate state.
    pub fn prepare_upgrade(&self) -> io::Result<codex_alab_input::Fence> {
        match &self.source {
            Ok(source) => source.prepare_upgrade(),
            Err(error) => Err(io::Error::new(error.kind(), error.to_string())),
        }
    }
    pub fn poll_next(&self, cx: &mut Context<'_>) -> Poll<Option<io::Result<Event>>> {
        match &self.source {
            Ok(source) => source
                .poll_where(cx, |event| matches!(event, InternalEvent::Event(_)))
                .map(|event| {
                    event.map(|event| {
                        event.and_then(|event| match event {
                            InternalEvent::Event(event) => Ok(event),
                            _ => Err(io::Error::other("input filter returned a query response")),
                        })
                    })
                }),
            Err(error) => Poll::Ready(Some(Err(io::Error::new(error.kind(), error.to_string())))),
        }
    }
}
pub fn buffer_input(bytes: &[u8]) -> io::Result<()> {
    if !owned() {
        return crossterm::event::buffer_input(bytes);
    }
    broker().buffer_input(bytes, crossterm::terminal::is_raw_mode_enabled()?)
}
pub fn discard_buffered_input() -> io::Result<crossterm::event::InputDiscardStatus> {
    if !owned() {
        return crossterm::event::discard_buffered_input();
    }
    broker().discard_buffered_input()
}
struct ThreadWake(std::thread::Thread);
impl Wake for ThreadWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}
fn wait<T>(
    timeout: Option<Duration>,
    mut poll: impl FnMut(&mut Context<'_>) -> Poll<Option<io::Result<T>>>,
) -> io::Result<Option<T>> {
    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let deadline = timeout.and_then(|timeout| Instant::now().checked_add(timeout));
    loop {
        match poll(&mut context) {
            Poll::Ready(Some(result)) => return result.map(Some),
            Poll::Ready(None) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "terminal input ended",
                ));
            }
            Poll::Pending => {}
        }
        if let Some(deadline) = deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(None);
            }
            std::thread::park_timeout(remaining);
        } else {
            std::thread::park();
        }
    }
}
pub fn poll(timeout: Duration) -> io::Result<bool> {
    if !owned() {
        return crossterm::event::poll(timeout);
    }
    let source = broker().subscribe(crossterm::terminal::is_raw_mode_enabled()?)?;
    Ok(wait(Some(timeout), |cx| {
        source.poll_available(cx, |event| matches!(event, InternalEvent::Event(_)))
    })?
    .is_some())
}
pub fn read() -> io::Result<Event> {
    if !owned() {
        return crossterm::event::read();
    }
    let source = InputStream::default();
    wait(None, |cx| source.poll_next(cx))?
        .ok_or_else(|| io::Error::other("input read returned no event"))
}
pub fn cursor_position(
    fallback: impl FnOnce() -> io::Result<ratatui::layout::Position>,
) -> io::Result<ratatui::layout::Position> {
    if !owned() {
        return fallback();
    }
    let source = broker().subscribe(crossterm::terminal::is_raw_mode_enabled()?)?;
    // Remove old queued cursor replies while preserving the person's input.
    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    while matches!(
        source.poll_where(&mut context, |event| matches!(
            event,
            InternalEvent::CursorPosition(..)
        )),
        Poll::Ready(Some(Ok(_)))
    ) {}
    let mut output = io::stdout().lock();
    output.write_all(b"\x1b[6n")?;
    output.flush()?;
    drop(output);
    match wait(Some(Duration::from_millis(250)), |cx| {
        source.poll_where(cx, |event| {
            matches!(event, InternalEvent::CursorPosition(..))
        })
    })? {
        Some(InternalEvent::CursorPosition(x, y)) => Ok(ratatui::layout::Position { x, y }),
        _ => Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "terminal cursor response timed out",
        )),
    }
}
/// Preserve the existing protected-screen policy using this same input owner.
/// The caller supplies its platform's non-consuming OS-queue readability check.
pub fn discard_pending(readable: impl Fn() -> io::Result<bool>) -> io::Result<()> {
    discard_buffered_input()?;
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "timed out discarding buffered terminal input",
            ));
        }
        if poll(remaining.min(Duration::from_millis(10)))? {
            read()?;
            continue;
        }
        if discard_buffered_input()? != crossterm::event::InputDiscardStatus::Complete {
            continue;
        }
        match readable() {
            Ok(true) => continue,
            Ok(false) => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::fd::AsRawFd;
    use std::os::fd::FromRawFd;
    #[test]
    fn native_frontend_shares_replay_quarantine_streams_and_queries() {
        const CASE: &str = "ALAB_INPUT_FRONTEND_CASE";
        const TEST: &str =
            "input::tests::native_frontend_shares_replay_quarantine_streams_and_queries";
        const SUCCESS: &str = "ALAB_INPUT_FRONTEND_OK";
        if std::env::var_os(CASE).is_none() {
            let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", TEST, "--nocapture"])
                .env(CASE, "native")
                .stdout(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(20);
            loop {
                if let Some(status) = child.try_wait().unwrap() {
                    use std::io::Read;
                    let mut output = String::new();
                    child
                        .stdout
                        .take()
                        .unwrap()
                        .read_to_string(&mut output)
                        .unwrap();
                    assert!(
                        status.success() && output.contains(SUCCESS),
                        "native frontend did not finish: {output}"
                    );
                    return;
                }
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!("native frontend timed out");
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        let (mut master, mut slave) = (-1, -1);
        assert_eq!(
            unsafe {
                libc::openpty(
                    &raw mut master,
                    &raw mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            },
            0
        );
        let (mut master, slave) = unsafe {
            (
                std::fs::File::from_raw_fd(master),
                std::fs::File::from_raw_fd(slave),
            )
        };
        let mut settings: libc::termios = unsafe { std::mem::zeroed() };
        assert_eq!(
            unsafe { libc::tcgetattr(slave.as_raw_fd(), &raw mut settings) },
            0
        );
        unsafe {
            libc::cfmakeraw(&raw mut settings);
        }
        assert_eq!(
            unsafe { libc::tcsetattr(slave.as_raw_fd(), libc::TCSANOW, &raw const settings) },
            0
        );
        assert_eq!(
            unsafe { libc::fcntl(slave.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) },
            0
        );
        let input = Arc::new(slave);
        assert!(INPUT.set(Broker::new(move || input.try_clone())).is_ok());
        codex_alab_core::activate();
        buffer_input(b"a").unwrap();
        assert!(poll(Duration::ZERO).unwrap());
        assert!(
            matches!(read().unwrap(), Event::Key(key) if key.code == crossterm::event::KeyCode::Char('a'))
        );
        let stream = InputStream::default();
        let mut reply = master.try_clone().unwrap();
        let peer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            reply.write_all(b"x\x1b[3;4Ry").unwrap();
        });
        assert_eq!(
            cursor_position(|| panic!("native query called legacy reader")).unwrap(),
            ratatui::layout::Position { x: 3, y: 2 }
        );
        peer.join().unwrap();
        for wanted in ['x', 'y'] {
            assert!(
                matches!(wait(Some(Duration::from_secs(2)), |cx| stream.poll_next(cx)).unwrap(), Some(Event::Key(key)) if key.code == crossterm::event::KeyCode::Char(wanted))
            );
        }
        drop(stream);
        buffer_input(b"\x1b]10;unfinished").unwrap();
        let mut reply = master.try_clone().unwrap();
        let peer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            reply.write_all(b"rest\x1b\\typeahead").unwrap();
        });
        discard_pending(|| Ok(false)).unwrap();
        peer.join().unwrap();
        assert!(!poll(Duration::ZERO).unwrap());
        master.write_all(b"z").unwrap();
        assert!(
            matches!(read().unwrap(), Event::Key(key) if key.code == crossterm::event::KeyCode::Char('z'))
        );
        println!("{SUCCESS}");
    }
}
