// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! THE SESSION'S OPENING AND THE WORD A TURN ENDS UNDER, at the runtime (review of gap 32):
//! a person's turn that ends in a passive spell is judged under Land's rule; a resumed thread
//! whose books the resume scan did not read, a late completion, a fork and a thread a vanilla
//! spell covered are each past the session's opening, so a person's quick message there is
//! carried under Land, never left to them; a new chat (`/new`, `/clear`) opens a session of its
//! own (aterm's `opened_fresh`), but a thread that carries on another's work does not; a
//! thread back from a vanilla spell stands at its own last turn end, the history read; and the
//! person's hand that a read racing the supervisor's connection missed is read again at the
//! turn's end.

use super::*;
use codex_alab_aterm::Options as ClientOptions;
use codex_alab_core::session::Shared;
use codex_protocol::config_types::CollaborationMode;
use codex_protocol::config_types::Settings;
use codex_protocol::protocol::TurnAbortReason;
use decide::Mode;

/// An autopilot with no aterm to claim: every turn here ends in a passive spell that is no Off
/// (no claim proven, the supervisor's word unknown).
fn fixture() -> (Arc<Autopilot>, ExtensionData, Arc<ThreadRt>) {
    codex_alab_core::activate();
    let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
    let sup = Supervisor::new(crate::SupervisorOptions {
        shared,
        endpoint: Arc::new(|| None),
        client: ClientOptions::default(),
    });
    let autopilot = Autopilot::new(sup, None, None, Timing::default(), |_| true);
    let (store, rt) = thread(&autopilot);
    (autopilot, store, rt)
}

/// A root thread kept as `on_thread_start` keeps one.
fn thread(autopilot: &Autopilot) -> (ExtensionData, Arc<ThreadRt>) {
    let id = ThreadId::new();
    let store = ExtensionData::new(id.to_string());
    store.insert(ThreadRt::new(id, None, false, Duration::ZERO));
    let rt = store.get::<ThreadRt>().expect("native runtime");
    autopilot.keep_thread(&rt);
    (store, rt)
}

/// Core accepts `turn` on the thread `store` holds.
async fn start(autopilot: &Autopilot, store: &ExtensionData, turn: &ExtensionData) {
    let mode = CollaborationMode {
        mode: ModeKind::Default,
        settings: Settings {
            model: "gpt-5".into(),
            reasoning_effort: None,
            developer_instructions: None,
        },
    };
    autopilot
        .on_turn_start(TurnStartInput {
            turn_id: turn.level_id(),
            collaboration_mode: &mode,
            token_usage_at_turn_start: None,
            session_store: &ExtensionData::new("session"),
            thread_store: store,
            turn_store: turn,
        })
        .await;
}

/// The person's message opens `turn`, typed by their hand, as `on_item_completed` books one.
fn typed_by_hand(rt: &ThreadRt, turn: &ExtensionData) {
    lock(&turn.get_or_init(TurnTrack::default).0).by_person = true;
    rt.with_state(|state| {
        state.observe_person_turn();
        state.observe_person_hand(clock());
    });
}

/// `turn` ends clean, answered at once.
async fn stop(autopilot: &Autopilot, store: &ExtensionData, turn: &ExtensionData) {
    autopilot
        .on_turn_stop(TurnStopInput {
            session_store: &ExtensionData::new("session"),
            thread_store: store,
            turn_store: turn,
        })
        .await;
}

/// A RESUMED THREAD IS PAST ITS OPENING UNLESS ITS BOOKS, READ BEFORE A TURN HERE, SAY IT NEVER
/// WORKED (review of gap 32, findings 1, 5 and 10): a person's quick message whose turn beat
/// the resume scan, or one that ends after a scan that could not read the history, is carried
/// (no exchange of theirs, nothing left); after a scan that read a history with no work it
/// opens the session, and is left to them. NEGATIVE CONTROL: the same message on a thread never
/// resumed opens its session.
#[tokio::test]
async fn a_resumed_thread_whose_books_were_not_read_is_past_its_opening() {
    for case in ["fresh", "beaten", "unread", "read, no work"] {
        let (autopilot, store, rt) = fixture();
        let turn = ExtensionData::new("the person's turn");
        if case != "fresh" {
            rt.resumed.store(true, Ordering::SeqCst);
        }
        if case == "read, no work" {
            autopilot.set_history(Vec::new());
        }
        if case == "beaten" {
            start(&autopilot, &store, &turn).await;
            Arc::clone(&autopilot).resume_scan(Arc::clone(&rt)).await;
        } else {
            if case != "fresh" {
                Arc::clone(&autopilot).resume_scan(Arc::clone(&rt)).await;
            }
            start(&autopilot, &store, &turn).await;
        }
        assert!(!rt.state().past_opening, "{case}: no turn ended yet");
        typed_by_hand(&rt, &turn);
        stop(&autopilot, &store, &turn).await;
        let state = rt.state();
        let opening = matches!(case, "fresh" | "read, no work");
        assert_eq!(state.ended_under, Some(Mode::Land), "{case}");
        assert_eq!(
            (state.exchange, state.left),
            (opening, opening),
            "{case}: {state:?}"
        );
        assert!(state.past_opening, "{case}");
    }
}

