// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! THE ACCOUNT HANDOFF IS ASKED FOR ONLY UNTIL THE WALL'S OWN RESET (2026-11-12). A turn that
//! ended at a usage wall asks the host to move the conversation to another account
//! (`codex-rotation prepare`). `not-needed` releases the point to the wall's ordinary resume.
//! `waiting`, an error and an old host's unknown verb are asked again every five seconds, but
//! only until the reset the wall itself names: from then the account the session is on can run
//! again, and the resume is typed on it. Against alab-aterm's in-process fake aterm and a
//! scratch ChatGPT login codex's own login code wrote; the model is a loopback mock.
//!
//! HOW A RESET IS BROUGHT (review, 2026-11-12). A wall's reset is named when the model's 429
//! is answered, never before the harness starts, so no start-up time counts against it. All
//! but one test name it an hour ahead and bring it by having the host sleep
//! ([`Walled::host_sleeps`]); the awake case waits for a reset a few seconds after the 429.

use super::login_fixtures as fixtures;
use super::renewal_admission::subscription_fixture;
use super::*;
use codex_alab_core::session::Inherited;
use codex_alab_core::session::OwedAct;
use codex_alab_core::session::Recovery;
use codex_alab_core::session::RecoveryWall;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

/// The host policy under which a usage wall asks for another account (`codex_rotation`,
/// `accounts`; `relaunch` is the fake's default) and, failing one, waits the wall out
/// (`resume_limits`, `limit_wait`). `continue` is off so the resume is the only act typed.
const POLICY: &[(&str, &str)] = &[
    ("human_grace_s", "0"),
    ("codex_rotation", "1"),
    ("accounts", "wall"),
    ("resume_limits", "true"),
    ("limit_wait", "true"),
    ("continue", "false"),
];

/// [`POLICY`] with `limit_wait` off: a usage wall is asked about as before (the rotation
/// policy does not read that switch) and, with no handoff, is left to a person.
const POLICY_NO_WAIT: &[(&str, &str)] = &[
    ("human_grace_s", "0"),
    ("codex_rotation", "1"),
    ("accounts", "wall"),
    ("resume_limits", "true"),
    ("limit_wait", "false"),
    ("continue", "false"),
];

const ROTATION: &str = "codex-rotation";
/// The attention a usage wall raises where its wait is switched off.
const LIMIT_OFF: &str = "resume_limits or limit_wait is off";
const RESUME: &str = "usage-resume@v1";
const LAPSED: &str = "the usage wall's reset came with no account handoff bound";
/// The journal's line for a usage wall's own decision while its reset is ahead:
/// `wait <n>s for usage-resume@v1 (...)`.
const WAITS: &str = "s for usage-resume@v1";

/// A session at a usage wall whose 429 named `resets_at` (epoch seconds) as it was answered.
struct Walled {
    h: Harness,
    resets_at: u64,
    /// The model's requests: the walled turn's, then the resume's.
    model: responses::ResponseMock,
    /// The exact handoff request the Autopilot sends for the walled turn.
    prepare: String,
    /// The host's time asleep since boot, as the Autopilot reads it here: unchanged unless a
    /// test has the host sleep ([`Walled::host_sleeps`]), whatever this Mac does meanwhile.
    asleep: Arc<std::sync::Mutex<Duration>>,
    /// What the record held at each write-through, oldest first ([`Written`]).
    written: Arc<std::sync::Mutex<Vec<Written>>>,
    _refresh: wiremock::MockServer,
}

/// What one write-through of the session record carried: the owed act and the recovery, as
/// the record's writer would have read them at that moment. The harness keeps no record
/// file, so its writer's hook only takes this snapshot.
type Written = (Option<OwedAct>, Option<Recovery>);

/// The receipt `written`'s handoff intent is bound to; `None` for an unbound intent, and for
/// a record that holds no intent.
fn receipt_written(written: &Written) -> Option<&str> {
    written.0.as_ref()?.renewal.as_ref()?.receipt.as_deref()
}

/// Whether `written` holds a handoff intent no receipt binds.
fn unbound_written(written: &Written) -> bool {
    written
        .0
        .as_ref()
        .and_then(|owed| owed.renewal.as_ref())
        .is_some_and(|renewal| renewal.receipt.is_none())
}

/// Whether `written` holds a usage wall's recovery.
fn wall_written(written: &Written) -> bool {
    matches!(
        written.1,
        Some(Recovery {
            wall: RecoveryWall::Usage { .. },
            ..
        })
    )
}

impl Walled {
    /// Every handoff request the host was sent.
    fn prepares(&self) -> usize {
        self.h
            .fake
            .requests()
            .iter()
            .filter(|request| request.contains(" codex-rotation prepare "))
            .count()
    }

    fn resumes(&self) -> usize {
        self.h
            .fake
            .acts()
            .iter()
            .filter(|act| *act == RESUME)
            .count()
    }

