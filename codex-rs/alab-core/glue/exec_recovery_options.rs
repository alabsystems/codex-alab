// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Original per-turn exec settings carried through an account handoff. These
//! values are metadata, never permission to resume or select credentials.

use crate::CodexThread;
use codex_alab_core::quota::HeadlessExec;
use codex_alab_core::renewal::TurnAdmission;
use codex_extension_api::ExtensionDataInit;
use codex_protocol::ThreadId;
use codex_protocol::config_types::CollaborationMode;
use codex_protocol::config_types::ReasoningSummary;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result as CodexResult;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::ThreadSettingsOverrides;
use codex_protocol::turn_input::CyberAccessProgram;
use codex_protocol::turn_input::PromptCacheTtl;
use codex_protocol::turn_input::TurnStartOptions;

#[path = "exec_prompt_cache_lifetime.rs"]
mod cache_lifetime;
use cache_lifetime::CacheLifetime;
use serde::Deserialize;
use serde::Serialize;

#[path = "exec_tree_wire.rs"]
mod tree_wire;
pub use tree_wire::ExecThreadSnapshot;
pub use tree_wire::ExecTreeSnapshot;

#[path = "exec_tree_capture.rs"]
mod tree_capture;
pub use tree_capture::CommittedUpgradeForest;
pub use tree_capture::DecodedUpgradeForest;
pub use tree_capture::ExecCapturedThread;
pub use tree_capture::ExecCapturedTree;
pub use tree_capture::ExecPreparedTree;
pub use tree_capture::PreparedUpgradeForest;
pub use tree_capture::UpgradeForestSnapshot;

#[path = "exec_child_suspension.rs"]
mod child_suspension;
pub use child_suspension::ExecPreparedChild;

#[path = "exec_ephemeral_history.rs"]
pub(crate) mod ephemeral;
pub use super::input_queue::exec_input_state::ExecInputBatch;
pub use super::input_queue::exec_input_state::ExecInputSnapshot;
pub use super::input_queue::exec_input_state::PreparedExecIdleInput;
pub use super::input_queue::exec_input_state::PreparedExecInputBatch;
pub use ephemeral::ExecEphemeralHistory;

#[path = "exec_idle_status.rs"]
mod idle_status;
pub(crate) use idle_status::restore as restore_idle_status;

/// Settings captured from an actual running exec turn. Deserialization is not
/// authentication: the host must bind these fields to its native receipt and
/// supply live admission before Core can use them for recovery.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecRecoveryOptions {
    thread: ThreadId,
    turn: String,
    review: bool,
    schema: Option<serde_json::Value>,
    direct_only_tool_namespaces: Option<Vec<String>>,
    service_tier: Option<String>,
    prompt_cache_ttl: Option<CacheLifetime>,
    cyber_access_program: Option<CyberAccessProgram>,
    helper_choice: Option<crate::agent::helper_daybreak::turn::alab_exec_helper::ActiveChoice>,
    step: ExecStepSettings,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExecStepSettings {
    collaboration_mode: CollaborationMode,
    summary: ReasoningSummary,
    approval_policy: AskForApproval,
    disabled_plugin_ids: Vec<String>,
}

impl ExecRecoveryOptions {
    pub(crate) fn helper_choice(
        &self,
    ) -> Option<&crate::agent::helper_daybreak::turn::alab_exec_helper::ActiveChoice> {
        self.helper_choice.as_ref()
    }

    /// Capture the named regular or frozen review turn without reading mutable CLI files.
    /// The caller must still stop execution and flush its history before handoff.
    pub async fn capture(thread: &CodexThread, expected_turn: &str) -> Option<Self> {
        thread.thread_extension_data().get::<HeadlessExec>()?;
        let active = thread.session.active_turn.lock().await;
        let task = active.as_ref()?.task.as_ref()?;
        let context = &task.turn_context;
        if !crate::tasks::review_recovery::capture_ready(task.kind, context)
            || task.cancellation_token.is_cancelled()
            || context.sub_id != expected_turn
            || context.extension_data.get::<HeadlessExec>().is_none()
        {
            return None;
        }
        Some(Self::from_context(thread.session.thread_id, context))
    }

