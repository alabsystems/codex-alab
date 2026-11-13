// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use crate::InternalEvent;
use crate::Parser;
use crate::invalid;
use std::fs::File;
use std::io;
use std::io::IsTerminal;
use std::io::Read;
use std::io::Write;
use std::os::fd::AsFd;
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::task::Context;
use std::task::Poll;
use std::task::Waker;
use std::thread::JoinHandle;

/// Match crossterm's input selection while obtaining an independent open-file
/// description, so nonblocking reads never change the caller's stdin flags.
pub fn open_terminal_input() -> io::Result<File> {
    let stdin = std::io::stdin();
    let original = if stdin.is_terminal() {
        Some(File::from(stdin.as_fd().try_clone_to_owned()?))
    } else {
        None
    };
    let mut name = vec![0u8; libc::PATH_MAX as usize];
    let path = if let Some(original) = &original {
        // SAFETY: the held original descriptor and bounded writable name buffer
        // remain valid. ttyname_r returns an errno value rather than setting it.
        let code =
            unsafe { libc::ttyname_r(original.as_raw_fd(), name.as_mut_ptr().cast(), name.len()) };
        if code != 0 {
            return Err(io::Error::from_raw_os_error(code));
        }
        let name = std::ffi::CStr::from_bytes_until_nul(&name).map_err(io::Error::other)?;
        std::path::Path::new(std::ffi::OsStr::from_bytes(name.to_bytes()))
    } else {
        std::path::Path::new("/dev/tty")
    };
    let input = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)?;
    if let Some(original) = original {
        let held = input.metadata()?;
        let expected = original.metadata()?;
        if (held.dev(), held.ino(), held.rdev())
            != (expected.dev(), expected.ino(), expected.rdev())
        {
            return Err(invalid("terminal input identity changed during open"));
        }
    }
    Ok(input)
}