    /// The host sleeps for `slept`: the Autopilot's own clock does not move.
    fn host_sleeps(&self, slept: Duration) {
        *self
            .asleep
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) += slept;
    }

    /// The wall's reset has come, by the wall clock the 429 named it on.
    fn reset_passed(&self) -> bool {
        now_ms() / 1000 >= self.resets_at
    }

    /// Every request the host was sent under `verb` of the handoff protocol.
    fn rotation_requests(&self, verb: &str) -> usize {
        let needle = format!(" {ROTATION} {verb} ");
        self.h
            .fake
            .requests()
            .iter()
            .filter(|request| request.contains(&needle))
            .count()
    }

    /// The receipt the handoff intent on record is bound to.
    fn bound_receipt(&self) -> Option<String> {
        self.h.shared.owed_act()?.renewal?.receipt
    }

    /// The reset of the usage wall on record for the walled turn, in epoch milliseconds: the
    /// recovery an unbound handoff intent carries beside it for the process after a restart.
    fn wall_on_record(&self) -> Option<u64> {
        let recovery = self.h.shared.recovery()?;
        let asked = self.h.shared.owed_act()?.native_origin?.turn_id;
        assert_eq!(recovery.thread, self.h.thread);
        assert_eq!(
            recovery.turn_id, asked,
            "the wall of the turn the ask names"
        );
        match recovery.wall {
            RecoveryWall::Usage { reset_at_ms, .. } => reset_at_ms,
            other => panic!("not a usage wall: {other:?}"),
        }
    }

    /// The handoff intent is on record (it is published before the host is asked).
    fn intent_stands(&self) -> bool {
        self.h
            .shared
            .owed_act()
            .is_some_and(|owed| owed.renewal.is_some())
    }

    /// The receipt a host would bind this walled turn's handoff to.
    fn handoff(&self) -> Handoff {
        let id = "1".repeat(32);
        let receipt = self
            .prepare
            .split_once(" prepare ")
            .map(|(_, origin)| {
                format!(
                    "id={id} {origin} target_account={} target_user={}",
                    "c".repeat(64),
                    "d".repeat(64)
                )
            })
            .unwrap_or_else(|| panic!("the ask names no origin: {}", self.prepare));
        let status = format!("@{} {ROTATION} status id={id}", self.h.fake.sid());
        Handoff {
            id,
            receipt,
            status,
        }
    }

    /// The host answers the held ask `ready` and every `status` after it `waiting`, with one
    /// receipt: codex-alab binds it and sends `commit` (the fake knows no `commit` and answers
    /// `ERR`, so the handoff stays bound and unfinished), then asks about it.
    async fn bound(&self) -> Handoff {
        let handoff = self.handoff();
        self.h.fake.reply_once(
            &self.prepare,
            &format!("OK {ROTATION} state=ready {}\n", handoff.receipt),
        );
        self.h.fake.reply_always(
            &handoff.status,
            &format!("OK {ROTATION} state=waiting {}\n", handoff.receipt),
        );
        self.h.fake.open_gate(ROTATION);
        wait_until("the receipt is bound and asked about", || {
            self.bound_receipt().as_ref() == Some(&handoff.id)
                && self.rotation_requests("status") >= 1
        })
        .await;
        assert_eq!(
            self.rotation_requests("commit"),
            1,
            "the bound receipt was committed: {:?}",
            self.h.fake.requests()
        );
        handoff
    }

    /// Every write-through of the record so far, oldest first.
    fn written(&self) -> Vec<Written> {
        self.written
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Every `harness policy` read the host was sent.
    fn policy_reads(&self) -> usize {
        self.h
            .fake
            .requests()
            .iter()
            .filter(|request| request.ends_with(" harness policy"))
            .count()
    }

    /// The resume's turn ran on the mock and its yield was left (`continue` is off).
    async fn resumed(&self) {
        wait_until("the usage resume's yield is left", || {
            self.resumes() == 1
                && self.model.requests().len() == 2
                && journal_has(&self.h, "nothing (continue is off)")
        })
        .await;
        let requests = self.model.requests();
        assert_eq!(
            requests[1].message_input_texts("user").last().cloned(),
            Some(codex_alab_autopilot::words::LIMIT_CLEARED.to_string()),
            "the wall's own resume, on the account the session is on"
        );
        assert!(
            self.h.fake.attention().is_empty(),
            "never attention: {:?}",
            self.h.fake.attention()
        );
        assert!(!self.intent_stands(), "the handoff intent was released");
    }
}

/// The seconds a usage wall's own decision waits for its reset, as `journal` says it
/// ([`WAITS`]: `<thread>: wait <n>s for usage-resume@v1 (...)`); the first such line.
fn usage_wait_s(journal: &[String]) -> Option<u64> {
    journal.iter().find_map(|line| {
        line.split_once(WAITS)?
            .0
            .rsplit_once(": wait ")?
            .1
            .parse()
            .ok()
    })
}

/// A wait for a reset an hour ahead, less what the test may have taken to reach it.
const NEARLY_AN_HOUR: std::ops::RangeInclusive<u64> = 3300..=3600;

/// A receipt for a walled turn's handoff ([`Walled::handoff`]).
struct Handoff {
    id: String,
    /// The receipt's fields, as every answer about it carries them.
    receipt: String,
    /// The exact `status` request for it.
    status: String,
}

/// A turn of a ChatGPT login ends at a usage wall whose reset is `resets_in_s` seconds after
/// its 429 is answered, and the Autopilot asks the host for a handoff. With `held`, that
/// first request waits at the fake's gate (a test scripts its answer, then opens the gate).
/// `resumes`: the model answers one more request, the resume's.
async fn walled(resets_in_s: u64, held: bool, resumes: bool) -> Walled {
    walled_with(resets_in_s, held, resumes, Duration::ZERO).await
}

/// [`walled`], under a reset grace of `grace` ([`Timing::reset_grace`]).
async fn walled_with(resets_in_s: u64, held: bool, resumes: bool, grace: Duration) -> Walled {
    walled_under(POLICY, resets_in_s, held, resumes, grace).await
}

/// [`walled_with`], under the host policy `policy` in place of [`POLICY`].
async fn walled_under(
    policy: &[(&str, &str)],
    resets_in_s: u64,
    held: bool,
    resumes: bool,
    grace: Duration,
) -> Walled {
    let (login, identity, refresh) = subscription_fixture("rotation").await;
    // The harness's own sequence answers nothing: the handler below names the reset as it
    // answers the walled request, which no fixed reply can.
    let h = harness_login(
        Mode::Current,
        Vec::new(),
        policy,
        &[],
        Some(0),
        Some(login),
        Timing {
            reset_grace: grace,
            reset_jitter: Duration::ZERO,
            ..Timing::default()
        },
        |_| {},
        None,
    )
    .await;
    // The record's writer, installed before anything is written through.
    let written = Arc::new(std::sync::Mutex::new(Vec::new()));
    let writes = Arc::clone(&written);
    let shared = h.shared;
    assert!(shared.set_flush(Box::new(move || {
        writes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((shared.owed_act(), shared.recovery()));
        true
    })));
    let named = Arc::new(AtomicU64::new(0));
    let names = Arc::clone(&named);
    let answered = AtomicU64::new(0);
    let model = responses::mount_response_handler(&h._server, move |_| {
        match answered.fetch_add(1, Ordering::SeqCst) {
            0 => {
                let resets_at = now_ms() / 1000 + resets_in_s;
                names.store(resets_at, Ordering::SeqCst);
                wiremock::ResponseTemplate::new(429).set_body_json(serde_json::json!({
                    "error": {
                        "type": "usage_limit_reached",
                        "message": "limit reached",
                        "resets_at": resets_at,
                        "plan_type": "pro"
                    }
                }))
            }
            1 if resumes => responses::sse_response(reply("resumed", "Resumed after the reset.")),
            // A request no test expects: the turn it belongs to fails loudly.
            _ => wiremock::ResponseTemplate::new(418),
        }
    })
    .await;
    assert!(
        h.fake
            .wait_for(|f| f.claim().is_some_and(|c| c.proven_pid.is_some()))
            .await
    );
    let asleep = Arc::new(std::sync::Mutex::new(Duration::from_secs(5000)));
    let read = Arc::clone(&asleep);
    h.autopilot.read_host_sleep_as(move || {
        *read
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    });
    if held {
        h.fake.gate(ROTATION);
    }
    h.test
        .submit_turn("do the task")
        .await
        .unwrap_or_else(|err| panic!("the walled turn: {err}"));
    let turn = h
        .shared
        .latest_turn(&h.thread)
        .unwrap_or_else(|| panic!("the walled turn has no id"));
    let prepare = format!(
        "@{} {ROTATION} prepare thread={} turn={turn} source_account={} source_user={}",
        h.fake.sid(),
        h.thread,
        identity.account,
        identity.user
    );
    let resets_at = named.load(Ordering::SeqCst);
    assert_ne!(resets_at, 0, "the walled request was never answered");
    let walled = Walled {
        h,
        resets_at,
        model,
        prepare,
        asleep,
        written,
        _refresh: refresh,
    };
    if held {
        wait_until("the handoff is asked for", || {
            !walled.h.fake.gated_requests().is_empty()
        })
        .await;
        assert_eq!(
            walled.h.fake.gated_requests(),
            std::slice::from_ref(&walled.prepare),
            "the ask names the walled turn and the login's hashed identity"
        );
    } else {
        wait_until("the handoff is asked for", || walled.prepares() > 0).await;
        assert_eq!(
            walled
                .h
                .fake
                .requests()
                .iter()
                .find(|request| request.contains(" codex-rotation prepare ")),
            Some(&walled.prepare)
        );
    }
    assert_eq!(
        walled.resumes(),
        0,
        "nothing is typed while the host is asked"
    );
    assert_eq!(walled.model.requests().len(), 1);
    walled
}

