// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A resumed thread's history as the carry-on reads it, from a rollout in the paginated
//! store's form (whose `load_history` the store refuses): the lines are the end-to-end
//! run's own (2026-10-16), the turn ids shortened.

use pretty_assertions::assert_eq;

use crate::extension::scan_history;

const PERSON: &str = r#"{"timestamp":"2026-10-16T22:06:56.447Z","ordinal":1,"type":"event_msg","payload":{"type":"task_started","turn_attribution":{"turn_id":"t1","turn_trigger":"user","parent_turn_id":null,"initiating_agent_path":null,"root_turn_id":"t1"},"turn_id":"t1","root_turn_id":"t1","started_at":1792188416,"model_context_window":258400,"collaboration_mode_kind":"default"}}"#;
const DONE: &str = r#"{"timestamp":"2026-10-16T22:06:56.463Z","ordinal":13,"type":"event_msg","payload":{"type":"task_complete","root_turn_id":"t1","turn_id":"t1","last_agent_message":"Here is step one of the plan. I will continue when asked.","started_at":1792188416,"completed_at":1792188416,"duration_ms":30,"time_to_first_token_ms":28}}"#;
const OURS: &str = r#"{"timestamp":"2026-10-16T22:07:01.441Z","ordinal":14,"type":"event_msg","payload":{"type":"task_started","turn_attribution":{"turn_id":"t2","turn_trigger":"alab_autopilot","parent_turn_id":null,"initiating_agent_path":null,"root_turn_id":"t2"},"turn_id":"t2","root_turn_id":"t2","started_at":1792188421,"model_context_window":258400,"collaboration_mode_kind":"default"}}"#;

async fn scan(lines: &[&str]) -> (bool, Option<String>) {
    let dir = tempfile::tempdir().unwrap_or_else(|err| panic!("scratch dir: {err}"));
    let path = dir.path().join("rollout.jsonl");
    std::fs::write(&path, lines.join("\n") + "\n").unwrap_or_else(|err| panic!("{err}"));
    let (items, _, _) = codex_core::RolloutRecorder::load_rollout_items(&path)
        .await
        .unwrap_or_else(|err| panic!("rollout: {err}"));
    scan_history(&items)
}

#[tokio::test]
async fn a_paginated_rollout_names_the_task_and_the_last_reply() {
    assert_eq!(
        scan(&[PERSON, DONE, OURS]).await,
        (
            true,
            Some("Here is step one of the plan. I will continue when asked.".to_string())
        )
    );
    // Negative control: only codex-alab's own turns are no task.
    assert_eq!(scan(&[OURS]).await, (false, None));
}

const NEED: &str = r#"{"timestamp":"2026-10-16T22:06:56.463Z","ordinal":13,"type":"event_msg","payload":{"type":"task_complete","root_turn_id":"t1","turn_id":"t1","last_agent_message":"Stage 1 is in.\nNEEDS YOU: the prod API key","started_at":1792188416,"completed_at":1792188416,"duration_ms":30,"time_to_first_token_ms":28}}"#;
const SILENT: &str = r#"{"timestamp":"2026-10-16T22:07:31.463Z","ordinal":20,"type":"event_msg","payload":{"type":"task_complete","root_turn_id":"t2","turn_id":"t2","last_agent_message":"Widened the tests; two more cases pass.","started_at":1792188421,"completed_at":1792188451,"duration_ms":30,"time_to_first_token_ms":28}}"#;
const GOAL: &str = r#"{"timestamp":"2026-10-16T22:08:01.441Z","ordinal":21,"type":"event_msg","payload":{"type":"task_started","turn_attribution":{"turn_id":"t3","turn_trigger":"goal","parent_turn_id":null,"initiating_agent_path":null,"root_turn_id":"t3"},"turn_id":"t3","root_turn_id":"t3","started_at":1792188481,"model_context_window":258400,"collaboration_mode_kind":"default"}}"#;

async fn needs(lines: &[&str]) -> crate::decide::Needs {
    let dir = tempfile::tempdir().unwrap_or_else(|err| panic!("scratch dir: {err}"));
    let path = dir.path().join("rollout.jsonl");
    std::fs::write(&path, lines.join("\n") + "\n").unwrap_or_else(|err| panic!("{err}"));
    let (items, _, _) = codex_core::RolloutRecorder::load_rollout_items(&path)
        .await
        .unwrap_or_else(|err| panic!("rollout: {err}"));
    crate::extension::history_needs(&items)
}

