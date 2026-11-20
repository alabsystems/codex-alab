// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::Session;
use super::restored_finding;
use codex_history::ResponseItemEnvelope;
use codex_history::RolloutItem;
use codex_protocol::ThreadId;
use codex_protocol::items::TurnItem;
use codex_protocol::items::UserMessageItem;
use codex_protocol::models::ResponseInputItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::ItemCompletedEvent;
use codex_protocol::protocol::ThreadHistoryMode;
use codex_protocol::user_input::UserInput;
use serde_json::Value;
use serde_json::json;

fn event(wire: Value) -> RolloutItem {
    RolloutItem::EventMsg(serde_json::from_value(wire).expect("valid persisted event"))
}

fn start(id: &str, trigger: &str) -> RolloutItem {
    event(json!({"type": "task_started", "turn_id": id,
                 "turn_attribution": {"turn_id": id, "turn_trigger": trigger}}))
}

fn complete(id: &str) -> RolloutItem {
    event(json!({"type": "task_complete", "turn_id": id}))
}

fn refusal(id: &str) -> RolloutItem {
    event(json!({"type": "task_complete", "turn_id": id, "error": {
        "message": "Review the cleanup.", "codex_error_info": "misalignment_policy_violation"
    }}))
}

fn accept(history: &mut Vec<RolloutItem>, turn_id: &str) {
    accept_item(
        history,
        turn_id,
        &[UserInput::Text {
            text: "List the generated files.".into(),
            text_elements: Vec::new(),
        }],
    );
}

fn accept_item(history: &mut Vec<RolloutItem>, turn_id: &str, input: &[UserInput]) {
    projected(history, turn_id, UserMessageItem::new(input));
}

fn projected(history: &mut Vec<RolloutItem>, turn_id: &str, item: UserMessageItem) {
    let legacy = item.as_legacy_event();
    history.push(RolloutItem::EventMsg(EventMsg::ItemCompleted(
        ItemCompletedEvent {
            thread_id: ThreadId::from_u128(1),
            turn_id: turn_id.into(),
            item: TurnItem::UserMessage(item),
            content_token_count: None,
            started_at_ms: None,
            completed_at_ms: 1,
        },
    )));
    history.push(RolloutItem::EventMsg(legacy));
}

fn assert_review(history: &[RolloutItem], expected: Option<&str>) {
    for mode in [ThreadHistoryMode::Legacy, ThreadHistoryMode::Paginated] {
        let persisted = codex_rollout::persisted_rollout_items(history, mode);
        assert_eq!(
            restored_finding(&persisted).as_deref(),
            expected,
            "{mode:?}"
        );
    }
}

#[test]
fn only_fresh_accepted_user_input_releases_a_restored_finding() {
    let mut history = vec![start("refused", "user"), refusal("refused")];
    history.extend([start("refused", "user"), complete("refused")]);
    accept(&mut history, "refused");
    assert_review(&history, Some("refused"));

    history.push(start("correction", "user"));
    assert_review(&history, Some("refused"));
    accept(&mut history, "correction");
    history.extend([
        complete("correction"),
        refusal("refused"),
        start("refused", "user"),
        complete("refused"),
    ]);
    assert_review(&history, None);
}

#[test]
fn queued_starts_and_automated_activity_do_not_release_a_restored_finding() {
    let mut history = vec![
        start("refused", "user"),
        refusal("refused"),
        start("queued", "queue"),
    ];
    history.push(RolloutItem::InterAgentCommunication(
        serde_json::from_value(json!({
            "author": "/root/helper", "recipient": "/root", "content": "Helper finished.",
            "trigger_turn": true
        }))
        .expect("valid agent message"),
    ));
    history.push(complete("queued"));
    // A delayed user item from the closed queue turn cannot acknowledge the finding.
    accept(&mut history, "queued");
    history.push(start("agent", "automation"));
    accept(&mut history, "agent");
    history.push(complete("agent"));
    assert_review(&history, Some("refused"));

    // Older producers persisted "queue" for both automatic and explicit starts.
    history.push(start("ambiguous-queue", "queue"));
    accept(&mut history, "ambiguous-queue");
    history.push(complete("ambiguous-queue"));
    history.push(event(json!({"type": "task_started", "turn_id": "delegated",
        "turn_attribution": {"turn_id": "delegated", "turn_trigger": "user", "parent_turn_id": "parent"}})));
    accept(&mut history, "delegated");
    assert_review(&history, Some("refused"));

    history.push(start("explicit-queue-start", "queue_review"));
    accept(&mut history, "explicit-queue-start");
    assert_review(&history, None);
}

