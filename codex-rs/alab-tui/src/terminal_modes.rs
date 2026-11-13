// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Preserve the original terminal mode independently of Crossterm's process-local
//! raw-mode cache. Snapshot bytes describe modes; they admit no SDK or execution.
use std::io;

pub fn enable_raw_mode() -> io::Result<()> {
    #[cfg(unix)]
    if crate::input::owned() {
        return native::enable();
    }
    crossterm::terminal::enable_raw_mode()
}
pub fn disable_raw_mode() -> io::Result<()> {
    #[cfg(unix)]
    if crate::input::owned() {
        return native::disable();
    }
    crossterm::terminal::disable_raw_mode()
}
#[cfg(unix)]
pub use native::Prepared;
#[cfg(unix)]
pub use native::Snapshot;
#[cfg(unix)]
pub use native::adopt;
#[cfg(unix)]
pub use native::prepare_upgrade;

/// Actual native original-mode ownership, not a decoded snapshot claim.
pub fn original_retained() -> bool {
    #[cfg(unix)]
    {
        native::original_retained()
    }
    #[cfg(not(unix))]
    false
}

/// Only a committed native terminal owner can preserve the TUI's exit modes.
/// Adopted snapshot data alone never establishes a source transfer.
pub fn preserve_on_exit() -> bool {
    #[cfg(unix)]
    if crate::input::owned() && !std::thread::panicking() {
        return native::preserve_on_exit();
    }
    false
}

#[cfg(unix)]
mod native {
    use codex_alab_core::upgrade_barrier::State as AdmissionState;
    use codex_alab_core::upgrade_barrier::Transfer;
    use serde::Deserialize;
    use serde::Serialize;
    use std::fs::File;
    use std::io;
    use std::mem::MaybeUninit;
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::MetadataExt;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::OnceLock;