/// THE NEEDS A HISTORY LEAVES STANDING (review of 2a9241bbf, findings 11, 15 and 18): a need
/// named in one report stands through a later report silent on it and a goal's own turn, and
/// counts as acked once a turn of codex-alab's began after it. NEGATIVE CONTROL: a person's
/// turn after it answered it.
#[tokio::test]
async fn a_history_leaves_the_needs_its_reports_named_standing() {
    let named = needs(&[PERSON, NEED]).await;
    assert_eq!(named.standing, ["the prod API key"]);
    assert_eq!(named.acked, None);
    let later = needs(&[PERSON, NEED, OURS, SILENT, GOAL]).await;
    assert_eq!(later.standing, named.standing);
    assert_eq!(later.acked, Some(named.key));
    // NEGATIVE CONTROL: the person's turn answered it.
    assert_eq!(
        needs(&[PERSON, NEED, PERSON, SILENT]).await,
        crate::decide::Needs::default()
    );
}

/// A PERSON'S TURN ANSWERS THE NEEDS AT ITS END, NEVER AT ITS START (aterm's `TurnEndState`
/// clears them at the point): a history that ends inside a person's turn (the process died
/// while it ran) leaves them standing. NEGATIVE CONTROL: the same turn, ended, clears them.
#[tokio::test]
async fn a_persons_turn_the_history_never_saw_end_leaves_the_needs_standing() {
    let open = needs(&[PERSON, NEED, PERSON]).await;
    assert_eq!(open.standing, ["the prod API key"]);
    assert_eq!(
        needs(&[PERSON, NEED, PERSON, SILENT]).await,
        crate::decide::Needs::default()
    );
}

const WALLED: &str = r#"{"timestamp":"2026-10-16T22:07:31.463Z","ordinal":20,"type":"event_msg","payload":{"type":"task_complete","root_turn_id":"t2","turn_id":"t2","last_agent_message":null,"error":{"message":"This request was refused by the cybersecurity access policy.","codex_error_info":"cyber_policy"},"started_at":1792188421,"completed_at":1792188451,"duration_ms":30}}"#;
const WALLED_SAYING: &str = r#"{"timestamp":"2026-10-16T22:07:31.463Z","ordinal":20,"type":"event_msg","payload":{"type":"task_complete","root_turn_id":"t2","turn_id":"t2","last_agent_message":"Stage 2 is half in.\nNEEDS YOU: the signing key","error":{"message":"stream disconnected before completion","codex_error_info":{"response_stream_disconnected":{"http_status_code":502}}},"started_at":1792188421,"completed_at":1792188451,"duration_ms":30}}"#;
const SAYING: &str = r#"{"timestamp":"2026-10-16T22:07:31.463Z","ordinal":20,"type":"event_msg","payload":{"type":"task_complete","root_turn_id":"t2","turn_id":"t2","last_agent_message":"Stage 2 is half in.\nNEEDS YOU: the signing key","started_at":1792188421,"completed_at":1792188451,"duration_ms":30}}"#;

const SAID_THEN_STOPPED: &str = r#"{"timestamp":"2026-10-16T22:07:30.000Z","ordinal":19,"type":"event_msg","payload":{"type":"agent_message","message":"Stage 2 is half in.\nNEEDS YOU: the signing key"}}"#;
const STOPPED: &str = r#"{"timestamp":"2026-10-16T22:07:31.463Z","ordinal":20,"type":"event_msg","payload":{"type":"turn_aborted","turn_id":"t2","reason":"interrupted"}}"#;

