// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! `/accounts use|auto` against alab-aterm's in-process FAKE aterm, never a live one: what is
//! refused with nothing sent, the one restart line an offered move sends, and the sentence
//! each answer leaves in the pane.

use super::*;
use crate::chatwidget::tests::make_chatwidget_manual_with_sender;
use crate::chatwidget::tests::render_bottom_popup;
use codex_alab_tui::aterm::accounts::Assignment;
use codex_alab_tui::aterm::accounts::Capabilities;
use codex_alab_tui::aterm::accounts::Pool;
use codex_alab_tui::aterm::accounts::Quota;
use codex_alab_tui::aterm::accounts::Rotation;
use codex_alab_tui::aterm::accounts::Session;
use codex_alab_tui::aterm::fake::FakeAccountMoves;
use codex_alab_tui::aterm::fake::FakeAterm;
use codex_alab_tui::session::BlockedKind;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

/// One frame of the displayed thread reading `state`, with an empty composer.
fn frame(shared: &Shared, state: ProgramState, queued_input: bool) {
    let dir = std::path::Path::new("/");
    shared.observe(
        &codex_alab_core::session::Frame {
            primary_thread: None,
            displayed_thread: None,
            rollout_path: None,
            cwd: dir,
            model: "model",
            codex_home: dir,
            embedded: false,
            state,
            draft_empty: true,
            queued_input,
            popup_open: false,
            editor_open: false,
            pending_user_turn: false,
        },
        /*now_ms*/ 1,
    );
}

/// A frame of `state` with `count` background terminals, as the footer counts them.
fn terminals(shared: &Shared, state: ProgramState, count: u32) {
    shared.set_background_terminals(count, Instant::now());
    frame(shared, state, /*queued_input*/ false);
}

/// A session's own shared state, fresh and at its record's idle point (a frame read idle with
/// an empty composer and no background terminal), for the test's lifetime (the process's copy
/// is `codex_alab_core::session::shared()`, which other tests share).
fn fresh() -> &'static Shared {
    let shared = Box::leak(Box::new(Shared::new()));
    terminals(shared, ProgramState::Idle, 0);
    shared
}

const MAIN: &str = "main-id";
const WORK: &str = "work-id";
const SPARE: &str = "spare-id";

fn account(id: &str, label: &str, eligible: bool) -> Account {
    Account {
        id: id.into(),
        label: label.into(),
        masked_email: None,
        condition: Condition::Usable,
        enrolled: true,
        held: false,
        eligible,
        quota: Quota {
            stale: true,
            observed_at: None,
            five_hour_used_percent: None,
            five_hour_reset_at: None,
            weekly_used_percent: None,
            weekly_reset_at: None,
        },
    }
}

/// A schema 2 snapshot of a session on `main`, with `work` and `spare` eligible and the
/// pool's choice `work`, offering both moves.
fn offered() -> Snapshot {
    Snapshot {
        schema: 2,
        observed_at: 1_800_000_000,
        request: "a".repeat(32),
        session: Session {
            sid: "s-a".into(),
            process_pid: 1,
            process_birth: "birth".into(),
            epoch: "epoch".into(),
        },
        revision: "b".repeat(64),
        assignment: Assignment {
            account_id: Some(MAIN.into()),
        },
        pool: Some(Pool {
            choice_id: Some(WORK.into()),
            pinned: false,
            rotation: Rotation::Wall,
        }),
        accounts: vec![
            account(MAIN, "main", false),
            account(WORK, "work", true),
            account(SPARE, "spare", true),
        ],
        capabilities: Capabilities {
            add: false,
            select: true,
            auto: true,
        },
    }
}

