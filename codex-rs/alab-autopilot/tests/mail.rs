// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The native inbox from the supervisor's side (CONTRACT §5 "inbox"), against alab-aterm's
//! in-process FAKE aterm (never a real one): the mail a typed `Inbox: task @<off>` names is
//! read only under a proven claim; a mail turn's end marks its row and reports back to the
//! sender; the status line's badge follows the `mail` stream, display only.

use std::sync::Arc;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;
use std::time::Duration;

use codex_alab_aterm::Options as ClientOptions;
use codex_alab_aterm::fake::FakeAterm;
use codex_alab_aterm::fake::Mode;
use codex_alab_autopilot::Supervisor;
use codex_alab_autopilot::SupervisorOptions;
use codex_alab_core::mail::MailTurn;
use codex_alab_core::mail::TriggerKind;
use codex_alab_core::mail::Verdict;
use codex_alab_core::session::Frame;
use codex_alab_core::session::ProgramState;
use codex_alab_core::session::Shared;
use codex_alab_core::session::now_ms;
use pretty_assertions::assert_eq;

struct Harness {
    fake: FakeAterm,
    shared: &'static Shared,
    sup: Arc<Supervisor>,
    frames: Arc<AtomicU32>,
    _dir: tempfile::TempDir,
}

async fn harness(mode: Mode) -> Harness {
    codex_alab_core::activate();
    let dir = tempfile::tempdir().unwrap_or_else(|err| panic!("scratch dir: {err}"));
    let fake = FakeAterm::start(dir.path(), mode)
        .await
        .unwrap_or_else(|err| panic!("fake aterm: {err}"));
    fake.set_policy_key("human_grace_s", "1");
    let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
    let frames = Arc::new(AtomicU32::new(0));
    let count = Arc::clone(&frames);
    shared.set_waker(Box::new(move || {
        count.fetch_add(1, Ordering::SeqCst);
    }));
    let endpoint = fake.endpoint();
    let sup = Supervisor::new(SupervisorOptions {
        shared,
        endpoint: Arc::new(move || Some(endpoint.clone())),
        client: ClientOptions::default(),
    });
    sup.start();
    shared.observe(
        &Frame {
            primary_thread: Some("019a0000-0000-7000-8000-0000000000aa"),
            displayed_thread: Some("019a0000-0000-7000-8000-0000000000aa"),
            rollout_path: None,
            cwd: std::path::Path::new("/work"),
            model: "gpt-5",
            codex_home: dir.path(),
            embedded: true,
            state: ProgramState::Idle,
            draft_empty: true,
            queued_input: false,
            popup_open: false,
            editor_open: false,
            pending_user_turn: false,
        },
        now_ms(),
    );
    Harness {
        fake,
        shared,
        sup,
        frames,
        _dir: dir,
    }
}