/// The host answers `not-needed`: one ask, then the wall's ordinary resume at its reset.
#[test]
fn a_declined_rotation_resumes_at_the_usage_reset() {
    big_stack(|| async {
        let walled = walled(3600, true, true).await;
        walled
            .h
            .fake
            .reply_once(&walled.prepare, "OK codex-rotation state=not-needed\n");
        walled.h.fake.open_gate(ROTATION);
        // The wall's reset is an hour ahead: declined, the session waits for it. The wait is
        // read where the released wall's own decision records it, not at the release: a
        // resume typed a moment after the release would pass a look taken there.
        wait_until("the released wall's wait is decided", || {
            journal_has(&walled.h, WAITS) || walled.resumes() > 0
        })
        .await;
        assert!(!walled.intent_stands(), "the declined intent was released");
        let waits = usage_wait_s(&walled.h.autopilot.supervisor().journal());
        assert!(
            waits.is_some_and(|waits| NEARLY_AN_HOUR.contains(&waits)),
            "the declined wall waits for its own reset, an hour ahead: {waits:?}"
        );
        assert_eq!(walled.resumes(), 0, "{:?}", walled.h.fake.acts());
        assert_eq!(walled.model.requests().len(), 1);
        walled.host_sleeps(Duration::from_secs(2 * 3600));
        walled.resumed().await;
        assert!(
            !walled.reset_passed(),
            "only the counted sleep can have brought this wall's reset"
        );
        assert_eq!(
            walled.prepares(),
            1,
            "a declined handoff is not asked again"
        );
        assert!(
            !journal_has(&walled.h, LAPSED),
            "the host declined; the ask did not lapse"
        );
    });
}

/// The host has no destination and answers `waiting` at every ask. The ask ends at the
/// wall's own reset and the session resumes on the account it is on. The reset comes while
/// the first ask is still unanswered (it waits at the fake's gate, so that pass has judged
/// the bound already), and that ask is the last: the pass that finds the reset passed gives
/// the handoff up before it asks, and nothing asks after it.
#[test]
fn a_handoff_still_waiting_at_the_reset_resumes_on_the_same_account() {
    big_stack(|| async {
        let walled = walled(3600, true, true).await;
        walled
            .h
            .fake
            .reply_always(&walled.prepare, "OK codex-rotation state=waiting\n");
        walled.host_sleeps(Duration::from_secs(2 * 3600));
        walled.h.fake.open_gate(ROTATION);
        walled.resumed().await;
        assert!(journal_has(&walled.h, LAPSED));
        assert!(
            !walled.reset_passed(),
            "only the counted sleep can have brought this wall's reset"
        );
        assert_eq!(
            walled.prepares(),
            1,
            "the host was asked once the reset had passed: {:?}",
            walled.h.fake.requests()
        );
    });
}

/// NEGATIVE CONTROL for the lapse: the same standing `waiting` at a wall whose reset is an
/// hour ahead is asked again and again, and nothing is typed.
#[test]
fn a_handoff_waiting_before_the_reset_keeps_asking() {
    big_stack(|| async {
        let walled = walled(3600, true, false).await;
        walled
            .h
            .fake
            .reply_always(&walled.prepare, "OK codex-rotation state=waiting\n");
        walled.h.fake.open_gate(ROTATION);
        wait_until("the host is asked a third time", || walled.prepares() >= 3).await;
        assert_eq!(walled.resumes(), 0, "{:?}", walled.h.fake.acts());
        assert_eq!(walled.model.requests().len(), 1);
        assert!(!journal_has(&walled.h, LAPSED));
        assert!(
            walled.intent_stands(),
            "the handoff intent stands while the wall does"
        );
    });
}

/// The host does not know the verb (`ERR unknown verb`, the fake's answer to an unscripted
/// `codex-rotation`): the same bound ends the asking. THE AWAKE CASE: no sleep is scripted,
/// and the reset is the wall clock's own, eight seconds after the 429 was answered.
#[test]
fn a_handoff_the_host_cannot_answer_resumes_at_the_reset() {
    big_stack(|| async {
        let walled = walled(8, false, true).await;
        walled.resumed().await;
        assert!(walled.reset_passed(), "resumed before the wall's reset");
        assert!(journal_has(&walled.h, LAPSED));
        assert!(walled.prepares() >= 1);
    });
}

/// THE HOST SLEPT THROUGH THE RESET WHILE THE HOST WAS BEING ASKED. The Autopilot's clock
/// stops in sleep, and the decision that counts sleep at the point is reached only once the
/// ask lets the point go, so the ask counts it itself: at the wake the reset has passed, the
/// ask ends and the resume is typed, not as many waking hours later as the host slept.
#[test]
fn a_reset_slept_through_while_the_host_is_asked_resumes_at_the_wake() {
    big_stack(|| async {
        let walled = walled(3600, true, true).await;
        walled
            .h
            .fake
            .reply_always(&walled.prepare, "OK codex-rotation state=waiting\n");
        walled.h.fake.open_gate(ROTATION);
        // Awake, an hour from the reset: asked again, nothing typed.
        wait_until("the host is asked again", || walled.prepares() >= 2).await;
        assert_eq!(walled.resumes(), 0, "{:?}", walled.h.fake.acts());
        assert!(!journal_has(&walled.h, LAPSED));
        walled.host_sleeps(Duration::from_secs(2 * 3600));
        walled.resumed().await;
        assert!(
            journal_has(&walled.h, "the host slept 7200 s"),
            "the sleep was counted at the point"
        );
        assert!(journal_has(&walled.h, LAPSED));
        assert!(
            !walled.reset_passed(),
            "only the counted sleep can have ended this wall: its reset is an hour ahead"
        );
    });
}

