// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Test-only clones of the SAME MessageProcessor resources, no reconstructed
//! configuration, manager, context, Native origin or accepted operation.
use super::*;
impl MessageProcessor {
    pub(crate) fn alab_selection_publication_parts_for_tests(
        &self,
    ) -> (ThreadRequestProcessor, Arc<OutgoingMessageSender>) {
        (self.thread_processor.clone(), self.outgoing.clone())
    }
}
