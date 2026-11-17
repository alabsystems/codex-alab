// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Source callback endpoint DATA. Never Native/App/Core/effect authority.
use codex_app_server_protocol::RequestId;
use serde::Deserialize;
use serde::Serialize;
pub(crate) const KEY: &str = "alab.retainedCallbacks";
pub(crate) const RESULT_KEY: &str = "alab.retainedCallbackObservation";
pub(crate) const MAX_IDS: usize = 256;
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Descriptor {
    pub epoch: String,
    pub ordinal: u64,
    pub operation: String,
    pub ids: Vec<RequestId>,
    pub proof: String,
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Projection {
    pub frontier: String,
    pub snapshot: String,
    pub thread_id: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum Request {
    PrepareInput {
        service: String,
        descriptor: Descriptor,
        operation: String,
        request_id: RequestId,
        value: serde_json::Value,
        previous: Option<input::InputDescriptor>,
    },
    PublishInput {
        service: String,
        descriptor: input::InputDescriptor,
        value: serde_json::Value,
        previous: Option<input::InputReceipt>,
    },
    RetireInput {
        service: String,
        receipt: input::InputReceipt,
        value: serde_json::Value,
    },
    Capture {
        service: String,
        operation: String,
        ids: Vec<RequestId>,
        previous: Option<Descriptor>,
    },
    Observe {
        service: String,
        descriptor: Descriptor,
        projection: Projection,
        after: u64,
    },
    Acknowledge {
        service: String,
        descriptor: Descriptor,
        observation: Observation,
        ids: Vec<RequestId>,
    },
    AcknowledgeLive {
        service: String,
        descriptor: Descriptor,
        ids: Vec<RequestId>,
    },
    ObserveClosed {
        service: String,
        descriptor: Descriptor,
        projection: Projection,
        ids: Vec<RequestId>,
    },
    AcknowledgeClosed {
        service: String,
        descriptor: Descriptor,
        observation: ClosedObservation,
        ids: Vec<RequestId>,
    },
    ObserveConfigured {
        service: String,
        descriptor: Descriptor,
        projection: ConfiguredProjection,
    },
    AcknowledgeConfigured {
        service: String,
        descriptor: Descriptor,
        observation: ConfiguredObservation,
        ids: Vec<RequestId>,
    },
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "camelCase", deny_unknown_fields)]
pub enum Outcome {
    Pending {},
    Response {},
    ClientError {},
    Cancelled {},
    Withdrawn {},
    Unknown {},
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Fact {
    pub(crate) request_id: RequestId,
    pub(crate) thread_id: Option<String>,
    pub(crate) outcome: Outcome,
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum Content {
    Current { facts: Vec<Fact> },
    Retired {},
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Observation {
    pub descriptor: Descriptor,
    pub projection: Projection,
    pub sequence: u64,
    pub content: Content,
    pub proof: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum Reply {
    InputPrepared {
        descriptor: input::InputDescriptor,
    },
    InputPublished {
        receipt: input::InputReceipt,
    },
    InputRetired {
        receipt: input::InputReceipt,
    },
    ObservedClosed {
        observation: ClosedObservation,
    },
    ObservedConfigured {
        observation: ConfiguredObservation,
    },
    LiveAcknowledged {
        descriptor: Descriptor,
        ids: Vec<RequestId>,
        retired: bool,
    },
    Captured {
        descriptor: Descriptor,
    },
    Observed {
        observation: Observation,
    },
    Acknowledged {
        descriptor: Descriptor,
        sequence: u64,
        ids: Vec<RequestId>,
        retired: bool,
    },
}
pub(crate) fn token(s: &str) -> bool {
    s.len() == 32
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub(crate) fn ids_valid(ids: &[RequestId]) -> bool {
    ids.len() <= MAX_IDS
        && ids.iter().all(|id| match id {
            RequestId::String(s) => !s.is_empty() && s.len() <= 256,
            RequestId::Integer(_) => true,
        })
        && ids.iter().enumerate().all(|(i, id)| !ids[..i].contains(id))
}
impl Descriptor {
    pub(crate) fn valid(&self) -> bool {
        token(&self.epoch)
            && self.ordinal > 0
            && token(&self.operation)
            && ids_valid(&self.ids)
            && self.proof.len() == 64
            && self
                .proof
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    }
}
impl Projection {
    pub(crate) fn valid(&self) -> bool {
        token(&self.frontier)
            && token(&self.snapshot)
            && !self.thread_id.is_empty()
            && self.thread_id.len() <= 256
    }
}
impl Request {
    pub(crate) fn service(&self) -> &str {
        match self {
            Self::PrepareInput { service, .. }
            | Self::PublishInput { service, .. }
            | Self::RetireInput { service, .. } => service,
            Self::Capture { service, .. }
            | Self::Observe { service, .. }
            | Self::Acknowledge { service, .. }
            | Self::AcknowledgeLive { service, .. }
            | Self::ObserveConfigured { service, .. }
            | Self::AcknowledgeConfigured { service, .. }
            | Self::ObserveClosed { service, .. }
            | Self::AcknowledgeClosed { service, .. } => service,
        }
    }
    pub(crate) fn valid(&self) -> bool {
        token(self.service())
            && match self {
                Self::PrepareInput {
                    descriptor,
                    operation,
                    request_id,
                    value,
                    previous,
                    ..
                } => {
                    serde_json::to_vec(value).is_ok_and(|wire| wire.len() <= 1_048_576)
                        && descriptor.valid()
                        && token(operation)
                        && descriptor.ids.contains(request_id)
                        && previous.as_ref().is_none_or(|prior| {
                            prior.valid()
                                && prior.operation == *operation
                                && prior.request_id == *request_id
                        })
                }
                Self::PublishInput {
                    descriptor,
                    value,
                    previous,
                    ..
                } => {
                    descriptor.valid()
                        && serde_json::to_vec(value).is_ok_and(|wire| wire.len() <= 1_048_576)
                        && previous
                            .as_ref()
                            .is_none_or(|prior| prior.valid() && prior.descriptor == *descriptor)
                }
                Self::RetireInput { receipt, value, .. } => {
                    receipt.valid()
                        && serde_json::to_vec(value).is_ok_and(|wire| wire.len() <= 1_048_576)
                }
                Self::ObserveClosed {
                    descriptor,
                    projection,
                    ids,
                    ..
                } => {
                    descriptor.valid()
                        && projection.valid()
                        && ids_valid(ids)
                        && ids.iter().all(|id| descriptor.ids.contains(id))
                }
                Self::AcknowledgeClosed {
                    descriptor,
                    observation,
                    ids,
                    ..
                } => {
                    descriptor == &observation.descriptor
                        && observation.valid()
                        && ids_valid(ids)
                        && ids.iter().all(|id| observation.ids.contains(id))
                }
                Self::AcknowledgeLive {
                    descriptor, ids, ..
                } => {
                    descriptor.valid()
                        && ids_valid(ids)
                        && ids.iter().all(|id| descriptor.ids.contains(id))
                }
                Self::ObserveConfigured {
                    descriptor,
                    projection,
                    ..
                } => descriptor.valid() && projection.valid(),
                Self::AcknowledgeConfigured {
                    descriptor,
                    observation,
                    ids,
                    ..
                } => {
                    descriptor == &observation.descriptor
                        && observation.valid()
                        && ids_valid(ids)
                        && ids.iter().all(|id| descriptor.ids.contains(id))
                }
                Self::Capture {
                    operation,
                    ids,
                    previous,
                    ..
                } => {
                    token(operation)
                        && ids_valid(ids)
                        && previous
                            .as_ref()
                            .is_none_or(|d| d.valid() && &d.operation == operation && &d.ids == ids)
                }
                Self::Observe {
                    descriptor,
                    projection,
                    ..
                } => descriptor.valid() && projection.valid(),
                Self::Acknowledge {
                    descriptor,
                    observation,
                    ids,
                    ..
                } => {
                    descriptor.valid()
                        && descriptor == &observation.descriptor
                        && observation.valid()
                        && ids_valid(ids)
                        && ids.iter().all(|id| descriptor.ids.contains(id))
                }
            }
    }
}

impl Observation {
    pub(crate) fn valid(&self) -> bool {
        self.descriptor.valid()
            && self.projection.valid()
            && self.sequence > 0
            && self.proof.len() == 64
            && self
                .proof
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            && match &self.content {
                Content::Retired {} => true,
                Content::Current { facts } => {
                    facts.len() == self.descriptor.ids.len()
                        && facts.iter().zip(&self.descriptor.ids).all(|(f, id)| {
                            &f.request_id == id
                                && f.thread_id
                                    .as_ref()
                                    .is_none_or(|s| !s.is_empty() && s.len() <= 256)
                        })
                }
            }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ConfiguredProjection {
    pub frontier: String,
    pub snapshot: String,
    pub runtime: String,
    pub version: u64,
}
impl ConfiguredProjection {
    pub(crate) fn valid(&self) -> bool {
        token(&self.frontier) && token(&self.snapshot) && token(&self.runtime)
    }
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ConfiguredObservation {
    pub descriptor: Descriptor,
    pub projection: ConfiguredProjection,
    pub content: Content,
    pub proof: String,
}
impl ConfiguredObservation {
    pub(crate) fn valid(&self) -> bool {
        self.descriptor.valid()
            && self.projection.valid()
            && self.proof.len() == 64
            && self
                .proof
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            && match &self.content {
                Content::Retired {} => true,
                Content::Current { facts } => {
                    facts.len() == self.descriptor.ids.len()
                        && facts.iter().zip(&self.descriptor.ids).all(|(fact, id)| {
                            &fact.request_id == id
                                && fact
                                    .thread_id
                                    .as_ref()
                                    .is_none_or(|thread| !thread.is_empty() && thread.len() <= 256)
                                && matches!(
                                    fact.outcome,
                                    Outcome::Response {}
                                        | Outcome::ClientError {}
                                        | Outcome::Cancelled {}
                                        | Outcome::Withdrawn {}
                                )
                        })
                }
            }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ClosedObservation {
    pub descriptor: Descriptor,
    pub projection: Projection,
    pub ids: Vec<RequestId>,
    pub content: Content,
    pub proof: String,
}
impl ClosedObservation {
    pub(crate) fn valid(&self) -> bool {
        self.descriptor.valid()
            && self.projection.valid()
            && ids_valid(&self.ids)
            && self.ids.iter().all(|id| self.descriptor.ids.contains(id))
            && self.proof.len() == 64
            && self
                .proof
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            && match &self.content {
                Content::Retired {} => true,
                Content::Current { facts } => {
                    facts.len() == self.ids.len()
                        && facts.iter().zip(&self.ids).all(|(fact, id)| {
                            &fact.request_id == id
                                && fact
                                    .thread_id
                                    .as_ref()
                                    .is_none_or(|thread| !thread.is_empty() && thread.len() <= 256)
                                && matches!(
                                    fact.outcome,
                                    Outcome::Response {}
                                        | Outcome::ClientError {}
                                        | Outcome::Cancelled {}
                                        | Outcome::Withdrawn {}
                                )
                        })
                }
            }
    }
}

#[path = "retained_input_metadata.rs"]
pub(crate) mod input;