#[test]
fn stale_turn_boundaries_cannot_supply_a_correction_or_displace_a_fresh_one() {
    let mut history = vec![
        start("old", "user"),
        complete("old"),
        start("refused", "user"),
        refusal("refused"),
        start("pending", "user"),
        start("old", "user"),
    ];
    accept(&mut history, "old");
    assert_review(&history, Some("refused"));
    history.push(start("fresh", "user"));
    history.extend([complete("old"), refusal("refused")]);
    accept(&mut history, "fresh");
    assert_review(&history, None);
}

#[test]
fn rolling_back_a_correction_restores_only_a_surviving_finding() {
    let mut history = vec![start("refused", "user")];
    accept(&mut history, "refused");
    history.extend([refusal("refused"), start("correction", "user")]);
    accept(&mut history, "correction");
    history.push(complete("correction"));
    assert_review(&history, None);

    let undo = event(json!({"type": "thread_rolled_back", "num_turns": 1}));
    history.push(undo.clone());
    assert_review(&history, Some("refused"));
    history.push(undo);
    assert_review(&history, None);
}

#[test]
fn a_sparse_finding_after_a_finished_turn_holds_until_genuine_correction() {
    let mut history = vec![start("old", "user"), complete("old"), refusal("sparse")];
    assert_review(&history, Some("sparse"));
    history.push(start("correction", "user"));
    history.push(refusal("unknown-during-correction"));
    assert_review(&history, Some("sparse"));
    accept(&mut history, "correction");
    history.extend([complete("correction"), refusal("sparse"), refusal("old")]);
    assert_review(&history, None);
}

fn raw_input(turn: &str, input: &[UserInput], metadata: Value) -> RolloutItem {
    let mut item = ResponseItem::from(ResponseInputItem::from(input.to_vec()));
    Session::stamp_response_item_for_history(&mut item, turn);
    RolloutItem::ResponseItem(ResponseItemEnvelope {
        item,
        metadata: serde_json::from_value(metadata).expect("persisted host metadata"),
    })
}

fn inferred_context(id: &str) -> RolloutItem {
    serde_json::from_value(json!({"type":"turn_context", "payload":{
        "turn_id":id, "cwd":"/tmp", "approval_policy":"never",
        "sandbox_policy":{"type":"read-only"}, "model":"test-model"
    }}))
    .expect("recorded context for the regular turn")
}

fn transcript_input() -> [UserInput; 1] {
    [UserInput::Text {
        text: "recorded session transcript".into(),
        text_elements: vec![],
    }]
}

#[test]
fn idle_and_active_history_only_transcripts_cannot_acknowledge_review() {
    let refused = vec![start("refused", "user"), refusal("refused")];
    for idle in [true, false] {
        let mut history = refused.clone();
        if idle {
            // The real recorder opens an unattributed history-only turn when idle.
            history.push(event(
                json!({"type":"task_started", "turn_id":"transcript"}),
            ));
        } else {
            // It joins the existing regular turn when that turn is still starting.
            history.push(start("transcript", "user"));
            history.push(inferred_context("transcript"));
        }
        let input = transcript_input();
        history.push(raw_input(
            "transcript",
            &input,
            json!({"user_input_order":1}),
        ));
        accept_item(&mut history, "transcript", &input);
        history.push(complete("transcript"));
        assert_review(&history, Some("refused"));
    }
}

