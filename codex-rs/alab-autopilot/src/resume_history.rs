// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Native attempt boundaries outrank a session record whose busy bit has not caught up.

use crate::decide::ThreadState;
use crate::decide::Timing;
use crate::decide::TurnEnd;
use crate::decide::Wall;
use codex_history::RolloutItem;
use codex_protocol::error::CodexErrorDetails;
use codex_protocol::protocol::CodexErrorInfo;
use codex_protocol::protocol::ErrorEvent;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::TurnAbortReason;
use codex_protocol::protocol::TurnAbortedEvent;
use codex_protocol::protocol::TurnCompleteEvent;
use std::time::Duration;
use std::time::Instant;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResumeSource {
    Other,
    Crash,
    /// Only after the existing crash proof also names HUP/TERM on the predecessor.
    Signal,
}

impl From<bool> for ResumeSource {
    fn from(relaunched: bool) -> Self {
        if relaunched { Self::Crash } else { Self::Other }
    }
}

impl ResumeSource {
    pub(crate) fn carries_on(self) -> bool {
        matches!(self, Self::Crash | Self::Signal)
    }
}

#[derive(Clone, Copy, Default)]
pub(crate) enum NativePoint<'a> {
    #[default]
    None,
    Running,
    Complete(&'a TurnCompleteEvent),
    Aborted(&'a TurnAbortedEvent),
    Superseded,
}

#[derive(Default)]
pub(crate) struct NativeHistory<'a> {
    pub(crate) turn_id: Option<&'a str>,
    pub(crate) started: bool,
    pub(crate) by_autopilot: bool,
    pub(crate) by_person: bool,
    pub(crate) saw_regular_input: bool,
    pub(crate) compacting: bool,
    pub(crate) policy_exhausted: bool,
    pub(crate) point: NativePoint<'a>,
    /// Only a first exact interruption of Running can be a signal teardown.
    /// Later accepted terminals revoke that exception until another native start.
    pub(crate) interrupted_running_turn: bool,
}

/// Whether durable history still names this exact unfinished regular turn.
/// This is a recovery precondition, never account or execution authority.
pub fn unfinished_recovery_turn(items: &[RolloutItem], turn: &str) -> bool {
    // A recovery records another start for the same logical turn, without a
    // second user input. Fold only those repeated starts: preserve terminals
    // and compaction state so a completed or aborted turn cannot be resurrected.
    let mut previous: Option<&codex_protocol::protocol::TurnStartedEvent> = None;
    let mut logical = Vec::with_capacity(items.len());
    for item in items {
        if let RolloutItem::EventMsg(EventMsg::TurnStarted(start)) = item {
            if let Some(earlier) = previous
                && earlier.turn_id == start.turn_id
            {
                if earlier.turn_attribution != start.turn_attribution {
                    return false;
                }
                continue;
            }
            previous = Some(start);
        }
        logical.push(item.clone());
    }
    let history = latest(&logical);
    valid_turn_id(turn)
        && history.turn_id == Some(turn)
        && history.started
        && history.saw_regular_input
        && !history.compacting
        && matches!(history.point, NativePoint::Running)
}