/// A TURN A WALL OR AN INTERRUPT ENDED ANSWERS NOTHING, AND NAMES THE NEEDS ITS COMPLETE
/// MESSAGES SAID, IN THE HISTORY (parity review S4; needs-wall-turn-unfolded), as the live
/// thread reads such a turn end (`ThreadState::observe_turn_end`): core keeps the turn's
/// terminal error on its completion event (`TurnCompleteEvent::error`, persisted whole in the
/// rollout), so a person's turn a wall ended leaves the need the report before it named
/// standing; and every agent message core completed is whole, the wall coming from a later
/// request of the turn, so a need it said stands (it was folded as S4 had it: none, and the
/// owner's NEEDS YOU line was lost for good). NEGATIVE CONTROLS: the same turns with no error,
/// a person's answering the need and codex-alab's naming one; a walled reply that names none
/// leaves none.
#[tokio::test]
async fn a_turn_a_wall_or_an_interrupt_ended_answers_no_need_and_names_its_own() {
    let walled = needs(&[PERSON, NEED, PERSON, WALLED]).await;
    assert_eq!(walled.standing, ["the prod API key"]);
    assert_eq!(
        needs(&[PERSON, NEED, PERSON, SILENT]).await,
        crate::decide::Needs::default(),
        "NEGATIVE CONTROL: the person's turn that ended answered it"
    );
    let cut = needs(&[PERSON, DONE, OURS, WALLED_SAYING]).await;
    assert_eq!(
        cut.standing,
        ["the signing key"],
        "a wall's turn names its need"
    );
    let stopped = needs(&[PERSON, DONE, OURS, SAID_THEN_STOPPED, STOPPED]).await;
    assert_eq!(
        stopped.standing,
        ["the signing key"],
        "an interrupted turn names its need"
    );
    assert_eq!(
        needs(&[PERSON, DONE, OURS, WALLED]).await,
        crate::decide::Needs::default(),
        "NEGATIVE CONTROL: a walled turn that said nothing names none"
    );
    let said = needs(&[PERSON, DONE, OURS, SAYING]).await;
    assert_eq!(
        said.standing,
        ["the signing key"],
        "NEGATIVE CONTROL: a reply that ended names its need"
    );
}

/// A RESUME AFTER aterm's RELAUNCH OR THE RE-EXEC PUTS THE HISTORY'S NEEDS BACK (review of
/// 2a9241bbf, findings 11 and 18): the exit took them down, and the resumed memory holds them
/// standing again, to be raised and acked. NEGATIVE CONTROL: any other resume (a person's own
/// `/resume`) restores none.
#[tokio::test]
async fn a_relaunch_or_the_re_exec_puts_the_historys_needs_back() {
    let dir = tempfile::tempdir().unwrap_or_else(|err| panic!("scratch dir: {err}"));
    let path = dir.path().join("rollout.jsonl");
    std::fs::write(&path, [PERSON, NEED].join("\n") + "\n").unwrap_or_else(|err| panic!("{err}"));
    let (items, _, _) = codex_core::RolloutRecorder::load_rollout_items(&path)
        .await
        .unwrap_or_else(|err| panic!("rollout: {err}"));
    let at = std::time::Instant::now();
    for (relaunched, owed, restored) in [
        (true, None, true),
        (false, Some((None, false)), true),
        (false, None, false),
    ] {
        let mut state = crate::decide::ThreadState::default();
        assert!(crate::extension::resume_into(
            &mut state, &items, relaunched, owed, 0, at
        ));
        let want: &[&str] = if restored { &["the prod API key"] } else { &[] };
        assert_eq!(state.needs.standing, want, "relaunched={relaunched}");
    }
}

const NEED_TOO: &str = r#"{"timestamp":"2026-10-16T22:08:31.463Z","ordinal":30,"type":"event_msg","payload":{"type":"task_complete","root_turn_id":"t3","turn_id":"t3","last_agent_message":"Stage 2 is in.\nNEEDS YOU: the signing key","started_at":1792188481,"completed_at":1792188511,"duration_ms":30,"time_to_first_token_ms":28}}"#;

async fn items(lines: &[&str]) -> Vec<codex_history::RolloutItem> {
    let dir = tempfile::tempdir().unwrap_or_else(|err| panic!("scratch dir: {err}"));
    let path = dir.path().join("rollout.jsonl");
    std::fs::write(&path, lines.join("\n") + "\n").unwrap_or_else(|err| panic!("{err}"));
    let (items, _, _) = codex_core::RolloutRecorder::load_rollout_items(&path)
        .await
        .unwrap_or_else(|err| panic!("rollout: {err}"));
    items
}

