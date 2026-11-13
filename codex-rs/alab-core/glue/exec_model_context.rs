// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Exact model-context state for a native, nonpersistent handoff. Runtime
//! cancellation tokens and process-local authorization revisions are renewed.

use super::*;
use crate::session::alab_exec_recovery_options::ephemeral::codec::MemoryItem;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    version: u32,
    items: Vec<MemoryItem>,
    sampling: (u64, u64),
    restored_through_sequence: u64,
    review_budgeting: Budgeting,
    rejected_audio_file_ids: BTreeSet<String>,
    file_audio_durations: BTreeMap<String, std::time::Duration>,
    guardian_include_additional_context: bool,
    review_history: Option<Vec<MemoryItem>>,
    review_generation: Option<u64>,
    retained_context: RetainedContext,
    review_mode: ReviewMode,
    retain_inherited_user_messages: bool,
    history_version: u64,
    reset_version: u64,
    user_message_revision: u64,
    token_info: Option<TokenUsageInfo>,
    last_server_reported_usage_item_count: usize,
    last_model_output_item_count: Option<usize>,
    reference_context_item: Option<TurnContextItem>,
    world_state_baseline: Option<WorldStateSnapshot>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ReviewMode {
    Legacy,
    Independent,
    ThreadOwned,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
enum Budgeting {
    Fixed,
    Elastic,
}
impl From<ContextBudgeting> for Budgeting {
    fn from(value: ContextBudgeting) -> Self {
        match value {
            ContextBudgeting::Fixed => Self::Fixed,
            ContextBudgeting::Elastic => Self::Elastic,
        }
    }
}
impl From<Budgeting> for ContextBudgeting {
    fn from(value: Budgeting) -> Self {
        match value {
            Budgeting::Fixed => Self::Fixed,
            Budgeting::Elastic => Self::Elastic,
        }
    }
}

/// Only validated data is retained here. Installation supplies the real new runtime's generator.
#[derive(Clone)]
pub(crate) struct PreparedExecContext {
    snapshot: Snapshot,
    items: Arc<Vec<ResponseItemEnvelope>>,
    review_history: Option<TranscriptHistory>,
    sampling: super::super::sampling::SamplingProgress,
    file_audio_durations: Arc<FileAudioDurations>,
}
impl PreparedExecContext {
    pub(crate) fn install(
        self,
        item_id_generator: ItemIdGenerator,
        fresh: &ContextManager,
    ) -> ContextManager {
        let snapshot = self.snapshot;
        let tightens_authorization =
            snapshot.retain_inherited_user_messages && !fresh.retain_inherited_user_messages;
        let mut restored = ContextManager {
            item_id_generator,
            sampling: self.sampling,
            restored_through_sequence: snapshot.restored_through_sequence,
            items: self.items,
            review_history: self.review_history,
            review_budgeting: snapshot.review_budgeting.into(),
            retained_context: Arc::new(snapshot.retained_context),
            rejected_audio_file_ids: Arc::new(snapshot.rejected_audio_file_ids),
            file_audio_durations: self.file_audio_durations,
            guardian_review_mode: match snapshot.review_mode {
                ReviewMode::Legacy => GuardianContextMode::Legacy,
                ReviewMode::Independent => GuardianContextMode::Independent,
                ReviewMode::ThreadOwned => GuardianContextMode::ThreadOwned,
            },
            guardian_include_additional_context: snapshot.guardian_include_additional_context,
            retain_inherited_user_messages: snapshot.retain_inherited_user_messages,
            history_version: snapshot.history_version,
            reset_version: snapshot.reset_version,
            user_message_revision: snapshot.user_message_revision,
            guardian_review_context_revision: next_guardian_review_context_revision(),
            token_info: snapshot.token_info,
            last_server_reported_usage_item_count: snapshot.last_server_reported_usage_item_count,
            last_model_output_item_count: snapshot.last_model_output_item_count,
            reference_context_item: snapshot.reference_context_item,
            world_state_baseline: snapshot.world_state_baseline,
        };
        if tightens_authorization {
            // Core rebuilt this authorization context from canonical history under
            // the destination's startup restriction. Captured policy cannot replace it.
            restored.retained_context = Arc::clone(&fresh.retained_context);
            restored.review_history = fresh.review_history.clone();
            restored.guardian_review_mode = fresh.guardian_review_mode;
        }
        restored.retain_inherited_user_messages &= fresh.retain_inherited_user_messages;
        if !restored.retain_inherited_user_messages {
            Arc::make_mut(&mut restored.retained_context).alab_restrict_inherited_messages();
            restored.alab_restrict_review_history();
        }
        restored
    }
}

impl ContextManager {
    /// A destination restriction covers independent review transcripts as well as
    /// retained facts. Preserve the model window and local review evidence; use
    /// stored envelope provenance rather than matching instruction text or IDs.
    pub(super) fn alab_restrict_review_history(&mut self) {
        if self.retain_inherited_user_messages {
            return;
        }
        let Some(history) = self.review_history.as_mut() else {
            return;
        };
        let checkpoint = history.checkpoint();
        let inherited = |entry: &codex_history::ResponseItemEnvelope| {
            entry
                .metadata
                .as_ref()
                .is_some_and(|metadata| metadata.inherited_user_message)
        };
        if checkpoint.0.iter().any(inherited) {
            // Native reset retains budgeting and advances prefix generation, so
            // existing review offsets cannot refer to the removed parent prefix.
            history.reset(checkpoint.0.iter().filter(|entry| !inherited(entry)));
        }
    }

    pub(crate) fn capture_exec_context(&self) -> Result<serde_json::Value, String> {
        // Deliberately exhaustive: new upstream context state must be reviewed
        // instead of silently receiving a default in the replacement process.
        let ContextManager {
            item_id_generator: _,
            sampling,
            restored_through_sequence,
            review_budgeting,
            rejected_audio_file_ids,
            file_audio_durations,
            guardian_include_additional_context,
            items,
            review_history,
            retained_context,
            guardian_review_mode,
            retain_inherited_user_messages,
            history_version,
            reset_version,
            user_message_revision,
            guardian_review_context_revision: _,
            token_info,
            last_server_reported_usage_item_count,
            last_model_output_item_count,
            reference_context_item,
            world_state_baseline,
        } = self;
        let capture = |items: &[ResponseItemEnvelope]| {
            items
                .iter()
                .map(MemoryItem::capture)
                .collect::<Result<Vec<_>, _>>()
        };
        let snapshot = Snapshot {
            version: 3,
            items: capture(items)?,
            sampling: sampling.capture_exec(),
            restored_through_sequence: *restored_through_sequence,
            review_budgeting: (*review_budgeting).into(),
            rejected_audio_file_ids: rejected_audio_file_ids.as_ref().clone(),
            file_audio_durations: file_audio_durations.capture_exec(),
            guardian_include_additional_context: *guardian_include_additional_context,
            review_history: review_history
                .as_ref()
                .map(|history| capture(&history.checkpoint().0))
                .transpose()?,
            review_generation: review_history.as_ref().map(TranscriptHistory::generation),
            retained_context: retained_context.as_ref().clone(),
            review_mode: match guardian_review_mode {
                GuardianContextMode::Legacy => ReviewMode::Legacy,
                GuardianContextMode::Independent => ReviewMode::Independent,
                GuardianContextMode::ThreadOwned => ReviewMode::ThreadOwned,
            },
            retain_inherited_user_messages: *retain_inherited_user_messages,
            history_version: *history_version,
            reset_version: *reset_version,
            user_message_revision: *user_message_revision,
            token_info: token_info.clone(),
            last_server_reported_usage_item_count: *last_server_reported_usage_item_count,
            last_model_output_item_count: *last_model_output_item_count,
            reference_context_item: reference_context_item.clone(),
            world_state_baseline: world_state_baseline.clone(),
        };
        serde_json::to_value(snapshot).map_err(|_| "model context could not be captured".into())
    }

    pub(crate) fn prepare_exec_context(
        value: serde_json::Value,
    ) -> Result<PreparedExecContext, String> {
        let mut snapshot: Snapshot =
            serde_json::from_value(value).map_err(|_| "invalid model context snapshot")?;
        if snapshot.version != 3
            || snapshot.restored_through_sequence > snapshot.sampling.0
            || snapshot.last_server_reported_usage_item_count > snapshot.items.len()
            || snapshot
                .last_model_output_item_count
                .is_some_and(|count| count > snapshot.items.len())
        {
            return Err("model context snapshot has invalid accounting boundaries".into());
        }
        let restore = |items: Vec<MemoryItem>| {
            items
                .into_iter()
                .map(MemoryItem::restore)
                .collect::<Result<Vec<_>, _>>()
        };
        let items = restore(std::mem::take(&mut snapshot.items))?;
        let sampling = super::super::sampling::SamplingProgress::restore_exec(snapshot.sampling)?;
        if items
            .iter()
            .filter_map(|item| item.metadata.as_ref())
            .any(|metadata| {
                metadata
                    .history_sequence
                    .is_some_and(|sequence| sequence > snapshot.sampling.0)
                    || metadata
                        .sampled_through_sequence
                        .is_some_and(|sequence| sequence > snapshot.sampling.1)
            })
        {
            return Err("history exceeds its captured sampling boundaries".into());
        }
        let file_audio_durations = Arc::new(FileAudioDurations::restore_exec(std::mem::take(
            &mut snapshot.file_audio_durations,
        ))?);
        let review_history = match (snapshot.review_history.take(), snapshot.review_generation) {
            (Some(items), Some(generation)) => {
                let items = restore(items)?;
                let mut review =
                    TranscriptHistory::new(generation, snapshot.review_budgeting.into());
                for item in &items {
                    review.record(item);
                }
                if review.checkpoint().0 != items {
                    return Err("restored review history changed its retained evidence".into());
                }
                Some(review)
            }
            (None, None) => None,
            _ => return Err("model context has inconsistent review history".into()),
        };
        Ok(PreparedExecContext {
            snapshot,
            items: Arc::new(items),
            review_history,
            sampling,
            file_audio_durations,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_context_retains_item_allocations_across_actual_install() {
        let model = serde_json::from_value(serde_json::json!({
            "salt": "item", "ordinal": 1000, "value": 42
        }))
        .unwrap();
        let id = codex_protocol::ResponseItemId::generated("msg", model);
        let mut item: ResponseItem = serde_json::from_value(serde_json::json!({
            "type": "message", "role": "assistant", "content": []
        }))
        .unwrap();
        item.set_id(Some(id.clone()));
        let mut original =
            ContextManager::new(crate::session::session::Session::new_item_id_generator());
        original.items = Arc::new(vec![ResponseItemEnvelope {
            item,
            metadata: None,
        }]);
        let restored =
            ContextManager::prepare_exec_context(original.capture_exec_context().unwrap())
                .unwrap()
                .install(
                    crate::session::session::Session::new_item_id_generator(),
                    &original,
                );
        let allocation = restored.items[0].item.id().unwrap().allocation().unwrap();
        assert_eq!(Some(allocation), id.allocation());
        assert_eq!(allocation.model_id.ordinal(), 1000);
    }

    #[test]
    fn memory_context_keeps_file_audio_in_both_tool_output_forms() {
        for kind in ["function_call_output", "custom_tool_call_output"] {
            let mut original =
                ContextManager::new(crate::session::session::Session::new_item_id_generator());
            let wire = serde_json::json!({
                "type": kind,
                "call_id": "retained-audio-call",
                "output": [{"type": "input_audio", "file_id": "retained-audio"}],
            });
            original.items = Arc::new(vec![ResponseItemEnvelope {
                item: serde_json::from_value(wire).unwrap(),
                metadata: None,
            }]);
            original.file_audio_durations = Arc::new(
                FileAudioDurations::restore_exec(BTreeMap::from([(
                    "retained-audio".into(),
                    std::time::Duration::from_millis(750),
                )]))
                .unwrap(),
            );
            let restored =
                ContextManager::prepare_exec_context(original.capture_exec_context().unwrap())
                    .unwrap()
                    .install(
                        crate::session::session::Session::new_item_id_generator(),
                        &original,
                    );
            assert_eq!(restored.items, original.items, "{kind}");
            assert_eq!(
                restored.items[0]
                    .item
                    .file_audio_ids()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
                vec!["retained-audio"],
                "{kind}"
            );
            assert_eq!(
                restored.file_audio_durations.capture_exec(),
                original.file_audio_durations.capture_exec(),
                "{kind}"
            );
        }
    }

    #[test]
    fn memory_context_retains_accounting_and_renews_process_evidence() {
        let mut original =
            ContextManager::new(crate::session::session::Session::new_item_id_generator());
        let item = ResponseItemEnvelope {
            item: ResponseItem::Message {
                id: None,
                encrypted_content: None,
                status: None,
                role: "user".into(),
                content: vec![ContentItem::InputText {
                    text: "retained original input".into(),
                }],
                phase: None,
                internal_chat_message_metadata_passthrough: None,
            },
            metadata: None,
        };
        original.restored_through_sequence = 8;
        original.review_budgeting = ContextBudgeting::Elastic;
        original.sampling =
            super::super::super::sampling::SamplingProgress::restore_exec((21, 13)).unwrap();
        original.rejected_audio_file_ids = Arc::new(BTreeSet::from(["rejected-audio".into()]));
        original.file_audio_durations = Arc::new(
            FileAudioDurations::restore_exec(BTreeMap::from([(
                "known-audio".into(),
                std::time::Duration::from_millis(750),
            )]))
            .unwrap(),
        );
        original.guardian_include_additional_context = false;
        original.items = Arc::new(vec![item.clone()]);
        let mut review = TranscriptHistory::new(10, original.review_budgeting);
        review.record(&item);
        original.review_history = Some(review);
        original.guardian_review_mode = GuardianContextMode::Independent;
        original.last_server_reported_usage_item_count = 1;
        original.last_model_output_item_count = Some(1);
        original.history_version = 9;
        original.reset_version = 4;
        original.user_message_revision = 7;
        original.retain_inherited_user_messages = true;
        let destination_generator = crate::session::session::Session::new_item_id_generator();
        let restored =
            ContextManager::prepare_exec_context(original.capture_exec_context().unwrap())
                .unwrap()
                .install(Arc::clone(&destination_generator), &original);
        assert!(Arc::ptr_eq(
            &restored.item_id_generator,
            &destination_generator
        ));
        assert!(!Arc::ptr_eq(
            &restored.item_id_generator,
            &original.item_id_generator
        ));
        assert_eq!(restored.sampling.capture_exec(), (21, 13));
        assert_eq!(restored.restored_through_sequence, 8);
        assert_eq!(restored.review_budgeting, ContextBudgeting::Elastic);
        assert_eq!(
            restored.rejected_audio_file_ids,
            original.rejected_audio_file_ids
        );
        assert_eq!(
            restored.file_audio_durations.capture_exec(),
            original.file_audio_durations.capture_exec()
        );
        assert!(!restored.guardian_include_additional_context);
        assert_eq!(restored.history_version, 9);
        assert_eq!(restored.reset_version, 4);
        assert_eq!(restored.user_message_revision, 7);
        assert!(restored.retain_inherited_user_messages);
        assert_eq!(restored.items, original.items);
        assert_eq!(restored.last_server_reported_usage_item_count, 1);
        assert_eq!(restored.last_model_output_item_count, Some(1));
        assert_eq!(
            restored.guardian_review_mode,
            GuardianContextMode::Independent
        );
        assert_eq!(
            restored.review_history.as_ref().unwrap().checkpoint(),
            original.review_history.as_ref().unwrap().checkpoint()
        );
        assert_ne!(
            restored.guardian_review_context_revision,
            original.guardian_review_context_revision
        );
    }

    #[test]
    fn restricted_snapshot_rechecks_stored_review_provenance() {
        let mut original =
            ContextManager::new(crate::session::session::Session::new_item_id_generator());
        let item = |text: &str, inherited| ResponseItemEnvelope {
            item: ResponseItem::Message {
                id: None,
                encrypted_content: None,
                status: None,
                role: "user".into(),
                content: vec![ContentItem::InputText { text: text.into() }],
                phase: None,
                internal_chat_message_metadata_passthrough: None,
            },
            metadata: Some(codex_history::CodexHarnessMetadata {
                inherited_user_message: inherited,
                ..Default::default()
            }),
        };
        let items = vec![
            item("inherited grant", true),
            item("local restriction", false),
        ];
        original.retain_inherited_user_messages = true;
        for entry in &items {
            let _ = original.record_retained_message(
                &entry.item,
                entry.metadata.as_ref(),
                super::super::user_authorization::RetainedMessageSource::Original,
            );
        }
        original.items = Arc::new(items.clone());
        original.review_budgeting = ContextBudgeting::Elastic;
        let mut review = TranscriptHistory::new(10, original.review_budgeting);
        review.reset(items.iter());
        original.review_history = Some(review);
        original.guardian_review_mode = GuardianContextMode::Independent;
        // A previously captured restricted runtime can still contain the old
        // independent-history defect. Its policy flag cannot vouch for its data.
        original.retain_inherited_user_messages = false;
        let fresh = ContextManager::new(crate::session::session::Session::new_item_id_generator());
        let restored =
            ContextManager::prepare_exec_context(original.capture_exec_context().unwrap())
                .unwrap()
                .install(Arc::clone(&fresh.item_id_generator), &fresh);
        assert_eq!(restored.items.as_ref(), &items, "preserve the model window");
        assert_eq!(restored.review_budgeting, ContextBudgeting::Elastic);
        assert_eq!(
            restored
                .original_legacy_user_messages()
                .map(|message| message.text)
                .collect::<Vec<_>>(),
            vec!["local restriction"]
        );
        assert!(!restored.retained_context.has_inherited_user_messages());
        assert_eq!(restored.retained_context.ordered_entries().filter(|(_, entry)|
            matches!(entry, codex_history::RetainedContextEntry::UserMessage(message)
                if message.text == "local restriction")).count(), 1);
        let review = restored.review_history.as_ref().unwrap();
        assert!(review.generation() > original.review_history.as_ref().unwrap().generation());
        let generation = review.generation();
        let mut repeated = restored;
        repeated.alab_restrict_review_history();
        assert_eq!(
            repeated.review_history.as_ref().unwrap().generation(),
            generation
        );
    }

    #[test]
    fn invalid_accounting_boundaries_are_rejected() {
        let mut encoded =
            ContextManager::new(crate::session::session::Session::new_item_id_generator())
                .capture_exec_context()
                .unwrap();
        encoded["last_server_reported_usage_item_count"] = serde_json::json!(1);
        assert!(ContextManager::prepare_exec_context(encoded).is_err());
    }
    #[test]
    fn sampling_and_audio_metadata_cannot_invent_a_trusted_boundary() {
        let original =
            ContextManager::new(crate::session::session::Session::new_item_id_generator());
        let mut invalid_progress = original.capture_exec_context().unwrap();
        invalid_progress["sampling"] = serde_json::json!([2, 3]);
        assert!(ContextManager::prepare_exec_context(invalid_progress).is_err());
        let mut invalid_restored = original.capture_exec_context().unwrap();
        invalid_restored["restored_through_sequence"] = serde_json::json!(1);
        assert!(ContextManager::prepare_exec_context(invalid_restored).is_err());
        let mut invalid_audio = original.capture_exec_context().unwrap();
        invalid_audio["file_audio_durations"] = serde_json::json!({"bad":{"secs":0,"nanos":0}});
        assert!(ContextManager::prepare_exec_context(invalid_audio).is_err());
    }
}
