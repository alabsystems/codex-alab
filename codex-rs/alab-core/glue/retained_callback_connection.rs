// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
use super::*;
impl MessageProcessor {
    pub(crate) fn alab_callback_connection_open(
        &self,
        id: ConnectionId,
        actual: &Arc<ConnectionSessionState>,
    ) {
        self.outgoing
            .alab_callback_connection_open(id, actual.rpc_gate.clone());
    }
}

impl MessageProcessor {
    pub(crate) fn alab_callback_connection_closing(
        &self,
        id: ConnectionId,
        actual: &Arc<ConnectionSessionState>,
    ) {
        self.outgoing
            .alab_callback_connection_closing(id, &actual.rpc_gate);
    }
    pub(crate) fn alab_callback_ingress_drained(&self, id: ConnectionId) {
        self.outgoing.alab_callback_ingress_drained(id);
    }
}

impl MessageProcessor {
    pub(crate) fn alab_original_input_ingress(
        &self,
        actual: &Arc<ConnectionSessionState>,
        request: &codex_app_server_protocol::JSONRPCRequest,
    ) -> Option<Arc<crate::outgoing_message::alab_callback_outcomes::ingress::FrameLease>> {
        let phase = request
            .params
            .as_ref()?
            .get("_meta")?
            .get("alab.retainedCallbacks")?
            .get("phase")?
            .as_str()?;
        if request.method != "config/read"
            || !matches!(phase, "prepareInput" | "publishInput" | "retireInput")
        {
            return None;
        }
        // DATA chooses extra custody only. The typed parser/current-origin and
        // private-cell/registry checks still exclusively authorize the request.
        self.outgoing.alab_original_input_frame(&actual.rpc_gate)
    }
}