struct State {
    parser: Parser,
    waker: Option<Waker>,
    terminal: bool,
    error: Option<io::Error>,
}
struct Shared {
    state: Mutex<State>,
    stop: AtomicBool,
}
/// A single original terminal reader. Its thread never outlives this owner.
/// Freeze joins the reader before returning the original descriptor and parser.
pub struct Driver {
    shared: Arc<Shared>,
    wake: UnixStream,
    consumed: UnixStream,
    worker: Option<JoinHandle<io::Result<File>>>,
}
pub struct Frozen {
    pub input: File,
    pub parser: Parser,
}
impl Driver {
    pub fn open_terminal(parser: Parser, raw_mode: bool) -> io::Result<Self> {
        let input = open_terminal_input()?;
        Self::start(input, parser, raw_mode)
    }
    /// Requires an independently owned nonblocking input description. This does
    /// not alter stdin's flags or borrow a descriptor number across exec.
    pub fn start(input: File, mut parser: Parser, raw_mode: bool) -> io::Result<Self> {
        Self::resume(input, &mut parser, raw_mode)
    }
    /// A failed constructor leaves the caller's accepted input untouched. The
    /// caller retains its original input description while lending a duplicate.
    pub fn resume(input: File, parser: &mut Parser, raw_mode: bool) -> io::Result<Self> {
        // SAFETY: fcntl only inspects the descriptor, retaining File ownership.
        let flags = unsafe { libc::fcntl(input.as_raw_fd(), libc::F_GETFL) };
        if flags < 0 {
            return Err(io::Error::last_os_error());
        }
        if flags & libc::O_NONBLOCK == 0 {
            return Err(invalid("input driver requires a nonblocking descriptor"));
        }
        // SAFETY: this owned descriptor's inheritance flag is private to it;
        // unlike O_NONBLOCK, FD_CLOEXEC does not alter a shared description.
        if unsafe { libc::fcntl(input.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
            return Err(io::Error::last_os_error());
        }
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                parser: Parser::default(),
                waker: None,
                terminal: false,
                error: None,
            }),
            stop: AtomicBool::new(false),
        });
        let (wake, receiver) = UnixStream::pair()?;
        wake.set_nonblocking(true)?;
        receiver.set_nonblocking(true)?;
        let signal_wake = wake.try_clone()?;
        // Consumer notifications have their own channel: they must not invent
        // resize events or crowd SIGWINCH notifications out of its pipe.
        let (consumed, consumption) = UnixStream::pair()?;
        consumed.set_nonblocking(true)?;
        consumption.set_nonblocking(true)?;
        let source = shared.clone();
        let (send, receive) = std::sync::mpsc::sync_channel::<Parser>(0);
        let (initialized, initialization) = std::sync::mpsc::sync_channel::<()>(0);
        let worker = std::thread::Builder::new()
            .name("codex-input".into())
            .spawn(move || {
                let initial = receive
                    .recv()
                    .map_err(|_| invalid("input parser was not transferred"))?;
                source
                    .state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .parser = initial;
                initialized
                    .send(())
                    .map_err(|_| invalid("input owner left during initialization"))?;
                let outcome = run(&source, input, receiver, consumption, signal_wake, raw_mode);
                let waker = {
                    let mut state = source
                        .state
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    state.terminal = true;
                    if let Err(error) = &outcome {
                        state.error = Some(io::Error::new(error.kind(), error.to_string()));
                    }
                    state.waker.take()
                };
                if let Some(waker) = waker {
                    waker.wake();
                }
                outcome
            })?;
        if let Err(error) = send.send(std::mem::take(parser)) {
            *parser = error.0;
            let _ = worker.join();
            return Err(invalid("input reader did not accept its parser"));
        }
        if initialization.recv().is_err() {
            let _ = worker.join();
            *parser = std::mem::take(
                &mut shared
                    .state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .parser,
            );
            return Err(invalid("input reader initialization failed"));
        }
        Ok(Self {
            shared,
            wake,
            consumed,
            worker: Some(worker),
        })
    }
    pub fn poll_event(&self, cx: &mut Context<'_>) -> Poll<Option<io::Result<InternalEvent>>> {
        self.poll_where(cx, |_| true)
    }
    pub fn poll_where(
        &self,
        cx: &mut Context<'_>,
        wanted: impl Fn(&InternalEvent) -> bool,
    ) -> Poll<Option<io::Result<InternalEvent>>> {
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let backpressured = state.parser.input_backpressured();
        if let Some(event) = state.parser.pop_where(wanted) {
            drop(state);
            if backpressured {
                let mut wake = &self.consumed;
                loop {
                    match wake.write(&[1]) {
                        Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                        _ => break,
                    }
                }
            }
            return Poll::Ready(Some(Ok(event)));
        }
        if let Some(error) = state.error.take() {
            return Poll::Ready(Some(Err(error)));
        }
        if state.terminal {
            return Poll::Ready(None);
        }
        if state
            .waker
            .as_ref()
            .is_none_or(|old| !old.will_wake(cx.waker()))
        {
            state.waker = Some(cx.waker().clone());
        }
        Poll::Pending
    }
    pub fn poll_available(
        &self,
        cx: &mut Context<'_>,
        wanted: impl Fn(&InternalEvent) -> bool,
    ) -> Poll<Option<io::Result<()>>> {
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.parser.has_where(wanted) {
            return Poll::Ready(Some(Ok(())));
        }
        if let Some(error) = &state.error {
            return Poll::Ready(Some(Err(io::Error::new(error.kind(), error.to_string()))));
        }
        if state.terminal {
            return Poll::Ready(None);
        }
        state.waker = Some(cx.waker().clone());
        Poll::Pending
    }
    fn join(&mut self) -> io::Result<File> {
        self.shared.stop.store(true, Ordering::Release);
        // A full wake buffer is already readable. This owner's sender remains
        // held through join, and the input description itself is nonblocking.
        loop {
            match self.wake.write(&[1]) {
                Ok(_) => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(_) => {
                    let _ = self.wake.shutdown(std::net::Shutdown::Both);
                    break;
                }
            }
        }
        self.worker
            .take()
            .ok_or_else(|| invalid("input reader was already joined"))?
            .join()
            .map_err(|_| invalid("input reader panicked"))?
    }
    pub fn freeze(mut self) -> io::Result<Frozen> {
        let mut parser = Parser::default();
        let input = self.freeze_into(&mut parser)?;
        Ok(Frozen { input, parser })
    }
    /// Always recover accepted parser data after joining, including when the
    /// terminal read failed. The caller owns its original description separately.
    pub fn freeze_into(&mut self, parser: &mut Parser) -> io::Result<File> {
        let outcome = self.join();
        *parser = std::mem::take(
            &mut self
                .shared
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .parser,
        );
        outcome
    }
}
impl Drop for Driver {
    fn drop(&mut self) {
        if self.worker.is_some() {
            let _ = self.join();
        }
    }
}
fn run(
    shared: &Shared,
    mut input: File,
    mut wake: UnixStream,
    mut consumption: UnixStream,
    signal_wake: UnixStream,
    raw_mode: bool,
) -> io::Result<File> {
    let signal =
        signal_hook::low_level::pipe::register(signal_hook::consts::SIGWINCH, signal_wake)?;
    struct Signal(signal_hook::SigId);
    impl Drop for Signal {
        fn drop(&mut self) {
            signal_hook::low_level::unregister(self.0);
        }
    }
    let _signal = Signal(signal);
    let mut buffer = [0; 1024];
    loop {
        if shared.stop.load(Ordering::Acquire) {
            return Ok(input);
        }
        let state = shared
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let backpressured = state.parser.input_backpressured();
        let remaining = state.parser.escape_remaining()?;
        drop(state);
        let mut fds = [
            libc::pollfd {
                // A negative descriptor also suppresses HUP/ERR; events=0
                // alone would spin on a hung-up terminal while backpressured.
                fd: if backpressured { -1 } else { input.as_raw_fd() },
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: wake.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: consumption.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        let timeout = remaining.map_or(-1, |time| {
            time.as_millis().clamp(1, i32::MAX as u128) as i32
        });
        // SAFETY: all descriptors and initialized pollfd entries remain held.
        let result = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as _, timeout) };
        if shared.stop.load(Ordering::Acquire) {
            return Ok(input);
        }
        if result < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if fds[2].revents & libc::POLLIN != 0 {
            let mut notifications = [0; 64];
            while consumption
                .read(&mut notifications)
                .is_ok_and(|size| size > 0)
            {}
        }
        let mut state = shared
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if shared.stop.load(Ordering::Acquire) {
            return Ok(input);
        }
        if fds[1].revents & libc::POLLIN != 0 {
            // SIGWINCH uses the same nonblocking wake channel as explicit stop.
            let mut signals = [0; 64];
            while wake.read(&mut signals).is_ok_and(|size| size > 0) {}
            let mut size = libc::winsize {
                ws_row: 0,
                ws_col: 0,
                ws_xpixel: 0,
                ws_ypixel: 0,
            };
            // SAFETY: size is writable and matches TIOCGWINSZ's native structure.
            if unsafe { libc::ioctl(input.as_raw_fd(), libc::TIOCGWINSZ, &raw mut size) } == 0 {
                state.parser.resize(size.ws_col, size.ws_row);
            }
        }
        if fds[0].revents & libc::POLLNVAL != 0 {
            return Err(invalid("input descriptor is invalid"));
        }
        if fds[0].revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0 {
            match input.read(&mut buffer) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "terminal input ended",
                    ));
                }
                Ok(size) => state.parser.feed(&buffer[..size], raw_mode)?,
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) => {}
                Err(error) => return Err(error),
            }
        }
        state.parser.flush_escape()?;
        let waker = if state.parser.has_events() {
            state.waker.take()
        } else {
            None
        };
        drop(state);
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

