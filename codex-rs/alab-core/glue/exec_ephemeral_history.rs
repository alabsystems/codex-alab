// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Canonical process-local history for an exec runtime without a live writer.
//! Recording does not serialize, persist, authorize, or replay any item. In
//! particular, compaction must not erase the original turn's lifecycle evidence.

use crate::session::session::Session;
use codex_alab_core::quota::ExecQuotaMember;
use codex_alab_core::quota::HeadlessExec;
use codex_extension_api::ExtensionData;
use codex_history::RolloutItem;
use codex_thread_store::StoredThreadHistory;
use codex_thread_store::ThreadStoreError;
use codex_thread_store::ThreadStoreResult;
use serde::Deserialize;
use serde::Serialize;
use std::sync::Mutex;

#[path = "exec_memory_codec.rs"]
pub(crate) mod codec;
#[path = "exec_memory_records.rs"]
mod records;

#[path = "exec_suspend_persistence.rs"]
mod suspension;
pub(crate) use suspension::open_turn;
pub(crate) use suspension::prepare as prepare_suspension;

/// Private, lossless history data. This is not an execution grant or a complete
/// runtime/configuration snapshot; restoration still needs native admission.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecEphemeralHistory {
    version: u8,
    thread: codex_protocol::ThreadId,
    items: Vec<records::MemoryRecord>,
    context: serde_json::Value,
}

impl std::fmt::Debug for ExecEphemeralHistory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExecEphemeralHistory")
            .field("thread", &self.thread)
            .field("items", &self.items.len())
            .finish_non_exhaustive()
    }
}

impl ExecEphemeralHistory {
    /// Seed a fresh ephemeral runtime without consulting persistent history.
    /// This prepares data only; Core still requires live admission to execute
    /// the original turn, and the caller supplies validated configuration.
    pub fn prepare_start(
        self,
        options: crate::StartThreadOptions,
        thread: codex_protocol::ThreadId,
        turn: &str,
    ) -> Result<crate::StartThreadOptions, String> {
        self.prepare_start_for(options, thread, Some(turn), true)
    }

    pub fn prepare_idle_start(
        self,
        options: crate::StartThreadOptions,
        thread: codex_protocol::ThreadId,
    ) -> Result<crate::StartThreadOptions, String> {
        self.prepare_start_for(options, thread, None, true)
    }

    /// Restore idle memory under the destination host's ordinary startup policy.
    /// No source data grants headless-exec identity or turn admission.
    pub fn prepare_idle_runtime_start(
        self,
        options: crate::StartThreadOptions,
        thread: codex_protocol::ThreadId,
    ) -> Result<crate::StartThreadOptions, String> {
        self.prepare_start_for(options, thread, None, false)
    }

    fn prepare_start_for(
        self,
        mut options: crate::StartThreadOptions,
        thread: codex_protocol::ThreadId,
        turn: Option<&str>,
        exec: bool,
    ) -> Result<crate::StartThreadOptions, String> {
        if !options.config.ephemeral
            || options.reserved_thread_id.is_some()
            || !matches!(options.initial_history, codex_history::InitialHistory::New)
        {
            return Err("memory recovery requires a fresh ephemeral runtime".into());
        }
        let context =
            crate::context_manager::ContextManager::prepare_exec_context(self.context.clone())?;
        let items = self.restore_for(thread, turn)?;
        options.initial_history =
            codex_history::InitialHistory::Resumed(codex_history::ResumedHistory {
                conversation_id: thread,
                history: std::sync::Arc::new(items.clone()),
                history_revision: None,
                rollout_path: None,
            });
        if exec {
            options.thread_extension_init.insert(HeadlessExec);
            options.turn_extension_init.insert(HeadlessExec);
        }
        options
            .thread_extension_init
            .insert(RestoreContext(context));
        options.thread_extension_init.insert(Journal {
            items: Mutex::new(items),
        });
        Ok(options)
    }

    /// Check the external native origin before returning any original items.
    /// Reading this data does not authorize starting or resuming the turn.
    pub fn restore(
        self,
        thread: codex_protocol::ThreadId,
        turn: &str,
    ) -> Result<Vec<RolloutItem>, String> {
        self.restore_for(thread, Some(turn))
    }