/// Wait until `done` holds, at most a minute: a hang detector, never a latency budget.
async fn wait_until(what: &str, done: impl Fn() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    while !done() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "never happened: {what}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// The TUI's submit path calls the reader from its own thread, never the runtime's.
async fn read(
    h: &Harness,
    kind: TriggerKind,
    off: u64,
) -> Result<codex_alab_core::mail::InboxMail, String> {
    let shared = h.shared;
    tokio::task::spawn_blocking(move || shared.read_mail(kind, off, Duration::from_secs(5)))
        .await
        .unwrap_or_else(|err| panic!("read: {err}"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_mail_a_typed_line_names_is_read_under_the_proven_claim() {
    let h = harness(Mode::Current).await;
    let (id, off) = h
        .fake
        .deliver("s-lead@n-lab", "task", "agent", "Run the tests.");
    wait_until("the claim is held", || h.sup.held()).await;
    let mail = read(&h, TriggerKind::Task, off).await.expect("mail");
    assert_eq!((mail.id, mail.off), (id, off));
    assert_eq!(mail.body, "Run the tests.");
    assert!(mail.sendable());
    // A relayed one reads too: the TUI holds it in the composer for a person.
    let (_, relayed) = h
        .fake
        .deliver("s-lead@n-lab", "task", "relayed", "From a screen.");
    let relayed = read(&h, TriggerKind::Task, relayed).await.expect("mail");
    assert!(!relayed.sendable());
    // Errors are said, and the line then goes as typed.
    assert!(read(&h, TriggerKind::Ask, off).await.is_err());
    assert!(read(&h, TriggerKind::Task, off + 999).await.is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn without_a_proven_claim_no_mail_is_read() {
    // Negative control: an aterm that never proves the claim.
    let h = harness(Mode::Old).await;
    let (_, off) = h.fake.deliver("s-lead@n-lab", "task", "agent", "go");
    wait_until("the supervisor read the old aterm", || {
        h.sup.journal().iter().any(|l| l.contains("never claims"))
    })
    .await;
    let read = read(&h, TriggerKind::Task, off).await;
    assert_eq!(read, Err("no proven aterm claim".to_string()));
    assert!(
        !h.fake.requests().iter().any(|r| r.contains("inbox")),
        "{:?}",
        h.fake.requests()
    );
}

fn turn(id: u64, off: u64) -> MailTurn {
    MailTurn {
        id,
        off,
        from: "s-lead@n-lab".to_string(),
        kind: "task".to_string(),
        text: "…".to_string(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_mail_turns_end_marks_the_row_and_reports_back() {
    let h = harness(Mode::Current).await;
    let (id, off) = h.fake.deliver("s-lead@n-lab", "task", "agent", "go");
    h.fake.set_status("seq", "41");
    h.fake.set_status("hash", "00112233445566aa");
    wait_until("the claim is held", || h.sup.held()).await;
    h.sup
        .receipt(
            turn(id, off),
            Verdict::Handled,
            Some("All green.".to_string()),
        )
        .await;
    assert_eq!(h.fake.receipts(), vec![(id, "handled".to_string())]);
    let posts = h.fake.posts();
    assert_eq!(posts.len(), 1, "{:?}", h.sup.journal());
    // A session sender is answered as `@s-…`, the one form aterm's bridge routes (a bare
    // `s-…@n-…` is answered `OK` locally and retired on the bridge).
    assert!(posts[0].options.contains(&"to=@s-lead@n-lab".to_string()));
    assert!(posts[0].routed);
    assert!(posts[0].options.contains(&"kind=report".to_string()));
    assert!(posts[0].options.contains(&format!("re={off}")));
    assert_eq!(posts[0].body, "seq=41 hash=00112233445566aa\nAll green.");
    assert!(
        h.fake.acts().iter().any(|a| a == "mail-report@v1"),
        "{:?}",
        h.fake.acts()
    );
    // An interrupted turn defers.
    let (id2, off2) = h.fake.deliver("s-lead@n-lab", "task", "agent", "again");
    h.sup
        .receipt(turn(id2, off2), Verdict::Deferred, None)
        .await;
    assert_eq!(
        h.fake.receipts().last(),
        Some(&(id2, "deferred".to_string()))
    );
    assert!(
        h.fake.posts()[1]
            .body
            .ends_with("(the turn was interrupted or stopped at an error)")
    );
}

/// No receipt or report goes without the claim; they are owed, and go once codex-alab claims
/// again (review of fea034d20, finding 1: they were dropped for good). Negative control: the
/// claim cleared, nothing is sent until the policy moves, however long it waits (another
/// mail's frame and the badge read it brings included).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_receipt_goes_without_the_claim_and_an_owed_one_goes_at_the_next_claim() {
    let h = harness(Mode::Current).await;
    let (id, off) = h.fake.deliver("s-lead@n-lab", "task", "agent", "go");
    wait_until("the claim is held", || h.sup.held()).await;
    h.fake.clear_claim();
    wait_until("passive", || !h.sup.held()).await;
    h.sup
        .receipt(turn(id, off), Verdict::Handled, Some("done".to_string()))
        .await;
    assert!(h.fake.receipts().is_empty());
    assert!(h.fake.posts().is_empty());
    // The wait before the policy moves: a frame for other mail comes, and still nothing goes.
    let _ = h.fake.deliver("s-lead@n-lab", "note", "agent", "fyi");
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(h.fake.receipts().is_empty(), "{:?}", h.fake.receipts());
    assert!(h.fake.posts().is_empty(), "{:?}", h.fake.posts());
    // The owner's policy moves: codex-alab claims again, and what it owed goes.
    h.fake.set_policy_key("continue_text", "keep at it");
    wait_until("the owed receipt and report go", || {
        h.fake.receipts() == vec![(id, "handled".to_string())] && h.fake.posts().len() == 1
    })
    .await;
    assert!(h.fake.posts()[0].body.ends_with("done"));
}

/// The turn's end holds its verdict first, and a badge read (another mail's frame) may send the
/// receipt before the receipt task runs: that task sends it no second time, and still posts the
/// report. Negative control: a receipt never held is sent by the receipt task itself.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_receipt_a_badge_read_already_sent_is_not_sent_again() {
    let h = harness(Mode::Current).await;
    let (id, off) = h.fake.deliver("s-lead@n-lab", "task", "agent", "go");
    wait_until("the claim is held", || h.sup.acting()).await;
    h.sup.hold_receipt(id, Verdict::Handled);
    let (id_q, off_q) = h.fake.deliver("s-lead@n-lab", "ask", "agent", "q");
    wait_until("the badge read sends the held receipt", || {
        h.fake.receipts() == vec![(id, "handled".to_string())]
    })
    .await;
    h.sup
        .receipt(turn(id, off), Verdict::Handled, Some("done".to_string()))
        .await;
    assert_eq!(h.fake.receipts(), vec![(id, "handled".to_string())]);
    assert_eq!(h.fake.posts().len(), 1, "{:?}", h.sup.journal());
    h.sup
        .receipt(turn(id_q, off_q), Verdict::Handled, None)
        .await;
    assert_eq!(
        h.fake.receipts(),
        vec![(id, "handled".to_string()), (id_q, "handled".to_string())]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_badge_follows_the_mail_stream_and_types_nothing() {
    let h = harness(Mode::Current).await;
    wait_until("the claim is held", || h.sup.held()).await;
    assert_eq!(h.shared.mail_badge(), None);
    let before = h.frames.load(Ordering::SeqCst);
    let (id, _) = h.fake.deliver("s-lead@n-lab", "task", "agent", "go");
    wait_until("the badge shows the task", || {
        h.shared.mail_badge().and_then(|b| b.text()).as_deref() == Some("✉ 1 task")
    })
    .await;
    assert!(h.frames.load(Ordering::SeqCst) > before, "a frame shows it");
    // Handled elsewhere (the watermark moved): the badge goes.
    let c = codex_alab_aterm::Client::connect(h.fake.endpoint(), ClientOptions::default())
        .await
        .unwrap_or_else(|err| panic!("connect: {err}"));
    codex_alab_aterm::mail::seen(&c, id, Verdict::Handled)
        .await
        .unwrap_or_else(|err| panic!("seen: {err}"));
    wait_until("the badge clears", || h.shared.mail_badge().is_none()).await;
    // Nothing was typed, posted or acted on for the mail.
    assert!(
        !h.fake
            .requests()
            .iter()
            .any(|r| r.contains(" send ") || r.contains(" paste ") || r.contains(" post ")),
        "{:?}",
        h.fake.requests()
    );
    // The claim goes: so does the badge.
    h.fake.deliver("s-lead@n-lab", "ask", "agent", "q");
    wait_until("the badge shows the ask", || {
        h.shared.mail_badge().is_some()
    })
    .await;
    h.fake.clear_claim();
    wait_until("the badge clears with the claim", || {
        h.shared.mail_badge().is_none()
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_receipt_waits_behind_an_older_unhandled_task() {
    let h = harness(Mode::Current).await;
    let (a, off_a) = h.fake.deliver("s-lead@n-lab", "task", "agent", "first");
    let (b, off_b) = h.fake.deliver("s-lead@n-lab", "task", "agent", "second");
    wait_until("the claim is held", || h.sup.held()).await;
    // @B's turn ends while @A is unhandled: no receipt passes @A, the report still goes,
    // and attention says why the receipt waits.
    h.sup
        .receipt(
            turn(b, off_b),
            Verdict::Handled,
            Some("B done.".to_string()),
        )
        .await;
    assert!(h.fake.receipts().is_empty(), "{:?}", h.fake.receipts());
    assert_eq!(h.fake.posts().len(), 1);
    let attention = h.fake.attention();
    assert!(
        attention
            .values()
            .any(|t| t.contains(&format!("waits behind the unhandled task @{off_a}"))),
        "{attention:?}"
    );
    // The badge still counts @A (and @B, whose receipt is owed).
    wait_until("the badge counts both", || {
        h.shared.mail_badge().and_then(|b| b.text()).as_deref() == Some("✉ 2 tasks")
    })
    .await;
    // @A's turn ends: both receipts go, in order, and the attention clears.
    h.sup
        .receipt(
            turn(a, off_a),
            Verdict::Handled,
            Some("A done.".to_string()),
        )
        .await;
    assert_eq!(
        h.fake.receipts(),
        vec![(a, "handled".to_string()), (b, "handled".to_string())]
    );
    assert!(h.fake.attention().is_empty(), "{:?}", h.fake.attention());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_owed_receipt_goes_once_the_older_task_is_handled_elsewhere() {
    let h = harness(Mode::Current).await;
    let (a, _) = h.fake.deliver("s-lead@n-lab", "task", "agent", "first");
    let (b, off_b) = h.fake.deliver("s-lead@n-lab", "task", "agent", "second");
    wait_until("the claim is held", || h.sup.held()).await;
    h.sup.receipt(turn(b, off_b), Verdict::Handled, None).await;
    assert!(h.fake.receipts().is_empty());
    // A person marks @A handled: the `inbox-seen` event lets @B's receipt go.
    let c = codex_alab_aterm::Client::connect(h.fake.endpoint(), ClientOptions::default())
        .await
        .unwrap_or_else(|err| panic!("connect: {err}"));
    codex_alab_aterm::mail::seen(&c, a, Verdict::Handled)
        .await
        .unwrap_or_else(|err| panic!("seen: {err}"));
    wait_until("the owed receipt goes", || {
        h.fake.receipts().contains(&(b, "handled".to_string()))
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_handled_mail_is_never_run_again() {
    let h = harness(Mode::Current).await;
    let (id, off) = h.fake.deliver("s-lead@n-lab", "task", "agent", "go");
    wait_until("the claim is held", || h.sup.held()).await;
    assert!(read(&h, TriggerKind::Task, off).await.is_ok());
    // The turn ended handled: the line recalled from history is refused at once (before the
    // receipt task even ran), and after the receipt too.
    h.sup.hold_receipt(id, Verdict::Handled);
    let again = read(&h, TriggerKind::Task, off).await;
    assert!(
        matches!(&again, Err(why) if why.contains("already handled")),
        "{again:?}"
    );
    h.sup.receipt(turn(id, off), Verdict::Handled, None).await;
    assert!(read(&h, TriggerKind::Task, off).await.is_err());
    assert_eq!(h.fake.posts().len(), 1, "one report, never two");
    // Positive control: an interrupted (deferred) mail may run again.
    let (id2, off2) = h.fake.deliver("s-lead@n-lab", "task", "agent", "again");
    h.sup
        .receipt(turn(id2, off2), Verdict::Deferred, None)
        .await;
    assert!(read(&h, TriggerKind::Task, off2).await.is_ok());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unanswered_badge_read_never_costs_the_claim() {
    // An unanswered exchange closes the connection it ran on: the badge reads on its own.
    let h = harness(Mode::Current).await;
    wait_until("the claim is held", || h.sup.held()).await;
    let claim_conn = h.fake.claim().and_then(|c| c.conn);
    h.fake.stall("inbox");
    h.fake.deliver("s-lead@n-lab", "task", "agent", "go");
    wait_until("the badge read gave up", || {
        h.sup
            .journal()
            .iter()
            .any(|l| l.contains("inbox unread for the badge"))
    })
    .await;
    assert!(h.sup.held(), "{:?}", h.sup.journal());
    assert_eq!(h.fake.claim().and_then(|c| c.conn), claim_conn);
    h.fake.unstall("inbox");
    h.fake.deliver("s-lead@n-lab", "ask", "agent", "q");
    wait_until("the badge shows both", || {
        h.shared.mail_badge().and_then(|b| b.text()).as_deref() == Some("✉ 1 task · 1 ask")
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_sender_the_bridge_cannot_route_gets_no_report() {
    let h = harness(Mode::Current).await;
    let (id, off) = h.fake.deliver("s-lead@n-lab", "task", "agent", "go");
    wait_until("the claim is held", || h.sup.held()).await;
    let mut t = turn(id, off);
    t.from = "lead".to_string();
    h.sup
        .receipt(t, Verdict::Handled, Some("done".to_string()))
        .await;
    assert_eq!(h.fake.receipts(), vec![(id, "handled".to_string())]);
    assert!(h.fake.posts().is_empty());
    assert!(
        h.sup.journal().iter().any(|l| l.contains("no report")),
        "{:?}",
        h.sup.journal()
    );
}

/// Review of 7e7622ff7, finding 5: a report whose post failed (aterm did not answer) is owed
/// again, and goes by itself a retry later (shortened here), with no mail frame, claim or
/// resume to bring it. Negative control: a report aterm refused is dropped and never posted
/// again, however many retries pass.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_report_whose_post_failed_is_owed_and_sent_again() {
    let h = harness(Mode::Current).await;
    h.sup.set_owed_retry(Duration::from_millis(200));
    let posts_tried = |f: &FakeAterm| {
        f.requests()
            .iter()
            .filter(|r| r.contains(" post to="))
            .count()
    };
    wait_until("the claim is held", || h.sup.acting()).await;
    // Refused: dropped.
    h.fake
        .refuse_posts(Some("ERR quota: the sender's inbox is full"));
    let (id, off) = h.fake.deliver("s-lead@n-lab", "task", "agent", "go");
    h.sup
        .receipt(turn(id, off), Verdict::Handled, Some("done".to_string()))
        .await;
    assert_eq!(h.fake.receipts(), vec![(id, "handled".to_string())]);
    assert_eq!(posts_tried(&h.fake), 1);
    h.fake.refuse_posts(None);
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(posts_tried(&h.fake), 1, "{:?}", h.fake.requests());
    assert!(h.fake.posts().is_empty());
    // Unanswered: owed again, and sent once aterm answers, by the retry alone. Its receipt
    // went before, with a badge read, so no `inbox-seen` event follows the failure to bring
    // another send.
    let (id2, off2) = h.fake.deliver("s-lead@n-lab", "task", "agent", "again");
    h.sup.hold_receipt(id2, Verdict::Handled);
    let _ = h.fake.deliver("s-lead@n-lab", "note", "agent", "fyi");
    wait_until("a badge read sends the receipt", || {
        h.fake.receipts().last() == Some(&(id2, "handled".to_string()))
    })
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    h.fake.stall("post");
    h.sup
        .receipt(
            turn(id2, off2),
            Verdict::Handled,
            Some("again done".to_string()),
        )
        .await;
    assert!(h.fake.posts().is_empty());
    assert!(
        h.sup
            .journal()
            .iter()
            .any(|l| l.contains("the report failed") && l.contains("still owed")),
        "{:?}",
        h.sup.journal()
    );
    h.fake.unstall("post");
    wait_until("the owed report goes on a retry", || {
        h.fake.posts().len() == 1
    })
    .await;
    assert!(h.fake.posts()[0].body.ends_with("again done"));
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(h.fake.posts().len(), 1, "posted once: {:?}", h.fake.posts());
}

/// Review of 7e7622ff7, finding 6: a report a badge read's send pops (one owed after that send
/// began, its stamp unread then) still opens with the screen's stamp. Negative control: the
/// receipt that send was there for goes first, as before.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_report_a_badge_read_sends_carries_the_stamp() {
    let h = harness(Mode::Current).await;
    h.fake.set_status("seq", "41");
    h.fake.set_status("hash", "00112233445566aa");
    let (id, off) = h.fake.deliver("s-lead@n-lab", "task", "agent", "go");
    wait_until("the claim is held", || h.sup.acting()).await;
    // The turn's end holds its receipt; a badge read's send (another mail's frame) starts with
    // no report owed and waits on the inbox rows.
    h.sup.hold_receipt(id, Verdict::Handled);
    let met = h.fake.gated_requests().len();
    h.fake.gate("inbox");
    let _ = h.fake.deliver("s-lead@n-lab", "note", "agent", "fyi");
    wait_until("the badge's send reads the rows", || {
        h.fake.gated_requests().len() > met
    })
    .await;
    // The turn's end owes its report, then connects for its own send (which waits its turn).
    let conns = h.fake.connections();
    let sup = Arc::clone(&h.sup);
    let receipt = tokio::spawn(async move {
        sup.receipt(
            turn(id, off),
            Verdict::Handled,
            Some("All green.".to_string()),
        )
        .await;
    });
    wait_until("the turn's end connects", || h.fake.connections() > conns).await;
    h.fake.open_gate("inbox");
    tokio::time::timeout(Duration::from_secs(60), receipt)
        .await
        .unwrap_or_else(|_| panic!("the turn's end hung"))
        .unwrap_or_else(|err| panic!("the turn's end: {err}"));
    assert_eq!(h.fake.receipts(), vec![(id, "handled".to_string())]);
    assert_eq!(h.fake.posts().len(), 1, "{:?}", h.sup.journal());
    assert_eq!(
        h.fake.posts()[0].body,
        "seq=41 hash=00112233445566aa\nAll green."
    );
}

/// Review of 7e7622ff7, finding 7: a standby that begins while a send reads the inbox rows
/// stops its receipts (CONTRACT §2: they go only while codex-alab acts); they go once it acts
/// again. Negative control: the same send with no standby sends the receipt.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_standby_while_receipts_are_flushed_holds_them() {
    let h = harness(Mode::Current).await;
    wait_until("the claim is held", || h.sup.acting()).await;
    for standby in [false, true] {
        let (id, _) = h.fake.deliver("s-lead@n-lab", "task", "agent", "go");
        h.shared.set_mail_badge(None);
        h.sup.hold_receipt(id, Verdict::Handled);
        let met = h.fake.gated_requests().len();
        h.fake.gate("inbox");
        let _ = h.fake.deliver("s-lead@n-lab", "note", "agent", "fyi");
        wait_until("the badge's send reads the rows", || {
            h.fake.gated_requests().len() > met
        })
        .await;
        if standby {
            h.fake.set_effective(false, "suspended");
            wait_until("on standby", || h.sup.standby().is_some()).await;
        }
        h.fake.open_gate("inbox");
        wait_until("the badge read after the send", || {
            h.shared.mail_badge().is_some()
        })
        .await;
        if standby {
            assert!(
                !h.fake.receipts().iter().any(|(r, _)| *r == id),
                "{:?} {:?}",
                h.fake.receipts(),
                h.sup.journal()
            );
            h.fake.set_effective(true, "enabled");
            wait_until("the receipt goes once acting again", || {
                h.fake.receipts().iter().any(|(r, _)| *r == id)
            })
            .await;
        } else {
            assert!(
                h.fake.receipts().iter().any(|(r, _)| *r == id),
                "{:?} {:?}",
                h.fake.receipts(),
                h.sup.journal()
            );
        }
    }
}