#[cfg(test)]
mod backpressure_tests {
    use super::*;
    use std::os::fd::OwnedFd;
    use std::time::Duration;
    use std::time::Instant;

    #[test]
    fn stalled_consumer_freezes_and_resumes_without_losing_kernel_queued_input() {
        let (input, mut writer) = UnixStream::pair().unwrap();
        input.set_nonblocking(true).unwrap();
        let total = 64 * 1024;
        let producer = std::thread::spawn(move || writer.write_all(&vec![b'a'; total]));
        let driver =
            Driver::start(File::from(OwnedFd::from(input)), Parser::default(), true).unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let state = driver.shared.state.lock().unwrap();
            if state.parser.input_backpressured() {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "reader never reached backpressure"
            );
            drop(state);
            std::thread::yield_now();
        }
        // Joining a reader held by backpressure must not require its consumer
        // to drain first. All accepted events survive the real checkpoint.
        let frozen = driver.freeze().unwrap();
        let bytes = frozen.parser.checkpoint().unwrap();
        let mut restored = Parser::restore(&bytes).unwrap();
        let mut accepted = 0;
        while let Some(event) = restored.pop() {
            assert!(
                matches!(event, InternalEvent::Event(crossterm::event::Event::Key(key)) if key.code == crossterm::event::KeyCode::Char('a'))
            );
            accepted += 1;
        }
        assert!((4096..5120).contains(&accepted));
        let driver = Driver::start(frozen.input, restored, true).unwrap();
        loop {
            if driver
                .shared
                .state
                .lock()
                .unwrap()
                .parser
                .input_backpressured()
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "restored reader never reached backpressure"
            );
            std::thread::yield_now();
        }
        let waker = std::task::Waker::noop();
        let mut context = Context::from_waker(waker);
        while accepted < total {
            match driver.poll_event(&mut context) {
                Poll::Ready(Some(Ok(event))) => {
                    assert!(
                        matches!(event, InternalEvent::Event(crossterm::event::Event::Key(key)) if key.code == crossterm::event::KeyCode::Char('a'))
                    );
                    accepted += 1;
                }
                Poll::Pending => {
                    assert!(
                        Instant::now() < deadline,
                        "consumer did not resume the reader"
                    );
                    std::thread::yield_now();
                }
                other => panic!("input ended before all bytes were delivered: {other:?}"),
            }
        }
        producer.join().unwrap().unwrap();
        drop(driver);
    }
}
