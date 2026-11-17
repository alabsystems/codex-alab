// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Same original physical session fields; no request-only view is an owner.
use super::*;
use crate::app::backend_sessions::alab_retained::EventData;
impl AppServerSession {
    pub(crate) fn alab_backend_is_owning(&self) -> bool {
        self.connection_owner.is_some()
    }
    pub(crate) fn alab_backend_deferred_data(&self) -> std::io::Result<Vec<EventData>> {
        self.alab_configured_events
            .iter()
            .map(|event| {
                if event.backend_id != self.backend_id {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "deferred session event changed actual backend",
                    ));
                }
                EventData::capture(&event.event)
            })
            .collect()
    }
}
