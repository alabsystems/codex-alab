// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! SAME actual controller transport's kernel peer observation, never a Native
//! package/Go/Source grant. Host image identity is independently revalidated.
use super::*;
pub struct ControllerPackageHostPeer {
    transport: Arc<source_member::Transport>,
    pid: u32,
}
impl Controller {
    pub fn package_host_peer(&self) -> io::Result<ControllerPackageHostPeer> {
        let pid = self.transport.server_pid()?;
        if pid <= 1 || pid == std::process::id() {
            return Err(unavailable());
        }
        Ok(ControllerPackageHostPeer {
            transport: self.transport.clone(),
            pid,
        })
    }
}
impl ControllerPackageHostPeer {
    pub fn pid_data(&self) -> u32 {
        self.pid
    }
    pub fn recheck(&self) -> io::Result<()> {
        if self.transport.server_pid()? == self.pid {
            Ok(())
        } else {
            Err(unavailable())
        }
    }
    pub fn is_for_controller(&self, actual: &Controller) -> bool {
        Arc::ptr_eq(&self.transport, &actual.transport)
    }
}
