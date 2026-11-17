// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! SAME Record owns the actual pre-await original Side result and outer fields.
//! Checkpoints contain UI/operation DATA only. Actual C2 Record/Native origin
//! must be installed independently before replaying the exact accepted wire.
use super::*;
use codex_app_server_client::ClientConstructionCheckpoint;
use codex_app_server_client::ClientConstructionOperation;
use codex_app_server_client::ClientPresentation;
use codex_app_server_client::SideRollbackCheckpoint;
use codex_app_server_client::SideRollbackOperation;
use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;
use serde::Deserialize;
use serde::Serialize;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::sync::Weak;
#[path = "owned_local_side_started_data.rs"]
mod started_data;
use started_data::StartedData;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
enum Storage {
    Attached,
    Temporary,
}
impl<'de> Deserialize<'de> for Storage {
    fn deserialize<D: serde::Deserializer<'de>>(reader: D) -> std::result::Result<Self, D::Error> {
        match String::deserialize(reader)?.as_str() {
            "attached" => Ok(Self::Attached),
            "temporary" => Ok(Self::Temporary),
            _ => Err(serde::de::Error::custom("unknown original Side storage")),
        }
    }
}
impl Storage {
    fn capture(value: SideChatStorage) -> Self {
        match value {
            SideChatStorage::Attached => Self::Attached,
            SideChatStorage::Temporary => Self::Temporary,
        }
    }
    fn value(self) -> SideChatStorage {
        match self {
            Self::Attached => SideChatStorage::Attached,
            Self::Temporary => SideChatStorage::Temporary,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "camelCase", deny_unknown_fields)]
enum Phase {
    Injecting {},
    InjectionObserved {
        result: std::result::Result<(), String>,
    },
    BoundaryObserved {
        result: std::result::Result<(), String>,
    },
    RollbackPending {
        boundary: std::result::Result<(), String>,
    },
    RollbackObserved {
        boundary: std::result::Result<(), String>,
        result: std::result::Result<(), String>,
    },
    SidebarObserved {
        boundary: std::result::Result<(), String>,
    },
    Unknown {},
}
/// Required Selected.application leaf for every actual pending Side creation.
/// Never serialize a Core, Controller, callback, Incoming, replay or SDK grant.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SideCreationCheckpoint {
    schema: u8,
    operation: ClientConstructionCheckpoint,
    original: StartedData,
    storage: Storage,
    inherited_profile: bool,
    legacy: bool,
    items: Vec<serde_json::Value>,
    phase: Phase,
    #[serde(deserialize_with = "required_rollback")]
    rollback: Option<SideRollbackCheckpoint>,
}
pub(crate) struct SideCreation {
    record: Weak<crate::owned_local_frontend::ConstructionRecord>,
    operation: ClientConstructionOperation,
    original: AppServerStartedThread,
    storage: Storage,
    inherited_profile: bool,
    legacy: bool,
    items: Vec<serde_json::Value>,
    phase: Mutex<Phase>,
    rollback: Mutex<Option<SideRollbackOperation>>,
}
impl std::fmt::Debug for SideCreation {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("SideCreation(<whole original UI payload; no Native authority>)")
    }
}
impl SideCreation {
    pub(crate) fn capture(
        record: &Arc<crate::owned_local_frontend::ConstructionRecord>,
        operation: ClientConstructionOperation,
        mut original: AppServerStartedThread,
        storage: SideChatStorage,
        inherited_profile: bool,
        legacy: bool,
        items: Vec<serde_json::Value>,
    ) -> Arc<Self> {
        original.alab_construction = None; // Record owns this payload; no back-link cycle.
        Arc::new(Self {
            record: Arc::downgrade(record),
            operation,
            original,
            storage: Storage::capture(storage),
            inherited_profile,
            legacy,
            items,
            phase: Mutex::new(Phase::Injecting {}),
            rollback: Mutex::new(None),
        })
    }
    pub(crate) fn matches(
        &self,
        original: &AppServerStartedThread,
        storage: SideChatStorage,
        inherited_profile: bool,
        legacy: bool,
        items: &[serde_json::Value],
    ) -> bool {
        self.original.session == original.session
            && self.original.turns == original.turns
            && self.original.blocks_direct_input == original.blocks_direct_input
            && self.original.task_tools_available == original.task_tools_available
            && self.storage == Storage::capture(storage)
            && self.inherited_profile == inherited_profile
            && self.legacy == legacy
            && self.items == items
    }
    pub(crate) fn checkpoint(&self) -> SideCreationCheckpoint {
        // Operation observation precedes every phase advance. Lock phase FIRST
        // so capture may be conservative (newer operation/older phase), never
        // falsely complete with an older pre-send operation checkpoint.
        let phase = self.phase.lock().unwrap_or_else(PoisonError::into_inner);
        let operation = self.operation.checkpoint();
        SideCreationCheckpoint {
            schema: 1,
            operation,
            original: StartedData::capture(&self.original),
            storage: self.storage,
            inherited_profile: self.inherited_profile,
            legacy: self.legacy,
            items: self.items.clone(),
            phase: phase.clone(),
            rollback: self
                .rollback
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_ref()
                .map(SideRollbackOperation::checkpoint),
        }
    }
    fn injection(&self, result: std::result::Result<(), String>) {
        *self.phase.lock().unwrap_or_else(PoisonError::into_inner) =
            Phase::InjectionObserved { result };
    }
    pub(crate) fn boundary(&self, result: &std::result::Result<(), String>) {
        *self.phase.lock().unwrap_or_else(PoisonError::into_inner) = Phase::BoundaryObserved {
            result: result.clone(),
        };
    }
    fn begin_rollback(
        &self,
        boundary: &std::result::Result<(), String>,
    ) -> Result<SideRollbackOperation> {
        if boundary.is_ok() || self.storage != Storage::Attached {
            bail!("original Side rollback requires its actual failed attached boundary")
        }
        let operation = {
            let mut slot = self.rollback.lock().unwrap_or_else(PoisonError::into_inner);
            match &*slot {
                Some(actual) => actual.clone(),
                None => {
                    let actual = SideRollbackOperation::new(&self.operation)?;
                    *slot = Some(actual.clone());
                    actual
                }
            }
        };
        *self.phase.lock().unwrap_or_else(PoisonError::into_inner) = Phase::RollbackPending {
            boundary: boundary.clone(),
        };
        Ok(operation)
    }
    pub(crate) fn rollback(
        &self,
        boundary: &std::result::Result<(), String>,
        result: std::result::Result<(), String>,
    ) {
        *self.phase.lock().unwrap_or_else(PoisonError::into_inner) = Phase::RollbackObserved {
            boundary: boundary.clone(),
            result,
        };
    }
    pub(crate) fn sidebar(&self, boundary: &std::result::Result<(), String>) {
        *self.phase.lock().unwrap_or_else(PoisonError::into_inner) = Phase::SidebarObserved {
            boundary: boundary.clone(),
        };
    }
    pub(crate) fn restore(
        record: &Arc<crate::owned_local_frontend::ConstructionRecord>,
        operation: ClientConstructionOperation,
        data: SideCreationCheckpoint,
    ) -> Result<Arc<Self>> {
        if data.schema != 1
            || serde_json::to_value(operation.checkpoint())?
                != serde_json::to_value(&data.operation)?
        {
            bail!("pending Side creation changed its exact restored operation");
        }
        let original = data.original.restore()?;
        let observed = operation
            .observed_result()
            .ok_or_else(|| eyre!("Side checkpoint lost original typed fork observation"))?;
        if observed.thread_id != original.session.thread_id.to_string()
            || observed.backend_task_tools != original.task_tools_available
        {
            bail!("Side checkpoint changed original converted result");
        }
        if matches!(
            data.phase,
            Phase::RollbackPending { .. } | Phase::RollbackObserved { .. }
        ) && data.rollback.is_none()
        {
            bail!("Side checkpoint lost its actual rollback operation")
        }
        let rollback = data
            .rollback
            .map(|checkpoint| SideRollbackOperation::restore(&operation, checkpoint))
            .transpose()?;
        let result = Arc::new(Self {
            record: Arc::downgrade(record),
            operation,
            original,
            storage: data.storage,
            inherited_profile: data.inherited_profile,
            legacy: data.legacy,
            items: data.items,
            phase: Mutex::new(data.phase),
            rollback: Mutex::new(rollback),
        });
        Ok(result)
    }
    /// Only an actually observed whole original object can be handed back here.
    /// In-flight injection is replayed separately through SAME operation/wire;
    /// no pending rollback is guessed successful or resubmitted from bytes.
    pub(crate) fn observed_sidebar(&self) -> Result<Option<SidebarFork>> {
        let phase = self.phase.lock().unwrap_or_else(PoisonError::into_inner);
        let Phase::SidebarObserved { boundary } = &*phase else {
            return Ok(None);
        };
        let mut thread = self.original.clone();
        thread.alab_construction = Some(
            self.record
                .upgrade()
                .ok_or_else(|| eyre!("actual Side caller owner unavailable"))?,
        );
        Ok(Some(SidebarFork {
            alab_retained_replay: None,
            thread,
            boundary: boundary.clone(),
            storage: self.storage.value(),
            inherited_profile: self.inherited_profile,
            legacy: self.legacy,
        }))
    }
}
impl AppServerSession {
    /// Normal function captures the SAME full original object/outer fields and
    /// actual owner BEFORE returning any fallible/pending injection future.
    pub(crate) fn alab_original_side_inject<'a>(
        &'a mut self,
        original: &'a AppServerStartedThread,
        storage: SideChatStorage,
        inherited_profile: bool,
        items: Vec<ResponseItem>,
    ) -> impl std::future::Future<Output = Result<ThreadInjectItemsResponse>> + Send + use<'a> {
        let captured = original
            .alab_construction
            .as_ref()
            .map(|record| {
                let wire_items = items
                    .iter()
                    .map(serde_json::to_value)
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                record.capture_side_creation(
                    original.clone(),
                    storage,
                    inherited_profile,
                    self.history_support == ThreadHistorySupport::LegacyOnly,
                    wire_items,
                )
            })
            .transpose();
        let child = original.session.thread_id;
        crate::boxed_future!(async move {
            let captured = captured?;
            let result = if let Some(captured) = &captured {
                captured
                    .operation
                    .run(codex_app_server_client::scope_original_side_injection(
                        self.thread_inject_items(child, items),
                    ))
                    .await
            } else {
                self.thread_inject_items(child, items).await
            };
            if let Some(captured) = captured {
                captured.injection(result.as_ref().map(|_| ()).map_err(ToString::to_string));
            }
            result
        })
    }
}
pub(crate) fn owner(original: &AppServerStartedThread) -> Option<Arc<SideCreation>> {
    original
        .alab_construction
        .as_ref()
        .and_then(|record| record.side_creation())
}