pub(crate) fn latest(items: &[RolloutItem]) -> NativeHistory<'_> {
    let mut history = NativeHistory::default();
    let mut known_turn_ids = std::collections::HashSet::new();
    let mut compacting = std::collections::HashSet::new();
    for item in items {
        // A regular child turn can begin with accepted agent mail rather than
        // a user-message event. Its stored turn attribution must match exactly.
        if history.started
            && matches!(history.point, NativePoint::Running)
            && history
                .turn_id
                .is_some_and(|turn| accepted_agent_input(item, turn))
        {
            history.saw_regular_input = true;
        }
        let RolloutItem::EventMsg(event) = item else {
            continue;
        };
        match event {
            EventMsg::TurnStarted(started) => {
                compacting.clear();
                if valid_turn_id(&started.turn_id) {
                    known_turn_ids.insert(started.turn_id.as_str());
                }
                let trigger = started
                    .turn_attribution
                    .as_ref()
                    .and_then(|attribution| attribution.turn_trigger.as_deref());
                history = NativeHistory {
                    turn_id: Some(started.turn_id.as_str()),
                    started: true,
                    by_autopilot: trigger == Some(crate::extension::TURN_TRIGGER),
                    by_person: trigger.is_none_or(|trigger| matches!(trigger, "user" | "queue")),
                    saw_regular_input: false,
                    compacting: false,
                    policy_exhausted: false,
                    point: NativePoint::Running,
                    interrupted_running_turn: false,
                };
            }
            EventMsg::UserMessage(_)
                if matches!(
                    history.point,
                    NativePoint::Complete(_) | NativePoint::Aborted(_) | NativePoint::Superseded
                ) =>
            {
                history.point = NativePoint::Superseded;
            }
            EventMsg::UserMessage(_)
                if history.started && matches!(history.point, NativePoint::Running) =>
            {
                history.saw_regular_input = true;
            }
            EventMsg::AgentMessage(message)
                if history.started && matches!(history.point, NativePoint::Running) =>
            {
                history.policy_exhausted |= exhausted(&message.message);
            }
            EventMsg::ItemStarted(event)
                if history.turn_id == Some(event.turn_id.as_str())
                    && matches!(history.point, NativePoint::Running) =>
            {
                if let codex_protocol::items::TurnItem::ContextCompaction(item) = &event.item {
                    compacting.insert(item.id.as_str());
                    history.compacting = true;
                }
            }
            EventMsg::ItemCompleted(event)
                if history.turn_id == Some(event.turn_id.as_str())
                    && matches!(history.point, NativePoint::Running) =>
            {
                match &event.item {
                    codex_protocol::items::TurnItem::UserMessage(_) => {
                        history.saw_regular_input = true
                    }
                    codex_protocol::items::TurnItem::ContextCompaction(item) => {
                        compacting.remove(item.id.as_str());
                        history.compacting = !compacting.is_empty();
                    }
                    codex_protocol::items::TurnItem::AgentMessage(message) => {
                        history.policy_exhausted |=
                            message.content.iter().any(|content| match content {
                                codex_protocol::items::AgentMessageContent::Text { text } => {
                                    exhausted(text)
                                }
                            });
                    }
                    _ => {}
                }
            }
            EventMsg::TurnAborted(aborted)
                if !matches!(history.point, NativePoint::Superseded)
                    // Stop/interrupt hooks may finish after a newer native start.
                    // Ignore only a positively known retired identity; a missing
                    // or unknown mismatched ID still revokes automatic action.
                    && !aborted.turn_id.as_deref().is_some_and(|id| {
                        Some(id) != history.turn_id && known_turn_ids.contains(id)
                    }) =>
            {
                history.interrupted_running_turn = matches!(history.point, NativePoint::Running)
                    && history.started
                    && aborted.turn_id.as_deref() == history.turn_id
                    && aborted.reason == TurnAbortReason::Interrupted;
                history.point = NativePoint::Aborted(aborted);
            }
            EventMsg::TurnComplete(complete) => {
                // A matching completion also closes the unfinished-turn exception
                // when cancellation already owns the terminal presentation.
                if history.turn_id == Some(complete.turn_id.as_str()) {
                    history.interrupted_running_turn = false;
                }
                if !matches!(
                    history.point,
                    NativePoint::Aborted(_) | NativePoint::Superseded
                ) && (!history.started
                    || history.turn_id.is_none_or(|id| id == complete.turn_id))
                {
                    if valid_turn_id(&complete.turn_id) {
                        known_turn_ids.insert(complete.turn_id.as_str());
                    }
                    history.turn_id = Some(complete.turn_id.as_str());
                    history.point = NativePoint::Complete(complete);
                }
            }
            _ => {}
        }
    }
    history
}

fn accepted_agent_input(item: &RolloutItem, turn: &str) -> bool {
    let (metadata, has_content, has_participants) = match item {
        RolloutItem::ResponseItem(envelope) => {
            let codex_protocol::models::ResponseItem::AgentMessage {
                author,
                recipient,
                content,
                internal_chat_message_metadata_passthrough,
                ..
            } = &envelope.item
            else {
                return false;
            };
            (
                internal_chat_message_metadata_passthrough.as_ref(),
                !content.is_empty(),
                !author.is_empty() && !recipient.is_empty(),
            )
        }
        RolloutItem::InterAgentCommunication(mail) => (
            mail.internal_chat_message_metadata_passthrough.as_ref(),
            !mail.content.is_empty()
                || mail
                    .encrypted_content
                    .as_ref()
                    .is_some_and(|value| !value.is_empty()),
            true,
        ),
        _ => return false,
    };
    has_content
        && has_participants
        && metadata.and_then(|metadata| metadata.turn_id.as_deref()) == Some(turn)
}

