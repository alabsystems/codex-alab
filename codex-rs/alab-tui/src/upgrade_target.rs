// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A target image's bounded handshake under an atpkg-owned signed build lease.
//! This reports compatibility, not package authenticity or permission to restart.

use std::io::Read;
use std::io::Write;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;

/// Released 0.11 callers send no dialect argument and reject unknown top-level
/// fields. Add optional capabilities to `extensions` on this wire format.
pub const PROTOCOL: u32 = 3;

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
    pub full_runtime: u32,
    pub runtime_readers: Vec<u32>,
    #[serde(default)]
    pub extensions: std::collections::BTreeMap<String, serde_json::Value>,
    pub exec_custody_adopted: bool,
    pub pid: u32,
    pub executable: PathBuf,
    pub device: u64,
    pub inode: u64,
    pub product_version: String,
}

impl Response {
    /// Exact signed-image/actual probe-child correlation for the Native E1
    /// compatibility extension. This deliberately supplies no legacy forest
    /// reader requirement, package admission or execution permission.
    pub(crate) fn validate_retained_image_identity(
        &self,
        admitted: &crate::upgrade_admission::Receipt,
        child_pid: u32,
    ) -> std::io::Result<()> {
        if self.protocol != PROTOCOL
            || child_pid == 0
            || self.pid != child_pid
            || self.executable != admitted.executable.canonicalize()?
            || self.device != admitted.device
            || self.inode != admitted.inode
            || self.product_version != admitted.version
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "retained target response does not match the actual admitted image",
            ));
        }
        Ok(())
    }
    /// Bind compatibility to the exact retained signed build and original probe
    /// child. This never turns a compatibility response into package admission.
    pub fn validate(
        &self,
        admitted: &crate::upgrade_admission::Receipt,
        child_pid: u32,
    ) -> std::io::Result<()> {
        if self.protocol != PROTOCOL
            || self.exec_custody != crate::upgrade_exec_custody::PROTOCOL
            || !self.accepts_runtime(crate::upgrade_state::VERSION)
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

    pub fn accepts_runtime(&self, version: u32) -> bool {
        self.full_runtime != 0
            && version != 0
            && self
                .runtime_readers
                .windows(2)
                .all(|pair| pair[0] < pair[1])
            && self
                .runtime_readers
                .first()
                .is_none_or(|version| *version != 0)
            && self.runtime_readers.contains(&version)
            && self.runtime_readers.contains(&self.full_runtime)
    }

    fn current(exec_custody: u32) -> std::io::Result<Self> {
        let executable = std::env::current_exe()?.canonicalize()?;
        let metadata = std::fs::metadata(&executable)?;
        let supports_runtime = exec_custody == crate::upgrade_exec_custody::PROTOCOL;
        Ok(Self {
            protocol: PROTOCOL,
            exec_custody,
            full_runtime: if supports_runtime {
                crate::upgrade_state::VERSION
            } else {
                0
            },
            runtime_readers: if supports_runtime {
                crate::upgrade_state::reader_versions()
            } else {
                Vec::new()
            },
            extensions: crate::upgrade_exec_custody::images::support_extensions(),
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
            &Response::current(crate::upgrade_exec_custody::startup_protocol())?,
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
    fn default_frame_is_readable_by_the_released_0_11_caller() {
        let response = Response::current(crate::upgrade_exec_custody::PROTOCOL).unwrap();
        let mut bytes = Vec::new();
        respond(&response, false, &mut std::io::empty(), &mut bytes).unwrap();
        let mut frame: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let readers: Vec<u32> = serde_json::from_value(frame["runtime_readers"].clone()).unwrap();
        let writer: u32 = serde_json::from_value(frame["full_runtime"].clone()).unwrap();
        assert!(readers.contains(&2), "0.11 writes runtime state version 2");
        assert!(readers.contains(&writer));
        assert!(readers.first().is_some_and(|version| *version > 0));
        assert!(readers.windows(2).all(|pair| pair[0] < pair[1]));
        if let Some(extensions) = frame.as_object_mut().unwrap().remove("extensions") {
            assert!(extensions.is_object());
        }
        let executable = std::env::current_exe().unwrap().canonicalize().unwrap();
        let metadata = std::fs::metadata(&executable).unwrap();
        // These field names and wire versions are from the shipped caller. A
        // round trip through today's Response would accept coordinated breaks.
        assert_eq!(
            frame,
            serde_json::json!({
                "protocol": 3,
                "exec_custody": 2,
                "full_runtime": writer,
                "runtime_readers": readers,
                "exec_custody_adopted": crate::upgrade_exec_custody::adopted(),
                "pid": std::process::id(),
                "executable": executable,
                "device": metadata.dev(),
                "inode": metadata.ino(),
                "product_version": crate::session::version(),
            })
        );
    }

    #[test]
    fn handshake_arrives_before_the_hold_and_eof_releases_it() {
        let (caller, mut target) = UnixStream::pair().unwrap();
        caller
            .set_read_timeout(Some(std::time::Duration::from_secs(60)))
            .unwrap();
        let mut input = target.try_clone().unwrap();
        let worker = std::thread::spawn(move || {
            respond(
                &Response::current(0).unwrap(),
                true,
                &mut input,
                &mut target,
            )
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
        let response = Response::current(0).unwrap();
        assert!(respond(&response, true, &mut &b"unexpected"[..], &mut Vec::new()).is_err());
        let mut response = response;
        response.product_version = "x".repeat(16 * 1024);
        let mut output = Vec::new();
        assert!(respond(&response, false, &mut std::io::empty(), &mut output).is_err());
        assert!(output.is_empty());
    }
}