    fn from_context(thread: ThreadId, context: &crate::session::turn_context::TurnContext) -> Self {
        let settings = context.next_step_settings.load();
        Self {
            thread,
            turn: context.sub_id.clone(),
            review: crate::tasks::review_recovery::has_consumer(context),
            schema: context.final_output_json_schema.clone(),
            direct_only_tool_namespaces: context.overridden_direct_only_tool_namespaces.clone(),
            // An absent override must clear a different default in the new
            // process, just as Core's daemon recovery capture does.
            service_tier: Some(settings.service_tier.clone().unwrap_or_else(|| {
                codex_protocol::config_types::SERVICE_TIER_DEFAULT_REQUEST_VALUE.to_owned()
            })),
            prompt_cache_ttl: context.prompt_cache_ttl.map(Into::into),
            cyber_access_program: context.cyber_access_program,
            helper_choice:
                crate::agent::helper_daybreak::turn::alab_exec_helper::ActiveChoice::capture(
                    context,
                ),
            step: ExecStepSettings {
                collaboration_mode: settings.effective_collaboration_mode().with_updates(
                    None,
                    Some(settings.effective_reasoning_effort()),
                    None,
                ),
                summary: settings.reasoning_summary,
                approval_policy: settings.approval_policy(),
                disabled_plugin_ids: context.disabled_plugin_ids.clone(),
            },
        }
    }

    /// Core applies these through its ordinary managed-constraint validation.
    /// The snapshot restores choices; it cannot grant an exception to current policy.
    /// Reviewer preference belongs to the restored thread settings, which retain
    /// requested and effective reviewers separately. Replaying the effective value
    /// here as an explicit request would overwrite the user's original preference.
    pub fn thread_settings(&self) -> ThreadSettingsOverrides {
        ThreadSettingsOverrides {
            collaboration_mode: Some(self.step.collaboration_mode.clone()),
            summary: Some(self.step.summary),
            approval_policy: Some(self.step.approval_policy),
            disabled_plugin_ids: Some(self.step.disabled_plugin_ids.clone()),
            ..Default::default()
        }
    }

    pub fn thread(&self) -> ThreadId {
        self.thread
    }

    pub fn turn(&self) -> &str {
        &self.turn
    }

    pub fn is_review(&self) -> bool {
        self.review
    }

    /// Canonical lifecycle evidence only, never permission to execute.
    pub fn matches_open_history(&self, history: &codex_thread_store::StoredThreadHistory) -> bool {
        use codex_history::RolloutItem;
        use codex_protocol::items::TurnItem;
        use codex_protocol::protocol::EventMsg;
        if history.thread_id != self.thread
            || ephemeral::open_turn(&history.items) != Some(self.turn.as_str())
        {
            return false;
        }
        let mut entered = false;
        for item in &history.items {
            match item {
                RolloutItem::EventMsg(EventMsg::ItemStarted(event))
                    if event.thread_id == self.thread
                        && event.turn_id == self.turn
                        && matches!(event.item, TurnItem::EnteredReviewMode(_)) =>
                {
                    entered = true
                }
                RolloutItem::EventMsg(EventMsg::ItemCompleted(event))
                    if event.thread_id == self.thread
                        && event.turn_id == self.turn
                        && matches!(event.item, TurnItem::EnteredReviewMode(_)) =>
                {
                    entered = true
                }
                RolloutItem::EventMsg(EventMsg::TurnComplete(event))
                    if event.turn_id == self.turn =>
                {
                    return false;
                }
                RolloutItem::EventMsg(EventMsg::TurnAborted(event))
                    if event.turn_id.is_none() || event.turn_id.as_deref() == Some(&self.turn) =>
                {
                    return false;
                }
                RolloutItem::EventMsg(EventMsg::ItemCompleted(event))
                    if event.turn_id == self.turn
                        && matches!(event.item, TurnItem::ExitedReviewMode(_)) =>
                {
                    return false;
                }
                _ => {}
            }
        }
        !self.review || entered
    }