/// A PERSON'S TURN THAT ENDS IN A PASSIVE SPELL THAT IS NO OFF (here no claim proven) IS JUDGED
/// UNDER LAND'S RULE, whatever word was folded before the spell (review of gap 32, findings 2,
/// 3, 7 and 11): the session's opening exchange is left to the person (Land's first decision
/// keeps it theirs, Cruise's takes it back), and their message once the session has worked is
/// carried. NEGATIVE CONTROLS: a Cruise folded before the spell leaves the opening exchange left
/// all the same (the word folded last would carry it), and no word read is no Off (which would
/// leave the later message to them).
#[tokio::test]
async fn a_passive_spell_ends_a_persons_turn_under_lands_rule() {
    for (case, folded, worked) in [
        (
            "the opening, Cruise folded before",
            Some(Mode::Cruise),
            false,
        ),
        ("the opening, nothing folded", None, false),
        (
            "past the opening, Cruise folded before",
            Some(Mode::Cruise),
            true,
        ),
        ("past the opening, nothing folded", None, true),
    ] {
        let (autopilot, store, rt) = fixture();
        rt.with_state(|state| {
            state.mode = folded;
            if worked {
                state.seed_worked();
            }
        });
        let turn = ExtensionData::new("the person's turn");
        start(&autopilot, &store, &turn).await;
        typed_by_hand(&rt, &turn);
        stop(&autopilot, &store, &turn).await;
        let state = rt.state();
        assert_eq!(state.ended_under, Some(Mode::Land), "{case}");
        assert_eq!(
            (state.exchange, state.left),
            (!worked, !worked),
            "{case}: {state:?}"
        );
        let mut land = state.clone();
        land.observe_mode(Mode::Land);
        assert_eq!(land.left, !worked, "{case}: under Land");
        let mut cruise = state;
        cruise.observe_mode(Mode::Cruise);
        assert!(!cruise.left, "{case}: Cruise takes it back");
    }
}

/// A LATE COMPLETION STILL PUTS THE SESSION PAST ITS OPENING (review of gap 32, finding 6): the
/// person's first message started a job they interrupted with a second; the first turn's end
/// reaches codex-alab after the second began and leaves the newer point unchanged, but a turn
/// ended here, so the second message, answered at once, is carried. NEGATIVE CONTROL: the same
/// quick message as the session's first is left to them (`fresh` in
/// `a_resumed_thread_whose_books_were_not_read_is_past_its_opening`).
#[tokio::test]
async fn a_late_completion_puts_the_session_past_its_opening() {
    let (autopilot, store, rt) = fixture();
    let first = ExtensionData::new("the person's first turn");
    let second = ExtensionData::new("the person's second turn");
    start(&autopilot, &store, &first).await;
    typed_by_hand(&rt, &first);
    start(&autopilot, &store, &second).await;
    autopilot
        .on_turn_abort(TurnAbortInput {
            reason: TurnAbortReason::Interrupted,
            session_store: &ExtensionData::new("session"),
            thread_store: &store,
            turn_store: &first,
        })
        .await;
    let state = rt.state();
    assert!(state.point.is_none(), "the newer turn's point is unchanged");
    assert!(state.past_opening);
    typed_by_hand(&rt, &second);
    stop(&autopilot, &store, &second).await;
    let state = rt.state();
    assert_eq!((state.exchange, state.left), (false, false), "{state:?}");
}