    static ADMISSION: OnceLock<Arc<AdmissionState>> = OnceLock::new();
    fn admission() -> &'static Arc<AdmissionState> {
        ADMISSION.get_or_init(|| Arc::new(AdmissionState::default()))
    }
    static ORIGINAL: Mutex<State> = Mutex::new(State {
        started: false,
        original: None,
    });
    struct State {
        started: bool,
        original: Option<Original>,
    }
    struct Original {
        terminal: File,
        mode: libc::termios,
    }
    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Identity {
        device: u64,
        inode: u64,
        rdevice: u64,
    }
    impl Identity {
        fn read(terminal: &File) -> io::Result<Self> {
            let metadata = terminal.metadata()?;
            Ok(Self {
                device: metadata.dev(),
                inode: metadata.ino(),
                rdevice: metadata.rdev(),
            })
        }
    }
    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Mode {
        flags: [u64; 4],
        characters: Vec<u8>,
        input_speed: u64,
        output_speed: u64,
        line: u8,
    }
    fn widen(value: impl Into<u64>) -> u64 {
        value.into()
    }
    fn narrow<T: TryFrom<u64>>(value: u64) -> io::Result<T> {
        T::try_from(value).map_err(|_| invalid("terminal mode value is out of range"))
    }
    impl Mode {
        fn capture(mode: &libc::termios) -> Self {
            Self {
                flags: [
                    widen(mode.c_iflag),
                    widen(mode.c_oflag),
                    widen(mode.c_cflag),
                    // XNU tty.c sets PENDIN on return to ICANON and clears it
                    // while processing input. It is live queue state, not mode
                    // configuration to replay from an older process.
                    widen(mode.c_lflag & !libc::PENDIN),
                ],
                characters: mode.c_cc.to_vec(),
                // SAFETY: these functions only inspect the initialized termios.
                input_speed: widen(unsafe { libc::cfgetispeed(mode) }),
                output_speed: widen(unsafe { libc::cfgetospeed(mode) }),
                #[cfg(any(target_os = "linux", target_os = "android"))]
                line: mode.c_line,
                #[cfg(not(any(target_os = "linux", target_os = "android")))]
                line: 0,
            }
        }
        fn decode(&self, mut base: libc::termios) -> io::Result<libc::termios> {
            if self.characters.len() != base.c_cc.len() {
                return Err(invalid("terminal control-character layout differs"));
            }
            base.c_iflag = narrow(self.flags[0])?;
            base.c_oflag = narrow(self.flags[1])?;
            base.c_cflag = narrow(self.flags[2])?;
            let flags: libc::tcflag_t = narrow(self.flags[3])?;
            if flags & libc::PENDIN != 0 {
                return Err(invalid("terminal snapshot contains pending-input state"));
            }
            base.c_lflag = flags | (base.c_lflag & libc::PENDIN);
            base.c_cc.copy_from_slice(&self.characters);
            #[cfg(any(target_os = "linux", target_os = "android"))]
            {
                base.c_line = self.line;
            }
            #[cfg(not(any(target_os = "linux", target_os = "android")))]
            if self.line != 0 {
                return Err(invalid("terminal line-discipline layout differs"));
            }
            // SAFETY: setters change only this initialized local termios value.
            if unsafe { libc::cfsetispeed(&mut base, narrow(self.input_speed)?) } != 0
                || unsafe { libc::cfsetospeed(&mut base, narrow(self.output_speed)?) } != 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(base)
        }
    }
    #[derive(Clone, Debug, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct Snapshot {
        version: u32,
        operating_system: String,
        terminal: Identity,
        original: Mode,
        running: Mode,
    }
    fn invalid(why: &'static str) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidData, why)
    }
    fn read(terminal: &File) -> io::Result<libc::termios> {
        let mut mode = MaybeUninit::uninit();
        // SAFETY: the live owned TTY initializes the writable termios on success.
        if unsafe { libc::tcgetattr(terminal.as_raw_fd(), mode.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { mode.assume_init() })
    }
    fn restore(original: &Original) -> io::Result<()> {
        // TCSANOW never drains or flushes accepted input.
        // SAFETY: the original holds both the live terminal and initialized mode.
        if unsafe { libc::tcsetattr(original.terminal.as_raw_fd(), libc::TCSANOW, &original.mode) }
            != 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
    pub(super) fn enable() -> io::Result<()> {
        let _admission = admission()
            .admit()
            .map_err(|refusal| io::Error::other(refusal.to_string()))?;
        let mut state = ORIGINAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.started = true;
        if state.original.is_none() {
            if crossterm::terminal::is_raw_mode_enabled()? {
                return Err(invalid("terminal raw mode started outside its owner"));
            }
            let terminal = codex_alab_input::open_terminal_input()?;
            let mode = read(&terminal)?;
            state.original = Some(Original { terminal, mode });
        }
        // On adoption Crossterm saves the already-raw mode. Our original owner
        // overrides that cached value on the next disable, including editor use.
        crossterm::terminal::enable_raw_mode()
    }
    pub(super) fn disable() -> io::Result<()> {
        let admitted = admission().admit();
        if let Err(refusal) = &admitted {
            // A committed source can still recover the real original terminal
            // after a panic. This never reopens transferred execution admission.
            if !std::thread::panicking() {
                let state = ORIGINAL
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if state.original.is_none() && !crossterm::terminal::is_raw_mode_enabled()? {
                    return Ok(());
                }
                return Err(io::Error::other(refusal.to_string()));
            }
        }
        disable_original()
    }
    fn disable_original() -> io::Result<()> {
        let mut state = ORIGINAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.started = true;
        crossterm::terminal::disable_raw_mode()?;
        if let Some(owner) = state.original.as_ref() {
            restore(owner)?;
            state.original = None;
        }
        Ok(())
    }
    fn require_raw(mode: &libc::termios) -> io::Result<()> {
        if mode.c_lflag & (libc::ICANON | libc::ECHO | libc::ISIG) != 0 {
            return Err(invalid("native terminal is not in raw mode"));
        }
        Ok(())
    }
    fn snapshot(state: &State) -> io::Result<Snapshot> {
        if !crate::input::owned() {
            return Err(invalid("terminal mode has no ALab owner"));
        }
        let original = state
            .original
            .as_ref()
            .ok_or_else(|| invalid("no original terminal mode"))?;
        if !crossterm::terminal::is_raw_mode_enabled()? {
            return Err(invalid("terminal raw mode is not running"));
        }
        let running = read(&original.terminal)?;
        require_raw(&running)?;
        Ok(Snapshot {
            version: 1,
            operating_system: std::env::consts::OS.into(),
            terminal: Identity::read(&original.terminal)?,
            original: Mode::capture(&original.mode),
            running: Mode::capture(&running),
        })
    }
    /// Native owner plus revocable admission; only `snapshot()` is transferable
    /// data. The final callback must not call terminal-mode operations itself.
    pub struct Prepared {
        transfer: Transfer,
        snapshot: Snapshot,
    }
    impl Prepared {
        pub fn snapshot(&self) -> &Snapshot {
            &self.snapshot
        }
        pub fn matches_input(&self, input: &codex_alab_input::Fence) -> io::Result<bool> {
            let terminal = &self.snapshot.terminal;
            Ok(input.terminal_identity()? == (terminal.device, terminal.inode, terminal.rdevice))
        }
        pub fn committed(&self) -> bool {
            self.transfer.belongs_to(admission()) && self.transfer.committed()
        }
        pub fn valid(&self) -> bool {
            let state = ORIGINAL
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            self.transfer.valid() && matches(&state, &self.snapshot).unwrap_or(false)
        }
        /// Hold the actual terminal boundary and admission through the caller's
        /// final input/Core/goal/SDK/host decision. Native modes are re-read first.
        pub fn commit_if(&mut self, decide: impl FnOnce() -> bool) -> io::Result<bool> {
            let state = ORIGINAL
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if !matches(&state, &self.snapshot)? {
                return Ok(false);
            }
            Ok(self.transfer.commit_if(decide))
        }
        /// Fatal exec/startup recovery restores this actual source's terminal,
        /// while its execution admission stays permanently transferred.
        pub fn restore_after_failure(&self) -> io::Result<()> {
            if !self.transfer.belongs_to(admission()) || !self.transfer.committed() {
                return Err(invalid("terminal source transfer was not committed"));
            }
            disable_original()
        }
    }
    fn matches(state: &State, expected: &Snapshot) -> io::Result<bool> {
        if state.original.is_none() || !crossterm::terminal::is_raw_mode_enabled()? {
            return Ok(false);
        }
        let current = snapshot(state)?;
        Ok(current.terminal == expected.terminal
            && current.original == expected.original
            && current.running == expected.running)
    }
    /// Prepare under native terminal ownership, before committing source capture.
    pub fn prepare_upgrade() -> io::Result<Prepared> {
        let state = ORIGINAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let snapshot = snapshot(&state)?;
        // Lock order is terminal boundary then admission. New mutations register
        // admission before waiting for the terminal, revoking this preparation.
        let transfer = admission()
            .prepare()
            .map_err(|refusal| io::Error::other(refusal.to_string()))?;
        Ok(Prepared { transfer, snapshot })
    }
    pub(super) fn original_retained() -> bool {
        ORIGINAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .original
            .is_some()
    }
    pub(super) fn preserve_on_exit() -> bool {
        let state = ORIGINAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.original.is_some() && admission().committed()
    }
    /// Restore data ownership on a freshly opened, matching native TTY. This
    /// does not change its modes or consume input. Call before the first enable.
    pub fn adopt(snapshot: &Snapshot) -> io::Result<()> {
        let _admission = admission()
            .admit()
            .map_err(|refusal| io::Error::other(refusal.to_string()))?;
        if !crate::input::owned()
            || snapshot.version != 1
            || snapshot.operating_system != std::env::consts::OS
        {
            return Err(invalid("terminal mode snapshot is incompatible"));
        }
        let mut state = ORIGINAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.started || state.original.is_some() || crossterm::terminal::is_raw_mode_enabled()?
        {
            return Err(invalid("terminal mode adoption is too late"));
        }
        let terminal = codex_alab_input::open_terminal_input()?;
        if Identity::read(&terminal)? != snapshot.terminal {
            return Err(invalid("terminal identity differs from its snapshot"));
        }
        let running = read(&terminal)?;
        require_raw(&running)?;
        if Mode::capture(&running) != snapshot.running {
            return Err(invalid("terminal modes changed after source capture"));
        }
        let mode = snapshot.original.decode(running)?;
        state.original = Some(Original { terminal, mode });
        Ok(())
    }
}
#[cfg(all(test, unix))]
#[path = "terminal_modes_tests.rs"]
mod tests;