    pub(crate) fn restore_for(
        self,
        thread: codex_protocol::ThreadId,
        turn: Option<&str>,
    ) -> Result<Vec<RolloutItem>, String> {
        if self.version != 1 || self.thread != thread {
            return Err("ephemeral history belongs to a different runtime".into());
        }
        let items = self
            .items
            .into_iter()
            .map(records::MemoryRecord::restore)
            .collect::<Result<Vec<_>, _>>()?;
        if suspension::open_turn(&items) != turn {
            return Err("ephemeral history does not match the captured turn state".into());
        }
        Ok(items)
    }
}

impl crate::CodexThread {
    /// Capture after the suspension owner has stopped this thread's producers.
    pub async fn exec_ephemeral_history(
        &self,
        turn: &str,
    ) -> Result<Option<ExecEphemeralHistory>, String> {
        self.capture_exec_memory(Some(turn)).await
    }

    pub(crate) async fn exec_idle_ephemeral_history(
        &self,
    ) -> Result<Option<ExecEphemeralHistory>, String> {
        self.capture_exec_memory(None).await
    }

    async fn capture_exec_memory(
        &self,
        turn: Option<&str>,
    ) -> Result<Option<ExecEphemeralHistory>, String> {
        if self.session.live_thread().is_some() {
            return Ok(None);
        }
        let history = history(&self.session).map_err(|error| error.to_string())?;
        if suspension::open_turn(&history.items) != turn {
            return Err("ephemeral history does not match the captured turn state".into());
        }
        let items = history
            .items
            .iter()
            .map(records::MemoryRecord::capture)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Some(ExecEphemeralHistory {
            version: 1,
            thread: history.thread_id,
            items,
            context: self
                .session
                .state
                .lock()
                .await
                .history
                .capture_exec_context()?,
        }))
    }
}

struct RestoreContext(crate::context_manager::PreparedExecContext);

// This marker records explicit ephemeral configuration, never exec admission.
struct EphemeralMemoryOwner;

pub(crate) fn prepare_owner(ephemeral: bool, init: &mut codex_extension_api::ExtensionDataInit) {
    if ephemeral {
        init.insert(EphemeralMemoryOwner);
    }
}

pub(crate) async fn restore_context_if_pending(session: &Session) {
    // This existing startup boundary runs before any task starts. Establish
    // empty history here for never-started idle members, never during capture.
    // An absent writer alone cannot turn a durable session into memory state.
    if session.live_thread().is_none()
        && owns_memory_history(&session.services.thread_extension_data)
        && session.thread_config_snapshot().await.ephemeral
    {
        session
            .services
            .thread_extension_data
            .get_or_init(Journal::default);
    }
    if let Some(context) = session
        .services
        .thread_extension_data
        .remove::<RestoreContext>()
    {
        let mut state = session.state.lock().await;
        state.history = context.0.clone().install(
            std::sync::Arc::clone(&session.item_id_generator),
            &state.history,
        );
    }
}

#[derive(Default)]
struct Journal {
    items: Mutex<Vec<RolloutItem>>,
}

pub(crate) fn record(session: &Session, items: &[RolloutItem]) {
    // No live writer is the actual persistence boundary, not a mutable CLI
    // flag. Ordinary persisted sessions retain only their canonical store.
    if session.live_thread().is_some() {
        return;
    }
    record_items(&session.services.thread_extension_data, items);
}

/// Internal sessions may intentionally use an isolated extension registry. Their
/// registered parent still owns the exec tree, so establish memory bookkeeping
/// before startup records history. This grants neither root identity nor admission.
pub(crate) fn inherit_internal_owner(
    parent: &ExtensionData,
    child: &mut codex_extension_api::ExtensionDataInit,
) {
    if parent.get::<HeadlessExec>().is_some() || parent.get::<ExecQuotaMember>().is_some() {
        child.insert(ExecQuotaMember);
    }
}

fn owns_memory_history(data: &ExtensionData) -> bool {
    data.get::<EphemeralMemoryOwner>().is_some()
        || data.get::<HeadlessExec>().is_some()
        || data.get::<ExecQuotaMember>().is_some()
}

