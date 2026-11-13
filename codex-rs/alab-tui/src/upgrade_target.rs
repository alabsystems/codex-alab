// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A target image's bounded handshake under an atpkg-owned signed build lease.
//! This reports compatibility, not package authenticity or permission to restart.

use std::io::Read;
use std::io::Write;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;

pub const PROTOCOL: u32 = 2;

/// Typed CLI entry, kept out of upstream's argument implementation.
#[derive(Debug, clap::Args)]
pub struct Command {
    /// Remain alive until the caller closes standard input.
    #[arg(long)]
    hold: bool,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    pub protocol: u32,
    pub exec_custody: u32,
    pub exec_custody_adopted: bool,
    pub pid: u32,
    pub executable: PathBuf,
    pub device: u64,
    pub inode: u64,
    pub product_version: String,
}

impl Response {
    /// Bind compatibility to the exact retained signed build and original probe
    /// child. This never turns a compatibility response into package admission.
    pub fn validate(
        &self,
        admitted: &crate::upgrade_admission::Receipt,
        child_pid: u32,
    ) -> std::io::Result<()> {
        if self.protocol != PROTOCOL
            || self.exec_custody != crate::upgrade_exec_custody::PROTOCOL
            || child_pid == 0
            || self.pid != child_pid
            || self.executable != admitted.executable.canonicalize()?
            || self.device != admitted.device
            || self.inode != admitted.inode
            || self.product_version != admitted.version
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "the upgrade target response does not match the admitted image",
            ));
        }
        Ok(())
    }

    fn current() -> std::io::Result<Self> {
        let executable = std::env::current_exe()?.canonicalize()?;
        let metadata = std::fs::metadata(&executable)?;
        Ok(Self {
            protocol: PROTOCOL,
            exec_custody: crate::upgrade_exec_custody::startup_protocol(),
            exec_custody_adopted: crate::upgrade_exec_custody::adopted(),
            pid: std::process::id(),
            executable,
            device: metadata.dev(),
            inode: metadata.ino(),
            product_version: crate::session::version().into(),
        })
    }
}

impl Command {
    /// Emit one JSON frame and optionally keep the enclosing native lease alive.
    /// A CLOEXEC caller pipe makes successful exec release this helper naturally.
    pub fn run(self) -> std::io::Result<()> {
        respond(
            &Response::current()?,
            self.hold,
            &mut std::io::stdin().lock(),
            &mut std::io::stdout().lock(),
        )
    }
}

fn respond(
    response: &Response,
    hold: bool,
    input: &mut impl Read,
    output: &mut impl Write,
) -> std::io::Result<()> {
    let mut bytes = serde_json::to_vec(response)?;
    // A malformed filesystem path cannot turn this into an unbounded IPC frame.
    if bytes.len() > 16 * 1024 {
        return Err(std::io::Error::other(
            "the upgrade target response is too large",
        ));
    }
    bytes.push(b'\n');
    output.write_all(&bytes)?;
    output.flush()?;
    if hold {
        let mut byte = [0];
        loop {
            match input.read(&mut byte) {
                Ok(0) => break,
                Ok(_) => return Err(std::io::Error::other("the upgrade hold expects only EOF")),
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;
    use std::os::unix::net::UnixStream;

    #[test]
    fn handshake_arrives_before_the_hold_and_eof_releases_it() {
        let (caller, mut target) = UnixStream::pair().unwrap();
        caller
            .set_read_timeout(Some(std::time::Duration::from_secs(60)))
            .unwrap();
        let mut input = target.try_clone().unwrap();
        let worker = std::thread::spawn(move || {
            respond(&Response::current().unwrap(), true, &mut input, &mut target)
        });
        let mut reader = std::io::BufReader::new(caller);
        let mut frame = String::new();
        reader.read_line(&mut frame).unwrap();
        let response: Response = serde_json::from_str(&frame).unwrap();
        assert_eq!(response.protocol, PROTOCOL);
        assert_eq!(response.pid, std::process::id());
        let metadata = std::fs::metadata(response.executable).unwrap();
        assert_eq!(
            (response.device, response.inode),
            (metadata.dev(), metadata.ino())
        );
        assert!(!worker.is_finished(), "the caller still owns the hold pipe");
        reader
            .get_mut()
            .shutdown(std::net::Shutdown::Write)
            .unwrap();
        worker.join().unwrap().unwrap();
    }

    #[test]
    fn unexpected_input_or_an_oversized_frame_refuses() {
        let response = Response::current().unwrap();
        assert!(respond(&response, true, &mut &b"unexpected"[..], &mut Vec::new()).is_err());
        let mut response = response;
        response.product_version = "x".repeat(16 * 1024);
        let mut output = Vec::new();
        assert!(respond(&response, false, &mut std::io::empty(), &mut output).is_err());
        assert!(output.is_empty());
    }
}