/// WHETHER A PERSON STOPPED THE THREAD'S GOAL SINCE NATIVE TURN `turn` BEGAN, as its durable
/// history says (review of the forever-persist lane, F2): upstream records each goal set a person
/// makes through the app-server (`thread/goal/set` with a `user` origin and an objective or a
/// status) as a host-annotated `user.goal` instruction in the thread's rollout, in the order it
/// came, before the store takes the set. One after `turn`'s start that does not set the goal
/// active (a status but active, a clear, or an objective alone, whose resulting status the
/// history does not say) is the person's hand on the goal the turn's wall blocked, so the image
/// after a re-exec or a relaunch binds no such goal again, even where the process before died
/// before its record took the stop out of its pace. A history that names no start of `turn`
/// cannot place the stop either way, and reads as one: a person's stop is never a wall's to
/// resume.
pub(crate) fn person_goal_stop_since(items: &[RolloutItem], turn: &str) -> bool {
    let Some(start) = items.iter().position(|item| {
        matches!(item, RolloutItem::EventMsg(EventMsg::TurnStarted(started)) if started.turn_id == turn)
    }) else {
        return true;
    };
    items[start..]
        .iter()
        .filter_map(person_goal_set)
        .any(|active| !active)
}

/// A PERSON'S GOAL SET AS UPSTREAM RECORDS IT ([`person_goal_stop_since`]): a user message of
/// one text whose host annotation (never its words alone) names it a `user.goal` instruction;
/// whether it set the goal active (one status line naming `active`, and no clear).
fn person_goal_set(item: &RolloutItem) -> Option<bool> {
    let RolloutItem::ResponseItem(envelope) = item else {
        return None;
    };
    let codex_protocol::models::ResponseItem::Message {
        role,
        content,
        internal_chat_message_metadata_passthrough: Some(metadata),
        ..
    } = &envelope.item
    else {
        return None;
    };
    let [kind] = metadata.content_item_kinds.as_deref()? else {
        return None;
    };
    let [codex_protocol::models::ContentItem::InputText { text }] = content.as_slice() else {
        return None;
    };
    if role != "user" || !matches!(kind.as_str(), "user.goal" | "user.goal.omitted") {
        return None;
    }
    let active = format!(
        "User set goal status: {}.",
        serde_json::json!(codex_protocol::protocol::ThreadGoalStatus::Active)
    );
    // Each statement is a line of its own; an objective is one JSON string, which holds no
    // newline, so none of its words can make a line.
    let mut set_active = false;
    for line in text.lines().map(str::trim) {
        if line == "User cleared the goal." {
            return Some(false);
        }
        if line.starts_with("User set goal status: ") {
            if line != active {
                return Some(false);
            }
            set_active = true;
        }
    }
    Some(set_active)
}

/// THE SESSION WORKED BEFORE THIS PROCESS, as its history says (aterm 791977cae8's
/// `seed_worked` and `point_seed`, read from codex's own books; gap 32): an act codex-alab typed
/// (a `TurnStarted` with its trigger, cut or not) or a turn that ended (`TurnComplete`,
/// `TurnAborted`). A turn that merely ran is no work, nor the opening turn a restart cut: one
/// with no terminal (a crash), or the running turn a signal's teardown interrupted
/// ([`ResumeSource::Signal`] over [`NativeHistory::interrupted_running_turn`]).
pub(crate) fn worked(items: &[RolloutItem], source: ResumeSource) -> bool {
    let mut ended = 0_usize;
    for item in items {
        let RolloutItem::EventMsg(event) = item else {
            continue;
        };
        match event {
            EventMsg::TurnStarted(started)
                if started
                    .turn_attribution
                    .as_ref()
                    .and_then(|attribution| attribution.turn_trigger.as_deref())
                    == Some(crate::extension::TURN_TRIGGER) =>
            {
                return true;
            }
            EventMsg::TurnComplete(_) | EventMsg::TurnAborted(_) => {
                ended = ended.saturating_add(1);
            }
            _ => {}
        }
    }
    let torn = source == ResumeSource::Signal && latest(items).interrupted_running_turn;
    ended > usize::from(torn)
}

fn exhausted(text: &str) -> bool {
    text.lines()
        .any(|line| line.trim() == crate::words::POLICY_EXHAUSTED)
}

/// Keep an earlier assistant's stop marker when a native completion selected a
/// different final answer. A user quotation or a foreign item grants no stop proof.
pub(crate) fn terminal_policy_exhausted(items: &[RolloutItem]) -> bool {
    let native = latest(items);
    native.policy_exhausted && matches!(native.point, NativePoint::Complete(_))
}

fn valid_turn_id(id: &str) -> bool {
    !id.trim().is_empty() && id.len() <= 512
}

