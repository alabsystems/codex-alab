// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Exact model-context state for a native, nonpersistent handoff. Runtime
//! cancellation tokens and process-local authorization revisions are renewed.

use super::*;
use crate::session::alab_exec_recovery_options::ephemeral::codec::MemoryItem;
use serde::Deserialize;
use serde::Serialize;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    version: u32,
    sampling: (u64, u64),
    items: Vec<MemoryItem>,
    review_history: Option<Vec<MemoryItem>>,
    review_generation: Option<u64>,
    review_budgeting: Budgeting,
    retained_context: RetainedContext,
    rejected_audio_file_ids: BTreeSet<String>,
    file_audio_durations: std::collections::BTreeMap<String, std::time::Duration>,
    review_mode: ReviewMode,
    guardian_include_additional_context: bool,
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

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ReviewMode {
    Legacy,
    Independent,
    ThreadOwned,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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

impl Budgeting {
    fn restore(self) -> ContextBudgeting {
        match self {
            Self::Fixed => ContextBudgeting::Fixed,
            Self::Elastic => ContextBudgeting::Elastic,
        }
    }
}

/// Validated data with no executable item generator from the previous process.
#[derive(Clone)]
pub(crate) struct PreparedExecContext {
    snapshot: Arc<Snapshot>,
    items: Arc<Vec<ResponseItemEnvelope>>,
    review_history: Option<TranscriptHistory>,
    sampling: crate::context_manager::sampling::SamplingProgress,
    file_audio_durations: Arc<FileAudioDurations>,
}

impl PreparedExecContext {
    pub(crate) fn bind(&self, item_id_generator: ItemIdGenerator) -> ContextManager {
        let snapshot = &self.snapshot;
        ContextManager {
            item_id_generator,
            sampling: self.sampling.clone(),
            items: Arc::clone(&self.items),
            review_history: self.review_history.clone(),
            review_budgeting: snapshot.review_budgeting.restore(),
            retained_context: Arc::new(snapshot.retained_context.clone()),
            rejected_audio_file_ids: Arc::new(snapshot.rejected_audio_file_ids.clone()),
            file_audio_durations: Arc::clone(&self.file_audio_durations),
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
            token_info: snapshot.token_info.clone(),
            last_server_reported_usage_item_count: snapshot.last_server_reported_usage_item_count,
            last_model_output_item_count: snapshot.last_model_output_item_count,
            reference_context_item: snapshot.reference_context_item.clone(),
            world_state_baseline: snapshot.world_state_baseline.clone(),
        }
    }
}

impl ContextManager {
    pub(crate) fn capture_exec_context(&self) -> Result<serde_json::Value, String> {
        // Deliberately exhaustive: new upstream context state must be reviewed
        // instead of silently receiving a default in the replacement process.
        let ContextManager {
            item_id_generator: _,
            sampling,
            items,
            review_history,
            review_budgeting,
            retained_context,
            rejected_audio_file_ids,
            file_audio_durations,
            guardian_review_mode,
            guardian_include_additional_context,
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
            version: 2,
            sampling: sampling.capture_exec_sampling(),
            items: capture(items)?,
            review_history: review_history
                .as_ref()
                .map(|history| capture(&history.checkpoint().0))
                .transpose()?,
            review_generation: review_history.as_ref().map(TranscriptHistory::generation),
            review_budgeting: (*review_budgeting).into(),
            retained_context: retained_context.as_ref().clone(),
            rejected_audio_file_ids: rejected_audio_file_ids.as_ref().clone(),
            file_audio_durations: file_audio_durations.capture_exec(),
            review_mode: match guardian_review_mode {
                GuardianContextMode::Legacy => ReviewMode::Legacy,
                GuardianContextMode::Independent => ReviewMode::Independent,
                GuardianContextMode::ThreadOwned => ReviewMode::ThreadOwned,
            },
            guardian_include_additional_context: *guardian_include_additional_context,
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
        if snapshot.version != 2
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
        let review_history = match (snapshot.review_history.take(), snapshot.review_generation) {
            (Some(items), Some(generation)) => {
                let items = restore(items)?;
                let mut review =
                    TranscriptHistory::new(generation, snapshot.review_budgeting.restore());
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
        let sampling = crate::context_manager::sampling::SamplingProgress::restore_exec_sampling(
            snapshot.sampling.0,
            snapshot.sampling.1,
        )?;
        let file_audio_durations =
            FileAudioDurations::restore_exec(std::mem::take(&mut snapshot.file_audio_durations))?;
        Ok(PreparedExecContext {
            snapshot: Arc::new(snapshot),
            items: Arc::new(items),
            review_history,
            sampling,
            file_audio_durations: Arc::new(file_audio_durations),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_context_retains_accounting_and_renews_process_evidence() {
        let mut original = ContextManager::default();
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
        original.items = Arc::new(vec![item.clone()]);
        let mut review = TranscriptHistory::new(10, ContextBudgeting::Elastic);
        review.record(&item);
        original.review_history = Some(review);
        original.review_budgeting = ContextBudgeting::Elastic;
        original.sampling =
            crate::context_manager::sampling::SamplingProgress::restore_exec_sampling(12, 7)
                .unwrap();
        original.guardian_include_additional_context = true;
        original.rejected_audio_file_ids = Arc::new(BTreeSet::from(["audio-file".into()]));
        original.file_audio_durations = Arc::new(
            FileAudioDurations::restore_exec(std::collections::BTreeMap::from([(
                "audio-file".into(),
                std::time::Duration::from_millis(2300),
            )]))
            .unwrap(),
        );
        original.guardian_review_mode = GuardianContextMode::Independent;
        original.last_server_reported_usage_item_count = 1;
        original.last_model_output_item_count = Some(1);
        original.history_version = 9;
        original.reset_version = 4;
        original.user_message_revision = 7;
        original.retain_inherited_user_messages = true;
        let fresh = ContextManager::default();
        let restored =
            ContextManager::prepare_exec_context(original.capture_exec_context().unwrap())
                .unwrap()
                .bind(Arc::clone(&fresh.item_id_generator));
        assert!(Arc::ptr_eq(
            &restored.item_id_generator,
            &fresh.item_id_generator
        ));
        assert!(!Arc::ptr_eq(
            &restored.item_id_generator,
            &original.item_id_generator
        ));
        assert_eq!(restored.sampling.capture_exec_sampling(), (12, 7));
        assert_eq!(restored.review_budgeting, ContextBudgeting::Elastic);
        assert!(restored.guardian_include_additional_context);
        assert_eq!(
            restored.rejected_audio_file_ids,
            original.rejected_audio_file_ids
        );
        assert_eq!(
            restored.file_audio_durations.capture_exec(),
            original.file_audio_durations.capture_exec()
        );
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
    fn invalid_accounting_boundaries_are_rejected() {
        let mut encoded = ContextManager::default().capture_exec_context().unwrap();
        encoded["last_server_reported_usage_item_count"] = serde_json::json!(1);
        assert!(ContextManager::prepare_exec_context(encoded).is_err());
        let mut encoded = ContextManager::default().capture_exec_context().unwrap();
        encoded["sampling"] = serde_json::json!([3, 4]);
        assert!(ContextManager::prepare_exec_context(encoded).is_err());
        let mut encoded = ContextManager::default().capture_exec_context().unwrap();
        encoded["version"] = serde_json::json!(1);
        assert!(ContextManager::prepare_exec_context(encoded).is_err());
    }
}