#[test]
fn a_legacy_client_can_still_acknowledge_review_without_native_turn_attribution() {
    let mut history = vec![
        start("refused", "user"),
        refusal("refused"),
        event(json!({"type":"task_started", "turn_id":"older-client"})),
        inferred_context("older-client"),
    ];
    let input = transcript_input();
    history.push(raw_input(
        "older-client",
        &input,
        json!({"user_input_order":1}),
    ));
    let mut item = UserMessageItem::new(&input);
    item.client_id = Some("actual-client-message".into());
    projected(&mut history, "older-client", item);
    assert_review(&history, None);
}

#[test]
fn a_direct_correction_receipt_names_the_finding_and_its_own_turn() {
    let input = [UserInput::Mention {
        name: "files".into(),
        path: "app://files".into(),
    }];
    for (trigger, raw_turn, original, inherited, expected) in [
        ("user", "explicit", "refused", false, None),
        ("user", "previous", "refused", false, Some("refused")),
        ("user", "explicit", "previous", false, Some("refused")),
        ("user", "explicit", "refused", true, Some("refused")),
        ("queue", "explicit", "refused", false, Some("refused")),
    ] {
        let mut history = vec![
            start("refused", "user"),
            refusal("refused"),
            start("explicit", trigger),
        ];
        // The structured input has no text/media projection in a legacy rollout.
        // In the stale-turn case an independently projected transcript follows it.
        history.push(raw_input(
            raw_turn,
            &input,
            json!({"user_input_order":1,
            "inherited_user_message":inherited, "alab_input_review_v1":{"corrected":original}}),
        ));
        if raw_turn != "explicit" {
            let transcript = transcript_input();
            accept_item(&mut history, "explicit", &transcript);
        } else {
            accept_item(&mut history, "explicit", &input);
        }
        assert_review(&history, expected);
    }
}

#[test]
fn a_client_message_id_cannot_override_an_automated_or_conflicting_input() {
    let text = [UserInput::Text {
        text: "Check progress.".into(),
        text_elements: vec![],
    }];
    for (owner, metadata, heartbeat) in [
        ("direct", json!({"user_input_order": 2}), true),
        (
            "direct",
            json!({"alab_input_review_v1": {"held":"refused"}}),
            false,
        ),
        (
            "direct",
            json!({"alab_input_review_v1": {"corrected":"different"}}),
            false,
        ),
        (
            "direct",
            json!({"alab_input_review_v1": {"corrected":"refused"}, "inherited_user_message":true}),
            false,
        ),
        (
            "earlier",
            json!({"alab_input_review_v1": {"corrected":"refused"}}),
            false,
        ),
    ] {
        let mut history = vec![
            start("refused", "user"),
            refusal("refused"),
            start("direct", "user"),
        ];
        let mut raw = raw_input(owner, &text, metadata);
        if heartbeat {
            // Old writers marked a scheduled single-text steer on its actual raw input.
            let RolloutItem::ResponseItem(envelope) = &mut raw else {
                unreachable!()
            };
            let ResponseItem::Message {
                internal_chat_message_metadata_passthrough: Some(origin),
                ..
            } = &mut envelope.item
            else {
                unreachable!()
            };
            origin.content_item_kinds = Some(vec![codex_protocol::models::ContentItemKind(
                codex_history::HEARTBEAT_CONTENT_KIND.to_owned(),
            )]);
        }
        history.push(raw);
        let mut item = UserMessageItem::new(&text);
        item.client_id = Some("message-with-client-id".into());
        projected(&mut history, "direct", item);
        assert_review(&history, Some("refused"));
        history.push(raw_input(
            "direct",
            &text,
            json!({"alab_input_review_v1":{"corrected":"refused"}}),
        ));
        accept_item(&mut history, "direct", &text);
        assert_review(&history, None);
    }
}
