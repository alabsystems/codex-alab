// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The original RequestContext retains the same accepted resource allocation.
//! No permission follows from Some, its type, a label, or a request/connection ID.
impl super::RequestContext {
    pub(crate) fn alab_with_connection_resource(
        mut self,
        resource: Option<codex_alab_core::connection_resource::ConnectionResource>,
    ) -> Self {
        self.alab_connection_resource = resource;
        self
    }
}

impl super::RequestContext {
    pub(crate) fn alab_request_id(&self) -> &super::ConnectionRequestId {
        &self.request_id
    }
}