#[test]
fn decide_asks_only_what_a_schema_two_snapshot_offers() {
    let snapshot = offered();
    assert_eq!(
        decide(&snapshot, false, "work", true),
        Ok(Target::Row("work".into()))
    );
    assert_eq!(decide(&snapshot, true, "", true), Ok(Target::Pool));

    // Schema 1 offers nothing, whoever acts.
    let mut one = offered();
    one.schema = 1;
    one.pool = None;
    for acting in [true, false] {
        assert_eq!(
            decide(&one, false, "work", acting),
            Err(CHANGES_UNAVAILABLE)
        );
        assert_eq!(decide(&one, true, "", acting), Err(CHANGES_UNAVAILABLE));
    }

    // A capability not offered: Autopilot's to give where it does not act.
    let mut closed = offered();
    closed.capabilities.select = false;
    closed.capabilities.auto = false;
    assert_eq!(
        decide(&closed, false, "work", true),
        Err(CHANGES_UNAVAILABLE)
    );
    assert_eq!(decide(&closed, true, "", true), Err(CHANGES_UNAVAILABLE));
    assert_eq!(decide(&closed, false, "work", false), Err(AUTOPILOT));
    assert_eq!(decide(&closed, true, "", false), Err(AUTOPILOT));
    // Each capability is its own.
    let mut no_auto = offered();
    no_auto.capabilities.auto = false;
    assert!(decide(&no_auto, false, "work", true).is_ok());
    assert_eq!(decide(&no_auto, true, "", true), Err(CHANGES_UNAVAILABLE));
    no_auto.pool.as_mut().unwrap().choice_id = Some(MAIN.into());
    assert_eq!(decide(&no_auto, true, "", true), Err(ALREADY_CHOICE));

    // Rotation off: aterm refuses every move, so neither capability is offered, and turning
    // Autopilot on would not help. The snapshot says why.
    let mut off = offered();
    off.capabilities.select = false;
    off.capabilities.auto = false;
    off.pool.as_mut().unwrap().rotation = Rotation::Off;
    for acting in [true, false] {
        assert_eq!(decide(&off, false, "work", acting), Err(refusal(Why::Off)));
        assert_eq!(decide(&off, true, "", acting), Err(refusal(Why::Off)));
    }
    off.pool.as_mut().unwrap().choice_id = Some(MAIN.into());
    assert_eq!(decide(&off, true, "", true), Err(refusal(Why::Off)));

    // The label names exactly one row, not the session's own, that aterm would take.
    assert_eq!(decide(&snapshot, false, "other", true), Err(UNVERIFIED));
    let mut twins = offered();
    twins.accounts[2].label = "work".into();
    assert_eq!(decide(&twins, false, "work", true), Err(UNVERIFIED));
    assert_eq!(decide(&snapshot, false, "main", true), Err(ALREADY));
    for (change, sentence) in [
        (
            (|row: &mut Account| row.enrolled = false) as fn(&mut Account),
            refusal(Why::NotEnrolled),
        ),
        (
            |row: &mut Account| row.condition = Condition::NotEnrolled,
            refusal(Why::NotEnrolled),
        ),
        (
            |row: &mut Account| row.condition = Condition::SignedOut,
            refusal(Why::SignedOut),
        ),
        (|row: &mut Account| row.held = true, refusal(Why::Held)),
        (
            |row: &mut Account| row.condition = Condition::Limited,
            refusal(Why::Limited),
        ),
        (
            |row: &mut Account| row.condition = Condition::Low,
            INELIGIBLE,
        ),
    ] {
        let mut walled = offered();
        walled.accounts[1].eligible = false;
        change(&mut walled.accounts[1]);
        assert_eq!(decide(&walled, false, "work", true), Err(sentence));
    }
}

#[test]
fn every_refusal_ends_saying_the_session_is_unchanged() {
    for why in [
        Why::NotTheHostsAgent,
        Why::NotRelaunchable,
        Why::NotSupervised,
        Why::DriverLease,
        Why::DriverTurn,
        Why::Halted,
        Why::StartUnread,
        Why::NoState,
        Why::ExitOwnerNotReady,
        Why::Unmanaged,
        Why::NotResumable,
        Why::RecoveryPending,
        Why::Off,
        Why::UnknownLabel,
        Why::NoChoice,
        Why::Current,
        Why::Same,
        Why::NotEnrolled,
        Why::SignedOut,
        Why::Held,
        Why::Limited,
        Why::Identity,
        Why::Unread,
        Why::RestartPending,
        Why::Busy,
    ] {
        let sentence = answered(Ok(Outcome::Refused(why)), String::new());
        assert!(
            sentence.ends_with("The session is unchanged."),
            "{why:?}: {sentence}"
        );
        assert!(
            !sentence.contains(why.word()),
            "{why:?} is worded, not echoed"
        );
    }
    assert_eq!(
        answered(Ok(Outcome::Refused(Why::NotResumable)), String::new()),
        refusal(Why::NotResumable)
    );
    for outcome in [Outcome::Denied, Outcome::Unsupported] {
        assert_eq!(answered(Ok(outcome), String::new()), CHANGES_UNAVAILABLE);
    }
    assert_eq!(answered(Ok(Outcome::Unknown), String::new()), UNREAD_ANSWER);
    assert_eq!(
        answered(Ok(Outcome::NotRestarted), String::new()),
        NOT_RESTARTED
    );
    assert!(NOT_RESTARTED.ends_with("The session is unchanged."));
    assert!(GONE.ends_with("The session is unchanged."));
    assert_eq!(answered(Ok(Outcome::Gone), String::new()), GONE);
    assert_eq!(answered(Err(Error::Unanswered), String::new()), NO_ANSWER);
    assert_eq!(answered(Err(Error::Closed), String::new()), NO_ANSWER);
    assert_eq!(
        answered(Err(Error::BadRequest("label".into())), String::new()),
        UNVERIFIED
    );
    assert_eq!(
        answered(Ok(Outcome::Restarting), "pending".into()),
        "pending"
    );
}

