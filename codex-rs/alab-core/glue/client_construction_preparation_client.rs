// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Real pre-send operation ownership/checkpoint DATA. It serializes no running
//! handle, manager, callback or SDK authority. C2 must still reauthorize its
//! actual Native connection and replay the same full original policy request.
use super::*;
use metadata::preparation::Completion;
use metadata::preparation::Method;
use metadata::preparation::Offer;
use metadata::preparation::Prepare;
use metadata::preparation::Request;
use serde::Deserialize;
use serde::Serialize;
use std::sync::Arc;

impl Method {
    fn parse(name: &str) -> Option<Self> {
        match name {
            "thread/start" => Some(Self::Start),
            "thread/fork" => Some(Self::Fork),
            "thread/resume" => Some(Self::Resume),
            _ => None,
        }
    }
}

fn encode_request(request: &Request) -> IoResult<serde_json::Value> {
    if !request.valid() {
        return Err(invalid("invalid client preparation request"));
    }
    serde_json::to_value(request).map_err(std::io::Error::other)
}

/// No Start policy exists until the original typed request has been built.
/// A projected Fork/Resume owner already has its original prospective policy.
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum ConstructionIntent {
    Start,
    Projected {
        selection: Box<ClientConstructionSelection>,
    },
}
impl ConstructionIntent {
    fn selection(
        &self,
        method: Method,
        parameters: &serde_json::Value,
    ) -> IoResult<ClientConstructionSelection> {
        match self {
            Self::Start if method == Method::Start => {
                let prospective_create = serde_json::from_value(parameters.clone())
                    .map_err(|_| invalid("original typed Start parameters are invalid"))?;
                Ok(ClientConstructionSelection {
                    prospective_create,
                    presentation: ClientPresentation::Primary,
                    prepared: None,
                })
            }
            Self::Projected { selection } if method != Method::Start => {
                Ok(selection.as_ref().clone())
            }
            _ => Err(invalid(
                "client construction method disagrees with its original intent",
            )),
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClientConstructionCheckpoint {
    schema: u32,
    retired: bool,
    preparation: String,
    intent: ConstructionIntent,
    frozen: Option<Prepare>,
    offer: Option<Offer>,
    result: Option<ClientConstructionResult>,
    mutation_attempted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    side_boundary: Option<side_boundary::Checkpoint>,
}
#[derive(Clone)]
pub struct ClientConstructionOperation(Arc<Mutex<ClientConstructionCheckpoint>>);
impl ClientConstructionOperation {
    /// Retain this owner before running the entire original new-chat future.
    /// Its exact Primary policy is derived once from the first original typed
    /// Start wire, never from placeholders, defaults or a copied UI projection.
    pub fn for_start() -> Self {
        Self(Arc::new(Mutex::new(ClientConstructionCheckpoint {
            schema: 2,
            retired: false,
            preparation: fresh(),
            intent: ConstructionIntent::Start,
            frozen: None,
            offer: None,
            result: None,
            mutation_attempted: false,
            side_boundary: None,
        })))
    }
    pub fn new(mut selection: ClientConstructionSelection) -> IoResult<Self> {
        if selection.prepared.is_some() {
            return Err(invalid(
                "new client choice cannot inherit an old prepared offer",
            ));
        }
        selection.prepared = None;
        Ok(Self(Arc::new(Mutex::new(ClientConstructionCheckpoint {
            schema: 2,
            retired: false,
            preparation: fresh(),
            intent: ConstructionIntent::Projected {
                selection: Box::new(selection),
            },
            frozen: None,
            offer: None,
            result: None,
            mutation_attempted: false,
            side_boundary: None,
        }))))
    }
    pub(super) fn fresh(selection: ClientConstructionSelection) -> Self {
        // Keep an invalid inherited offer as invalid input; freeze() denies it
        // before any preflight or mutation, rather than silently clearing it.
        Self(Arc::new(Mutex::new(ClientConstructionCheckpoint {
            schema: 2,
            retired: false,
            preparation: fresh(),
            intent: ConstructionIntent::Projected {
                selection: Box::new(selection),
            },
            frozen: None,
            offer: None,
            result: None,
            mutation_attempted: false,
            side_boundary: None,
        })))
    }
    pub fn checkpoint(&self) -> ClientConstructionCheckpoint {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
    /// Local freeze occurred before any prepare/mutation send. False permits
    /// retiring only a failed frontend record, never Native/Source effects.
    pub fn original_request_started_data(&self) -> bool {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .frozen
            .is_some()
    }
    pub fn restore(checkpoint: ClientConstructionCheckpoint) -> IoResult<Self> {
        if !Self::valid_side(&checkpoint)
            || checkpoint.schema != 2
            || !metadata::preparation::token(&checkpoint.preparation)
            || matches!(&checkpoint.intent, ConstructionIntent::Projected { selection } if selection.prepared.is_some())
            || checkpoint.frozen.as_ref().is_some_and(|frozen| {
                !frozen.valid()
                    || frozen.preparation != checkpoint.preparation
                    || match checkpoint
                        .intent
                        .selection(frozen.method, &frozen.parameters)
                    {
                        Ok(selection) => frozen.selection != selection,
                        Err(_) => true,
                    }
            })
            || checkpoint
                .offer
                .as_ref()
                .is_some_and(|offer| !offer.valid() || offer.preparation != checkpoint.preparation)
            || (checkpoint.offer.is_some() && checkpoint.frozen.is_none())
            || (checkpoint.mutation_attempted && checkpoint.offer.is_none())
            || (checkpoint.result.is_some() && !checkpoint.mutation_attempted)
            || checkpoint
                .result
                .as_ref()
                .is_some_and(|result| checkpoint.offer.as_ref() != Some(&result.prepared))
        {
            return Err(invalid("invalid client construction checkpoint"));
        }
        Ok(Self(Arc::new(Mutex::new(checkpoint))))
    }
    /// Actual pending UI owner must retain this same operation BEFORE awaiting
    /// its original work. This only scopes request construction, never UI ACK.
    pub async fn run<T>(&self, work: impl std::future::Future<Output = T>) -> T {
        CLIENT_SCOPE.scope(self.clone(), work).await
    }
    pub fn observed_result(&self) -> Option<ClientConstructionResult> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .result
            .clone()
    }
    pub fn prepared_offer(&self) -> Option<Offer> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .offer
            .clone()
    }
    pub(super) fn freeze(
        &self,
        wire: &codex_app_server_protocol::JSONRPCRequest,
    ) -> IoResult<Prepare> {
        let method = Method::parse(&wire.method)
            .ok_or_else(|| invalid("client construction method changed"))?;
        let parameters = wire
            .params
            .clone()
            .ok_or_else(|| invalid("client construction parameters missing"))?;
        let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let selection = state.intent.selection(method, &parameters)?;
        let frozen = Prepare {
            preparation: state.preparation.clone(),
            method,
            parameters,
            selection,
        };
        if state.retired
            || !frozen.valid()
            || state.frozen.as_ref().is_some_and(|old| old != &frozen)
        {
            return Err(invalid(
                "client construction cannot retry changed full arguments",
            ));
        }
        state.frozen = Some(frozen.clone());
        Ok(frozen)
    }
    pub(super) async fn prepare(
        &self,
        remote: &crate::remote::RemoteAppServerRequestHandle,
        original: &codex_app_server_protocol::JSONRPCRequest,
    ) -> IoResult<Result<Offer, codex_app_server_protocol::JSONRPCErrorError>> {
        let frozen = self.freeze(original)?;
        if let Some(offer) = self.prepared_offer() {
            return Ok(Ok(offer));
        }
        let selection = Request::Prepare(Box::new(frozen.clone()));
        let wire = codex_app_server_protocol::JSONRPCRequest {
            id: RequestId::String(format!("alab-client-prepare-{}", fresh())),
            method: "config/read".into(),
            params: Some(serde_json::json!({"includeLayers":false,"_meta":{
                (metadata::preparation::KEY):encode_request(&selection)?
            }})),
            trace: original.trace.clone(),
        };
        let mut value = match remote.request_json_rpc(wire).await? {
            Ok(value) => value,
            Err(error) => return Ok(Err(error)),
        };
        let body = value
            .as_object_mut()
            .ok_or_else(|| invalid("prepared config response is not an object"))?;
        let meta = body
            .get_mut(metadata::META)
            .and_then(serde_json::Value::as_object_mut)
            .ok_or_else(|| invalid("client preparation observation missing"))?;
        let completed: Completion = serde_json::from_value(
            meta.remove(metadata::preparation::RESULT_KEY)
                .ok_or_else(|| invalid("client preparation observation missing"))?,
        )
        .map_err(|_| invalid("invalid client preparation observation"))?;
        if meta.is_empty() {
            body.remove(metadata::META);
        }
        let _config: codex_app_server_protocol::ConfigReadResponse = serde_json::from_value(value)
            .map_err(|_| {
                invalid("client preparation did not preserve the actual typed config response")
            })?;
        let Completion::Prepared(offer) = completed else {
            return Err(invalid("client preparation was not admitted"));
        };
        if !offer.valid() || offer.preparation != frozen.preparation {
            return Err(invalid("client preparation observation disagrees"));
        }
        {
            let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
            if state.frozen.as_ref() != Some(&frozen)
                || state.offer.as_ref().is_some_and(|old| old != &offer)
            {
                return Err(invalid("client preparation changed its original operation"));
            }
            // Stored BEFORE any original mutating wire send. A restored C2
            // presents this SAME backend claim; Source never starts it twice.
            state.offer = Some(offer.clone());
        }
        Ok(Ok(offer))
    }
    /// Recover only SAME immutable preparation DATA, then ask the actual
    /// backend to retire an UNACCEPTED Native reservation. This never retries
    /// the original mutation or cancels a server-owned accepted worker.
    pub async fn retire_unaccepted(
        &self,
        handle: &AppServerRequestHandle,
    ) -> IoResult<ClientPreparationRetirement> {
        let AppServerRequestHandle::Remote(remote) = handle else {
            return Err(invalid(
                "client preparation retirement requires actual physical client",
            ));
        };
        let checkpoint = self.checkpoint();
        if checkpoint.retired {
            return Ok(ClientPreparationRetirement::Retired);
        }
        let Some(frozen) = checkpoint.frozen else {
            return Ok(ClientPreparationRetirement::NotPrepared);
        };
        let offer = match checkpoint.offer {
            Some(offer) => offer,
            None => {
                let original = codex_app_server_protocol::JSONRPCRequest {
                    id: RequestId::String(format!("alab-client-recover-{}", fresh())),
                    method: frozen.method.name().into(),
                    params: Some(frozen.parameters.clone()),
                    trace: None,
                };
                self.prepare(remote, &original)
                    .await?
                    .map_err(|_| invalid("preparation recovery was refused"))?
            }
        };
        let request = Request::Retire {
            preparation: offer.preparation.clone(),
            claim: offer.claim.clone(),
        };
        let wire = codex_app_server_protocol::JSONRPCRequest {
            id: RequestId::String(format!("alab-client-retire-{}", fresh())),
            method: "config/read".into(),
            params: Some(serde_json::json!({"includeLayers":false,"_meta":{
                (metadata::preparation::KEY):encode_request(&request)?
            }})),
            trace: None,
        };
        let value = remote
            .request_json_rpc(wire)
            .await?
            .map_err(|_| invalid("preparation retirement was refused"))?;
        let actual: Completion = serde_json::from_value(
            value
                .get("_meta")
                .and_then(|meta| meta.get(metadata::preparation::RESULT_KEY))
                .cloned()
                .ok_or_else(|| invalid("preparation retirement observation missing"))?,
        )
        .map_err(|_| invalid("invalid preparation retirement observation"))?;
        match actual {
            Completion::Retired { preparation, claim }
                if preparation == offer.preparation && claim == offer.claim =>
            {
                let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
                if state.offer.as_ref() != Some(&offer) {
                    return Err(invalid("preparation retirement owner changed"));
                }
                state.retired = true;
                Ok(ClientPreparationRetirement::Retired)
            }
            Completion::Accepted { preparation, claim }
                if preparation == offer.preparation && claim == offer.claim =>
            {
                Ok(ClientPreparationRetirement::Accepted)
            }
            _ => Err(invalid("preparation retirement changed original operation")),
        }
    }
    pub(super) fn begin_mutation(&self, offer: &Offer) -> IoResult<ClientConstructionSelection> {
        let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if state.retired || state.offer.as_ref() != Some(offer) {
            return Err(invalid("client prepared owner changed"));
        }
        state.mutation_attempted = true;
        let mut selection = state
            .frozen
            .as_ref()
            .ok_or_else(|| invalid("client prepared owner lost its frozen policy"))?
            .selection
            .clone();
        selection.prepared = Some(offer.clone());
        Ok(selection)
    }
    pub(super) fn record(&self, result: ClientConstructionResult) -> IoResult<()> {
        let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if !state.mutation_attempted
            || state.offer.as_ref() != Some(&result.prepared)
            || state.result.as_ref().is_some_and(|old| old != &result)
        {
            return Err(invalid("client construction completion changed"));
        }
        state.result = Some(result);
        Ok(())
    }
}
fn fresh() -> String {
    format!(
        "{:016x}{:016x}",
        codex_alab_core::native_source::connection_request_prefix(),
        codex_alab_core::native_source::connection_request_prefix()
    )
}

/// Scope is Source preparation only, never all-effects or Native reader cleanup.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ClientPreparationRetirement {
    NotPrepared,
    Retired,
    Accepted,
}
#[cfg(test)]
#[path = "client_construction_preparation_client_tests.rs"]
mod tests;

impl ClientConstructionOperation {
    fn side_selection(
        state: &ClientConstructionCheckpoint,
    ) -> IoResult<side_boundary::side_metadata::Selection> {
        if state.retired
            || !state.mutation_attempted
            || !matches!(&state.intent,ConstructionIntent::Projected{selection}
                if selection.presentation==ClientPresentation::SideConversation)
            || state
                .frozen
                .as_ref()
                .is_none_or(|x| x.method != Method::Fork)
        {
            return Err(invalid(
                "Side injection is not the original accepted Side fork intent",
            ));
        }
        let result = state
            .result
            .as_ref()
            .ok_or_else(|| invalid("original Side fork result has not been observed"))?;
        Ok(side_boundary::side_metadata::Selection {
            schema: 1,
            claim: result.prepared.claim.clone(),
        })
    }
    fn valid_side(state: &ClientConstructionCheckpoint) -> bool {
        let Some(side) = &state.side_boundary else {
            return true;
        };
        let Ok(selection) = Self::side_selection(state) else {
            return false;
        };
        let Ok(params) = serde_json::from_value::<codex_app_server_protocol::ThreadInjectItemsParams>(
            side.parameters.clone(),
        ) else {
            return false;
        };
        state
            .result
            .as_ref()
            .is_some_and(|result| result.thread_id == params.thread_id)
            && side.result.as_ref().is_none_or(|complete| {
                complete.valid()
                    && complete.claim == selection.claim
                    && complete.thread_id == params.thread_id
            })
    }
    pub(super) fn freeze_side(
        &self,
        wire: &codex_app_server_protocol::JSONRPCRequest,
    ) -> IoResult<side_boundary::side_metadata::Selection> {
        if wire.method != "thread/inject_items" {
            return Err(invalid("original Side injection method changed"));
        }
        let parameters = wire
            .params
            .clone()
            .ok_or_else(|| invalid("original Side injection params missing"))?;
        let params: codex_app_server_protocol::ThreadInjectItemsParams =
            serde_json::from_value(parameters.clone())
                .map_err(|_| invalid("original typed Side injection params invalid"))?;
        let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let selection = Self::side_selection(&state)?;
        if state
            .result
            .as_ref()
            .is_none_or(|result| result.thread_id != params.thread_id)
            || state
                .side_boundary
                .as_ref()
                .is_some_and(|old| old.parameters != parameters)
        {
            return Err(invalid(
                "Side injection cannot change original Core or full parameters",
            ));
        }
        if state.side_boundary.is_none() {
            state.side_boundary = Some(side_boundary::Checkpoint {
                parameters,
                result: None,
            });
        }
        Ok(selection)
    }
    pub(super) fn record_side(
        &self,
        complete: side_boundary::side_metadata::Completion,
    ) -> IoResult<()> {
        let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let selection = Self::side_selection(&state)?;
        let thread = state
            .result
            .as_ref()
            .ok_or_else(|| invalid("Side fork observation missing"))?
            .thread_id
            .clone();
        let side = state
            .side_boundary
            .as_mut()
            .ok_or_else(|| invalid("Side injection was not frozen before send"))?;
        if !complete.valid()
            || complete.claim != selection.claim
            || complete.thread_id != thread
            || side.result.as_ref().is_some_and(|old| old != &complete)
        {
            return Err(invalid(
                "Side injection observation changed original occurrence",
            ));
        }
        side.result = Some(complete);
        Ok(())
    }
}

impl ClientConstructionOperation {
    /// Exact original injection result DATA, never Native/currentness proof.
    pub fn side_boundary_observation_data(&self) -> Option<(String, String)> {
        let state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let complete = state.side_boundary.as_ref()?.result.as_ref()?;
        complete
            .valid()
            .then(|| (complete.thread_id.clone(), complete.occurrence.clone()))
    }
}

#[cfg(test)]
#[path = "retained_side_boundary_client_tests.rs"]
mod side_boundary_tests;

impl ClientConstructionOperation {
    pub(crate) fn side_rollback_data(&self) -> IoResult<(String, String)> {
        let p = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let frozen = p
            .frozen
            .as_ref()
            .filter(|frozen| {
                frozen.method == Method::Fork
                    && frozen.selection.presentation == ClientPresentation::SideConversation
            })
            .ok_or_else(|| invalid("original rollback requires observed Side Fork policy"))?;
        let result = p
            .result
            .as_ref()
            .filter(|result| {
                p.offer.as_ref() == Some(&result.prepared) && p.mutation_attempted && !p.retired
            })
            .ok_or_else(|| invalid("original Side Fork observation missing"))?;
        let _ = frozen;
        Ok((result.prepared.claim.clone(), result.thread_id.clone()))
    }
}

#[cfg(test)]
impl ClientConstructionOperation {
    pub(crate) fn side_codec_fixture() -> Result<Self, Box<dyn std::error::Error>> {
        side_boundary_tests::operation()
    }
}

impl ClientConstructionOperation {
    /// Observed construction DATA only. Server close still authenticates the
    /// actual Native A and selects only an existing private Scope/Core owner.
    pub fn side_view_data(&self) -> Option<crate::SideView> {
        let p = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let frozen = p.frozen.as_ref()?;
        if frozen.selection.presentation != ClientPresentation::SideConversation {
            return None;
        }
        let result = p.result.as_ref()?;
        if p.offer.as_ref() != Some(&result.prepared) || !p.mutation_attempted || p.retired {
            return None;
        }
        Some(crate::SideView {
            schema: 1,
            claim: result.prepared.claim.clone(),
            thread_id: result.thread_id.clone(),
        })
    }
}