/// A RECEIPT THE HOST HAS BOUND IS NEVER GIVEN UP, WHERE `follow_renewal` JUDGES THE BOUND
/// (review, 2026-11-12: the guard was pinned only in the rule's own unit test, and passing
/// `false` for it at the call site failed nothing). The host answers `ready`; codex-alab
/// binds the receipt and sends `commit`, after which the host may be restarting the session
/// on the other account and has charged its pool. The wall's reset then passes with the
/// host still answering `waiting` for that receipt: the intent keeps its receipt, the host
/// is still asked, and no usage resume is typed on the account the session is leaving.
#[test]
fn a_bound_handoff_is_never_given_up_at_the_reset() {
    big_stack(|| async {
        let walled = walled(3600, true, false).await;
        // While no receipt binds the intent, the wall is on record beside it.
        let carried = walled
            .wall_on_record()
            .unwrap_or_else(|| panic!("an unbound intent carries its wall's reset"));
        assert!(
            carried.abs_diff(walled.resets_at * 1000) <= 2000,
            "{carried} for a reset at {} s",
            walled.resets_at
        );
        let Handoff { id, .. } = walled.bound().await;
        // The host may now restart the session on the other account: that process must not
        // stand at the wall of the account it left.
        assert_eq!(
            walled.h.shared.recovery(),
            None,
            "a bound intent carries no wall"
        );
        // Only that wall is taken off the record: another turn's recovery is left alone.
        let another = Recovery {
            thread: walled.h.thread.clone(),
            turn_id: "a-later-turn".into(),
            point_at_ms: now_ms(),
            not_before_ms: now_ms() + 3_600_000,
            by_autopilot: true,
            by_person: false,
            wall: RecoveryWall::Server {
                what: "model at capacity".into(),
                cut_off: false,
                retry_at_ms: None,
            },
        };
        walled.h.shared.set_recovery(Some(another.clone()));
        // The reset an hour ahead passes: the host sleeps two.
        walled.host_sleeps(Duration::from_secs(2 * 3600));
        wait_until("the sleep is counted at the point", || {
            journal_has(&walled.h, "the host slept 7200 s")
        })
        .await;
        // The pass that counted the sleep is the one that judges the bound; two more
        // follow it, five seconds apart.
        let counted = walled.rotation_requests("status");
        wait_until("the host is still asked after the reset", || {
            walled.rotation_requests("status") >= counted + 2
                || walled.resumes() > 0
                || journal_has(&walled.h, LAPSED)
        })
        .await;
        assert!(
            !journal_has(&walled.h, LAPSED),
            "a bound receipt was given up at the wall's reset"
        );
        assert_eq!(walled.resumes(), 0, "{:?}", walled.h.fake.acts());
        assert_eq!(
            walled.model.requests().len(),
            1,
            "a resume was typed on the account the session is leaving"
        );
        assert_eq!(
            walled.bound_receipt(),
            Some(id),
            "the intent keeps the receipt the host bound"
        );
        assert_eq!(
            walled.prepares(),
            1,
            "a bound handoff is not prepared again"
        );
        assert_eq!(
            walled.h.shared.recovery(),
            Some(another),
            "another turn's recovery is not the bound intent's to remove"
        );
        assert!(
            !walled.reset_passed(),
            "only the counted sleep can have brought this wall's reset"
        );
    });
}

/// THE ASKED WALL LEAVES THE RECORD BEFORE `commit` IS SENT (second review, 2026-11-12). Once
/// `commit` is sent the host may restart the session on the other account, and that process
/// reads the record as it stood then. Nothing pinned the order: the wall's removal at the
/// binding site could be deleted, or moved after `commit`, with every test passing, because
/// the bound test looked only after the next pass, whose own durable step removes the wall,
/// and no record writer was installed, so no write was observed. `commit` is held at the
/// host here: while it is held the record holds the bound intent and no wall, and the last
/// write-through before it carried exactly that.
#[test]
fn the_asked_wall_leaves_the_record_before_commit_is_sent() {
    big_stack(|| async {
        let walled = walled(3600, true, false).await;
        let handoff = walled.handoff();
        walled.h.fake.reply_once(
            &walled.prepare,
            &format!("OK {ROTATION} state=ready {}\n", handoff.receipt),
        );
        walled.h.fake.reply_always(
            &handoff.status,
            &format!("OK {ROTATION} state=waiting {}\n", handoff.receipt),
        );
        // POSITIVE CONTROL: the unbound intent was written through with its wall.
        assert!(
            walled
                .written()
                .iter()
                .any(|written| unbound_written(written) && wall_written(written)),
            "the unbound intent reached the record with its wall: {:#?}",
            walled.written()
        );
        let commit = format!(
            "@{} {ROTATION} commit id={}",
            walled.h.fake.sid(),
            handoff.id
        );
        walled.h.fake.gate("codex-rotation commit");
        walled.h.fake.open_gate(ROTATION);
        wait_until("commit is held at the host", || {
            walled.h.fake.gated_requests().contains(&commit)
        })
        .await;
        // The pass is waiting on the host: nothing of its own is written meanwhile.
        let written = walled.written();
        assert_eq!(
            walled.h.shared.recovery(),
            None,
            "commit was sent while the record held the asked wall"
        );
        assert_eq!(walled.bound_receipt(), Some(handoff.id.clone()));
        let last = written
            .last()
            .unwrap_or_else(|| panic!("nothing was written through before commit"));
        assert_eq!(
            (receipt_written(last), last.1.as_ref()),
            (Some(handoff.id.as_str()), None),
            "the last write-through before commit: the bound intent, and no wall"
        );
        walled.h.fake.open_gate("codex-rotation commit");
        wait_until("the bound receipt is asked about", || {
            walled.rotation_requests("status") >= 1
        })
        .await;
        assert_eq!(walled.rotation_requests("commit"), 1);
        assert_eq!(walled.resumes(), 0, "{:?}", walled.h.fake.acts());
    });
}

/// A RECEIPT THE HOST RETIRES PUTS THE WALL BACK ON RECORD WITH THE UNBOUND INTENT (second
/// review, 2026-11-12: the third place the wall is carried had no test). The host binds a
/// receipt, then answers `retired` for it: the intent is unbound again, and the write that
/// records it carries the wall, so no record ever holds an unbound intent without the wall
/// that bounds the asking. The host then knows no destination (`ERR` to the next `prepare`),
/// sleeps through the reset, and the ask lapses there.
#[test]
fn a_retired_receipt_puts_the_wall_back_on_record() {
    big_stack(|| async {
        let walled = walled(3600, true, true).await;
        let handoff = walled.bound().await;
        assert_eq!(walled.h.shared.recovery(), None);
        let bound = walled.written().len();
        walled.h.fake.reply_always(
            &handoff.status,
            &format!("OK {ROTATION} state=retired {}\n", handoff.receipt),
        );
        wait_until("the retired receipt leaves the intent", || {
            walled.intent_stands() && walled.bound_receipt().is_none()
        })
        .await;
        wait_until("the unbound intent is written through", || {
            walled.written()[bound..].iter().any(unbound_written)
        })
        .await;
        let written = walled.written();
        let first = written[bound..]
            .iter()
            .find(|written| unbound_written(written))
            .unwrap_or_else(|| panic!("the unbound intent was written"));
        assert!(
            wall_written(first),
            "the write that unbound the intent carried no wall: {first:#?}"
        );
        let carried = walled
            .wall_on_record()
            .unwrap_or_else(|| panic!("the wall is on record again"));
        assert!(
            carried.abs_diff(walled.resets_at * 1000) <= 2000,
            "{carried} for a reset at {} s",
            walled.resets_at
        );
        // Unbound again, the host is asked afresh, and only until the wall's reset.
        wait_until("the host is asked to prepare again", || {
            walled.prepares() >= 2
        })
        .await;
        assert_eq!(walled.resumes(), 0, "{:?}", walled.h.fake.acts());
        assert!(!journal_has(&walled.h, LAPSED));
        walled.host_sleeps(Duration::from_secs(2 * 3600));
        walled.resumed().await;
        assert!(journal_has(&walled.h, LAPSED));
        assert!(
            !walled.reset_passed(),
            "only the counted sleep can have brought this wall's reset"
        );
    });
}