#[test]
fn the_pin_sentence_appears_only_when_pinned_to_another_row() {
    let pin = "The pool is pinned to";
    let snapshot = offered();
    let text = pending(&snapshot, &Target::Row("spare".into()));
    assert!(
        text.starts_with("Restarting this session on spare…"),
        "{text}"
    );
    assert!(text.contains("otherwise on the current one"));
    assert!(!text.contains(pin));
    let mut pinned = offered();
    pinned.pool.as_mut().unwrap().pinned = true;
    let text = pending(&pinned, &Target::Row("spare".into()));
    assert!(
        text.contains("The pool is pinned to work: this session will not rotate at a limit while the pin holds."),
        "{text}"
    );
    assert!(!pending(&pinned, &Target::Row("work".into())).contains(pin));
    let text = pending(&pinned, &Target::Pool);
    assert!(
        text.starts_with("Restarting this session on work…"),
        "{text}"
    );
    assert!(!text.contains(pin));
}

/// Wait for the next pane update a move posts; its text.
async fn next_text(events: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>) -> String {
    next_update(events).await.1
}

/// Wait for the next pane update a move posts: whether it is the move's last word
/// ([`Update::Outcome`], said in history where the pane is gone), and its text.
async fn next_update(
    events: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
) -> (bool, String) {
    let event = tokio::time::timeout(Duration::from_secs(10), events.recv())
        .await
        .expect("the move must say something")
        .expect("app event channel remains open");
    match event {
        AppEvent::AlabAccountsReady {
            text: Update::Observation(text),
            ..
        } => (false, text),
        AppEvent::AlabAccountsReady {
            text: Update::Outcome(text),
            ..
        } => (true, text),
        _ => panic!("a move posts only local account observations"),
    }
}

#[tokio::test]
async fn refused_commands_send_nothing_and_keep_the_draft() {
    for state in [
        "usage",
        "no-thread",
        "running",
        "queued",
        "pending",
        "turn-not-shown",
        "turn-seen-not-shown",
        "acting",
        "upgrading",
        "terminal",
        "question",
        "working",
        "no-frame",
    ] {
        let shared = if state == "no-frame" {
            Box::leak(Box::new(Shared::new()))
        } else {
            fresh()
        };
        let _act = (state == "acting").then(|| shared.begin_act());
        let (mut chat, _tx, mut events, mut ops) = make_chatwidget_manual_with_sender().await;
        chat.thread_id = (state != "no-thread").then(ThreadId::new);
        chat.apply_external_edit("unfinished work".into());
        while events.try_recv().is_ok() {}
        while ops.try_recv().is_ok() {}
        let sentence = match state {
            "usage" => USAGE,
            "no-thread" => NO_THREAD,
            "running" => {
                chat.bottom_pane.set_task_running(/*running*/ true);
                BUSY
            }
            "queued" => {
                chat.input_queue
                    .queued_user_messages
                    .push_back(crate::chatwidget::UserMessage::from("queued").into());
                BUSY
            }
            "pending" => {
                chat.input_queue.user_turn_pending_start = true;
                BUSY
            }
            // The widget reads idle: the session's own rest is what refuses.
            "turn-not-shown" => {
                // The primary thread's turn runs while the TUI shows another thread.
                assert!(shared.turn_started("primary-thread", "turn-1"));
                BUSY
            }
            "turn-seen-not-shown" => {
                shared.turn_seen("sub-agent-thread", "turn-1", /*running*/ true);
                BUSY
            }
            "acting" => BUSY,
            "upgrading" => {
                shared.set_exec_pending(true);
                BUSY
            }
            // The widget runs no turn, but the record would not say `idle`: a background
            // terminal keeps the frame working, a box keeps it blocked.
            "terminal" => {
                terminals(shared, ProgramState::Working, 1);
                TERMINALS
            }
            "question" => {
                frame(shared, ProgramState::Blocked(BlockedKind::Question), false);
                WAITING
            }
            // Working for another reason (a goal between its turns, MCP servers starting):
            // no turn to wait for or interrupt.
            "working" => {
                terminals(shared, ProgramState::Working, 0);
                BETWEEN
            }
            // No frame yet: the record says busy.
            _ => STARTING,
        };
        let label = if state == "usage" { "" } else { "work" };
        chat.alab_accounts_move("use", label, shared);
        // Wide enough that no sentence wraps.
        let pane = render_bottom_popup(&chat, 240);
        assert!(pane.contains(sentence), "{state}: {pane}");
        assert_eq!(
            chat.composer_text_with_pending(),
            "unfinished work",
            "{state}"
        );
        tokio::task::yield_now().await;
        assert!(events.try_recv().is_err(), "{state}: nothing was spawned");
        assert!(ops.try_recv().is_err(), "{state}: no model operation");
    }
}