fn record_items(data: &ExtensionData, items: &[RolloutItem]) {
    if !owns_memory_history(data) || items.is_empty() {
        return;
    }
    // These startup display notices are transient in the durable rollout
    // policy too. Keeping a fresh session's notices here would pollute the
    // exact source-history prefix while restoring an in-memory context.
    let mut canonical = items
        .iter()
        .filter(|item| {
            !matches!(
                item,
                RolloutItem::EventMsg(
                    codex_protocol::protocol::EventMsg::SessionConfigured(_)
                        | codex_protocol::protocol::EventMsg::DeprecationNotice(_)
                )
            )
        })
        .peekable();
    if canonical.peek().is_none() {
        return;
    }
    let journal = data.get_or_init(Journal::default);
    journal
        .items
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .extend(canonical.cloned());
}

pub(super) fn history(session: &Session) -> ThreadStoreResult<StoredThreadHistory> {
    let unavailable = || ThreadStoreError::Internal {
        message: "ephemeral exec has no canonical in-memory history".into(),
    };
    let data = &session.services.thread_extension_data;
    if !owns_memory_history(data) {
        return Err(unavailable());
    }
    let journal = data.get::<Journal>().ok_or_else(unavailable)?;
    let items = journal.items.lock().map_err(|_| unavailable())?.clone();
    Ok(StoredThreadHistory {
        thread_id: session.thread_id,
        items,
        // A memory journal is never evidence of a disk-store revision.
        revision: None,
    })
}

/// Canonical evidence encoded without the lossy public rollout wire format.
/// Store revisions are process-local and deliberately are not carried.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HistoryEvidence {
    thread: codex_protocol::ThreadId,
    records: Vec<records::MemoryRecord>,
}