/// Preserve ordinary original checkpoint policy. Owned Side uses ONLY the
/// result of its actual accepted injection/canonical recording; no foreground
/// file scan, timestamp/text match or post-RPC inferred cutoff is performed.
pub(crate) async fn checkpoint_policy(
    original: &AppServerStartedThread,
    ordinary: impl std::future::Future<Output = std::io::Result<Option<String>>>,
) -> std::io::Result<Option<String>> {
    let Some(actual) = owner(original) else {
        return ordinary.await;
    };
    let (thread, occurrence) = actual
        .operation
        .side_boundary_observation_data()
        .ok_or_else(|| std::io::Error::other("actual accepted Side recording result missing"))?;
    if thread != original.session.thread_id.to_string() {
        return Err(std::io::Error::other(
            "Side recording changed original child",
        ));
    }
    Ok(Some(occurrence))
}

impl SideCreationCheckpoint {
    pub(crate) fn operation_checkpoint(&self) -> &ClientConstructionCheckpoint {
        &self.operation
    }
    pub(crate) fn original_started_data(&self) -> Result<AppServerStartedThread> {
        self.original.clone().restore()
    }
}

impl SideCreation {
    /// Continue ONLY this already-built original UI object. Replaying a lost
    /// injection observation uses its exact frozen wire and backend-owned worker;
    /// it never forks/resumes Core or rebuilds Start/Fork policy.
    pub(crate) async fn resume_original(
        self: &Arc<Self>,
        view: &mut AppServerSession,
    ) -> Result<SidebarFork> {
        self.observe_rollback(view).await;
        if let Some(observed) = self.observed_sidebar()? {
            return Ok(observed);
        }
        let phase = self
            .phase
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let boundary = match phase {
            Phase::InjectionObserved {
                result: Err(message),
            } => Err(message),
            Phase::InjectionObserved { result: Ok(()) } => {
                let outcome = if self.storage == Storage::Attached {
                    checkpoint_policy(&self.original, std::future::ready(Ok(None)))
                        .await
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                } else {
                    Ok(())
                };
                self.boundary(&outcome);
                outcome
            }
            Phase::Injecting {} => {
                let items = self
                    .items
                    .iter()
                    .cloned()
                    .map(serde_json::from_value)
                    .collect::<std::result::Result<Vec<ResponseItem>, _>>()?;
                let injected = self
                    .operation
                    .run(codex_app_server_client::scope_original_side_injection(
                        view.thread_inject_items(self.original.session.thread_id, items),
                    ))
                    .await;
                let outcome = match injected {
                    Ok(_) if self.storage == Storage::Attached => {
                        checkpoint_policy(&self.original, std::future::ready(Ok(None)))
                            .await
                            .map(|_| ())
                            .map_err(|e| e.to_string())
                    }
                    result => result.map(|_| ()).map_err(|e| e.to_string()),
                };
                self.boundary(&outcome);
                outcome
            }
            Phase::BoundaryObserved { result } => result,
            Phase::RollbackObserved {
                boundary,
                result: Err(_),
            } => boundary,
            Phase::RollbackObserved {
                boundary: Err(message),
                result: Ok(()),
            } => bail!(
                "Failed to prepare side conversation {}: {}",
                self.original.session.thread_id,
                message
            ),
            Phase::RollbackObserved {
                boundary: Ok(()), ..
            } => bail!("Side checkpoint contains rollback without original boundary error"),
            Phase::RollbackPending { boundary } => boundary, // SAME stored operation replays observation, never a second delete.
            Phase::Unknown {} => bail!("original Side creation remains owned and unobserved"),
            Phase::SidebarObserved { .. } => bail!("whole observed Side object changed its owner"),
        };
        if boundary.is_err() && self.storage == Storage::Attached {
            let phase = self
                .phase
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone();
            if !matches!(phase, Phase::RollbackObserved { .. }) {
                let result = view
                    .alab_original_side_rollback(&self.original, &boundary)
                    .await;
                self.rollback(
                    &boundary,
                    result.as_ref().map(|_| ()).map_err(ToString::to_string),
                );
                self.observe_rollback(view).await;
                if result.is_ok() {
                    bail!(
                        "Failed to prepare side conversation {}: {}",
                        self.original.session.thread_id,
                        boundary
                            .as_ref()
                            .err()
                            .map(String::as_str)
                            .unwrap_or("original boundary failed")
                    )
                }
            }
        }
        self.sidebar(&boundary);
        self.observed_sidebar()?
            .ok_or_else(|| eyre!("whole original Side object unavailable"))
    }
}

