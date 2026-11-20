// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use crate::CodexThread;
use crate::TurnContext;
use crate::session::session::Session;
use crate::state::ActiveTurn;
use crate::state::TurnState;
use codex_history::CodexHarnessMetadata;
use codex_history::InputReview;
use codex_history::RolloutItem;
use codex_history::UserInputOrigin;
use codex_protocol::items::TurnItem;
use codex_protocol::protocol::CodexErrorInfo;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::TurnCompleteEvent;
use codex_protocol::protocol::user_message_preview;
use codex_protocol::turn_input::NotSubmittedReason;
use codex_protocol::turn_input::OBSERVED_STEER_FALLBACK_TRIGGER;
use codex_protocol::turn_input::QUEUE_REVIEW_TRIGGER;
use codex_protocol::turn_input::QueueDispatch;
use codex_protocol::turn_input::TurnInput;
use codex_protocol::turn_input::TurnInputRequest;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Mutex;

#[path = "queue_review_rollback.rs"]
mod rollback;

/// If the raw latest terminal was removed by rollback, replay the surviving native
/// lifecycle instead. Keep identified late events with their turn and unscoped events
/// with the accepted start that owns them.
pub fn history_without_rolled_back_finding<'a>(
    items: &'a [RolloutItem],
    turn: &str,
) -> Option<impl Iterator<Item = &'a RolloutItem>> {
    let discarded = rollback::discarded_turns(items);
    if !discarded.contains(turn) {
        return None;
    }
    let mut removed = false;
    Some(items.iter().filter(move |item| {
        match item {
            RolloutItem::EventMsg(EventMsg::TurnStarted(start)) => {
                removed = discarded.contains(start.turn_id.as_str());
                !removed
            }
            RolloutItem::EventMsg(EventMsg::ThreadRolledBack(_)) => {
                removed = false;
                false
            }
            RolloutItem::EventMsg(EventMsg::TurnComplete(done)) => {
                !discarded.contains(done.turn_id.as_str())
            }
            RolloutItem::EventMsg(EventMsg::TurnAborted(abort)) => abort
                .turn_id
                .as_deref()
                .map_or(!removed, |id| !discarded.contains(id)),
            RolloutItem::EventMsg(EventMsg::ItemCompleted(done)) => {
                !discarded.contains(done.turn_id.as_str())
            }
            RolloutItem::EventMsg(EventMsg::ItemStarted(start)) => {
                !discarded.contains(start.turn_id.as_str())
            }
            _ => !removed,
        }
    }))
}

impl CodexThread {
    /// Whether the durable queue must wait for an explicitly started user turn.
    pub async fn alab_queue_review_pending(&self) -> bool {
        self.session.awaiting_input_review().await
    }
}

impl Session {
    /// Called with the active-turn lock held, like publication of the finished finding.
    /// A stale strict steer must not invite the client to start or retarget that input.
    pub(super) async fn strict_steer_refusal(
        &self,
        active: Option<&ActiveTurn>,
        expected: Option<&str>,
    ) -> Option<NotSubmittedReason> {
        let expected = expected?;
        if !codex_alab_core::is_alab_binary()
            || active
                .and_then(|slot| slot.task.as_ref())
                .is_some_and(|task| task.turn_context.sub_id == expected)
        {
            return None;
        }
        if let Some(reason) = self.automatic_start_refusal().await {
            return Some(reason);
        }
        // A task can have been taken before its terminal result is published.
        active
            .filter(|slot| slot.finishing.is_some())
            .map(|_| NotSubmittedReason::NotIdle)
    }