/// A NEED AN OFF TOOK DOWN STAYS DOWN ACROSS THE RE-EXEC AND THE RELAUNCH (review of the
/// forever autopilot, 2026-10-20): the history shows no Off, so the record's `needsOff` names
/// the set it took down, and the new image restores none of it. NEGATIVE CONTROLS: a record
/// naming no Off, or another set, restores the history's needs; and a set the worker named
/// after the Off (the history ends on it) is restored whatever the record says.
#[tokio::test]
async fn a_need_an_off_took_down_is_not_put_back_by_the_next_image() {
    let at = std::time::Instant::now();
    let named = items(&[PERSON, NEED]).await;
    let off = crate::extension::history_needs(&named).key;
    assert_ne!(off, 0);
    let resume = |items: &[codex_history::RolloutItem], relaunched, owed, needs_off| {
        let mut state = crate::decide::ThreadState::default();
        assert!(crate::extension::resume_into(
            &mut state, items, relaunched, owed, needs_off, at
        ));
        state.needs.standing
    };
    for (relaunched, owed) in [(true, None), (false, Some((None, false)))] {
        let why = format!("relaunched={relaunched}");
        assert_eq!(
            resume(&named, relaunched, owed.clone(), off),
            Vec::<String>::new(),
            "{why}"
        );
        // NEGATIVE CONTROL: no Off took a set down, or another set.
        assert_eq!(
            resume(&named, relaunched, owed.clone(), 0),
            ["the prod API key"],
            "{why}"
        );
        assert_eq!(
            resume(&named, relaunched, owed.clone(), off ^ 1),
            ["the prod API key"],
            "{why}"
        );
        // NEGATIVE CONTROL: the worker named a set after the Off; it stands.
        let later = items(&[PERSON, NEED, OURS, NEED_TOO]).await;
        assert_eq!(
            resume(&later, relaunched, owed, off),
            ["the signing key"],
            "{why}"
        );
    }
}

/// THE HISTORY'S NEEDS ARE READ OVER THE WHOLE TURN (aterm main e0da8d68df's `turn_said`): a
/// need said in an earlier agent message of a turn (a legacy history's `AgentMessage` event,
/// a paginated one's completed agent message item) stands beside the reply's, as the live
/// turn end folds it. NEGATIVE CONTROL: a turn whose only message is its reply, and earlier
/// messages that name no need, leave none.
#[tokio::test]
async fn a_need_said_earlier_in_a_turn_is_restored_from_the_history() {
    use codex_protocol::protocol::AgentMessageEvent;
    use codex_protocol::protocol::EventMsg;
    use codex_protocol::protocol::ItemCompletedEvent;
    let legacy = |text: &str| {
        codex_history::RolloutItem::EventMsg(EventMsg::AgentMessage(AgentMessageEvent {
            moderation: None,
            message: text.to_string(),
            phase: None,
            memory_citation: None,
            delivery: None,
            questions: None,
        }))
    };
    let paginated = |text: &str| {
        codex_history::RolloutItem::EventMsg(EventMsg::ItemCompleted(ItemCompletedEvent {
            content_token_count: None,
            thread_id: codex_protocol::ThreadId::new(),
            turn_id: "t1".to_string(),
            item: codex_protocol::items::TurnItem::AgentMessage(
                codex_protocol::items::AgentMessageItem {
                    moderation: None,
                    id: "agent-1".to_string(),
                    content: vec![codex_protocol::items::AgentMessageContent::Text {
                        text: text.to_string(),
                    }],
                    phase: None,
                    memory_citation: None,
                    delivery: None,
                    questions: None,
                },
            ),
            started_at_ms: None,
            completed_at_ms: 0,
        }))
    };
    let reply = "Here is step one of the plan. I will continue when asked.";
    let person = items(&[PERSON]).await;
    let done = items(&[DONE]).await;
    let turn =
        |said: Vec<codex_history::RolloutItem>| [person.clone(), said, done.clone()].concat();
    let need = "Pushed the branch.\nNEEDS YOU: the prod API key";
    for (form, said) in [
        ("legacy", vec![legacy(need), legacy(reply)]),
        ("paginated", vec![paginated(need), paginated(reply)]),
    ] {
        let needs = crate::extension::history_needs(&turn(said));
        assert_eq!(needs.standing, ["the prod API key"], "{form}");
    }
    // NEGATIVE CONTROLS.
    for (why, said) in [
        ("the reply alone", vec![legacy(reply)]),
        (
            "no need earlier",
            vec![paginated("Pushed the branch."), paginated(reply)],
        ),
    ] {
        assert_eq!(
            crate::extension::history_needs(&turn(said)),
            crate::decide::Needs::default(),
            "{why}"
        );
    }
}

