// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::RejectedModel;
use super::RetriedModel;
use super::Session;
use super::StepContext;
use crate::client_common::Prompt;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result;
use codex_protocol::models::ResponseItem;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

struct RefusedInput(Arc<Prompt>);
struct RetryInput(Mutex<Option<(Arc<RefusedInput>, CodexErr)>>);

impl StepContext {
    pub(in crate::session) fn alab_retain_refused_input(&self, prompt: &Arc<Prompt>) {
        if self
            .extension_data
            .get::<RejectedModel>()
            .is_some_and(|r| r.replay)
        {
            self.extension_data.insert(RefusedInput(Arc::clone(prompt)));
        }
    }

    pub(in crate::session) fn alab_arm_retirement_retry(&self, original: CodexErr) {
        if let Some(input) = self.extension_data.remove::<RefusedInput>() {
            super::stream::restore_speculative_response(self);
            self.turn
                .extension_data
                .insert(RetryInput(Mutex::new(Some((input, original)))));
        }
    }

    pub(super) async fn alab_has_complete_retirement_input(&self, session: &Session) -> bool {
        let Some(input) = self.extension_data.get::<RefusedInput>() else {
            return false;
        };
        let state = session.state.lock().await;
        if !completed_calls(&input.0.input, state.history.raw_items()) {
            return false;
        }
        drop(state);
        self.alab_no_outstanding_tool(session).await
    }

    async fn alab_no_outstanding_tool(&self, session: &Session) -> bool {
        let turn_state = {
            let active = session.active_turn.lock().await;
            let Some(active) = active.as_ref() else {
                return false;
            };
            if !active.task.as_ref().is_some_and(|task| {
                Arc::ptr_eq(&task.turn_context, &self.turn)
                    && !task.cancellation_token.is_cancelled()
            }) {
                return false;
            }
            Arc::clone(&active.turn_state)
        };
        if turn_state.lock().await.alab_has_pending_tool_response()
            || session.services.code_mode_service.alab_has_active_cells()
        {
            return false;
        }
        !session
            .services
            .unified_exec_manager
            .alab_terminal_operation_pending()
            .await
    }

    pub(in crate::session) async fn alab_verify_retirement_input(
        &self,
        session: &Session,
        next: &[ResponseItem],
    ) -> Result<()> {
        if !codex_alab_core::is_alab_binary() {
            return Ok(());
        }
        let Some(retry) = self.turn.extension_data.remove::<RetryInput>() else {
            return Ok(());
        };
        let Some((previous, error)) = retry
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        else {
            return Ok(());
        };
        let retry = self.turn.extension_data.get::<RetriedModel>();
        let approved = retry.as_ref().is_some_and(|retry| {
            retry.destination == self.settings.model_info.slug
                && session
                    .services
                    .models_manager
                    .refreshed_model_catalog()
                    .is_some_and(|current| Arc::ptr_eq(&retry.catalog, &current))
        });
        if approved
            && same_conversation(&previous.0.input, next)
            && (!retry.is_some_and(|retry| retry.continuation)
                || self.alab_no_outstanding_tool(session).await)
        {
            Ok(())
        } else {
            Err(error)
        }
    }
}

// The new step owns its developer and system context. The model must still receive the same
// user input, assistant output, tool invocations and original materialized results in order.
fn same_conversation(previous: &[ResponseItem], next: &[ResponseItem]) -> bool {
    fn conversation(item: &&ResponseItem) -> bool {
        !matches!(item, ResponseItem::Message {role, ..} if role == "developer" || role == "system")
    }
    previous
        .iter()
        .filter(conversation)
        .eq(next.iter().filter(conversation))
}

fn completed_calls<'a>(
    input: &[ResponseItem],
    history: impl Iterator<Item = &'a ResponseItem>,
) -> bool {
    use crate::tools::input_call_id;
    use crate::tools::is_matching_tool_output;
    use crate::tools::output_call_id;

    // The prompt renderer may synthesize a placeholder for an unpaired historical call.
    // Require a real source output for each sent call, including the most recent one.
    struct CallEvidence<'a> {
        call: &'a ResponseItem,
        projected_output: bool,
        recorded_call: bool,
        recorded_output: bool,
    }

    let mut calls = HashMap::new();
    for item in input {
        if let Some(id) = input_call_id(item) {
            let evidence = CallEvidence {
                call: item,
                projected_output: false,
                recorded_call: false,
                recorded_output: false,
            };
            if calls.insert(id, evidence).is_some() {
                return false;
            }
        } else if let Some(id) = output_call_id(item) {
            let Some(evidence) = calls.get_mut(id) else {
                return false;
            };
            if evidence.projected_output || !is_matching_tool_output(evidence.call, item) {
                return false;
            }
            evidence.projected_output = true;
        }
    }
    if calls.is_empty() {
        return false;
    }
    for item in history {
        if let Some(id) = input_call_id(item)
            && let Some(evidence) = calls.get_mut(id)
        {
            if evidence.recorded_call {
                return false;
            }
            evidence.recorded_call = true;
        } else if let Some(id) = output_call_id(item)
            && let Some(evidence) = calls.get_mut(id)
        {
            if !evidence.recorded_call
                || evidence.recorded_output
                || !is_matching_tool_output(evidence.call, item)
            {
                return false;
            }
            evidence.recorded_output = true;
        }
    }
    calls.values().all(|evidence| {
        evidence.projected_output && evidence.recorded_call && evidence.recorded_output
    })
}
