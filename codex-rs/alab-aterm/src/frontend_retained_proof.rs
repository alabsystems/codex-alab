// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Explicit debug qualification surface over the same actual IO owners.
//! The feature/marker is not Native admission or a continuation grant.
pub use super::retained_service::AttachmentObservation;
pub use super::retained_service::RetainedAttachment;
pub use super::retained_service::RetainedPhase;
pub use super::retained_service::RetainedStart;
pub use super::retained_service::ServiceObservation;
pub use super::retained_service::prepare_attachment;
pub use super::retained_service::prepare_start;

pub async fn bind_backend(
    controller: super::Controller,
) -> std::io::Result<codex_alab_core::native_incoming::Incoming> {
    Ok(super::retained_service::bind_backend(controller)
        .await?
        .into_incoming())
}