/// Compaction starts carry no user item and select no new account program. Restore the
/// last regular message's native context, including an explicit Standard/None selection.
pub(crate) fn cyber_program(
    items: &[RolloutItem],
) -> Option<codex_protocol::turn_input::CyberAccessProgram> {
    let mut current = None;
    let mut regular = None;
    for item in items {
        match item {
            RolloutItem::EventMsg(EventMsg::TurnStarted(started)) => {
                current = Some(started.turn_id.as_str())
            }
            RolloutItem::EventMsg(EventMsg::UserMessage(_)) => regular = current,
            _ => {}
        }
    }
    let regular = regular?;
    items
        .iter()
        .rev()
        .find_map(|item| match item {
            RolloutItem::TurnContext(context) if context.turn_id.as_deref() == Some(regular) => {
                Some(context.cyber_access_program)
            }
            _ => None,
        })
        .flatten()
}

/// An attributed harness start after a refusal spends the one continuation, even when
/// its message has not reached disk yet. The fixed submitted instruction is additional
/// evidence. A copied banner, another agent or a person's quotation never grants authority.
/// A later explicit person turn starts a new sequence.
pub(crate) fn policy_continued(items: &[RolloutItem]) -> bool {
    let mut attempted = false;
    let mut ours = false;
    let mut turn_id = None;
    let mut refused = false;
    let mut unattributed_person = false;
    for item in items {
        let RolloutItem::EventMsg(event) = item else {
            continue;
        };
        match event {
            EventMsg::TurnStarted(started) => {
                let trigger = started
                    .turn_attribution
                    .as_ref()
                    .and_then(|attribution| attribution.turn_trigger.as_deref());
                ours = trigger == Some(crate::extension::TURN_TRIGGER);
                unattributed_person = started.turn_attribution.is_some() && trigger.is_none();
                if trigger == Some("user") || trigger == Some("queue") {
                    attempted = false;
                    refused = false;
                }
                // Spending the continuation at its native start also covers a crash
                // before its UserMessage reaches disk. Ambiguity only restricts: this
                // never grants a request, changes access or reconstructs an owed act.
                attempted |= ours && refused;
                turn_id = Some(started.turn_id.as_str());
            }
            EventMsg::TurnComplete(complete)
                if turn_id.is_none_or(|id| id == complete.turn_id)
                    && complete
                        .error
                        .as_ref()
                        .and_then(|error| error.codex_error_info.as_ref())
                        == Some(&CodexErrorInfo::CyberPolicy) =>
            {
                refused = true;
                unattributed_person = false;
            }
            _ if unattributed_person
                && (matches!(event, EventMsg::UserMessage(_))
                    || matches!(event, EventMsg::ItemCompleted(done)
                        if Some(done.turn_id.as_str()) == turn_id
                            && matches!(done.item, codex_protocol::items::TurnItem::UserMessage(_)))) =>
            {
                attempted = false;
                refused = false;
                unattributed_person = false;
            }
            EventMsg::TurnComplete(complete) if Some(complete.turn_id.as_str()) == turn_id => {
                unattributed_person = false;
            }
            EventMsg::TurnAborted(aborted) if aborted.turn_id.as_deref() == turn_id => {
                unattributed_person = false;
            }
            EventMsg::UserMessage(message)
                if ours
                    && message
                        .message
                        .strip_prefix(crate::words::POLICY_CONTINUE)
                        .is_some_and(|suffix| {
                            suffix.is_empty()
                                || suffix.strip_prefix(' ') == Some(crate::words::POLICY_BOUNDARY)
                        }) =>
            {
                attempted = true;
            }
            _ => {}
        }
    }
    attempted
}