/// A WALL THE RECOVERED TURN MEETS IS NOT THE BOUND INTENT'S TO REMOVE (second review,
/// 2026-11-12). The turn recovered on the destination account reuses the asked turn's id, and
/// a bound intent stands until that turn is acknowledged, so "a usage wall on record under the
/// asked turn's id" also names a wall the recovered turn met on the account it moved to. The
/// bound branch removed it on every pass. It now removes the asked wall only until the
/// recovered turn is reserved (`AccountHandoff::turn`, which the admission writes to the
/// record before the turn is admitted).
///
/// The reservation is written here as that admission writes it, straight to the record; the
/// admission itself (a committed receipt read on the destination login and a native turn
/// recovered under it) is `renewal_consume.rs`'s ground and is not run here.
#[test]
fn a_wall_the_recovered_turn_meets_is_not_the_bound_intents_to_remove() {
    big_stack(|| async {
        let walled = walled(3600, true, false).await;
        let Handoff { id, .. } = walled.bound().await;
        // NEGATIVE CONTROL: before the reservation, a usage wall under the asked turn's id
        // is the asked wall, and binding took it off the record.
        assert_eq!(
            walled.h.shared.recovery(),
            None,
            "a bound intent carries no wall"
        );
        let shared = walled.h.shared;
        let turn = shared
            .owed_act()
            .and_then(|owed| owed.native_origin)
            .map(|origin| origin.turn_id)
            .unwrap_or_else(|| panic!("the intent names its turn"));
        // A pass that read the intent before this write publishes its own copy once more;
        // every later pass reads the reservation, so the second write stands.
        let reserved = || {
            shared
                .owed_act()
                .and_then(|owed| owed.renewal)
                .and_then(|renewal| renewal.turn)
                .as_ref()
                == Some(&turn)
        };
        for _ in 0..2 {
            if !reserved() {
                let mut owed = shared
                    .owed_act()
                    .unwrap_or_else(|| panic!("the bound intent is on record"));
                if let Some(renewal) = owed.renewal.as_mut() {
                    renewal.turn = Some(turn.clone());
                }
                assert!(shared.publish_renewal(shared.upgrade_generation(), owed));
            }
            let asked = walled.rotation_requests("status");
            wait_until("a pass asks about the receipt", || {
                walled.rotation_requests("status") > asked
            })
            .await;
        }
        assert!(reserved(), "the reservation stands on record");
        // The wall the recovered turn met on the account it moved to.
        let met = Recovery {
            thread: walled.h.thread.clone(),
            turn_id: turn,
            point_at_ms: now_ms(),
            not_before_ms: now_ms() + 3_600_000,
            by_autopilot: true,
            by_person: false,
            wall: RecoveryWall::Usage {
                reset_at_ms: Some(now_ms() + 3_600_000),
                reset_label: None,
                quota: None,
            },
        };
        shared.set_recovery(Some(met.clone()));
        let asked = walled.rotation_requests("status");
        wait_until("two more passes ask about the receipt", || {
            walled.rotation_requests("status") >= asked + 2
        })
        .await;
        assert_eq!(
            shared.recovery(),
            Some(met),
            "the recovered turn's own wall was taken off the record"
        );
        assert_eq!(walled.bound_receipt(), Some(id));
        assert_eq!(walled.resumes(), 0, "{:?}", walled.h.fake.acts());
    });
}

/// A BOUND RECEIPT KEEPS THE POINT WHILE THE HOST CANNOT BE READ (second review, 2026-11-12;
/// the defect is older than the lapse and stands on main for weekly walls). `follow_renewal`
/// reads the host's policy before it asks about the receipt, and let the point go whenever
/// that read gave no permission, a failed read included. `evaluate` then decided the usage
/// wall as this account's own: with the reset passed it typed the usage resume on the account
/// the session was leaving, after `commit` was sent. The host here answers one policy read
/// with an `ERR` (the claim's connection stays up, so codex-alab still acts) once the wall's
/// reset has passed in the host's sleep: nothing is typed, the old account's wall does not go
/// back on record, the receipt stays bound, and the next pass that reads the host asks about
/// it again.
#[test]
fn a_bound_handoff_keeps_its_point_while_the_host_cannot_be_read() {
    big_stack(|| async {
        let walled = walled(3600, true, false).await;
        let Handoff { id, .. } = walled.bound().await;
        assert_eq!(walled.h.shared.recovery(), None);
        // The reset an hour ahead passes: from here a point let go resumes at once.
        walled.host_sleeps(Duration::from_secs(2 * 3600));
        wait_until("the sleep is counted at the point", || {
            journal_has(&walled.h, "the host slept 7200 s")
        })
        .await;
        let read = walled.policy_reads();
        walled
            .h
            .fake
            .answer_policy_with(Some("ERR harness policy unavailable"));
        wait_until("a policy read fails", || walled.policy_reads() > read).await;
        walled.h.fake.answer_policy_with(None);
        let asked = walled.rotation_requests("status");
        wait_until("the host is asked about the receipt again", || {
            walled.rotation_requests("status") > asked
                || walled.resumes() > 0
                || walled.h.shared.recovery().is_some()
        })
        .await;
        assert_eq!(
            walled.resumes(),
            0,
            "a usage resume was typed on the account the session is leaving: {:?}",
            walled.h.fake.acts()
        );
        assert_eq!(walled.model.requests().len(), 1);
        assert_eq!(
            walled.h.shared.recovery(),
            None,
            "the wall of the account the session is leaving went back on record"
        );
        assert_eq!(
            walled.bound_receipt(),
            Some(id),
            "the intent keeps the receipt the host bound"
        );
        assert!(
            walled.rotation_requests("status") > asked,
            "the next readable pass asks about the receipt"
        );
        assert!(!journal_has(&walled.h, LAPSED));
        assert_eq!(walled.prepares(), 1);
        assert!(
            !walled.reset_passed(),
            "only the counted sleep can have brought this wall's reset"
        );
    });
}