#[test]
fn a_move_is_asked_only_at_the_records_idle_point() {
    // At rest: asked, at the command and after it.
    let shared = fresh();
    assert_eq!(unquiet(shared, false), None);
    assert_eq!(unquiet(shared, true), None);
    // A turn just ended (done, or error) is rest: the record says `idle`.
    for state in [ProgramState::Done, ProgramState::Error] {
        let shared = fresh();
        frame(shared, state, false);
        assert_eq!(unquiet(shared, true), None, "{state:?}");
    }
    // A key since the last frame is a draft after the command, never the command's own Enter.
    let shared = fresh();
    shared.human_input(/*now_ms*/ 2);
    assert_eq!(unquiet(shared, false), None);
    assert_eq!(unquiet(shared, true), Some(BUSY_SINCE));
    // Background terminals, a waiting box, work between turns and a start are named, at the
    // command and after it.
    for since in [false, true] {
        let shared = fresh();
        terminals(shared, ProgramState::Working, 2);
        assert_eq!(unquiet(shared, since), Some(TERMINALS), "{since}");
        let shared = fresh();
        terminals(shared, ProgramState::Working, 0);
        assert_eq!(unquiet(shared, since), Some(BETWEEN), "{since}");
        let unseen: &'static Shared = Box::leak(Box::new(Shared::new()));
        assert_eq!(unquiet(unseen, since), Some(STARTING), "{since}");
        for kind in [
            BlockedKind::Permission,
            BlockedKind::Question,
            BlockedKind::Auth,
        ] {
            let shared = fresh();
            frame(shared, ProgramState::Blocked(kind), false);
            assert_eq!(unquiet(shared, since), Some(WAITING), "{kind:?}");
        }
    }
    // A turn, an act or queued input comes first, whatever the frame reads.
    let busy = |since: bool| if since { BUSY_SINCE } else { BUSY };
    for since in [false, true] {
        let shared = fresh();
        terminals(shared, ProgramState::Working, 1);
        let _act = shared.begin_act();
        assert_eq!(unquiet(shared, since), Some(busy(since)));
        let shared = fresh();
        terminals(shared, ProgramState::Working, 1);
        assert!(shared.turn_started("primary-thread", "turn-1"));
        assert_eq!(unquiet(shared, since), Some(busy(since)));
        let shared = fresh();
        frame(shared, ProgramState::Idle, /*queued_input*/ true);
        assert_eq!(unquiet(shared, since), Some(busy(since)));
        // A turn the TUI only saw is no part of the record, but the restart ends it.
        let shared = fresh();
        shared.turn_seen("sub-agent-thread", "turn-1", /*running*/ true);
        assert_eq!(unquiet(shared, since), Some(busy(since)));
    }
}