impl HistoryEvidence {
    pub(crate) fn capture(history: &StoredThreadHistory) -> Result<Self, String> {
        Ok(Self {
            thread: history.thread_id,
            records: history
                .items
                .iter()
                .map(records::MemoryRecord::capture)
                .collect::<Result<_, _>>()?,
        })
    }
    pub(crate) fn restore(
        &self,
        thread: codex_protocol::ThreadId,
    ) -> Result<StoredThreadHistory, String> {
        if self.thread != thread {
            return Err("tree history belongs to another thread".into());
        }
        Ok(StoredThreadHistory {
            thread_id: thread,
            items: self
                .records
                .clone()
                .into_iter()
                .map(records::MemoryRecord::restore)
                .collect::<Result<_, _>>()?,
            revision: None,
        })
    }
    pub(crate) fn verify(&self, history: &StoredThreadHistory) -> Result<(), String> {
        let current = Self::capture(history)?;
        if serde_json::to_value(self).map_err(|e| e.to_string())?
            != serde_json::to_value(current).map_err(|e| e.to_string())?
        {
            return Err("tree history changed after source capture".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_history::ResponseItemEnvelope;
    use codex_protocol::models::ResponseItem;
    use codex_protocol::protocol::EventMsg;
    use codex_protocol::protocol::TurnStartedEvent;

    #[test]
    fn internal_memory_ownership_requires_an_exec_parent_without_granting_root_identity() {
        for kind in [0, 1, 2, 3] {
            let parent = ExtensionData::new("parent");
            if kind == 1 {
                parent.insert(HeadlessExec);
            } else if kind == 2 {
                parent.insert(ExecQuotaMember);
            } else if kind == 3 {
                parent.insert(EphemeralMemoryOwner);
            }
            let mut child = codex_extension_api::ExtensionDataInit::new();
            inherit_internal_owner(&parent, &mut child);
            assert_eq!(
                child.get::<ExecQuotaMember>().is_some(),
                matches!(kind, 1 | 2)
            );
            assert!(child.get::<HeadlessExec>().is_none());
        }
    }

    #[tokio::test]
    async fn missing_durable_writer_is_not_initialized_as_empty_ephemeral() {
        let (mut session, _) = crate::session::tests::make_session_and_context().await;
        session.services.live_thread = None;
        session
            .services
            .thread_extension_data
            .insert(ExecQuotaMember);
        assert!(!session.thread_config_snapshot().await.ephemeral);
        restore_context_if_pending(&session).await;
        assert!(history(&session).is_err());
    }

    #[test]
    fn idle_memory_codec_rejects_an_unfinished_or_foreign_turn() {
        let thread = codex_protocol::ThreadId::new();
        let start = RolloutItem::EventMsg(EventMsg::TurnStarted(TurnStartedEvent {
            turn_id: "original".into(),
            turn_attribution: None,
            root_turn_id: None,
            trace_id: None,
            started_at: None,
            model_context_window: None,
            collaboration_mode_kind: Default::default(),
        }));
        let complete = RolloutItem::EventMsg(EventMsg::TurnComplete(
            codex_protocol::protocol::TurnCompleteEvent {
                root_turn_id: None,
                turn_id: "original".into(),
                last_agent_message: None,
                error: None,
                started_at: None,
                completed_at: None,
                duration_ms: None,
                time_to_first_token_ms: None,
            },
        ));
        let memory = |items: &[RolloutItem]| ExecEphemeralHistory {
            version: 1,
            thread,
            context: serde_json::Value::Null,
            items: items
                .iter()
                .map(records::MemoryRecord::capture)
                .collect::<Result<_, _>>()
                .unwrap(),
        };
        assert!(memory(&[]).restore_for(thread, None).is_ok());
        assert!(
            memory(std::slice::from_ref(&start))
                .restore_for(thread, None)
                .is_err()
        );
        let finished = memory(&[start, complete]);
        assert!(finished.clone().restore_for(thread, None).is_ok());
        assert!(finished.clone().restore(thread, "original").is_err());
        assert!(
            finished
                .restore_for(codex_protocol::ThreadId::new(), None)
                .is_err()
        );
    }

    #[test]
    fn ephemeral_journal_keeps_execution_and_excludes_startup_notices() {
        let data = ExtensionData::new("memory-notices");
        data.insert(ExecQuotaMember);
        let notice = RolloutItem::EventMsg(EventMsg::DeprecationNotice(
            codex_protocol::protocol::DeprecationNoticeEvent {
                summary: "fixture startup notice".into(),
                details: None,
            },
        ));
        record_items(&data, std::slice::from_ref(&notice));
        assert!(data.get::<Journal>().is_none());
        let started = RolloutItem::EventMsg(EventMsg::TurnStarted(TurnStartedEvent {
            turn_id: "original".into(),
            turn_attribution: None,
            root_turn_id: None,
            trace_id: None,
            started_at: None,
            model_context_window: None,
            collaboration_mode_kind: Default::default(),
        }));
        record_items(&data, &[notice, started]);
        let journal = data.get::<Journal>().unwrap();
        let items = journal.items.lock().unwrap();
        assert_eq!(items.len(), 1);
        assert!(
            matches!(&items[0], RolloutItem::EventMsg(EventMsg::TurnStarted(event)) if event.turn_id == "original")
        );
    }

    #[test]
    fn only_exec_retains_items_without_serialization_or_compaction_loss() {
        let data = ExtensionData::new("ephemeral-test");
        let started = RolloutItem::EventMsg(EventMsg::TurnStarted(TurnStartedEvent {
            turn_id: "original-turn".into(),
            turn_attribution: None,
            root_turn_id: None,
            trace_id: None,
            started_at: None,
            model_context_window: None,
            collaboration_mode_kind: Default::default(),
        }));
        record_items(&data, std::slice::from_ref(&started));
        assert!(data.get::<Journal>().is_none());
        data.insert(HeadlessExec);
        record_items(&data, std::slice::from_ref(&started));
        let compacted = RolloutItem::ResponseItem(ResponseItemEnvelope {
            item: ResponseItem::Compaction {
                id: None,
                encrypted_content: "opaque-original-checkpoint".into(),
                internal_chat_message_metadata_passthrough: None,
            },
            metadata: None,
        });
        record_items(&data, std::slice::from_ref(&compacted));
        let journal = data.get::<Journal>().unwrap();
        let items = journal.items.lock().unwrap();
        assert_eq!(items.len(), 2);
        assert!(
            matches!(&items[0], RolloutItem::EventMsg(EventMsg::TurnStarted(event)) if event.turn_id == "original-turn")
        );
        assert!(
            matches!(&items[1], RolloutItem::ResponseItem(envelope) if matches!(&envelope.item, ResponseItem::Compaction { encrypted_content, .. } if encrypted_content == "opaque-original-checkpoint"))
        );
    }
}