#[cfg(test)]
#[path = "owned_local_side_creation_tests.rs"]
mod tests;

fn required_rollback<'de, D: serde::Deserializer<'de>>(
    reader: D,
) -> std::result::Result<Option<SideRollbackCheckpoint>, D::Error> {
    Option::deserialize(reader)
}
impl SideCreation {
    pub(super) async fn observe_rollback(&self, view: &AppServerSession) {
        let captured = matches!(
            *self.phase.lock().unwrap_or_else(PoisonError::into_inner),
            Phase::RollbackObserved { .. } | Phase::SidebarObserved { .. }
        );
        if !captured {
            return;
        }
        let operation = self
            .rollback
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if let Some(operation) = operation
            && let Err(error) = operation.observe_captured(&view.request_handle()).await
        {
            tracing::warn!(%error,"original Side rollback observation remains retained");
        }
    }
}
impl AppServerSession {
    pub(crate) fn alab_original_side_rollback(
        &mut self,
        original: &AppServerStartedThread,
        boundary: &std::result::Result<(), String>,
    ) -> impl std::future::Future<Output = Result<()>> + Send + use<> {
        let captured = owner(original)
            .map(|actual| actual.begin_rollback(boundary))
            .transpose();
        let child = original.session.thread_id;
        let mut view = self.request_view();
        crate::boxed_future!(async move {
            match captured? {
                Some(operation) => operation.run(view.thread_delete(child)).await,
                None => view.thread_delete(child).await,
            }
        })
    }
}
