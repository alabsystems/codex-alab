// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Native fixture evidence and tracing for its exact validated child.

pub(super) struct HandshakeWitness<'a> {
    fake: &'a codex_alab_aterm::fake::FakeAterm,
    path: std::path::PathBuf,
    persisted: bool,
}

impl<'a> HandshakeWitness<'a> {
    pub(super) fn new(
        fake: &'a codex_alab_aterm::fake::FakeAterm,
        path: std::path::PathBuf,
    ) -> Self {
        Self {
            fake,
            path,
            persisted: false,
        }
    }

    pub(super) fn persist(&mut self) -> std::io::Result<()> {
        let bytes = serde_json::to_vec_pretty(&self.fake.handshake_events())
            .map_err(std::io::Error::other)?;
        std::fs::write(&self.path, bytes)?;
        self.persisted = true;
        Ok(())
    }
}

impl Drop for HandshakeWitness<'_> {
    fn drop(&mut self) {
        if !self.persisted {
            // Preserve setup/collection failures without replacing their original error.
            let _ = self.persist();
        }
    }
}

pub(super) fn init() -> color_eyre::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("off,codex_alab_autopilot=debug")
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .without_time()
        .try_init()
        .map_err(|error| {
            color_eyre::eyre::eyre!("could not install isolated launch diagnostics: {error}")
        })
}