/// A fake aterm showing this process the pool of [`offered`], and a client on it.
async fn fake_host(moves: bool) -> (tempfile::TempDir, FakeAterm, Client) {
    let directory = tempfile::tempdir().unwrap();
    let host = FakeAterm::start(directory.path(), codex_alab_tui::aterm::fake::Mode::Current)
        .await
        .unwrap();
    let row = |id: &str, label: &str, eligible: bool| {
        serde_json::json!({
            "id": id, "label": label, "masked_email": null, "condition": "usable",
            "enrolled": true, "held": false, "eligible": eligible,
            "quota": {"stale": true, "observed_at": null, "five_hour_used_percent": null,
                "five_hour_reset_at": null, "weekly_used_percent": null,
                "weekly_reset_at": null},
        })
    };
    host.set_account_pool(Some((
        "birth",
        Some(MAIN),
        serde_json::json!([
            row(MAIN, "main", false),
            row(WORK, "work", true),
            row(SPARE, "spare", true)
        ]),
    )));
    host.set_account_moves(FakeAccountMoves {
        choice: Some(WORK.into()),
        pinned: false,
        rotation: "wall".into(),
        select: true,
        auto: true,
    });
    host.answer_account_moves(moves);
    let client = Client::connect(host.endpoint(), codex_alab_tui::aterm::Options::default())
        .await
        .unwrap();
    (directory, host, client)
}

fn restarts(host: &FakeAterm) -> Vec<String> {
    host.requests()
        .into_iter()
        .filter(|line| line.split(' ').nth(1) == Some("restart"))
        .collect()
}

fn ask(auto: bool, label: &str) -> Ask {
    Ask {
        auto,
        label: label.into(),
        autopilot_acting: true,
        keyed: None,
    }
}