/// THE RESET GRACE IS THE AUTOPILOT'S OWN, WHERE `follow_renewal` JUDGES THE BOUND (review,
/// 2026-11-12: every test here ran with no grace, and a zero passed at the call site failed
/// nothing). Under ten minutes of grace, a reset five minutes past is still asked about and
/// nothing is typed; ten minutes more and the ask is given up.
#[test]
fn a_handoff_is_asked_for_through_the_reset_grace() {
    big_stack(|| async {
        let walled = walled_with(3600, true, true, Duration::from_secs(600)).await;
        walled
            .h
            .fake
            .reply_always(&walled.prepare, "OK codex-rotation state=waiting\n");
        // The reset comes and five minutes more pass while the first ask is unanswered.
        walled.host_sleeps(Duration::from_secs(3600 + 300));
        walled.h.fake.open_gate(ROTATION);
        wait_until("the sleep is counted at the point", || {
            journal_has(&walled.h, "the host slept 3900 s")
        })
        .await;
        let counted = walled.prepares();
        wait_until("the host is still asked inside the grace", || {
            walled.prepares() >= counted + 2
                || walled.resumes() > 0
                || journal_has(&walled.h, LAPSED)
        })
        .await;
        assert!(
            !journal_has(&walled.h, LAPSED),
            "the ask was given up inside the reset's grace"
        );
        assert_eq!(walled.resumes(), 0, "{:?}", walled.h.fake.acts());
        assert_eq!(walled.model.requests().len(), 1);
        assert!(walled.intent_stands());
        // The rest of the grace passes.
        walled.host_sleeps(Duration::from_secs(600));
        walled.resumed().await;
        assert!(journal_has(&walled.h, LAPSED));
        assert!(
            !walled.reset_passed(),
            "only the counted sleep can have brought this wall's reset"
        );
    });
}

/// A DECLINED HANDOFF WHOSE WALL IS LEFT TO A PERSON LEAVES NO WALL ON RECORD (second review,
/// 2026-11-12). With the wall's wait switched off the released point is decided as attention,
/// which leaves no recovery, so the wall the ask carried stayed on record: nothing waited
/// for its reset, yet the TUI polled usage at its recovery pace for as long as the record
/// held it, where before the ask carried a wall no recovery was on record at all.
#[test]
fn a_declined_handoff_left_to_a_person_leaves_no_wall_on_record() {
    big_stack(|| async {
        let walled = walled_under(POLICY_NO_WAIT, 3600, true, false, Duration::ZERO).await;
        // Asked for, the wall is carried whatever its own decision will be.
        assert!(
            walled.wall_on_record().is_some(),
            "an unbound intent carries its wall's reset"
        );
        walled
            .h
            .fake
            .reply_once(&walled.prepare, "OK codex-rotation state=not-needed\n");
        walled.h.fake.open_gate(ROTATION);
        wait_until("the released wall is left to a person", || {
            walled
                .h
                .fake
                .attention()
                .values()
                .any(|text| text.contains(LIMIT_OFF))
        })
        .await;
        assert!(!walled.intent_stands(), "the handoff intent was released");
        assert_eq!(
            walled.h.shared.recovery(),
            None,
            "the asked wall stayed on record after its point was left to a person"
        );
        assert_eq!(walled.resumes(), 0, "{:?}", walled.h.fake.acts());
        assert_eq!(walled.model.requests().len(), 1);
        assert_eq!(walled.prepares(), 1);
    });
}

/// AN ASKED WALL STAYS ON RECORD WHILE ITS INTENT STANDS (verification of the second review,
/// 2026-11-12: with the guard in `drop_asked_wall_left_to_a_person` disabled every test here
/// passed). One pass that cannot read the host lets an unbound point go without releasing
/// its intent; with the wall's wait switched off that pass decides attention. The wall is
/// still the ask's to carry: taken off, the record would hold an unbound intent and no
/// wall until a later pass put it back, and a crash in between is the unbounded ask the
/// carry exists to prevent.
#[test]
fn an_asked_wall_stays_on_record_while_its_intent_stands_and_the_host_is_unread() {
    big_stack(|| async {
        let walled = walled_under(POLICY_NO_WAIT, 3600, true, false, Duration::ZERO).await;
        walled
            .h
            .fake
            .reply_always(&walled.prepare, "OK codex-rotation state=waiting\n");
        walled.h.fake.open_gate(ROTATION);
        let carried = walled.wall_on_record();
        assert!(carried.is_some(), "an unbound intent carries its wall");
        let read = walled.policy_reads();
        walled
            .h
            .fake
            .answer_policy_with(Some("ERR harness policy unavailable"));
        wait_until("a policy read fails", || walled.policy_reads() > read).await;
        wait_until(
            "the pass that let the point go leaves it to a person",
            || {
                walled
                    .h
                    .fake
                    .attention()
                    .values()
                    .any(|text| text.contains(LIMIT_OFF))
            },
        )
        .await;
        assert!(walled.intent_stands(), "the intent was not released");
        assert_eq!(
            walled.wall_on_record(),
            carried,
            "the wall of an intent that still stands was taken off the record"
        );
        assert_eq!(walled.resumes(), 0, "{:?}", walled.h.fake.acts());
        assert_eq!(walled.model.requests().len(), 1);
    });
}

/// NEGATIVE CONTROL for the two tests around this one: only a wall the ask carried is taken
/// off. A wall the released point's own decision left (its wait, with the reset an hour
/// ahead) stays on record when that wait is then switched off and the point goes to a
/// person, as it did before the ask carried any wall: a restart still stands at it and
/// decides it under the policy of that moment.
#[test]
fn a_wall_its_own_decision_left_stays_on_record_when_the_wait_is_switched_off() {
    big_stack(|| async {
        let walled = walled(3600, true, false).await;
        walled
            .h
            .fake
            .reply_once(&walled.prepare, "OK codex-rotation state=not-needed\n");
        walled.h.fake.open_gate(ROTATION);
        wait_until("the released wall's wait is decided", || {
            journal_has(&walled.h, WAITS)
        })
        .await;
        let decided = walled.h.shared.recovery();
        assert!(
            matches!(
                decided,
                Some(Recovery {
                    wall: RecoveryWall::Usage { .. },
                    ..
                })
            ),
            "the wait left the wall's recovery: {decided:?}"
        );
        walled.h.fake.set_policy_key("limit_wait", "false");
        wait_until("the wall is left to a person", || {
            walled
                .h
                .fake
                .attention()
                .values()
                .any(|text| text.contains(LIMIT_OFF))
        })
        .await;
        assert_eq!(
            walled.h.shared.recovery(),
            decided,
            "a decision's own recovery is not the ask's to remove"
        );
        assert_eq!(walled.resumes(), 0, "{:?}", walled.h.fake.acts());
    });
}

