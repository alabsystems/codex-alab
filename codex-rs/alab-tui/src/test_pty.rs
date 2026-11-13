// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Ownership and output collection for native controlling-terminal fixtures.

use std::fs::File;
use std::io;
use std::io::Read;
use std::process::Child;
use std::process::Command;
use std::process::ExitStatus;

pub struct PtyChild {
    child: Child,
    master: Option<File>,
}

impl PtyChild {
    /// Consume the command so its parent-side slave descriptors close as soon
    /// as spawning finishes. Only the child retains the terminal's slave end.
    pub fn spawn(mut command: Command, master: File) -> io::Result<Self> {
        let child = command.spawn()?;
        Ok(Self {
            child,
            master: Some(master),
        })
    }

    pub fn master(&mut self) -> &mut File {
        match self.master.as_mut() {
            Some(master) => master,
            None => panic!("PTY is owned until cleanup"),
        }
    }

    /// Read one bounded chunk; the caller keeps checking its deadline and the
    /// child's exit status even when a child produces output continuously.
    pub fn read_output(&mut self, output: &mut Vec<u8>) -> io::Result<usize> {
        let mut buffer = [0; 4096];
        let read = loop {
            match self.master().read(&mut buffer) {
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                result => break result,
            }
        };
        match read {
            Ok(size) => {
                output.extend_from_slice(&buffer[..size]);
                Ok(size)
            }
            Err(error)
                if error.kind() == io::ErrorKind::WouldBlock
                    || error.raw_os_error() == Some(libc::EIO) =>
            {
                // A closed PTY reports EOF on macOS and EIO on Linux. Neither
                // proves child success; callers must still inspect try_wait.
                Ok(0)
            }
            Err(error) => Err(error),
        }
    }

    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }
}

impl Drop for PtyChild {
    fn drop(&mut self) {
        // A terminal close may wait for unread output. Release the reader
        // before forced cleanup, including when an assertion is unwinding.
        drop(self.master.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
