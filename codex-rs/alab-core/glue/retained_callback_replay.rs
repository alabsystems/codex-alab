// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Keep actual Source endpoint pins with the original pending request snapshot.
use super::*;
use std::result::Result;
pub(crate) struct PendingRequests {
    requests: Vec<ServerRequest>,
    #[cfg(unix)]
    snapshot: Option<alab_callback_outcomes::Snapshot>,
}
impl PendingRequests {
    pub(super) fn new(
        outgoing: &OutgoingMessageSender,
        callbacks: &HashMap<RequestId, PendingCallbackEntry>,
        requests: Vec<ServerRequest>,
    ) -> Self {
        #[cfg(unix)]
        let snapshot = outgoing
            .alab_callback_outcomes
            .as_ref()
            .map(|ledger| ledger.snapshot(callbacks, &requests));
        #[cfg(not(unix))]
        let _ = (outgoing, callbacks);
        Self {
            requests,
            #[cfg(unix)]
            snapshot,
        }
    }
    #[cfg(unix)]
    pub(super) fn reserve(
        &self,
        outgoing: &OutgoingMessageSender,
        recipient: ConnectionId,
        human: bool,
    ) -> std::io::Result<Option<alab_callback_outcomes::ReplayReservation>> {
        match (&outgoing.alab_callback_outcomes, &self.snapshot) {
            (None, None) => Ok(None),
            (Some(ledger), Some(snapshot)) => ledger
                .reserve(snapshot, &self.requests, recipient, human)
                .map(Some),
            _ => Err(std::io::Error::other(
                "actual callback snapshot owner changed",
            )),
        }
    }
}
impl std::ops::Deref for PendingRequests {
    type Target = Vec<ServerRequest>;
    fn deref(&self) -> &Self::Target {
        &self.requests
    }
}
impl IntoIterator for PendingRequests {
    type Item = ServerRequest;
    type IntoIter = std::vec::IntoIter<ServerRequest>;
    fn into_iter(self) -> Self::IntoIter {
        self.requests.into_iter()
    }
}
#[cfg(test)]
impl std::fmt::Debug for PendingRequests {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.requests, out)
    }
}
#[cfg(test)]
impl PartialEq<Vec<ServerRequest>> for PendingRequests {
    fn eq(&self, other: &Vec<ServerRequest>) -> bool {
        &self.requests == other
    }
}
impl OutgoingMessageSender {
    #[cfg(unix)]
    pub(super) fn alab_register_callback_endpoint(
        &self,
        request: &ServerRequest,
        thread: Option<ThreadId>,
        recipients: Option<&[ConnectionId]>,
        sender: oneshot::Sender<ClientRequestResult>,
    ) -> Result<
        (
            Option<alab_callback_outcomes::Endpoint>,
            oneshot::Sender<ClientRequestResult>,
        ),
        (),
    > {
        let result = self
            .alab_callback_outcomes
            .as_ref()
            .map_or(Ok(None), |ledger| {
                ledger.register(request, thread, recipients)
            });
        match result {
            Ok(endpoint) => Ok((endpoint, sender)),
            Err(_) => {
                let _sent = sender.send(Err(JSONRPCErrorError {
                    code: -32000,
                    message: "Source callback admission unavailable".into(),
                    data: None,
                }));
                Err(())
            }
        }
    }
}
