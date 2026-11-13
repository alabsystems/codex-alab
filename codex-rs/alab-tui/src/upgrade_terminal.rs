// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual terminal and input owners supplied at the TUI's capture boundary.
//! Their snapshots are data only; this pair supplies no SDK/exec admission.
use crate::input::UpgradeInput;
use crate::terminal_modes;
use std::io;

pub struct Prepared {
    input: UpgradeInput,
    modes: terminal_modes::Prepared,
    committed: bool,
    startup: Option<crate::upgrade_startup::Snapshot>,
}
impl Prepared {
    /// The TUI must first check its reply filter under the real event-broker
    /// lock. This takes its exact joined input fence, not a decoded checkpoint.
    pub fn prepare(input: UpgradeInput) -> io::Result<Self> {
        let modes = terminal_modes::prepare_upgrade()?;
        if !input.valid() || !modes.matches_input(&input)? {
            return Err(io::Error::other(
                "terminal and input ownership do not match",
            ));
        }
        Ok(Self {
            input,
            modes,
            committed: false,
            startup: None,
        })
    }
    pub fn with_startup(mut self, startup: crate::upgrade_startup::Snapshot) -> io::Result<Self> {
        startup.validate()?;
        self.startup = Some(startup);
        Ok(self)
    }
    pub fn startup_snapshot(&self) -> Option<&crate::upgrade_startup::Snapshot> {
        self.startup.as_ref()
    }
    pub fn input_snapshot(&self) -> &[u8] {
        self.input.snapshot()
    }
    pub fn mode_snapshot(&self) -> &terminal_modes::Snapshot {
        self.modes.snapshot()
    }
    pub fn valid(&self) -> bool {
        self.modes.valid() && self.input.valid()
    }
    pub fn committed(&self) -> bool {
        self.committed && self.modes.committed() && self.input.committed()
    }
    /// Both actual native boundaries remain held through the caller's ONE final
    /// Core/goal/SDK/host decision. Do not call terminal/input operations inside it.
    pub fn commit_if(&mut self, decide: impl FnOnce() -> bool) -> io::Result<bool> {
        if self.committed {
            return Ok(false);
        }
        let accepted = self.modes.commit_if(|| self.input.commit_if(decide))?;
        self.committed = accepted;
        Ok(accepted)
    }
    pub fn restore_after_failure(&self) -> io::Result<()> {
        if !self.committed() {
            return Err(io::Error::other("terminal capture was not committed"));
        }
        self.modes.restore_after_failure()
    }
}
