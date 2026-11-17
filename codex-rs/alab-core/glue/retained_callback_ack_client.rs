// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
use super::*;
/// Source OWN closed endpoint DATA under a genuine configured/NoPrimary receipt.
/// It supplies no Core, App permission, Native Go or Hold release.
pub struct ConfiguredCallbackObservation {
    observation: metadata::ConfiguredObservation,
}
impl ConfiguredCallbackObservation {
    pub fn facts(&self) -> Option<&[Fact]> {
        match &self.observation.content {
            metadata::Content::Current { facts } => Some(facts),
            metadata::Content::Retired {} => None,
        }
    }
    pub fn ticket_retired(&self) -> bool {
        matches!(self.observation.content, metadata::Content::Retired {})
    }
}
impl CallbackCapture {
    /// Only AFTER the actual same-original-A App handler consumed/dismissed the
    /// exact views. Decoding, transport completion and flags do not apply them.
    pub async fn acknowledge_live(
        &self,
        handle: &AppServerRequestHandle,
        original: ClientRequest,
        ids: Vec<RequestId>,
    ) -> Result<CallbackRetirement, TypedRequestError> {
        let request = metadata::Request::AcknowledgeLive {
            service: self.service(),
            descriptor: self.descriptor.clone(),
            ids: ids.clone(),
        };
        live_reply(
            send(handle, original, &request).await?,
            &self.descriptor,
            &ids,
        )
    }
    pub async fn observe_configured(
        &self,
        handle: &AppServerRequestHandle,
        original: ClientRequest,
        receipt: &crate::retained_configured::Receipt,
    ) -> Result<ConfiguredCallbackObservation, TypedRequestError> {
        let projection = configured(receipt);
        let request = metadata::Request::ObserveConfigured {
            service: self.service(),
            descriptor: self.descriptor.clone(),
            projection: projection.clone(),
        };
        let metadata::Reply::ObservedConfigured { observation } =
            send(handle, original, &request).await?
        else {
            return Err(invalid("configured callback reply kind changed"));
        };
        if !observation.valid()
            || observation.descriptor != self.descriptor
            || observation.projection != projection
        {
            return Err(invalid("configured callback reply correlation changed"));
        }
        Ok(ConfiguredCallbackObservation { observation })
    }
    /// Actual App reconciliation +the real ConfigurationCommit are required;
    /// this attests only endpoint/UI cleanup and never releases automatic work.
    pub async fn acknowledge_configured(
        &self,
        handle: &AppServerRequestHandle,
        original: ClientRequest,
        observation: &ConfiguredCallbackObservation,
        commit: &crate::retained_configured::ConfigurationCommit,
        ids: Vec<RequestId>,
    ) -> Result<CallbackRetirement, TypedRequestError> {
        if observation.observation.descriptor != self.descriptor
            || observation.observation.projection != configured(commit.receipt())
        {
            return Err(invalid(
                "configured callback acknowledgement changed actual receipt",
            ));
        }
        let request = metadata::Request::AcknowledgeConfigured {
            service: self.service(),
            descriptor: self.descriptor.clone(),
            observation: observation.observation.clone(),
            ids: ids.clone(),
        };
        live_reply(
            send(handle, original, &request).await?,
            &self.descriptor,
            &ids,
        )
    }
    pub async fn observe_closed_runtime(
        &self,
        handle: &AppServerRequestHandle,
        original: ClientRequest,
        receipt: &crate::retained_frontier::Receipt,
        ids: Vec<RequestId>,
    ) -> Result<ClosedCallbackObservation, TypedRequestError> {
        self.observe_closed(
            handle,
            original,
            projection(
                receipt.frontier_data(),
                receipt.snapshot_data(),
                receipt.thread_id_data(),
            ),
            ids,
        )
        .await
    }
    pub async fn observe_closed_source(
        &self,
        handle: &AppServerRequestHandle,
        original: ClientRequest,
        receipt: &crate::source_selection::Receipt,
        ids: Vec<RequestId>,
    ) -> Result<ClosedCallbackObservation, TypedRequestError> {
        self.observe_closed(
            handle,
            original,
            projection(
                receipt.frontier_data(),
                receipt.snapshot_data(),
                receipt.thread_id_data(),
            ),
            ids,
        )
        .await
    }
    async fn observe_closed(
        &self,
        handle: &AppServerRequestHandle,
        original: ClientRequest,
        projection: metadata::Projection,
        ids: Vec<RequestId>,
    ) -> Result<ClosedCallbackObservation, TypedRequestError> {
        let request = metadata::Request::ObserveClosed {
            service: self.service(),
            descriptor: self.descriptor.clone(),
            projection: projection.clone(),
            ids: ids.clone(),
        };
        let metadata::Reply::ObservedClosed { observation } =
            send(handle, original, &request).await?
        else {
            return Err(invalid("closed callback reply kind changed"));
        };
        if !observation.valid()
            || observation.descriptor != self.descriptor
            || observation.projection != projection
            || observation.ids != ids
        {
            return Err(invalid(
                "closed callback cut changed captured/current identity",
            ));
        }
        Ok(ClosedCallbackObservation { observation })
    }
    pub async fn acknowledge_closed_runtime(
        &self,
        handle: &AppServerRequestHandle,
        original: ClientRequest,
        observation: &ClosedCallbackObservation,
        activation: &crate::retained_frontier::Activation,
        ids: Vec<RequestId>,
    ) -> Result<CallbackRetirement, TypedRequestError> {
        let r = activation.receipt();
        self.acknowledge_closed(
            handle,
            original,
            observation,
            projection(r.frontier_data(), r.snapshot_data(), r.thread_id_data()),
            ids,
        )
        .await
    }
    pub async fn acknowledge_closed_source(
        &self,
        handle: &AppServerRequestHandle,
        original: ClientRequest,
        observation: &ClosedCallbackObservation,
        activation: &crate::source_selection::Activation,
        ids: Vec<RequestId>,
    ) -> Result<CallbackRetirement, TypedRequestError> {
        let r = activation.receipt();
        self.acknowledge_closed(
            handle,
            original,
            observation,
            projection(r.frontier_data(), r.snapshot_data(), r.thread_id_data()),
            ids,
        )
        .await
    }
    async fn acknowledge_closed(
        &self,
        handle: &AppServerRequestHandle,
        original: ClientRequest,
        observation: &ClosedCallbackObservation,
        projection: metadata::Projection,
        ids: Vec<RequestId>,
    ) -> Result<CallbackRetirement, TypedRequestError> {
        if observation.observation.descriptor != self.descriptor
            || observation.observation.projection != projection
        {
            return Err(invalid(
                "closed callback acknowledgement changed its current App frontier",
            ));
        }
        let request = metadata::Request::AcknowledgeClosed {
            service: self.service(),
            descriptor: self.descriptor.clone(),
            observation: observation.observation.clone(),
            ids: ids.clone(),
        };
        live_reply(
            send(handle, original, &request).await?,
            &self.descriptor,
            &ids,
        )
    }
}
fn configured(receipt: &crate::retained_configured::Receipt) -> metadata::ConfiguredProjection {
    metadata::ConfiguredProjection {
        frontier: receipt.frontier_data().into(),
        snapshot: receipt.snapshot_data().into(),
        runtime: receipt.runtime_data().into(),
        version: receipt.slot_version_data(),
    }
}
fn live_reply(
    reply: metadata::Reply,
    expected: &metadata::Descriptor,
    ids: &[RequestId],
) -> Result<CallbackRetirement, TypedRequestError> {
    let metadata::Reply::LiveAcknowledged {
        descriptor,
        ids: actual,
        retired,
    } = reply
    else {
        return Err(invalid("callback semantic retirement reply kind changed"));
    };
    if &descriptor != expected || actual != ids {
        return Err(invalid("callback semantic retirement correlation changed"));
    }
    Ok(CallbackRetirement {
        ids: actual,
        retired,
    })
}

/// Immutable OWN closed endpoint facts; current App proof does not make the
/// old endpoint's Core current or grant Core/effects/model permission.
pub struct ClosedCallbackObservation {
    observation: metadata::ClosedObservation,
}
impl ClosedCallbackObservation {
    pub fn facts(&self) -> Option<&[Fact]> {
        match &self.observation.content {
            metadata::Content::Current { facts } => Some(facts),
            metadata::Content::Retired {} => None,
        }
    }
    pub fn ticket_retired(&self) -> bool {
        matches!(self.observation.content, metadata::Content::Retired {})
    }
}