/// A NEW CHAT OPENS A SESSION OF ITS OWN (edges-3; aterm's `opened_fresh`, aterm 9d81004bdb and
/// f9fcb4400b: a fresh program's banner, and a later taskless screen of the same process,
/// `/clear`, open the next exchange's session, whatever the tab's books say): `/new` or
/// `/clear` in a process one of whose turns ended starts a thread at its opening, so under
/// Land the person's first quick question in it is theirs (the owner, 2026-10-27: "the user
/// question exception is only for the first message of a session"). This replaces gap 32's
/// review finding 4, under which such a thread was past its opening "as aterm's loop, one per
/// tab, judges a session's opening once", which aterm's live policy no longer does. A RESUMED
/// THREAD'S BOOKS DECIDE ITS OWN: past its opening where its history shows work, at it where
/// they show none, whatever this process did before. NEGATIVE CONTROLS: the thread a turn ended
/// on is past its opening, and its own next quick message is carried.
#[tokio::test]
async fn a_new_chat_opens_its_own_session_and_a_resume_reads_its_books() {
    let (autopilot, store, rt) = fixture();
    let (_early_store, early) = thread(&autopilot);
    let turn = ExtensionData::new("a turn");
    start(&autopilot, &store, &turn).await;
    stop(&autopilot, &store, &turn).await;
    assert!(rt.state().past_opening);
    assert!(
        !early.state().past_opening,
        "kept before any turn ended here"
    );
    let (later_store, later) = thread(&autopilot);
    assert!(
        !later.state().past_opening,
        "a new chat after a turn ended here"
    );
    // The new chat's opening question, answered at once under Land, is theirs.
    let question = ExtensionData::new("what does src/foo.rs do?");
    start(&autopilot, &later_store, &question).await;
    typed_by_hand(&later, &question);
    stop(&autopilot, &later_store, &question).await;
    let state = later.state();
    assert_eq!(state.ended_under, Some(Mode::Land));
    assert_eq!((state.exchange, state.left), (true, true), "{state:?}");
    // NEGATIVE CONTROL: the thread that worked carries its own next quick message.
    let next = ExtensionData::new("a quick message on the thread that worked");
    start(&autopilot, &store, &next).await;
    typed_by_hand(&rt, &next);
    stop(&autopilot, &store, &next).await;
    let state = rt.state();
    assert_eq!((state.exchange, state.left), (false, false), "{state:?}");
    // A resumed thread's history, read before any turn here.
    for worked in [false, true] {
        let (autopilot, _store, rt) = fixture();
        rt.resumed.store(true, Ordering::SeqCst);
        let history = if worked {
            [
                serde_json::json!({"type":"task_started", "turn_id":"an-old-turn",
                    "model_context_window":258400, "collaboration_mode_kind":"default"}),
                serde_json::json!({"type":"task_complete", "turn_id":"an-old-turn",
                    "last_agent_message":"Stage 1 is in."}),
            ]
            .map(|value| {
                codex_history::RolloutItem::EventMsg(
                    serde_json::from_value::<EventMsg>(value).expect("a rollout event"),
                )
            })
            .to_vec()
        } else {
            Vec::new()
        };
        autopilot.set_history(history);
        Arc::clone(&autopilot).resume_scan(Arc::clone(&rt)).await;
        assert_eq!(rt.state().past_opening, worked);
        let (_later_store, later) = thread(&autopilot);
        assert!(
            !later.state().past_opening,
            "a new chat kept after a resume whose history worked: {worked}"
        );
    }
}