    /// A relay may steer a running turn, but cannot replace a finishing task while
    /// its terminal finding is still being published. Reserve before applying settings.
    #[expect(
        clippy::await_holding_invalid_type,
        reason = "the native review check and relay reservation must be atomic"
    )]
    pub(super) async fn reserve_relay_start(
        &self,
        explicit: bool,
    ) -> Result<Option<Arc<Mutex<TurnState>>>, NotSubmittedReason> {
        if explicit || !codex_alab_core::is_alab_binary() {
            return Ok(None);
        }
        let mut active = self.active_turn.lock().await;
        if let Some(reason) = self.automatic_start_refusal().await {
            return Err(reason);
        }
        if active.is_some() {
            return Err(NotSubmittedReason::NotIdle);
        }
        let turn = active.get_or_insert_with(ActiveTurn::default);
        Ok(Some(Arc::clone(&turn.turn_state)))
    }

    pub(super) async fn awaiting_input_review(&self) -> bool {
        codex_alab_core::is_alab_binary() && self.state.lock().await.alab_queue_review.is_some()
    }

    pub(super) async fn automatic_start_refusal(
        &self,
    ) -> Option<codex_protocol::turn_input::NotSubmittedReason> {
        if !codex_alab_core::is_alab_binary() {
            return None;
        }
        let turn_id = self.state.lock().await.alab_queue_review.clone()?;
        Some(codex_protocol::turn_input::NotSubmittedReason::MisalignmentReview { turn_id })
    }

    /// Called with the matching finishing task's idle reservation still held.
    pub(crate) async fn record_finished_review(&self, event: &EventMsg) {
        if codex_alab_core::is_alab_binary()
            && let Some(id) = finding(event)
        {
            self.state.lock().await.alab_queue_review = Some(id.to_owned());
        }
    }
}

pub(super) fn explicit_input(request: &TurnInputRequest) -> bool {
    matches!(&request.input, TurnInput::UserInput { content, .. } if !content.is_empty())
        && match request.alab_queue_dispatch {
            Some(QueueDispatch::Explicit) => request.start.turn_trigger.as_deref() == Some("queue"),
            Some(QueueDispatch::Automatic) => false,
            None => person(request.start.turn_trigger.as_deref()),
        }
        && request.start.parent_turn_id.is_none()
        && request.start.initiating_agent_path.is_none()
}

/// After both admission decisions have read the restrictive trigger, preserve an
/// actual human message's ordinary turn attribution if its fallback is admitted.
pub(super) fn normalize_observed_steer_start(request: &mut TurnInputRequest) {
    if request.start.turn_trigger.as_deref() == Some(OBSERVED_STEER_FALLBACK_TRIGGER)
        && request.alab_queue_dispatch.is_none()
        && request.start.parent_turn_id.is_none()
        && request.start.initiating_agent_path.is_none()
        && request.start.root_turn_id.is_none()
        && matches!(&request.input, TurnInput::UserInput { content, .. } if !content.is_empty())
    {
        request.start.turn_trigger = Some("user".into());
    }
}

struct ManualQueueReview;

pub(super) fn mark_manual_queue(turn: &TurnContext) {
    turn.extension_data.insert(ManualQueueReview);
}

/// Keep the original queue trigger for provider metadata, helper admission and future wakes.
pub(super) fn recorded_attribution(turn: &TurnContext) -> codex_history::TurnAttribution {
    let mut source = turn.attribution();
    if turn.extension_data.get::<ManualQueueReview>().is_some() {
        source.turn_trigger = Some(QUEUE_REVIEW_TRIGGER.to_owned());
    } else if source.turn_trigger.as_deref() == Some(QUEUE_REVIEW_TRIGGER) {
        // This string is meaningful only when Core vouched for this particular selection.
        source.turn_trigger = Some("queue".to_owned());
    }
    source
}

fn finding(event: &EventMsg) -> Option<&str> {
    match event {
        EventMsg::TurnComplete(done)
            if done
                .error
                .as_ref()
                .and_then(|e| e.codex_error_info.as_ref())
                == Some(&CodexErrorInfo::MisalignmentPolicyViolation) =>
        {
            Some(&done.turn_id)
        }
        _ => None,
    }
}

fn person(trigger: Option<&str>) -> bool {
    matches!(trigger, None | Some("user"))
}

fn user_item(event: &EventMsg) -> Option<&str> {
    match event {
        EventMsg::ItemCompleted(done) if matches!(&done.item, TurnItem::UserMessage(item) if !item.content.is_empty()) => {
            Some(&done.turn_id)
        }
        _ => None,
    }
}

