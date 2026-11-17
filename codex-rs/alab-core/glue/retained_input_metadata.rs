// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Original immutable human-input publication DATA; not Core/effect authority.
use super::*;
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct InputDescriptor {
    pub epoch: String,
    pub ordinal: u64,
    pub operation: String,
    pub request_id: RequestId,
    pub proof: String,
}
impl InputDescriptor {
    pub(crate) fn valid(&self) -> bool {
        token(&self.epoch)
            && self.ordinal > 0
            && token(&self.operation)
            && ids_valid(std::slice::from_ref(&self.request_id))
            && proof(&self.proof)
    }
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "camelCase", deny_unknown_fields)]
pub enum PublicationOutcome {
    Published {},
    ReceiverClosed {},
    SealedClosed {},
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct InputReceipt {
    pub descriptor: InputDescriptor,
    pub outcome: PublicationOutcome,
    pub proof: String,
}
impl InputReceipt {
    pub(crate) fn valid(&self) -> bool {
        self.descriptor.valid() && proof(&self.proof)
    }
}
fn proof(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