/// The same where the `not-needed` comes after a restart. The process that restored the wall
/// from its record never published it, so nothing in its memory says the wall is an asked
/// one until its first pass finds it beside the unbound intent; that pass comes before any
/// answer can release the intent.
#[test]
fn a_handoff_declined_after_a_restart_and_left_to_a_person_leaves_no_wall_on_record() {
    big_stack(|| async {
        let walled = walled(3600, true, false).await;
        let restarted = restarted_under(&walled, POLICY_NO_WAIT).await;
        assert!(
            restarted.shared.recovery().is_some(),
            "the restarted process holds the wall its record carried"
        );
        restarted
            .fake
            .reply_once(&restarted.prepare, "OK codex-rotation state=not-needed\n");
        restarted.fake.open_gate(ROTATION);
        wait_until("the released wall is left to a person", || {
            restarted
                .fake
                .attention()
                .values()
                .any(|text| text.contains(LIMIT_OFF))
        })
        .await;
        assert!(
            !restarted.intent_stands(),
            "the handoff intent was released"
        );
        assert_eq!(
            restarted.shared.recovery(),
            None,
            "the asked wall stayed on record after its point was left to a person"
        );
        assert_eq!(restarted.resumes(), 0, "{:?}", restarted.fake.acts());
        assert_eq!(walled.model.requests().len(), 1);
        assert_eq!(restarted.prepares(), 1);
    });
}

/// The process that takes the session over after a restart: a new Autopilot, supervisor and
/// host connection over a resume of the same rollout, started with what the process before
/// left in its record.
///
/// HOW THE RECORD REACHES IT HERE IS NOT HOW IT DOES IN USE (second review, 2026-11-12).
/// [`restarted`] seeds the new process's `Shared` directly and names the record as inherited,
/// which is what the record's writer does for a same-pid image, an adopted update's re-exec.
/// A session does not re-exec while it asks for a handoff: the ask's recheck is a timer that
/// owes no act, and such a wait holds the live upgrade. The restart it can take is a crash
/// or a kill and aterm's relaunch, where the dead record is swept as a predecessor and its
/// recovery restored only under aterm's lease; that path, through the real writer and the
/// real scan, is `crash_wait_tests.rs`
/// (`a_relaunch_of_a_session_killed_while_asking_for_a_handoff_stands_at_its_wall`). What
/// these tests add is the restored wall acted on by a running process: asked about, lapsed,
/// declined, resumed.
struct Restarted {
    fake: FakeAterm,
    shared: &'static Shared,
    autopilot: Arc<Autopilot>,
    /// The handoff request for the walled turn, as this process's host connection names it.
    prepare: String,
    asleep: Arc<std::sync::Mutex<Duration>>,
    _test: TestCodex,
    _dir: tempfile::TempDir,
}

impl Drop for Restarted {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!(
                "restarted supervisor journal: {:#?}",
                self.autopilot.supervisor().journal()
            );
            eprintln!("restarted fake requests: {:#?}", self.fake.requests());
        }
    }
}

impl Restarted {
    fn prepares(&self) -> usize {
        self.fake
            .requests()
            .iter()
            .filter(|request| request.contains(" codex-rotation prepare "))
            .count()
    }

    fn resumes(&self) -> usize {
        self.fake.acts().iter().filter(|act| *act == RESUME).count()
    }

    fn journal_has(&self, needle: &str) -> bool {
        self.autopilot
            .supervisor()
            .journal()
            .iter()
            .any(|line| line.contains(needle))
    }

    fn intent_stands(&self) -> bool {
        self.shared
            .owed_act()
            .is_some_and(|owed| owed.renewal.is_some())
    }

    fn host_sleeps(&self, slept: Duration) {
        *self
            .asleep
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) += slept;
    }
}

/// The deadline of a restarted supervisor's calls to its host: longer than the twelve
/// seconds a resume's scan waits for the launch probe, whose `status` read [`restarted`]
/// holds at the host until the thread's manager is bound.
const PROBE_HELD: Duration = Duration::from_secs(30);

/// End `walled`'s process and start the next one on its session, seeded as a same-pid image
/// is ([`Restarted`] says why that is a stand-in): the new process is given the record the
/// old one held (its handoff intent and its recovery), and
/// resumes the rollout on the same scratch login. Its first handoff request waits at its
/// host's gate. The model is the first process's loopback mock.
async fn restarted(walled: &Walled) -> Restarted {
    restarted_under(walled, POLICY).await
}

