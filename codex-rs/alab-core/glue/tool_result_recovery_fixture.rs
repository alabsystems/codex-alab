// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::TestAppServer;

impl TestAppServer {
    /// For crash recovery, kill and reap this fixture while its client transport is still open.
    pub async fn kill_and_reap(&mut self) -> std::io::Result<()> {
        self.process.kill().await
    }
}
