// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use codex_core::CodexThread;
use codex_core::config::Config;
use codex_core::config::ConfigOverrides;
use codex_protocol::ThreadId;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::EventMsg;
use codex_rollout::InitialHistory;
use codex_rollout::RolloutItem;
use codex_state::ThreadMetadata;
use codex_thread_store::StoredThread;
use std::collections::HashMap;

type SavedModel<'a> = (&'a str, &'a str, Option<ReasoningEffort>);

pub(super) struct ModelRequest {
    model: bool,
    effort: bool,
}

impl ModelRequest {
    pub(super) fn capture(
        typed: &ConfigOverrides,
        overrides: Option<&HashMap<String, serde_json::Value>>,
    ) -> Self {
        let has = |key| overrides.is_some_and(|values| values.contains_key(key));
        Self {
            model: typed.model.is_some() || has("model"),
            effort: has("model_reasoning_effort"),
        }
    }

    pub(super) fn restore_resume(
        &self,
        config: &mut Config,
        history: &InitialHistory,
        metadata: Option<&ThreadMetadata>,
        thread: Option<&StoredThread>,
    ) {
        let InitialHistory::Resumed(history) = history else {
            return;
        };
        if !codex_alab_core::active() {
            return;
        }
        let saved = metadata
            .and_then(|metadata| {
                Some((
                    metadata.model.as_deref()?,
                    metadata.model_provider.as_str(),
                    metadata.reasoning_effort.clone(),
                ))
            })
            .or_else(|| thread.and_then(stored_model))
            .or_else(|| {
                history_model(
                    &history.history,
                    history.conversation_id,
                    thread.map(|t| t.model_provider.as_str()),
                )
            });
        self.apply(config, saved);
    }

    pub(super) async fn restore_fork(
        &self,
        config: &mut Config,
        thread: &StoredThread,
        history: &[RolloutItem],
        prediction: bool,
        parent: Option<&CodexThread>,
    ) {
        if prediction || !codex_alab_core::active() {
            return;
        }
        if let Some(parent) = parent {
            let snapshot = parent.thread_settings_snapshot().await;
            self.apply(
                config,
                Some((
                    &snapshot.model,
                    &snapshot.model_provider_id,
                    snapshot.reasoning_effort,
                )),
            );
            return;
        }
        self.apply(
            config,
            stored_model(thread)
                .or_else(|| history_model(history, thread.thread_id, Some(&thread.model_provider))),
        );
    }

    fn apply(&self, config: &mut Config, saved: Option<SavedModel<'_>>) {
        let Some((model, provider, effort)) = saved else {
            return;
        };
        if model.is_empty() || provider != config.model_provider_id {
            return;
        }
        if !self.model && config.alab_follows_saved_model() {
            config.model = Some(model.to_owned());
            config.alab_saved_model_can_follow.clone_from(&config.model);
        }
        if !self.effort && !config.alab_has_scoped_effort() {
            config.model_reasoning_effort = effort;
        }
    }
}

fn stored_model(thread: &StoredThread) -> Option<SavedModel<'_>> {
    Some((
        thread.model.as_deref()?,
        &thread.model_provider,
        thread.reasoning_effort.clone(),
    ))
}

// Older stores can have only a TurnContext. Its serving provider comes from
// the owning session header or the thread record. Ignore copied fork history.
fn history_model<'a>(
    history: &'a [RolloutItem],
    thread_id: ThreadId,
    stored_provider: Option<&'a str>,
) -> Option<SavedModel<'a>> {
    let mut own = true;
    let mut provider = stored_provider;
    let mut latest = None;
    for item in history {
        match item {
            RolloutItem::SessionMeta(header) => {
                own = header.meta.id == thread_id;
                provider = header.meta.model_provider.as_deref().or(stored_provider);
            }
            RolloutItem::EventMsg(EventMsg::ThreadSettingsApplied(event))
                if event.thread_id.map_or(own, |owner| owner == thread_id) =>
            {
                let settings = &event.thread_settings;
                latest = Some((
                    settings.model.as_str(),
                    settings.model_provider_id.as_str(),
                    settings.reasoning_effort.clone(),
                ));
            }
            RolloutItem::TurnContext(turn) if own => {
                if let Some(provider) = provider {
                    latest = Some((turn.model.as_str(), provider, turn.effort.clone()));
                }
            }
            _ => {}
        }
    }
    latest
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_protocol::models::PermissionProfile;
    use codex_protocol::protocol::AskForApproval;
    use codex_protocol::protocol::SessionMeta;
    use codex_protocol::protocol::SessionMetaLine;
    use codex_protocol::protocol::TurnContextItem;

    fn header(id: ThreadId, provider: &str) -> RolloutItem {
        RolloutItem::SessionMeta(Box::new(SessionMetaLine {
            meta: SessionMeta {
                id,
                model_provider: Some(provider.to_string()),
                ..SessionMeta::default()
            },
            git: None,
        }))
    }

    fn turn(model: &str, effort: ReasoningEffort) -> RolloutItem {
        RolloutItem::TurnContext(TurnContextItem {
            turn_id: None,
            root_turn_id: None,
            direct_only_tool_namespaces: None,
            disabled_plugin_ids: None,
            cwd: std::env::current_dir()
                .expect("cwd")
                .try_into()
                .expect("absolute cwd"),
            approval_policy: AskForApproval::Never,
            approvals_reviewer: None,
            permission_profile: PermissionProfile::read_only(),
            active_permission_profile: None,
            model: model.to_owned(),
            comp_hash: None,
            collaboration_mode: None,
            multi_agent_version: None,
            realtime_active: None,
            cyber_access_program: None,
            effort: Some(effort),
            summary: None,
        })
    }

    #[test]
    fn legacy_turn_restores_its_model_effort_and_provider() {
        let own = ThreadId::new();
        let foreign = ThreadId::new();
        let history = [
            header(own, "custom"),
            turn("custom-model", ReasoningEffort::High),
            header(foreign, "openai"),
            turn("gpt-6.1-sol", ReasoningEffort::Ultra),
        ];
        assert_eq!(
            history_model(&history, own, None),
            Some(("custom-model", "custom", Some(ReasoningEffort::High)))
        );
        assert_eq!(
            history_model(&history, foreign, None),
            Some(("gpt-6.1-sol", "openai", Some(ReasoningEffort::Ultra)))
        );
        assert_eq!(history_model(&history[1..2], own, None), None);
        assert_eq!(
            history_model(&history[1..2], own, Some("legacy")),
            Some(("custom-model", "legacy", Some(ReasoningEffort::High)))
        );
    }
}