/// [`restarted`], with the new process's host under `policy` in place of [`POLICY`].
async fn restarted_under(walled: &Walled, policy: &[(&str, &str)]) -> Restarted {
    let owed = walled
        .h
        .shared
        .owed_act()
        .unwrap_or_else(|| panic!("the handoff intent is on record"));
    let recovery = walled.h.shared.recovery();
    let rollout = walled
        .h
        .test
        .codex
        .rollout_path()
        .unwrap_or_else(|| panic!("the walled turn's rollout"));
    walled
        .h
        .test
        .codex
        .shutdown_and_wait()
        .await
        .unwrap_or_else(|err| panic!("the first process's thread: {err}"));

    let dir = tempfile::tempdir().unwrap_or_else(|err| panic!("scratch dir: {err}"));
    let fake = FakeAterm::start(dir.path(), Mode::Current)
        .await
        .unwrap_or_else(|err| panic!("fake aterm: {err}"));
    for (key, value) in policy {
        fake.set_policy_key(key, value);
    }
    fake.gate(ROTATION);
    // The resume's scan of the history waits for the launch probe (aterm's first `status`
    // read), which is held here until the thread's manager is bound: a real process binds it
    // before any thread is resumed, and this harness gets the manager only from the resume.
    // The order is bounded, not strict: the scan waits for the probe at most twelve seconds
    // (`LAUNCH_PROBE_WAIT`) and then reads the history through whatever manager is bound. A
    // resume and binding slower than that fail below ("the resume read the rollout"); they
    // never pass falsely. The held read itself must outlast that wait, so this supervisor's
    // calls get a longer deadline than the client's five seconds ([`PROBE_HELD`]): under the
    // default the read timed out first and the scan went on after five seconds, not twelve.
    fake.gate("status");
    let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
    // What the record's writer seeds a same-pid image with, before any native work.
    shared.set_owed_act(Some(owed.clone()));
    shared.set_recovery(recovery.clone());
    assert!(shared.set_inherited(Inherited {
        session_id: Some(walled.h.thread.clone()),
        owed_act: Some(owed),
        recovery,
        ..Default::default()
    }));
    let endpoint = fake.endpoint();
    let sup = Supervisor::new(SupervisorOptions {
        shared,
        endpoint: Arc::new(move || Some(endpoint.clone())),
        client: ClientOptions {
            timeout: PROBE_HELD,
            ..ClientOptions::default()
        },
    });
    let mut builder = ExtensionRegistryBuilder::<Config>::new();
    let autopilot = Autopilot::new(
        sup,
        None,
        None,
        Timing {
            reset_grace: Duration::ZERO,
            reset_jitter: Duration::ZERO,
            ..Timing::default()
        },
        |_| true,
    );
    autopilot.register(&mut builder);
    let asleep = Arc::new(std::sync::Mutex::new(Duration::from_secs(9000)));
    let read = Arc::clone(&asleep);
    autopilot.read_host_sleep_as(move || {
        *read
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    });
    let home = Arc::clone(&walled.h.test.home);
    let manager = fixtures::manager(home.path(), codex_login::AuthCredentialsStoreMode::File).await;
    let test = test_codex()
        .with_extensions(Arc::new(builder.build()))
        .with_config(|config| {
            config.model_provider.request_max_retries = Some(0);
            config.model_provider.stream_max_retries = Some(0);
        })
        .with_auth_manager(manager)
        .resume(&walled.h._server, home, rollout)
        .await
        .unwrap_or_else(|err| panic!("the resumed thread: {err}"));
    autopilot.bind_thread_manager(Arc::downgrade(&test.thread_manager));
    let thread = test.session_configured.thread_id.to_string();
    assert_eq!(thread, walled.h.thread, "the same session");
    shared.observe(
        &Frame {
            primary_thread: Some(&thread),
            displayed_thread: Some(&thread),
            rollout_path: None,
            cwd: test.cwd_path(),
            model: "gpt-5",
            codex_home: test.codex_home_path(),
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
    fake.open_gate("status");
    assert!(
        fake.wait_for(|f| f.claim().is_some_and(|c| c.proven_pid.is_some()))
            .await
    );
    let prepare = walled
        .prepare
        .split_once(' ')
        .map(|(_, request)| format!("@{} {request}", fake.sid()))
        .unwrap_or_else(|| panic!("the ask: {}", walled.prepare));
    let restarted = Restarted {
        fake,
        shared,
        autopilot,
        prepare,
        asleep,
        _test: test,
        _dir: dir,
    };
    let asked = || {
        restarted
            .fake
            .gated_requests()
            .into_iter()
            .filter(|request| request.contains(" codex-rotation "))
            .collect::<Vec<_>>()
    };
    wait_until("the carried handoff is asked for again", || {
        !asked().is_empty()
    })
    .await;
    assert_eq!(
        asked(),
        std::slice::from_ref(&restarted.prepare),
        "the carried intent is asked about, for the same turn and login"
    );
    assert!(
        !restarted.journal_has("its history unread"),
        "the resume read the rollout"
    );
    restarted
}

/// THE BOUND ON ASKING SURVIVES A PROCESS RESTART (review, 2026-11-12). The handoff intent is
/// durable and a usage wall is not rebuilt from the rollout (its recorded error names no
/// reset), so the process after a restart held the intent with no wall: a host whose
/// `waiting` stands was asked every five seconds without end, on an account that could long
/// run again. An unbound intent now carries its wall's recovery in the same record. The
/// first process ends while its ask is unanswered; the second, given that record, stands at
/// the wall again, asks the host, and gives the ask up when the reset an hour ahead passes
/// in its own host's sleep.
#[test]
fn a_handoff_carried_across_a_restart_is_given_up_at_the_walls_reset() {
    big_stack(|| async {
        let walled = walled(3600, true, true).await;
        let carried = walled
            .wall_on_record()
            .unwrap_or_else(|| panic!("an unbound intent carries its wall's reset"));
        assert!(
            carried.abs_diff(walled.resets_at * 1000) <= 2000,
            "{carried} for a reset at {} s",
            walled.resets_at
        );
        let restarted = restarted(&walled).await;
        restarted
            .fake
            .reply_always(&restarted.prepare, "OK codex-rotation state=waiting\n");
        restarted.fake.open_gate(ROTATION);
        // NEGATIVE CONTROL: with the reset an hour ahead the host is asked again and again
        // and nothing is typed.
        wait_until("the restarted process keeps asking", || {
            restarted.prepares() >= 3
        })
        .await;
        assert_eq!(restarted.resumes(), 0, "{:?}", restarted.fake.acts());
        assert!(!restarted.journal_has(LAPSED));
        assert_eq!(walled.model.requests().len(), 1);
        assert!(restarted.intent_stands());
        restarted.host_sleeps(Duration::from_secs(2 * 3600));
        wait_until("the wall's own resume is typed", || {
            restarted.resumes() == 1
        })
        .await;
        assert!(restarted.journal_has(LAPSED));
        let asked = restarted.prepares();
        resumed_after_restart(&walled, &restarted, asked).await;
    });
}

/// The resume's turn ran in the restarted process: the wall's own resume, typed once, the
/// host asked `asked` times in all.
async fn resumed_after_restart(walled: &Walled, restarted: &Restarted, asked: usize) {
    wait_until("the usage resume's yield is left", || {
        walled.model.requests().len() == 2 && restarted.journal_has("nothing (continue is off)")
    })
    .await;
    assert_eq!(restarted.resumes(), 1, "{:?}", restarted.fake.acts());
    assert_eq!(
        walled.model.requests()[1]
            .message_input_texts("user")
            .last()
            .cloned(),
        Some(codex_alab_autopilot::words::LIMIT_CLEARED.to_string()),
        "the wall's own resume, on the account the session is on"
    );
    assert!(
        restarted.fake.attention().is_empty(),
        "never attention: {:?}",
        restarted.fake.attention()
    );
    assert!(
        !restarted.intent_stands(),
        "the handoff intent was released"
    );
    assert_eq!(
        restarted.prepares(),
        asked,
        "the host is not asked once the intent is released: {:?}",
        restarted.fake.requests()
    );
    assert!(
        !walled.reset_passed(),
        "only the counted sleep can have brought this wall's reset"
    );
}

/// A `not-needed` THAT COMES AFTER A RESTART LEAVES THE WALL TO RESUME FROM (review,
/// 2026-11-12). The intent was released and the process, which never saw the wall, had no
/// point: nothing was ever typed. Given the wall's recovery, the released point is the usage
/// wall again: nothing is typed while its reset is an hour ahead, and the resume is typed
/// when it comes, with no lapse (the host answered).
#[test]
fn a_handoff_declined_after_a_restart_resumes_at_the_walls_reset() {
    big_stack(|| async {
        let walled = walled(3600, true, true).await;
        let restarted = restarted(&walled).await;
        restarted
            .fake
            .reply_once(&restarted.prepare, "OK codex-rotation state=not-needed\n");
        restarted.fake.open_gate(ROTATION);
        wait_until("the released wall's wait is decided", || {
            restarted.journal_has(WAITS) || restarted.resumes() > 0
        })
        .await;
        assert!(
            !restarted.intent_stands(),
            "the declined intent was released"
        );
        let waits = usage_wait_s(&restarted.autopilot.supervisor().journal());
        assert!(
            waits.is_some_and(|waits| NEARLY_AN_HOUR.contains(&waits)),
            "the restored wall waits for its own reset, an hour ahead: {waits:?}"
        );
        assert_eq!(
            restarted.resumes(),
            0,
            "typed with the reset an hour ahead: {:?}",
            restarted.fake.acts()
        );
        assert_eq!(walled.model.requests().len(), 1);
        restarted.host_sleeps(Duration::from_secs(2 * 3600));
        resumed_after_restart(&walled, &restarted, 1).await;
        assert!(
            !restarted.journal_has(LAPSED),
            "the host answered: nothing lapsed"
        );
    });
}