#[tokio::test]
async fn an_offered_move_posts_its_sentence_then_sends_one_restart() {
    let pid = std::process::id();
    for (auto, label, word, name) in [
        (false, "spare", "account=spare", "spare"),
        (true, "", "account-auto", "work"),
    ] {
        let (_directory, host, client) = fake_host(/*moves*/ true).await;
        let (chat, tx, mut events, mut ops) = make_chatwidget_manual_with_sender().await;
        while events.try_recv().is_ok() {}
        while ops.try_recv().is_ok() {}
        host.gate("restart");
        let running = tokio::spawn(move_session(
            Some(client),
            Some("birth".into()),
            ask(auto, label),
            Weak::new(),
            None,
            tx,
            fresh(),
        ));
        tokio::time::timeout(Duration::from_secs(10), async {
            while host.gated_requests().is_empty() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("the restart is asked");
        // The restart has reached aterm and is unanswered: the pending sentence is already
        // in the pane's queue, and nothing after it.
        let text = events
            .try_recv()
            .expect("pending posted before the request");
        let AppEvent::AlabAccountsReady {
            text: Update::Observation(text),
            ..
        } = text
        else {
            panic!("a local observation");
        };
        assert!(
            text.starts_with(&format!("Restarting this session on {name}…")),
            "{text}"
        );
        assert!(events.try_recv().is_err());
        host.open_gate("restart");
        assert!(
            next_text(&mut events)
                .await
                .starts_with("Restarting this session on")
        );
        running.await.unwrap();
        assert_eq!(
            restarts(&host),
            [format!("@{} restart pid={pid} {word}", host.sid())]
        );
        assert!(ops.try_recv().is_err(), "no model operation");
        drop(chat);
    }
}

#[tokio::test]
async fn an_old_aterm_restarts_nothing_and_says_changes_are_unavailable() {
    // An aterm before schema 2: the schema 1 fallback offers nothing, and nothing is asked.
    let (_directory, host, client) = fake_host(/*moves*/ false).await;
    host.answer_account_schema_two(false);
    let (_chat, tx, mut events, _ops) = make_chatwidget_manual_with_sender().await;
    while events.try_recv().is_ok() {}
    move_session(
        Some(client),
        Some("birth".into()),
        ask(false, "spare"),
        Weak::new(),
        None,
        tx,
        fresh(),
    )
    .await;
    assert_eq!(next_text(&mut events).await, CHANGES_UNAVAILABLE);
    assert!(events.try_recv().is_err());
    assert!(restarts(&host).is_empty());
    let snapshots = host
        .requests()
        .into_iter()
        .filter(|line| line.contains(" codex-accounts snapshot "))
        .count();
    assert_eq!(snapshots, 2, "schema 2, then schema 1 once");

    // A snapshot that offers the move from an aterm whose restart has no account words:
    // `ERR bad restart argument`, the same sentence, and nothing restarted there either.
    let (_directory, host, client) = fake_host(/*moves*/ false).await;
    let (_chat, tx, mut events, _ops) = make_chatwidget_manual_with_sender().await;
    while events.try_recv().is_ok() {}
    move_session(
        Some(client),
        Some("birth".into()),
        ask(false, "spare"),
        Weak::new(),
        None,
        tx,
        fresh(),
    )
    .await;
    assert!(
        next_text(&mut events)
            .await
            .starts_with("Restarting this session on spare…")
    );
    assert_eq!(next_text(&mut events).await, CHANGES_UNAVAILABLE);
    assert_eq!(restarts(&host).len(), 1);
}

#[tokio::test]
async fn each_answer_leaves_its_sentence() {
    let pid = std::process::id();
    for (reply, sentence) in [
        (
            "ERR restart unavailable: account-not-resumable".to_owned(),
            refusal(Why::NotResumable).to_owned(),
        ),
        (
            "ERR restart unavailable: not-supervised".to_owned(),
            refusal(Why::NotSupervised).to_owned(),
        ),
        // aterm's last fence, judged as it came to signal: nothing restarted. Every way aterm
        // finds no rest is named, a record it could not read among them, and no cause it
        // cannot know is claimed.
        (
            "ERR restart unavailable: account-busy".to_owned(),
            "aterm did not find this session at rest when it came to restart it (a turn, an \
             act, typing or unread input, or a session record it could not read), so it \
             restarted nothing. The session is unchanged."
                .to_owned(),
        ),
        (
            "ERR restart unavailable: exit-owner-not-ready".to_owned(),
            refusal(Why::ExitOwnerNotReady).to_owned(),
        ),
        (
            "ERR denied native operation authority".to_owned(),
            CHANGES_UNAVAILABLE.to_owned(),
        ),
        ("ERR denied".to_owned(), CHANGES_UNAVAILABLE.to_owned()),
        (
            "ERR bad restart argument: account=spare".to_owned(),
            CHANGES_UNAVAILABLE.to_owned(),
        ),
        (
            format!("OK restarting pid {pid} cause=asked account=main"),
            UNREAD_ANSWER.to_owned(),
        ),
        (format!("OK restart gone pid {pid}"), GONE.to_owned()),
    ] {
        let (_directory, host, client) = fake_host(/*moves*/ true).await;
        host.reply_once(
            &format!("@{} restart pid={pid} account=spare", host.sid()),
            &format!("{reply}\n"),
        );
        let (_chat, tx, mut events, _ops) = make_chatwidget_manual_with_sender().await;
        while events.try_recv().is_ok() {}
        move_session(
            Some(client),
            Some("birth".into()),
            ask(false, "spare"),
            Weak::new(),
            None,
            tx,
            fresh(),
        )
        .await;
        assert!(
            !next_update(&mut events).await.0,
            "{reply}: the pending sentence is the pane's alone"
        );
        let (last, text) = next_update(&mut events).await;
        assert_eq!(text, sentence, "{reply}");
        // A carried restart's word is the pending sentence, which names the account: the
        // process ends, and it stays in the pane.
        let carried = sentence.starts_with("Restarting");
        assert_eq!(last, !carried, "{reply}: the last word outlives the pane");
        assert!(!sentence.contains("account-"), "{reply}: never echoed");
    }
    // What the snapshot refuses is never asked of aterm.
    let (_directory, host, client) = fake_host(/*moves*/ true).await;
    let (_chat, tx, mut events, _ops) = make_chatwidget_manual_with_sender().await;
    while events.try_recv().is_ok() {}
    move_session(
        Some(client),
        Some("birth".into()),
        ask(false, "main"),
        Weak::new(),
        None,
        tx,
        fresh(),
    )
    .await;
    assert_eq!(next_text(&mut events).await, ALREADY);
    assert!(restarts(&host).is_empty());
}

#[tokio::test]
async fn a_record_that_cannot_be_written_asks_nothing() {
    let (_directory, host, client) = fake_host(/*moves*/ true).await;
    let shared = fresh();
    assert!(shared.set_flush(Box::new(|| false)));
    let (_chat, tx, mut events, _ops) = make_chatwidget_manual_with_sender().await;
    while events.try_recv().is_ok() {}
    move_session(
        Some(client),
        Some("birth".into()),
        ask(false, "spare"),
        Weak::new(),
        None,
        tx,
        shared,
    )
    .await;
    assert!(next_text(&mut events).await.starts_with("Restarting"));
    assert_eq!(next_text(&mut events).await, NOT_WRITTEN);
    assert!(events.try_recv().is_err());
    assert!(restarts(&host).is_empty());
}

#[tokio::test]
async fn a_session_busy_since_the_command_asks_nothing() {
    // The command was quiet when typed; while aterm's pool read is still unanswered, a turn,
    // an act, queued input, a key, a background terminal or a box begins. The restart would
    // end it, so none is asked, and no record is written for aterm to read.
    for state in ["turn", "act", "queued", "key", "terminal", "question"] {
        let (_directory, host, client) = fake_host(/*moves*/ true).await;
        let shared = fresh();
        let writes = Arc::new(AtomicUsize::new(0));
        let counted = writes.clone();
        assert!(shared.set_flush(Box::new(move || {
            counted.fetch_add(1, Ordering::SeqCst);
            true
        })));
        let (_chat, tx, mut events, _ops) = make_chatwidget_manual_with_sender().await;
        while events.try_recv().is_ok() {}
        host.gate("codex-accounts");
        let running = tokio::spawn(move_session(
            Some(client),
            Some("birth".into()),
            ask(false, "spare"),
            Weak::new(),
            None,
            tx,
            shared,
        ));
        assert!(
            host.wait_for(|host| !host.gated_requests().is_empty())
                .await,
            "{state}: the pool is asked"
        );
        let (act, sentence) = match state {
            "turn" => {
                assert!(shared.turn_started("primary-thread", "turn-1"));
                (None, BUSY_SINCE)
            }
            "act" => (Some(shared.begin_act()), BUSY_SINCE),
            "queued" => {
                frame(shared, ProgramState::Idle, /*queued_input*/ true);
                (None, BUSY_SINCE)
            }
            "key" => {
                shared.human_input(/*now_ms*/ 1);
                (None, BUSY_SINCE)
            }
            "terminal" => {
                terminals(shared, ProgramState::Working, 1);
                (None, TERMINALS)
            }
            _ => {
                frame(shared, ProgramState::Blocked(BlockedKind::Question), false);
                (None, WAITING)
            }
        };
        host.open_gate("codex-accounts");
        assert!(
            next_text(&mut events).await.starts_with("Restarting"),
            "{state}"
        );
        assert_eq!(next_text(&mut events).await, sentence, "{state}");
        running.await.unwrap();
        drop(act);
        assert!(restarts(&host).is_empty(), "{state}: no restart was sent");
        assert_eq!(
            writes.load(Ordering::SeqCst),
            0,
            "{state}: the record aterm reads is written only at rest"
        );
    }
}

#[tokio::test]
async fn a_session_busy_during_the_records_write_asks_nothing() {
    // At rest when the record is written, but a key or a background terminal comes during
    // the write: aterm would refuse it as `account-busy`; the client names it and asks nothing.
    for state in ["key", "terminal"] {
        let (_directory, host, client) = fake_host(/*moves*/ true).await;
        let shared = fresh();
        assert!(shared.set_flush(Box::new(move || {
            if state == "key" {
                shared.human_input(/*now_ms*/ 2);
            } else {
                terminals(shared, ProgramState::Working, 1);
            }
            true
        })));
        let (_chat, tx, mut events, _ops) = make_chatwidget_manual_with_sender().await;
        while events.try_recv().is_ok() {}
        move_session(
            Some(client),
            Some("birth".into()),
            ask(false, "spare"),
            Weak::new(),
            None,
            tx,
            shared,
        )
        .await;
        assert!(next_text(&mut events).await.starts_with("Restarting"));
        let sentence = if state == "key" {
            BUSY_SINCE
        } else {
            TERMINALS
        };
        assert_eq!(next_text(&mut events).await, sentence, "{state}");
        assert!(events.try_recv().is_err());
        assert!(restarts(&host).is_empty(), "{state}: no restart was sent");
    }
}

#[tokio::test]
async fn the_commands_own_key_waits_for_its_frame_before_the_record_is_written() {
    // The command's own Enter reads as a draft until the next frame shows the composer empty,
    // and the pool's read can answer before that frame comes. The move waits for it, bounded,
    // and is then asked; a key since the command is a draft, refused at once. CONTROL: a TUI
    // that draws no frame within the bound refuses the move, nothing asked.
    for case in ["frame", "key-since", "no-frame"] {
        let (_directory, host, client) = fake_host(/*moves*/ true).await;
        let shared = fresh();
        // The command's own Enter, the person's last input as the command ran.
        shared.human_input(/*now_ms*/ 5);
        let keyed = shared.last_human_input_ms();
        if case == "key-since" {
            shared.human_input(/*now_ms*/ 6);
        }
        let (_chat, tx, mut events, _ops) = make_chatwidget_manual_with_sender().await;
        while events.try_recv().is_ok() {}
        let running = tokio::spawn(move_session(
            Some(client),
            Some("birth".into()),
            Ask {
                keyed,
                ..ask(false, "spare")
            },
            Weak::new(),
            None,
            tx,
            shared,
        ));
        assert!(
            next_text(&mut events).await.starts_with("Restarting"),
            "{case}"
        );
        // The pool has answered and the pending sentence is posted: the frame comes after.
        let asked = Instant::now();
        if case == "frame" {
            tokio::time::sleep(Duration::from_millis(50)).await;
            frame(shared, ProgramState::Idle, /*queued_input*/ false);
        }
        let last = next_text(&mut events).await;
        running.await.unwrap();
        if case == "frame" {
            assert!(last.starts_with("Restarting"), "{case}: {last}");
            assert_eq!(restarts(&host).len(), 1, "{case}: asked once");
        } else {
            assert_eq!(last, BUSY_SINCE, "{case}");
            assert!(restarts(&host).is_empty(), "{case}: nothing asked");
        }
        if case == "no-frame" {
            assert!(
                asked.elapsed() >= OWN_KEY_FRAME / 2,
                "{case}: waited for its frame"
            );
        }
        if case == "key-since" {
            assert!(
                asked.elapsed() < OWN_KEY_FRAME,
                "{case}: refused without waiting"
            );
        }
    }
}

#[tokio::test]
async fn a_moves_last_word_is_said_in_history_where_its_pane_is_gone() {
    // The person closes the pane while aterm weighs the move (its Esc can itself refuse it):
    // the move's last word, a fixed sentence naming no account, is said once in history.
    // CONTROLS: the pending sentence of a closed pane is dropped, and a last word whose pane
    // is open stays in the pane, no history written.
    let (mut chat, _tx, mut events, mut ops) = make_chatwidget_manual_with_sender().await;
    while events.try_recv().is_ok() {}
    while ops.try_recv().is_ok() {}
    let thread = chat.thread_id();
    let open = chat.alab_accounts_view();
    chat.alab_accounts_ready(
        open.clone(),
        thread,
        Update::Outcome(refusal(Why::Limited).into()),
    );
    let pane = render_bottom_popup(&chat, 240);
    assert!(pane.contains(refusal(Why::Limited)), "{pane}");
    assert!(
        events.try_recv().is_err(),
        "CONTROL: an open pane writes no history"
    );
    chat.bottom_pane
        .dismiss_view_by_id(super::super::accounts_pool::VIEW_ID);
    assert!(open.upgrade().is_none());
    chat.alab_accounts_ready(
        open.clone(),
        thread,
        Update::Observation("Restarting this session on work…".into()),
    );
    assert!(
        events.try_recv().is_err(),
        "CONTROL: a pending sentence is the pane's alone"
    );
    chat.alab_accounts_ready(open, thread, Update::Outcome(refusal(Why::Busy).into()));
    let mut said = Vec::new();
    while let Ok(event) = events.try_recv() {
        if let AppEvent::InsertHistoryCell(cell) = event {
            said.push(
                cell.display_lines(/*width*/ 240)
                    .iter()
                    .map(|line| {
                        line.spans
                            .iter()
                            .map(|span| span.content.as_ref())
                            .collect::<String>()
                    })
                    .collect::<Vec<_>>()
                    .join(" "),
            );
        }
    }
    assert_eq!(said.len(), 1, "said once: {said:?}");
    assert!(said[0].contains(refusal(Why::Busy)), "{said:?}");
    assert!(ops.try_recv().is_err(), "no model operation");
}
