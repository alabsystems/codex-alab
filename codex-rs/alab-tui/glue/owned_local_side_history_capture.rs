// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Retain a real original Incoming in SAME Openings before S2 is polled.
//! Original observe/preparing/abandon already maintain its real FIFO updates.
use super::*;
use color_eyre::eyre::Result;
use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;
impl App {
    pub(crate) async fn alab_begin_side_history_capture(
        &mut self,
        server: &mut AppServerSession,
        target: Target,
    ) -> Result<uuid::Uuid> {
        if server.backend_id() != target.backend_id
            || self.saved_openings.active.contains_key(&target.key())
        {
            bail!("an original side attachment already owns this store");
        }
        let request = uuid::Uuid::new_v4();
        let incoming = Incoming::begin(self, server, target, request).await;
        self.saved_openings.active.insert(
            target.key(),
            Flight {
                request,
                started: Instant::now(),
                retired: false,
                incoming: Some(incoming),
            },
        );
        Ok(request)
    }
    pub(crate) fn alab_check_side_history_capture(
        &self,
        target: &Target,
        request: uuid::Uuid,
    ) -> Result<()> {
        let flight = self
            .saved_openings
            .active
            .get(&target.key())
            .ok_or_else(|| eyre!("original side Incoming missing"))?;
        if flight.request != request
            || flight.retired
            || !flight.incoming.as_ref().is_some_and(|incoming| {
                incoming.owns_channel(
                    self.thread_event_channels.get(&target.child),
                    target.backend_id,
                )
            })
        {
            bail!("original side attachment no longer current");
        }
        Ok(())
    }
    pub(in crate::app) fn alab_take_side_history_capture(
        &mut self,
        target: &Target,
        request: uuid::Uuid,
    ) -> Result<Incoming> {
        self.alab_check_side_history_capture(target, request)?;
        let mut flight = self
            .saved_openings
            .active
            .remove(&target.key())
            .ok_or_else(|| eyre!("original side capture was removed"))?;
        flight
            .incoming
            .take()
            .ok_or_else(|| eyre!("original side capture was released"))
    }
}