    fn apply(
        &self,
        thread: ThreadId,
        turn: &str,
        runtime_is_exec: bool,
        has_live_admission: bool,
        mut options: TurnStartOptions,
    ) -> CodexResult<TurnStartOptions> {
        if self.thread != thread || self.turn != turn || !runtime_is_exec || !has_live_admission {
            return Err(CodexErr::Fatal(
                "exec recovery settings lack matching live original-turn admission".to_owned(),
            ));
        }
        // Preserve Core's restored attribution and lineage. Only the original
        // per-turn settings missing from that reconstruction are supplied here.
        options.final_output_json_schema.clone_from(&self.schema);
        options
            .direct_only_tool_namespaces
            .clone_from(&self.direct_only_tool_namespaces);
        options.service_tier.clone_from(&self.service_tier);
        options.prompt_cache_ttl = self.prompt_cache_ttl.map(Into::into);
        options.cyber_access_program = if self.helper_choice.is_some() {
            None
        } else {
            self.cyber_access_program
        };
        Ok(options)
    }
}

impl CodexThread {
    /// Read canonical latest-turn evidence through the thread store, including
    /// start/input records older than the latest compaction's model context.
    pub async fn exec_recovery_history(
        &self,
    ) -> codex_thread_store::ThreadStoreResult<codex_thread_store::StoredThreadHistory> {
        if self.session.live_thread().is_none() {
            return ephemeral::history(&self.session);
        }
        self.session
            .services
            .thread_store
            .load_latest_turn_history(codex_thread_store::LoadThreadHistoryParams {
                thread_id: self.session.thread_id,
                include_archived: false,
            })
            .await
    }
}

pub(crate) fn restore(
    thread: &CodexThread,
    turn: &str,
    init: Option<&ExtensionDataInit>,
    options: TurnStartOptions,
) -> CodexResult<TurnStartOptions> {
    let Some(init) = init else { return Ok(options) };
    let Some(saved) = init.get::<ExecRecoveryOptions>() else {
        return Ok(options);
    };
    if saved.is_review() != crate::tasks::review_recovery::has_prepared(init) {
        return Err(CodexErr::InvalidRequest(
            "recovery task kind lacks its matching review bindings".into(),
        ));
    }
    saved.apply(
        thread.session.thread_id,
        turn,
        thread
            .thread_extension_data()
            .get::<HeadlessExec>()
            .is_some(),
        init.get::<TurnAdmission>().is_some(),
        options,
    )
}