// A compaction's turn as core wrote it (the end-to-end test
// `a_compactions_turn_answers_no_need_in_the_resumed_history`, 2026-10-21; ids shortened):
// its start names no attribution, and it holds no user message.
const COMPACT: &str = r#"{"timestamp":"2026-10-21T13:08:02.936Z","type":"event_msg","payload":{"type":"task_started","turn_id":"t4","root_turn_id":"t4","started_at":1792588082,"model_context_window":258400,"collaboration_mode_kind":"default"}}"#;
const COMPACTED: &str = r#"{"timestamp":"2026-10-21T13:08:02.959Z","type":"event_msg","payload":{"type":"context_compacted"}}"#;
const COMPACT_DONE: &str = r#"{"timestamp":"2026-10-21T13:08:02.967Z","type":"event_msg","payload":{"type":"task_complete","root_turn_id":"t4","turn_id":"t4","last_agent_message":null,"started_at":1792588082,"completed_at":1792588082,"duration_ms":30}}"#;
// The same unattributed start holding the person's message: a history from before core
// attributed turns.
const TYPED: &str = r#"{"timestamp":"2026-10-21T13:09:12.281Z","type":"event_msg","payload":{"type":"user_message","message":"here is the key","images":[],"local_images":[],"audio":[],"local_audio":[],"text_elements":[]}}"#;

/// A COMPACTION'S TURN IS NOBODY'S IN THE HISTORY (parity review R1), as the live thread reads
/// it (no `UserMessage` item opened it): it answers no need and is no person's task, whoever
/// asked for it. NEGATIVE CONTROLS: an unattributed turn that holds the person's message (a
/// history from before core attributed turns) is a person's, answering the need and naming
/// the task; and a paginated history's completed user message item counts as the event does.
#[tokio::test]
async fn a_compactions_turn_answers_nothing_and_is_no_task_in_the_history() {
    let compacted = needs(&[PERSON, NEED, COMPACT, COMPACTED, COMPACT_DONE]).await;
    assert_eq!(compacted.standing, ["the prod API key"]);
    assert_eq!(compacted.acked, None, "no act of codex-alab's acked it");
    assert_eq!(
        scan(&[OURS, COMPACT, COMPACTED, COMPACT_DONE]).await,
        (false, None),
        "codex-alab's own compaction after its own turn is no task"
    );
    // NEGATIVE CONTROLS: the person's message makes the unattributed turn theirs.
    assert_eq!(
        needs(&[PERSON, NEED, COMPACT, TYPED, COMPACT_DONE]).await,
        crate::decide::Needs::default()
    );
    assert_eq!(
        scan(&[OURS, COMPACT, TYPED, COMPACT_DONE]).await,
        (true, None)
    );
    let typed =
        codex_history::RolloutItem::EventMsg(codex_protocol::protocol::EventMsg::ItemCompleted(
            codex_protocol::protocol::ItemCompletedEvent {
                content_token_count: None,
                thread_id: codex_protocol::ThreadId::new(),
                turn_id: "t4".to_string(),
                item: codex_protocol::items::TurnItem::UserMessage(
                    codex_protocol::items::UserMessageItem::new(&[
                        codex_protocol::user_input::UserInput::Text {
                            text: "here is the key".to_string(),
                            text_elements: Vec::new(),
                        },
                    ]),
                ),
                started_at_ms: None,
                completed_at_ms: 0,
            },
        ));
    let before = items(&[PERSON, NEED, COMPACT]).await;
    let after = items(&[COMPACT_DONE]).await;
    let paginated = [before, vec![typed], after].concat();
    assert_eq!(
        crate::extension::history_needs(&paginated),
        crate::decide::Needs::default()
    );
    assert!(crate::extension::scan_history(&paginated).0);
}