/// Restore only attention or revocation here, after no authenticated Recovery could restore
/// the point with its original deadlines and native identity. A PERSON'S ESC THE RECORD CARRIES
/// FOR THE TURN THE HISTORY'S ABORT ENDED ([`ThreadState::esc_seed`]; the forever-persist lane,
/// item a) makes that abort the person's: stood at whatever the source (never a signal
/// teardown's, which the record would not name), held from the Esc as the process before held
/// it ([`ThreadState::esc_at`]).
pub(crate) fn restore_terminal(
    state: &mut ThreadState,
    items: &[RolloutItem],
    source: ResumeSource,
    at: Instant,
) {
    let history = latest(items);
    let esc = state
        .esc_seed
        .take()
        .filter(|(_, turn)| history.turn_id == Some(turn.as_str()));
    if matches!(history.point, NativePoint::Superseded) {
        state.carry_on = false;
        state.carry_on_not_before = None;
        state.owed = None;
        state.point = None;
        state.point_turn_id = None;
        state.recovery_not_before = None;
        return;
    }
    let (wall, aborted) = match history.point {
        NativePoint::Aborted(aborted) => {
            if esc.is_none()
                && history.interrupted_running_turn
                && (crate::owed_origin::interrupted_host_turn(state.owed.as_ref(), &history)
                    || (source == ResumeSource::Signal
                        && history.started
                        && aborted.reason == TurnAbortReason::Interrupted
                        && aborted
                            .turn_id
                            .as_deref()
                            .is_some_and(|id| Some(id) == history.turn_id)))
            {
                return;
            }
            (None, true)
        }
        NativePoint::Complete(complete) => {
            let Some(error) = complete
                .error
                .as_ref()
                .filter(|error| error.affects_turn_status())
            else {
                return;
            };
            let wall = recorded_wall(&history, error)
                .filter(|wall| matches!(wall, Wall::Fatal { .. } | Wall::Login | Wall::Policy));
            let Some(wall) = wall else { return };
            (Some(wall), false)
        }
        _ => return,
    };
    state.observe_turn_end(
        TurnEnd {
            at,
            by_autopilot: history.by_autopilot,
            by_person: history.by_person,
            text: None,
            earlier: None,
            questions: false,
            worked: Duration::ZERO,
            wall,
            aborted,
            ran_tools: false,
            restored: true,
        },
        &Timing::default(),
    );
    state.point_turn_id = history.turn_id.map(str::to_owned);
    if let Some((at, _)) = esc.filter(|_| aborted) {
        state.esc_at = Some(at);
    }
}

/// The wall a history's completed turn recorded (`error`, one that affects the turn's status),
/// classified as its turn's end would have been: any class, the caller keeping the ones it may
/// stand at.
fn recorded_wall(history: &NativeHistory<'_>, error: &ErrorEvent) -> Option<Wall> {
    let info = error
        .codex_error_info
        .as_ref()
        .unwrap_or(&CodexErrorInfo::Other);
    if *info == CodexErrorInfo::CyberPolicy && (!history.saw_regular_input || history.compacting) {
        return Some(Wall::Fatal {
            why: "a cybersecurity policy refused context compaction or its operation cannot be proven; automatic work cannot replay that history".into(),
        });
    }
    if *info == CodexErrorInfo::Other {
        // Native Other loses the distinction between fatal config/request errors and exhausted
        // response-processing errors. A saved Recovery can prove the latter; the completion
        // alone authorizes no automatic retry.
        return Some(Wall::Fatal {
            why: "the last turn failed and its detailed recovery state is unavailable".into(),
        });
    }
    crate::extension::classify(
        info,
        &CodexErrorDetails::Stream("recorded terminal refusal".into()),
        false,
    )
}

/// THE HISTORY'S LAST TURN END, STOOD AT AGAIN where a vanilla spell hid it from this memory
/// (`extension::rewatch_bound`, at the look that takes the spell in; edges-1, edges-2): a clean
/// end, an interrupt's, or a wall's of any class (a usage window's included, whose ladder then
/// waits it out), whoever's turn it was, at `at`, restored (the answer to no act of this
/// process), with the work its record says it did. FOREVER CRUISE: "Cruise never parks", so
/// the turns the person ran by hand under `/alab off` leave a point Cruise continues at and
/// Land carries on from, where without it nothing was typed until the person typed again. A
/// turn still running, or one a newer request superseded, stands at none: the end the runtime
/// sees next is the point. Returns whether a point stands.
pub(crate) fn restore_end(state: &mut ThreadState, items: &[RolloutItem], at: Instant) -> bool {
    let history = latest(items);
    let worked = |ms: Option<i64>| {
        ms.and_then(|ms| u64::try_from(ms).ok())
            .map_or(Duration::ZERO, Duration::from_millis)
    };
    let (text, wall, aborted, worked) = match history.point {
        NativePoint::Complete(complete) => (
            complete.last_agent_message.clone(),
            complete
                .error
                .as_ref()
                .filter(|error| error.affects_turn_status())
                .and_then(|error| recorded_wall(&history, error)),
            false,
            worked(complete.duration_ms),
        ),
        NativePoint::Aborted(aborted) => (None, None, true, worked(aborted.duration_ms)),
        NativePoint::None | NativePoint::Running | NativePoint::Superseded => return false,
    };
    state.observe_turn_end(
        TurnEnd {
            at,
            by_autopilot: history.by_autopilot,
            by_person: history.by_person,
            text,
            earlier: None,
            questions: false,
            worked,
            wall,
            aborted,
            ran_tools: false,
            restored: true,
        },
        &Timing::default(),
    );
    state.point_turn_id = history.turn_id.map(str::to_owned);
    true
}

#[cfg(test)]
#[path = "resume_history_tests.rs"]
mod tests;