pub(super) async fn input_decision(
    session: &Session,
    turn: &TurnContext,
    metadata: &super::UserInputMetadata,
    input: &[codex_protocol::user_input::UserInput],
) -> Option<InputReview> {
    if !codex_alab_core::is_alab_binary() {
        return None;
    }
    if let Some(order) = turn
        .extension_data
        .get::<codex_alab_core::goal_dispatch::TurnInputOrder>()
    {
        order.record(metadata.acceptance_order);
    }
    let state = session.state.lock().await;
    let finding = state.alab_queue_review.as_ref()?;
    let id = turn.sub_id.as_str();
    let corrects = finding != id
        && state.last_started_turn_id.as_deref() == Some(id)
        && metadata.alab_explicit_input
        && metadata.acceptance_order.is_some()
        && !input.is_empty()
        && (person(turn.turn_metadata_state.current_turn_trigger().as_deref())
            || turn.extension_data.get::<ManualQueueReview>().is_some())
        && turn.turn_metadata_state.parent_turn_id().is_none()
        && turn.turn_metadata_state.initiating_agent_path().is_none();
    Some(if corrects {
        InputReview::Corrected(finding.clone())
    } else {
        InputReview::Held(finding.clone())
    })
}

/// Reserve the observer before moving the input into the queue. Core calls it
/// only once the write succeeds, still holding this regular turn's admission lock.
pub(super) fn goal_steer(turn: &TurnContext, input: &super::TurnInput) -> impl FnOnce() + use<> {
    let tracked = match input {
        super::TurnInput::UserInput { metadata, .. } => metadata.acceptance_order.zip(
            turn.extension_data
                .get::<codex_alab_core::goal_dispatch::TurnInputOrder>(),
        ),
        _ => None,
    };
    move || {
        if let Some((order, tracked)) = tracked {
            tracked.accept_steer(order);
        }
    }
}

pub(super) fn history_metadata(
    order: Option<u64>,
    review: Option<InputReview>,
) -> Option<CodexHarnessMetadata> {
    (order.is_some() || review.is_some()).then_some(CodexHarnessMetadata {
        user_input_order: order,
        alab_input_review_v1: review,
        ..Default::default()
    })
}

pub(super) async fn acknowledge(
    session: &Session,
    turn: &TurnContext,
    review: Option<InputReview>,
) {
    let Some(InputReview::Corrected(finding)) = review else {
        return;
    };
    let mut state = session.state.lock().await;
    if state.last_started_turn_id.as_deref() == Some(turn.sub_id.as_str())
        && state.alab_queue_review.as_ref() == Some(&finding)
    {
        state.alab_queue_review = None;
    }
}

// Earlier writers could leave only presentation events. A turn-stamped raw
// item needs the old client's identity; a known noncorrection has no fallback.
enum Projection {
    Unscoped,
    Client,
    Ignore,
}

impl Projection {
    fn can_acknowledge(&self, client: Option<&str>, visible: bool) -> bool {
        match self {
            Self::Unscoped => client.is_some() || visible,
            Self::Client => client.is_some(),
            Self::Ignore => false,
        }
    }
}

pub(super) fn restore(items: &[RolloutItem]) -> Option<String> {
    if !codex_alab_core::is_alab_binary() {
        return None;
    }
    restored_finding(items)
}

