// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Kernel-observed SAME controller host opens an actual signed image lease.
//! Paths/builds remain candidate DATA; only the original native admission worker
//! can return Received. No legacy PreparedExec, argv/PATH host, or forest path.
use super::*;
use crate::frontend_control::Controller;
use crate::frontend_control::ControllerPackageHostPeer;
use std::io;
use std::time::Instant;
pub struct RetainedHost {
    host: Host,
    peer: ControllerPackageHostPeer,
}
fn refused(message: impl std::fmt::Display) -> io::Error {
    io::Error::other(message.to_string())
}
impl RetainedHost {
    pub fn capture(actual: &Controller) -> io::Result<Self> {
        let peer = actual.package_host_peer()?;
        let pid = peer.pid_data();
        let identity = crate::identity::of_pid(pid)?;
        let path = image_path(pid)?.canonicalize()?;
        let image = File::open(&path)?;
        let result = Self {
            host: Host {
                pid,
                identity,
                path,
                image,
            },
            peer,
        };
        result.recheck()?;
        Ok(result)
    }
    pub fn is_for_controller(&self, actual: &Controller) -> bool {
        self.peer.is_for_controller(actual)
    }
    pub fn recheck(&self) -> io::Result<()> {
        self.peer.recheck()?;
        let host = &self.host;
        if self.peer.pid_data() != host.pid
            || crate::identity::of_pid(host.pid)? != host.identity
            || image_path(host.pid)?.canonicalize()? != host.path
        {
            return Err(refused("original retained package host changed"));
        }
        let opened = host.image.metadata()?;
        let named = std::fs::symlink_metadata(&host.path)?;
        let identity =
            |m: &std::fs::Metadata| (m.dev(), m.ino(), m.len(), m.mtime(), m.mtime_nsec());
        if !opened.is_file()
            || !named.is_file()
            || opened.permissions().mode() & 0o111 == 0
            || identity(&opened) != identity(&named)
        {
            return Err(refused("original retained package host image changed"));
        }
        Ok(())
    }
    /// Call only inside a task/result slot already owned by the actual C1
    /// Attempt before its first await. Caller cancellation must retain a late
    /// real Received result; this API itself does not infer task cancellation.
    pub async fn admit_target(
        &self,
        expected: PathBuf,
        build: u64,
        deadline: Instant,
    ) -> io::Result<crate::upgrade_admission::Received> {
        self.recheck()?;
        let received =
            crate::upgrade_admission::admit(self.host.path.clone(), expected, build, deadline)
                .await?;
        self.recheck()?;
        Ok(received)
    }
}
