// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Genuine per-slot service binding, not Current or a new Native permission.
use super::*;
impl FrontendTenure {
    pub(crate) fn alab_backend_service_data(&self) -> io::Result<&str> {
        // Historical identity DATA remains available for a genuinely parked,
        // closed entry. It does not say that this delivery is current/open.
        Ok(self.target.service_data())
    }
    pub(crate) fn alab_backend_delivery_open(&self) -> bool {
        !self.delivery.closing()
    }
}