/// The recorded terminal that automatic producers still owe a direct user correction.
pub fn pending_misalignment_review(items: &[RolloutItem]) -> Option<&TurnCompleteEvent> {
    if !items
        .iter()
        .any(|item| matches!(item, RolloutItem::EventMsg(e) if finding(e).is_some()))
    {
        return None;
    }
    let discarded = rollback::discarded_turns(items);
    let mut seen = HashSet::new();
    let mut latest = None;
    let mut running = false;
    let mut person_turn = None;
    let mut legacy_person = false;
    let mut regular_turn = false;
    let mut projection = Projection::Unscoped;
    let mut held = None;
    for item in items {
        let event = match item {
            RolloutItem::EventMsg(event) => event,
            RolloutItem::TurnContext(context) => {
                if person_turn.is_some() && context.turn_id.as_deref() == person_turn {
                    regular_turn = true;
                }
                continue;
            }
            RolloutItem::ResponseItem(response) if response.item.is_user_message() => {
                if let Some(candidate) = person_turn {
                    let owner = response.item.turn_id();
                    if let Some(owner) = owner {
                        legacy_person = candidate == owner;
                    }
                    let metadata = response.metadata.as_ref();
                    let decision = metadata.and_then(|data| data.alab_input_review_v1.as_ref());
                    let foreign = metadata.is_some_and(|data| {
                        data.inherited_user_message
                            || data.compaction_output
                            || data.client_authored
                    }) || !UserInputOrigin::from_message(&response.item).is_user();
                    if !foreign
                        && owner == Some(candidate)
                        && let Some(InputReview::Corrected(id)) = decision
                        && held.is_some_and(|finding: &TurnCompleteEvent| finding.turn_id == *id)
                    {
                        held = None;
                    }
                    // A UI/client ID is a compatibility fallback only for old data.
                    // An explicit negative, conflicting, or inherited receipt cannot
                    // acquire a different meaning from its following projection.
                    projection = if decision.is_some() || foreign || !legacy_person {
                        Projection::Ignore
                    } else if owner.is_some() {
                        Projection::Client
                    } else {
                        Projection::Unscoped
                    };
                }
                continue;
            }
            _ => continue,
        };
        match event {
            EventMsg::TurnStarted(start) => {
                let id = start.turn_id.as_str();
                if !id.trim().is_empty() && id.len() <= 512 && seen.insert(id) {
                    latest = Some(id);
                    running = true;
                    let direct = start.turn_attribution.as_ref().is_none_or(|source| {
                        source.turn_id == id
                            && (person(source.turn_trigger.as_deref())
                                || source.turn_trigger.as_deref() == Some(QUEUE_REVIEW_TRIGGER))
                            && source.parent_turn_id.is_none()
                            && source.initiating_agent_path.is_none()
                    });
                    person_turn = (!discarded.contains(id) && direct).then_some(id);
                    legacy_person = person_turn.is_some();
                    // Idle realtime transcript recording also emits an unattributed
                    // start and a complete user item. Older regular turns can prove
                    // their origin with their matching context and input evidence.
                    regular_turn = direct && start.turn_attribution.is_some();
                    projection = Projection::Unscoped;
                } else if latest != Some(id) {
                    // A legacy message has no turn ID to distinguish this replay from a correction.
                    legacy_person = false;
                }
            }
            EventMsg::TurnComplete(done) => {
                let id = done.turn_id.as_str();
                let unseen = seen.insert(id);
                if latest == Some(id) || (!running && unseen) {
                    latest = Some(id);
                    running = false;
                    person_turn = None;
                    if !discarded.contains(id) && finding(event).is_some() {
                        held = Some(done);
                    }
                }
            }
            // Legacy rollouts persist the user item without a turn ID. Only an open,
            // newly accepted person turn after the finding can own it.
            EventMsg::UserMessage(message)
                if held.is_some()
                    && person_turn.is_some()
                    && legacy_person
                    && regular_turn
                    && projection.can_acknowledge(
                        message.client_id.as_deref(),
                        user_message_preview(message).is_some(),
                    ) =>
            {
                held = None;
            }
            EventMsg::TurnAborted(abort) => {
                if let Some(id) = abort.turn_id.as_deref() {
                    seen.insert(id);
                }
                if abort.turn_id.is_none() || abort.turn_id.as_deref() == latest {
                    person_turn = None;
                    running = false;
                }
            }
            EventMsg::ItemCompleted(item)
                if regular_turn
                    && person_turn.is_some_and(|id| user_item(event) == Some(id))
                    && matches!(&item.item, TurnItem::UserMessage(user) if projection.can_acknowledge(user.client_id.as_deref(), true)) =>
            {
                held = None
            }
            _ => {}
        }
    }
    held
}

fn restored_finding(items: &[RolloutItem]) -> Option<String> {
    pending_misalignment_review(items).map(|finding| finding.turn_id.clone())
}

#[cfg(test)]
#[path = "queue_review_tests.rs"]
mod tests;