/// THROUGH THE REAL THREAD MANAGER (review of gap 32, finding 4; edges-3): a fork is past its
/// opening (`on_thread_ready` reads its source off its config, on a task of its own), and a new
/// thread core starts after it (`/new`, `/clear`: `on_thread_start`) opens a session of its
/// own, as aterm's `opened_fresh` does; but A THREAD THAT CARRIES ON ANOTHER'S WORK, which the
/// TUI names as it starts it (plan mode's "clear context and implement", whose plan is
/// submitted as the thread starts, and a provider error's continuation:
/// `Shared::continue_work`), is past its opening (review of the needs-edges lane: Land typed
/// nothing over the implemented plan). NEGATIVE CONTROLS: a new thread core starts in a process
/// with no work and no fork is at its opening, and the continuation's mark is used up, so the
/// new chat after it opens its own.
#[test]
fn a_fork_and_a_continuation_are_past_their_opening_and_a_new_chat_opens_its_own() {
    core_test_support::test_codex::run_test_with_large_stack(
        "codex-alab opening of a fork",
        || async {
            codex_alab_core::activate();
            let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
            let sup = Supervisor::new(crate::SupervisorOptions {
                shared,
                endpoint: Arc::new(|| None),
                client: ClientOptions::default(),
            });
            let mut builder = ExtensionRegistryBuilder::<Config>::new();
            let autopilot = Autopilot::new(sup, None, None, Timing::default(), |_| true);
            autopilot.register(&mut builder);
            let server = core_test_support::responses::start_mock_server().await;
            let test = core_test_support::test_codex::test_codex()
                .with_extensions(Arc::new(builder.build()))
                .build(&server)
                .await?;
            autopilot.bind_thread_manager(Arc::downgrade(&test.thread_manager));
            let past_opening = |id: ThreadId| {
                autopilot
                    .thread_rt(&id.to_string())
                    .unwrap_or_else(|| panic!("{id} is kept"))
                    .state()
                    .past_opening
            };
            let fresh = test
                .thread_manager
                .start_thread(codex_core::StartThreadOptions::new(test.config.clone()))
                .await?;
            assert!(!past_opening(fresh.thread_id), "a new thread, no work");
            let fork = test
                .thread_manager
                .fork_thread_from_history(
                    codex_core::ForkSnapshot::TruncateBeforeNthUserMessage(usize::MAX),
                    codex_core::StartThreadOptions::new(test.config.clone()),
                    codex_history::InitialHistory::Resumed(codex_history::ResumedHistory {
                        conversation_id: test.session_configured.thread_id,
                        history: Arc::new(Vec::new()),
                        history_revision: None,
                        rollout_path: None,
                    }),
                )
                .await?;
            // The fork's source is read on a task of its own (`on_thread_ready` never suspends
            // core's ready path): wait for it, a hang detector, never a latency budget.
            let read = tokio::time::Instant::now();
            while !past_opening(fork.thread_id) {
                assert!(
                    read.elapsed() < Duration::from_secs(60),
                    "a fork is past its opening"
                );
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            let later = test
                .thread_manager
                .start_thread(codex_core::StartThreadOptions::new(test.config.clone()))
                .await?;
            assert!(
                !past_opening(later.thread_id),
                "a new chat started after the fork opens its own session"
            );
            shared.continue_work(&later.thread_id.to_string());
            let continued = test
                .thread_manager
                .start_thread(codex_core::StartThreadOptions::new(test.config.clone()))
                .await?;
            assert!(
                past_opening(continued.thread_id),
                "a thread that carries on another's work"
            );
            let after = test
                .thread_manager
                .start_thread(codex_core::StartThreadOptions::new(test.config.clone()))
                .await?;
            assert!(
                !past_opening(after.thread_id),
                "the continuation's mark is used up: a new chat after it opens its own"
            );
            Ok(())
        },
    )
    .expect("native core test");
}

/// The words the TUI sends for a person's message, as core completes its item.
fn message(text: &str) -> TurnItem {
    TurnItem::UserMessage(codex_protocol::items::UserMessageItem {
        id: "a person's message".into(),
        client_id: None,
        content: vec![UserInput::Text {
            text: text.into(),
            text_elements: Vec::new(),
        }],
    })
}

/// The agent's answer, as core completes its item.
fn answer(text: &str) -> TurnItem {
    TurnItem::AgentMessage(codex_protocol::items::AgentMessageItem {
        id: "the agent's answer".into(),
        content: vec![codex_protocol::items::AgentMessageContent::Text { text: text.into() }],
        phase: None,
        memory_citation: None,
        delivery: None,
        questions: None,
    })
}

/// A person's turn through every hook core calls for it: its start, their message, the agent's
/// answer and its end.
async fn persons_turn(
    autopilot: &Autopilot,
    store: &ExtensionData,
    turn: &ExtensionData,
    said: &str,
    reply: &str,
) {
    start(autopilot, store, turn).await;
    autopilot
        .on_item_completed(store, turn, &message(said))
        .await;
    autopilot
        .on_item_completed(store, turn, &answer(reply))
        .await;
    stop(autopilot, store, turn).await;
}

/// The TUI's own root thread: registered by a frame, and kept by the autopilot.
fn root(shared: &Shared, rt: &ThreadRt) {
    shared.observe(
        &codex_alab_core::session::Frame {
            primary_thread: Some(rt.key()),
            displayed_thread: Some(rt.key()),
            rollout_path: None,
            cwd: std::path::Path::new("/work"),
            model: "gpt-5",
            codex_home: std::path::Path::new("/h/.codex"),
            embedded: true,
            state: codex_alab_core::session::ProgramState::Idle,
            draft_empty: true,
            queued_input: false,
            popup_open: false,
            editor_open: false,
            pending_user_turn: false,
        },
        1,
    );
    shared.note_root_thread(rt.key());
    assert!(shared.is_root_thread(rt.key()));
}

/// The session's policy under `autopilot`, as aterm's defaults compose it.
fn policy_under(autopilot: Mode) -> decide::Policy {
    decide::Policy {
        continue_on: true,
        continue_text: Some("keep going".to_string()),
        continue_per_hour: None,
        approve_all: true,
        answer_questions: true,
        answer_text: Some("Pick the recommended option.".to_string()),
        questions: decide::Questions::Recommended,
        retry_api_errors: true,
        probe_api: true,
        resume_limits: true,
        limit_wait: true,
        compact_on_context_wall: true,
        upgrade: true,
        upgrade_ask: false,
        facts: false,
        human_grace: Duration::from_secs(120),
        autopilot,
        texts: decide::Texts::default(),
    }
}

/// A person's turn the history holds, answered by `reply` after `worked_ms` of work.
fn persons_history(reply: &str, worked_ms: u64) -> Vec<codex_history::RolloutItem> {
    [
        serde_json::json!({"type":"task_started", "turn_id":"t-vanilla",
            "turn_attribution": {"turn_id":"t-vanilla", "turn_trigger":"user",
                "parent_turn_id":null, "initiating_agent_path":null, "root_turn_id":"t-vanilla"},
            "model_context_window":258400, "collaboration_mode_kind":"default"}),
        serde_json::json!({"type":"task_complete", "turn_id":"t-vanilla",
            "last_agent_message": reply, "duration_ms": worked_ms}),
    ]
    .map(|value| {
        codex_history::RolloutItem::EventMsg(
            serde_json::from_value::<EventMsg>(value).expect("a rollout event"),
        )
    })
    .to_vec()
}

/// AFTER A VANILLA SPELL CODEX-ALAB DECIDES AT NO POINT THE SPELL LEFT BEHIND, AND STANDS AT THE
/// THREAD'S OWN LAST TURN END (edges-1; review of the needs-edges lane: FOREVER CRUISE, "Cruise
/// never parks"): while `/alab off` stood, every hook stood aside, so the person's own turns
/// (their answer to the agent's question, and the work after it) moved no point here; back on,
/// the point before the spell is another conversation's, and the decision's look reads the
/// history instead: the person's last turn end is the point, so Cruise continues there and Land
/// carries the work on, the session past its opening (before, nothing was typed until the
/// person typed again). NEGATIVE CONTROL: a spell no turn ran in leaves the point standing,
/// decided as before, and reads no history.
#[tokio::test]
async fn a_vanilla_spell_leaves_no_stale_point_and_stands_at_its_last_turn_end() {
    let t = Timing::default();
    let cruise = policy_under(Mode::Cruise);
    let reply = "Named it lexer; next I wire it into the parser.";
    for spell_turn in [true, false] {
        let (autopilot, store, rt) = fixture();
        rt.with_state(|state| {
            state.observe_person_turn();
            state.observe_turn_end(
                decide::TurnEnd {
                    at: clock(),
                    by_autopilot: true,
                    by_person: false,
                    text: Some("Should I name the new module parser or lexer?".into()),
                    earlier: None,
                    questions: false,
                    worked: Duration::from_secs(600),
                    wall: None,
                    aborted: false,
                    ran_tools: true,
                    restored: false,
                },
                &t,
            );
        });
        let before = decide::decide(&rt.state(), &cruise, &t, clock(), Duration::ZERO);
        assert!(
            matches!(before.act, decide::Act::Type { .. }),
            "the question is answered: {before:?}"
        );
        rt.set_vanilla(true);
        if spell_turn {
            let turn = ExtensionData::new("the person's own answer, under vanilla");
            persons_turn(&autopilot, &store, &turn, "call it lexer", reply).await;
        }
        rt.set_vanilla(false);
        autopilot.set_history(persons_history(reply, 20 * 60 * 1000));
        Arc::clone(&autopilot).evaluate(Arc::clone(&rt)).await;
        let state = rt.state();
        let after = decide::decide(&state, &cruise, &t, clock(), Duration::ZERO);
        if spell_turn {
            let point = state.point.as_ref().expect("the history's last turn end");
            assert_eq!(point.text.as_deref(), Some(reply), "{state:?}");
            assert!(point.by_person && point.restored, "{point:?}");
            assert!(state.task && state.past_opening, "{state:?}");
            assert_ne!(after.act, before.act, "never the old question");
            for (mode, rule) in [
                (Mode::Cruise, words::RULE_CONTINUE),
                (Mode::Land, words::RULE_LAND),
            ] {
                let mut folded = state.clone();
                folded.observe_mode(mode);
                let d = decide::decide(&folded, &policy_under(mode), &t, clock(), Duration::ZERO);
                assert!(
                    matches!(&d.act, decide::Act::Type { rule: typed, .. } if *typed == rule),
                    "{mode:?} acts at the person's last turn end: {d:?}"
                );
            }
        } else {
            assert_eq!(
                after.act, before.act,
                "NEGATIVE CONTROL: no turn ran unseen"
            );
        }
    }
}

/// A PERSON'S MESSAGE AFTER A VANILLA SPELL IS PAST THE SESSION'S OPENING (edges-2): the person
/// worked by hand under `/alab off` (no turn end was seen, no resume read), so their next quick
/// message, back on, is no opening exchange: Land carries it and decides its instruction there.
/// NEGATIVE CONTROL: the same message as the session's first is theirs.
#[tokio::test]
async fn a_persons_message_after_a_vanilla_spell_is_past_the_opening() {
    let t = Timing::default();
    for spell in [true, false] {
        let (autopilot, store, rt) = fixture();
        if spell {
            rt.set_vanilla(true);
            let turn = ExtensionData::new("an hour's work under vanilla");
            persons_turn(
                &autopilot,
                &store,
                &turn,
                "refactor the parser",
                "Refactored it; the edits are not committed.",
            )
            .await;
            rt.set_vanilla(false);
        }
        let turn = ExtensionData::new("commit this and push");
        start(&autopilot, &store, &turn).await;
        // The turn whose start takes the spell in keeps its own native origin.
        assert_eq!(
            rt.state().native_origin,
            Some(codex_alab_core::session::NativeOrigin {
                turn_id: turn.level_id().to_string(),
                phase: codex_alab_core::session::NativePhase::Running,
            }),
            "spell={spell}"
        );
        typed_by_hand(&rt, &turn);
        stop(&autopilot, &store, &turn).await;
        let state = rt.state();
        assert_eq!(state.ended_under, Some(Mode::Land), "spell={spell}");
        assert_eq!(
            (state.exchange, state.left),
            (!spell, !spell),
            "spell={spell}: {state:?}"
        );
        let d = decide::decide(
            &state,
            &policy_under(Mode::Land),
            &t,
            clock(),
            Duration::ZERO,
        );
        if spell {
            assert_ne!(d.act, decide::Act::Nothing, "Land carries it: {d:?}");
        } else {
            assert_eq!(d.act, decide::Act::Nothing, "theirs: {d:?}");
        }
    }
}

/// THE PERSON'S HAND THAT A READ RACING THE SUPERVISOR'S CONNECTION MISSED IS READ AGAIN AT THE
/// TURN'S END (edges-6; aterm reads the hand from the reading it judges the point by, so the
/// race never arises there; the skeptic who called the 1 s bound designed is overruled: the
/// owner's exception leaves the session's opening message to the person, and a read that
/// raced the connection says nothing about whose it was): `codex "<question>"` submits the
/// prompt while the supervisor still connects, the message-time read gets no answer, and the
/// read at the turn's end names the person's Enter, so under Land the opening exchange is
/// theirs. NEGATIVE CONTROL: neither read names a hand, and the exchange is carried.
#[tokio::test]
async fn a_hand_read_that_raced_the_connection_is_read_again_at_the_turns_end() {
    for (case, read_again, theirs) in [
        ("read again at the end", Some(Duration::from_secs(1)), true),
        ("never read", None, false),
    ] {
        let (autopilot, store, rt) = fixture();
        root(autopilot.sup.shared(), &rt);
        autopilot.sup.answer_hand_reads([None, read_again]);
        let turn = ExtensionData::new("codex \"what does this repo do?\"");
        persons_turn(
            &autopilot,
            &store,
            &turn,
            "what does this repo do?",
            "It parses foo files into an AST.",
        )
        .await;
        let state = rt.state();
        assert_eq!(state.ended_under, Some(Mode::Land), "{case}");
        assert_eq!(
            (state.exchange, state.left),
            (theirs, theirs),
            "{case}: {state:?}"
        );
    }
}

/// A POINT LEFT TO THE PERSON CROSSES THE LIVE UPGRADE'S RE-EXEC (edges-5; aterm re-applies a
/// LEFT record in its next loop and lets Cruise act there): Land's opening exchange left to the
/// person moves with no notice, and the image after stands at that point again, still theirs
/// under Land, so a later switch to Cruise takes it back and continues, as the image before
/// would have. NEGATIVE CONTROLS: Land keeps it theirs in the new image, and a history whose
/// last reply moved since restores no point.
#[tokio::test]
async fn a_point_left_to_the_person_crosses_the_re_exec() {
    let t = Timing::default();
    let reply = "It parses foo files into an AST.";
    // The image before: the opening exchange, left under Land.
    let (before, store, rt) = fixture();
    let turn = ExtensionData::new("the session's opening question");
    start(&before, &store, &turn).await;
    typed_by_hand(&rt, &turn);
    before
        .on_item_completed(&store, &turn, &answer(reply))
        .await;
    stop(&before, &store, &turn).await;
    assert!(rt.state().left, "{:?}", rt.state());
    before.sync_pace(&rt);
    let pace = before
        .supervisor()
        .shared()
        .pace(rt.key())
        .unwrap_or_else(|| panic!("the record carries the thread's pace"));
    let history = |last: &str| {
        [
            serde_json::json!({"type":"task_started", "turn_id":"t1",
                "turn_attribution": {"turn_id":"t1", "turn_trigger":"user",
                    "parent_turn_id":null, "initiating_agent_path":null, "root_turn_id":"t1"},
                "model_context_window":258400, "collaboration_mode_kind":"default"}),
            serde_json::json!({"type":"task_complete", "turn_id":"t1",
                "last_agent_message": last}),
        ]
        .map(|value| {
            codex_history::RolloutItem::EventMsg(
                serde_json::from_value::<EventMsg>(value).expect("a rollout event"),
            )
        })
        .to_vec()
    };
    // The image after the re-exec: the record hands it the pace, and its resume scan reads the
    // history.
    let (_after, resumed) = re_exec(rt.id, pace.clone(), history(reply)).await;
    let state = resumed.state();
    assert!(state.left && state.point.is_some(), "{state:?}");
    // The image before's LEFT record stands at aterm: this image records none again there.
    assert_eq!(
        *lock(&resumed.left_recorded),
        Some((0, state.point_at().expect("the point stood at again")))
    );
    let mut cruise = state.clone();
    cruise.observe_mode(Mode::Cruise);
    let d = decide::decide(
        &cruise,
        &policy_under(Mode::Cruise),
        &t,
        clock(),
        Duration::ZERO,
    );
    assert!(
        matches!(&d.act, decide::Act::Type { rule, .. } if *rule == words::RULE_CONTINUE),
        "Cruise takes it back and continues: {d:?}"
    );
    // NEGATIVE CONTROL: Land keeps it theirs.
    let mut land = state;
    land.observe_mode(Mode::Land);
    let d = decide::decide(
        &land,
        &policy_under(Mode::Land),
        &t,
        clock(),
        Duration::ZERO,
    );
    assert_eq!(d.act, decide::Act::Nothing, "{d:?}");
    // NEGATIVE CONTROL: the history's last reply moved; no point is restored.
    let (_after, moved) = re_exec(rt.id, pace, history("Something else since.")).await;
    let state = moved.state();
    assert!(!state.left && state.point.is_none(), "{state:?}");
    assert_eq!(*lock(&moved.left_recorded), None);
}

/// The image after the live upgrade's re-exec, resuming thread `id`: the record hands it `pace`,
/// and its resume scan reads `history`.
async fn re_exec(
    id: ThreadId,
    pace: codex_alab_core::session::Pace,
    history: Vec<codex_history::RolloutItem>,
) -> (Arc<Autopilot>, Arc<ThreadRt>) {
    let (after, _store, _other) = fixture();
    let store = ExtensionData::new(id.to_string());
    store.insert(ThreadRt::new(id, None, false, Duration::ZERO));
    let rt = store.get::<ThreadRt>().expect("native runtime");
    after.keep_thread(&rt);
    let shared = after.supervisor().shared();
    assert!(shared.set_inherited(codex_alab_core::session::Inherited {
        exec_generation: 1,
        ..codex_alab_core::session::Inherited::default()
    }));
    shared.set_pace(rt.key(), pace);
    rt.resumed.store(true, Ordering::SeqCst);
    after.set_history(history);
    Arc::clone(&after).resume_scan(Arc::clone(&rt)).await;
    (after, rt)
}

/// A TURN BEGUN IN A VANILLA SPELL AND ENDED AFTER IT IS THE THREAD'S NEWEST (edges-1; review of
/// the needs-edges lane, S2): its start hook stood aside, but it was accepted all the same, so
/// its end, once `/alab on` stood, is the thread's point, never a late completion of the turn
/// before it (which left the thread at no point, Cruise typing nothing). NEGATIVE CONTROL: the
/// turn before it, ending late after the spell's turn began, is that late completion and moves
/// no point.
#[tokio::test]
async fn a_turn_begun_in_a_vanilla_spell_ends_as_the_threads_newest() {
    let t = Timing::default();
    let (autopilot, store, rt) = fixture();
    let first = ExtensionData::new("the person's first turn");
    start(&autopilot, &store, &first).await;
    typed_by_hand(&rt, &first);
    rt.set_vanilla(true);
    let spell = ExtensionData::new("a turn begun under vanilla");
    start(&autopilot, &store, &spell).await;
    rt.set_vanilla(false);
    autopilot
        .on_turn_abort(TurnAbortInput {
            reason: TurnAbortReason::Interrupted,
            session_store: &ExtensionData::new("session"),
            thread_store: &store,
            turn_store: &first,
        })
        .await;
    assert!(
        rt.state().point.is_none(),
        "NEGATIVE CONTROL: the turn before is a late completion: {:?}",
        rt.state()
    );
    let reply = "Built half the lexer; the tokenizer is next.";
    autopilot
        .on_item_completed(&store, &spell, &answer(reply))
        .await;
    stop(&autopilot, &store, &spell).await;
    let state = rt.state();
    let point = state
        .point
        .as_ref()
        .expect("the spell's turn end is the point");
    assert_eq!(point.text.as_deref(), Some(reply), "{state:?}");
    assert!(state.task && state.past_opening, "{state:?}");
    for (mode, rule) in [
        (Mode::Cruise, words::RULE_CONTINUE),
        (Mode::Land, words::RULE_LAND),
    ] {
        let mut folded = state.clone();
        folded.observe_mode(mode);
        let d = decide::decide(&folded, &policy_under(mode), &t, clock(), Duration::ZERO);
        assert!(
            matches!(&d.act, decide::Act::Type { rule: typed, .. } if *typed == rule),
            "{mode:?} acts at it: {d:?}"
        );
    }
}

/// A RESUME UNDER VANILLA READS ITS BOOKS ONCE CODEX-ALAB LOOKS AGAIN (edges-2; review of the
/// needs-edges lane, Y6): `on_thread_resume` stood aside, so no resume scan ran, but it marked
/// the thread unwatched, and the first decision's look after `/alab on` reads the history: the
/// thread stands at its last turn end, past its opening, with the NEEDS YOU line the history
/// leaves standing on the menu bar (the owner, 2026-10-19: "if you need something from the
/// user, be LOUDER ABOUT IT ... but NEVER let that block you"). NEGATIVE CONTROLS: a thread no
/// spell covered reads no books at a look (nothing stands, no need is raised), and a set an Off
/// took down (vanilla gives back a held claim) stays down while the point still stands.
#[tokio::test]
async fn a_resume_under_vanilla_reads_its_books_once_codex_alab_looks_again() {
    let reply = "Stage 1 is in.\nNEEDS YOU: the prod API key";
    let history = persons_history(reply, 20 * 60 * 1000);
    for case in [
        "resumed under vanilla",
        "no spell",
        "an Off took the set down",
    ] {
        let (autopilot, store, rt) = fixture();
        if case != "no spell" {
            rt.set_vanilla(true);
            autopilot
                .on_thread_resume(ThreadResumeInput {
                    session_store: &ExtensionData::new("session"),
                    thread_store: &store,
                })
                .await;
            rt.set_vanilla(false);
            assert!(
                !rt.resumed.load(Ordering::SeqCst),
                "{case}: no scan is pending"
            );
        }
        if case == "an Off took the set down" {
            autopilot
                .supervisor()
                .shared()
                .set_needs_off(history_needs(&history).key);
        }
        autopilot.set_history(history.clone());
        Arc::clone(&autopilot).evaluate(Arc::clone(&rt)).await;
        let state = rt.state();
        if case == "no spell" {
            assert!(state.point.is_none(), "{case}: {state:?}");
            assert!(state.needs.standing.is_empty(), "{case}: {state:?}");
            assert!(!state.past_opening, "{case}: {state:?}");
            continue;
        }
        let point = state.point.as_ref().expect("the history's last turn end");
        assert_eq!(point.text.as_deref(), Some(reply), "{case}");
        assert!(state.past_opening, "{case}: {state:?}");
        let raised: &[&str] = if case == "an Off took the set down" {
            &[]
        } else {
            &["the prod API key"]
        };
        assert_eq!(state.needs.standing, raised, "{case}: {state:?}");
    }
}
