// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Compile the SAME client decoder against the SAME actual metadata types in
//! this server unit-test crate; no copied carrier/restore implementation.
use super::metadata;
#[path = "initial_selection_offer.rs"]
mod decoder;
pub(crate) use decoder::take_offer;