#[path = "exec_transfer.rs"]
pub(crate) mod transfer;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_history_requires_entry_and_cannot_revive_a_closed_turn() {
        use codex_history::RolloutItem;
        use codex_protocol::protocol::EventMsg;
        let mut options = saved();
        options.review = true;
        let event =
            |value| RolloutItem::EventMsg(serde_json::from_value::<EventMsg>(value).unwrap());
        let mut history = codex_thread_store::StoredThreadHistory {
            thread_id: options.thread,
            revision: None,
            items: vec![event(
                serde_json::json!({"type":"task_started","turn_id":"original","model_context_window":null}),
            )],
        };
        assert!(!options.matches_open_history(&history));
        history.items.insert(0, event(serde_json::json!({"type":"item_completed","thread_id":options.thread,"turn_id":"original","item":{"type":"EnteredReviewMode","id":"entry","target":{"type":"custom","instructions":"review"},"user_facing_hint":"Review"}})));
        assert!(options.matches_open_history(&history));
        history.items.push(event(serde_json::json!({"type":"task_complete","turn_id":"original","last_agent_message":null})));
        history.items.push(event(serde_json::json!({"type":"task_started","turn_id":"original","model_context_window":null})));
        assert!(!options.matches_open_history(&history));
        history.items.remove(2);
        history.items.push(event(
            serde_json::json!({"type":"turn_aborted","turn_id":"original","reason":"interrupted"}),
        ));
        assert!(!options.matches_open_history(&history));
    }

    pub(super) fn saved() -> ExecRecoveryOptions {
        ExecRecoveryOptions {
            thread: ThreadId::new(),
            turn: "original".to_owned(),
            review: false,
            schema: Some(serde_json::json!({"type": "object", "additionalProperties": false})),
            direct_only_tool_namespaces: Some(vec!["original-tools".into()]),
            service_tier: Some("priority".to_owned()),
            prompt_cache_ttl: Some(CacheLifetime::TwelveHours),
            cyber_access_program: Some(CyberAccessProgram::DaybreakBlue),
            helper_choice: None,
            step: ExecStepSettings {
                collaboration_mode: CollaborationMode {
                    mode: codex_protocol::config_types::ModeKind::Default,
                    settings: codex_protocol::config_types::Settings {
                        model: "original-model".into(),
                        reasoning_effort: None,
                        developer_instructions: None,
                    },
                },
                summary: ReasoningSummary::Detailed,
                approval_policy: AskForApproval::Never,
                disabled_plugin_ids: vec!["disabled-plugin".into()],
            },
        }
    }

    #[test]
    fn captured_step_choices_replace_replacement_defaults() {
        let saved = saved();
        let settings = saved.thread_settings();
        assert_eq!(
            settings.collaboration_mode,
            Some(saved.step.collaboration_mode)
        );
        assert_eq!(settings.summary, Some(ReasoningSummary::Detailed));
        assert_eq!(settings.approval_policy, Some(AskForApproval::Never));
        assert!(settings.approvals_reviewer.is_none());
        assert_eq!(
            settings.disabled_plugin_ids,
            Some(vec!["disabled-plugin".into()])
        );
        assert!(settings.permission_profile.is_none());
        assert!(settings.sandbox_policy.is_none());
    }

    #[test]
    fn original_exec_settings_preserve_recovered_lineage() {
        let saved = saved();
        let restored = saved
            .apply(
                saved.thread,
                &saved.turn,
                true,
                true,
                TurnStartOptions {
                    root_turn_id: Some("causal-root".to_owned()),
                    turn_trigger: Some("exec".to_owned()),
                    ..Default::default()
                },
            )
            .expect("matching metadata with live admission");
        assert_eq!(restored.final_output_json_schema, saved.schema);
        assert_eq!(restored.service_tier, saved.service_tier);
        assert_eq!(
            restored.direct_only_tool_namespaces,
            saved.direct_only_tool_namespaces
        );
        assert!(matches!(
            restored.prompt_cache_ttl,
            Some(PromptCacheTtl::TwelveHours)
        ));
        assert_eq!(restored.cyber_access_program, saved.cyber_access_program);
        assert_eq!(restored.root_turn_id.as_deref(), Some("causal-root"));
        assert_eq!(restored.turn_trigger.as_deref(), Some("exec"));
    }

    #[test]
    fn explicit_empty_tool_namespaces_clear_replacement_turn_defaults() {
        let mut saved = saved();
        saved.direct_only_tool_namespaces = Some(Vec::new());
        let restored = saved
            .apply(
                saved.thread,
                &saved.turn,
                true,
                true,
                TurnStartOptions {
                    direct_only_tool_namespaces: Some(vec!["replacement-tools".into()]),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(restored.direct_only_tool_namespaces, Some(Vec::new()));
    }

    #[test]
    fn serialized_settings_cannot_admit_or_change_an_origin() {
        let saved: ExecRecoveryOptions = serde_json::from_str(
            &serde_json::to_string(&saved()).expect("serialize captured settings"),
        )
        .expect("restore metadata");
        for (thread, turn, exec, admitted) in [
            (ThreadId::new(), "original", true, true),
            (saved.thread, "different", true, true),
            (saved.thread, "original", false, true),
            (saved.thread, "original", true, false),
        ] {
            assert!(
                saved
                    .apply(thread, turn, exec, admitted, TurnStartOptions::default())
                    .is_err()
            );
        }
    }
}

#[path = "exec_quota_observation.rs"]
pub(crate) mod quota_observation;

/// Process-local metadata from the real started turn, never authority to recover.
struct StartedCacheLifetime {
    turn: String,
    value: Option<PromptCacheTtl>,
}
pub(crate) fn remember_prompt_cache_lifetime(
    session: &super::Session,
    context: &super::turn_context::TurnContext,
) {
    session
        .services
        .thread_extension_data
        .insert(StartedCacheLifetime {
            turn: context.sub_id.clone(),
            value: context.prompt_cache_ttl,
        });
}
impl CodexThread {
    /// A fresh runtime without this observation cannot infer a lifetime from rollout.
    pub fn recovery_prompt_cache_ttl(&self, turn: &str) -> Option<PromptCacheTtl> {
        self.thread_extension_data()
            .get::<StartedCacheLifetime>()
            .filter(|saved| saved.turn == turn)
            .and_then(|saved| saved.value)
    }
}
