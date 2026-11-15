// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A USAGE WALL'S MOVE REPORTS ITS RESUMED TURN SERVED (`codex-rotation complete`;
//! `alab/PARALLEL-LOGINS.md` gap 5, step C2). Once the receipt of a move to another account is
//! confirmed consumed, the process on the destination tells the host the resumed turn served,
//! so the host can prove the destination for every session walled behind it rather than hold
//! them for its ten-minute canary hold. Once per receipt, best effort, only for a turn that
//! ended with no error, and only while that end is no older than the hold; nothing of the
//! intent, its release or the schedule depends on the answer.
//!
//! Against alab-aterm's in-process fake aterm, which knows no `codex-rotation` verb (so an
//! unscripted report is answered as an old host answers it, unsupported), scratch ChatGPT logins
//! codex's own login code wrote, and a loopback model. The walled attempt fails on a refused
//! login, as in `renewal_consume.rs`: the consume and the report judge only the resumed turn,
//! which runs on the login the receipt names as its destination. Never a real credential,
//! provider or aterm.

use super::login_fixtures as fixtures;
use super::renewal_admission::subscription_fixture;
use super::*;
use codex_alab_aterm::renewal::Identity;
use codex_alab_core::account_handoff::Destination;
use codex_alab_core::account_handoff::Purpose;
use codex_alab_core::session::AccountHandoff;
use codex_alab_core::session::Inherited;
use codex_alab_core::session::NativeOrigin;
use codex_alab_core::session::NativePhase;
use codex_alab_core::session::OwedAct;
use codex_core::StartIfIdleSubmission;
use codex_core::WithTurnExtensionData;
use codex_extension_api::ExtensionDataInit;
use codex_protocol::error::CodexErrorDetails;
use codex_protocol::protocol::CodexErrorInfo;
use codex_protocol::turn_input::RecoverTurnRequest;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

const VERB: &str = "codex-rotation";
/// The journal's line for a report that was sent (`renewal_completion.rs`).
const REPORTED: &str = "its completion report, asked";
/// The journal's line for a consumed move whose resumed turn did not serve.
const NOT_SERVED: &str = "its resumed turn did not serve on the destination";
/// The journal's line for a consumed move whose resumed turn ended past the canary hold.
const STALE: &str = "canary hold; no completion is reported";
/// The journal's words for a report whose resumed turn's end passed the hold between two asks.
const STALE_ASK: &str = "the resumed turn's end passed the canary hold";
/// What an old host's refusal of the report leaves, in the journal's words.
const UNSUPPORTED: &str = "the host takes no such report";

/// A policy under which nothing is typed after a recovered turn and no follow pass asks the
/// host about the move (no `codex_rotation`): the stop and its retry are all that act.
const QUIET: &[(&str, &str)] = &[("continue", "false"), ("human_grace_s", "0")];

/// A host that moves a session at a usage wall (`relaunch` is the fake's default).
const ROTATION: &[(&str, &str)] = &[
    ("human_grace_s", "0"),
    ("codex_rotation", "1"),
    ("accounts", "wall"),
    ("resume_limits", "true"),
    ("limit_wait", "true"),
    ("continue", "false"),
];

fn receipt_id() -> String {
    "c".repeat(32)
}

/// The account the walled turn's usage wall was on: never the destination's.
fn source() -> Identity {
    Identity {
        account: "a".repeat(64),
        user: "b".repeat(64),
    }
}

/// The model refusing the scratch login, so a turn ends at an error.
fn refused() -> wiremock::ResponseTemplate {
    wiremock::ResponseTemplate::new(401).set_body_json(serde_json::json!({
        "error":{"message":"expired","type":"invalid_request_error","code":"invalid_api_key"}
    }))
}

/// Counts the turn stops that every Autopilot hook ran before (it is registered after it).
struct Stops(Arc<AtomicU64>);

impl codex_extension_api::TurnLifecycleContributor for Stops {
    fn on_turn_stop<'a>(
        &'a self,
        _input: codex_extension_api::TurnStopInput<'a>,
    ) -> codex_extension_api::ExtensionFuture<'a, ()> {
        Box::pin(async move {
            self.0.fetch_add(1, Ordering::SeqCst);
        })
    }
}

/// AN ERROR CORE REPORTS TO THE TURN'S LIFECYCLE ALONE, at the one stop it is armed for: a
/// contributor ahead of the Autopilot hands it `on_turn_error` before the Autopilot's own stop,
/// as core does for an error it sends no error event for and so persists nothing of (a post-turn
/// compaction's usage limit, `core/src/session/turn.rs`): the turn's terminal in history reads
/// clean. The error is one `classify` names no wall for (an interruption core reports as an
/// error), so the turn stands at no wall and only the stop's own mark that it met an error
/// tells it from a served turn.
#[derive(Default)]
struct ErrorAtStop {
    autopilot: std::sync::OnceLock<std::sync::Weak<Autopilot>>,
    armed: AtomicBool,
}

impl ErrorAtStop {
    fn bind(&self, autopilot: &Arc<Autopilot>) {
        assert!(self.autopilot.set(Arc::downgrade(autopilot)).is_ok());
    }

    fn arm(&self) {
        self.armed.store(true, Ordering::SeqCst);
    }
}

impl codex_extension_api::TurnLifecycleContributor for ErrorAtStop {
    fn on_turn_stop<'a>(
        &'a self,
        input: codex_extension_api::TurnStopInput<'a>,
    ) -> codex_extension_api::ExtensionFuture<'a, ()> {
        Box::pin(async move {
            if !self.armed.swap(false, Ordering::SeqCst) {
                return;
            }
            let Some(autopilot) = self.autopilot.get().and_then(std::sync::Weak::upgrade) else {
                return;
            };
            autopilot
                .on_turn_error(codex_extension_api::TurnErrorInput {
                    turn_id: input.turn_store.level_id(),
                    error: CodexErrorInfo::Other,
                    error_details: &CodexErrorDetails::Interrupted,
                    session_store: input.session_store,
                    thread_store: input.thread_store,
                    turn_store: input.turn_store,
                })
                .await;
        })
    }
}

/// A session whose first attempt at a turn ended at an error, on the login a move's receipt
/// names as its destination; the walled turn's id and the record's write-throughs.
struct Walled {
    h: Harness,
    wall: String,
    /// The login the session runs on, hashed as the host names it: the move's destination.
    destination: Identity,
    stops: Arc<AtomicU64>,
    flushed: Arc<AtomicU64>,
    _refresh: wiremock::MockServer,
}

async fn walled(
    label: &str,
    policy: &[(&str, &str)],
    timing: Timing,
    resumed: Vec<wiremock::ResponseTemplate>,
) -> Walled {
    walled_with(label, policy, timing, resumed, None).await
}

/// [`walled`], with `before` registered ahead of the Autopilot.
async fn walled_with(
    label: &str,
    policy: &[(&str, &str)],
    timing: Timing,
    resumed: Vec<wiremock::ResponseTemplate>,
    before: Option<Arc<dyn codex_extension_api::TurnLifecycleContributor>>,
) -> Walled {
    let (login, destination, refresh) = subscription_fixture(label).await;
    let stops = Arc::new(AtomicU64::new(0));
    let mut replies = vec![refused(), refused()];
    replies.extend(resumed);
    let h = harness_login_with_error_hook(
        Mode::Current,
        replies,
        policy,
        &[],
        Some(0),
        Some(login),
        timing,
        |_| {},
        None,
        ErrorHooks {
            before,
            after: Some(Arc::new(Stops(Arc::clone(&stops)))),
            ..Default::default()
        },
    )
    .await;
    h.test
        .submit_turn("Recover this original native request.")
        .await
        .unwrap_or_else(|err| panic!("the walled attempt: {err}"));
    wait_until("the walled attempt stopped", || {
        stops.load(Ordering::SeqCst) == 1
    })
    .await;
    let wall = h
        .shared
        .latest_turn(&h.thread)
        .unwrap_or_else(|| panic!("the walled turn has no id"));
    assert!(
        h.test
            .codex
            .state_db()
            .unwrap_or_else(|| panic!("custody DB"))
            .thread_goals()
            .alab_capture_auth_standalone(h.test.session_configured.thread_id, &wall)
            .await
            .unwrap_or_else(|err| panic!("standalone native proof: {err}")),
        "the walled turn is standalone"
    );
    let flushed = Arc::new(AtomicU64::new(0));
    let writes = Arc::clone(&flushed);
    assert!(h.shared.set_flush(Box::new(move || {
        writes.fetch_add(1, Ordering::SeqCst);
        true
    })));
    Walled {
        h,
        wall,
        destination,
        stops,
        flushed,
        _refresh: refresh,
    }
}

impl Walled {
    /// The bound intent of a move of `purpose` for the walled turn, as the record carries it
    /// once its receipt is committed: not yet accepted, its one walled start counted.
    fn intent(&self, purpose: Purpose) -> OwedAct {
        let (from, to) = match purpose {
            Purpose::Login => (self.destination.clone(), None),
            Purpose::WeeklyQuota | Purpose::LoginRotation => (
                source(),
                Some(Destination {
                    account: self.destination.account.clone(),
                    user: self.destination.user.clone(),
                }),
            ),
        };
        OwedAct {
            thread: self.h.thread.clone(),
            native_origin: Some(NativeOrigin {
                turn_id: self.wall.clone(),
                phase: NativePhase::Complete,
            }),
            renewal: Some(AccountHandoff {
                purpose,
                destination: to,
                openai_scope: true,
                local: false,
                account: from.account,
                user: from.user,
                receipt: Some(receipt_id()),
                turn: None,
                accepted: false,
                starts: 1,
                goal_id: None,
            }),
            rule: Some(purpose.rule().into()),
            text: Some("Continue the original task.".into()),
            mode: Some("land".into()),
            ..Default::default()
        }
    }

    /// `verb` with this session's selector, as the host is sent it.
    fn line(&self, request: &str) -> String {
        format!("@{} {request}", self.h.fake.sid())
    }

    /// The usage wall's receipt's eight fields, as every answer about it carries them.
    fn receipt(&self) -> String {
        let source = source();
        format!(
            "id={} thread={} turn={} source_account={} source_user={} target_account={} \
             target_user={}",
            receipt_id(),
            self.h.thread,
            self.wall,
            source.account,
            source.user,
            self.destination.account,
            self.destination.user
        )
    }

    fn consume(&self) -> String {
        self.line(&format!(
            "{VERB} consume id={} turn={}",
            receipt_id(),
            self.wall
        ))
    }

    fn status(&self) -> String {
        self.line(&format!("{VERB} status id={}", receipt_id()))
    }

    fn complete(&self) -> String {
        self.line(&format!(
            "{VERB} complete id={} turn={}",
            receipt_id(),
            self.wall
        ))
    }

    fn consumed(&self) -> String {
        format!("OK {VERB} state=consumed {}\n", self.receipt())
    }

    /// Every request the host was sent that contains `needle`.
    fn sent(&self, needle: &str) -> Vec<String> {
        self.h
            .fake
            .requests()
            .into_iter()
            .filter(|line| line.contains(needle))
            .collect()
    }

    /// Every completion report the host was sent, under any verb.
    fn reports(&self) -> Vec<String> {
        self.sent(" complete ")
    }

    fn journal_lines(&self, needle: &str) -> usize {
        self.h
            .autopilot
            .supervisor()
            .journal()
            .iter()
            .filter(|line| line.contains(needle))
            .count()
    }

    fn rt(&self) -> Arc<codex_alab_autopilot::extension::ThreadRt> {
        self.h
            .autopilot
            .thread_rt(&self.h.thread)
            .unwrap_or_else(|| panic!("the thread's runtime"))
    }

    /// Recover the walled turn under its own id. With `custody`, the admission requires the
    /// intent on record and reserves the turn in it, as the native admission does
    /// (`renewal_consume.rs`); without, it admits the turn alone.
    async fn recover(&self, custody: bool) {
        let shared = self.h.shared;
        let expected = self.wall.clone();
        let thread = self.h.thread.clone();
        let mut init = ExtensionDataInit::new();
        init.insert(codex_alab_core::renewal::TurnAdmission::new(move |turn| {
            let expected = expected.clone();
            let thread = thread.clone();
            Box::pin(async move {
                if turn != expected {
                    return Err("wrong original turn".into());
                }
                if custody {
                    let mut owed = shared.owed_act().ok_or("missing restored custody")?;
                    owed.renewal.as_mut().ok_or("missing native receipt")?.turn = Some(turn);
                    if !shared.publish_renewal(shared.admission_generation(), owed)
                        || !shared.flush_record()
                    {
                        return Err("admission changed".into());
                    }
                }
                if !shared.admit_recovered_turn(&thread, &expected) {
                    return Err("admission changed".into());
                }
                Ok(())
            })
        }));
        let accepted = self
            .h
            .test
            .codex
            .recover_turn_if_idle(WithTurnExtensionData::new(
                RecoverTurnRequest {
                    turn_id: self.wall.clone(),
                    thread_settings: Default::default(),
                    trace: None,
                    prompt_cache_ttl: None,
                    cyber_access_program: None,
                },
                init,
            ))
            .await
            .unwrap_or_else(|err| panic!("original-ID admission: {err}"));
        assert!(
            matches!(accepted, StartIfIdleSubmission::Started { turn_id, .. } if turn_id == self.wall)
        );
    }

    /// The resumed turn's stop, the Autopilot's acknowledgment at it included.
    async fn resumed_stop(&self) {
        wait_until("the resumed turn stopped", || {
            self.stops.load(Ordering::SeqCst) == 2
        })
        .await;
    }

    async fn journal_says(&self, what: &str, needle: &str) {
        wait_until(what, || self.journal_lines(needle) > 0).await;
    }
}

/// A served resumed turn, its quota receipt consumed at its stop: the move's intent on
/// record, the consume answered `consumed`, and the turn recovered.
async fn consumed_at_stop(label: &str, timing: Timing) -> Walled {
    consumed_at_stop_with(label, timing, None).await
}

/// [`consumed_at_stop`], with `before` registered ahead of the Autopilot.
async fn consumed_at_stop_with(
    label: &str,
    timing: Timing,
    before: Option<Arc<dyn codex_extension_api::TurnLifecycleContributor>>,
) -> Walled {
    let walled = walled_with(
        label,
        QUIET,
        timing,
        vec![responses::sse_response(reply(
            "recovered",
            "Original work.",
        ))],
        before,
    )
    .await;
    walled
        .h
        .shared
        .set_owed_act(Some(walled.intent(Purpose::WeeklyQuota)));
    walled
        .h
        .fake
        .reply_once(&walled.consume(), &walled.consumed());
    walled
}

fn quick() -> Timing {
    Timing {
        min_work: Duration::ZERO,
        ..Timing::default()
    }
}

/// An [`ErrorAtStop`] to register ahead of the Autopilot, as the harness takes it.
fn error_at_stop() -> (
    Arc<ErrorAtStop>,
    Option<Arc<dyn codex_extension_api::TurnLifecycleContributor>>,
) {
    let hook = Arc::new(ErrorAtStop::default());
    let before: Arc<dyn codex_extension_api::TurnLifecycleContributor> = hook.clone();
    (hook, Some(before))
}

/// A SERVED RESUMED TURN REPORTS ITS MOVE COMPLETE ONCE, AFTER THE CONSUME: exactly one
/// `codex-rotation complete id=<receipt> turn=<walled turn>`, sent after the consume its
/// stop confirmed, and one journal line that says how long the turn ran and what the host
/// answered (this fake, an old host: unsupported). The intent is released and written through
/// as without the report, and the report writes nothing more.
#[test]
fn a_served_resumed_turn_reports_its_move_complete_once_after_the_consume() {
    big_stack(|| async {
        let walled = consumed_at_stop("complete-served", quick()).await;
        walled.recover(true).await;
        walled.resumed_stop().await;
        let released = walled.flushed.load(Ordering::SeqCst);
        walled
            .journal_says("the report's outcome is said", REPORTED)
            .await;
        let requests = walled.h.fake.requests();
        let consumed = requests
            .iter()
            .position(|line| *line == walled.consume())
            .unwrap_or_else(|| panic!("the consume was sent: {requests:#?}"));
        let reported = requests
            .iter()
            .position(|line| *line == walled.complete())
            .unwrap_or_else(|| panic!("the report was sent: {requests:#?}"));
        assert!(consumed < reported, "the report follows the consume");
        assert_eq!(walled.reports(), vec![walled.complete()]);
        assert_eq!(walled.h.shared.owed_act(), None, "the intent is released");
        assert_eq!(walled.journal_lines(REPORTED), 1);
        assert_eq!(walled.journal_lines(UNSUPPORTED), 1);
        assert!(
            walled.journal_lines("(it ran ") == 1,
            "the resumed turn's length is said"
        );
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert_eq!(walled.reports().len(), 1, "never twice");
        assert_eq!(
            walled.flushed.load(Ordering::SeqCst),
            released,
            "the report writes nothing through"
        );
        assert_eq!(
            walled.h.mock.requests().len(),
            3,
            "the resumed turn ran once"
        );
    });
}

/// A RESUMED TURN THAT ERRORS REPORTS NOTHING: the destination refuses it, its receipt is
/// consumed as before, and no report goes; the journal says why.
#[test]
fn a_resumed_turn_that_errors_reports_nothing() {
    big_stack(|| async {
        let walled = walled(
            "complete-errored",
            QUIET,
            quick(),
            vec![refused(), refused()],
        )
        .await;
        walled
            .h
            .shared
            .set_owed_act(Some(walled.intent(Purpose::WeeklyQuota)));
        walled
            .h
            .fake
            .reply_once(&walled.consume(), &walled.consumed());
        walled.recover(true).await;
        walled.resumed_stop().await;
        walled
            .journal_says("the unserved turn is said", NOT_SERVED)
            .await;
        assert_eq!(
            walled.sent(&walled.consume()).len(),
            1,
            "consumed as before"
        );
        assert_eq!(walled.h.shared.owed_act(), None);
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert!(walled.reports().is_empty(), "{:#?}", walled.reports());
        assert_eq!(walled.journal_lines(REPORTED), 0);
        assert_eq!(
            walled.h.mock.requests().len(),
            4,
            "the resumed turn was refused"
        );
    });
}

/// A RESUMED TURN THAT MET AN ERROR NO WALL NAMES IS NOT REPORTED AT ITS STOP: the turn met an
/// error core reported to its lifecycle alone ([`ErrorAtStop`]), one `classify` names no wall
/// for, so it stops at no wall and its terminal reads clean. Its stop confirms the consume as
/// before, and only the stop's own mark that the turn met an error keeps the report from going;
/// the journal says why. (POSITIVE CONTROL: the same turn with no error,
/// `a_served_resumed_turn_reports_its_move_complete_once_after_the_consume`.)
#[test]
fn a_resumed_turn_that_met_an_error_no_wall_names_is_not_reported_at_its_stop() {
    big_stack(|| async {
        let (hook, before) = error_at_stop();
        let walled = consumed_at_stop_with("complete-unwalled-error", quick(), before).await;
        hook.bind(&walled.h.autopilot);
        hook.arm();
        walled.recover(true).await;
        walled.resumed_stop().await;
        assert_eq!(
            walled.h.shared.owed_act(),
            None,
            "the stop consumed it as before"
        );
        assert_eq!(walled.sent(&walled.consume()).len(), 1);
        walled
            .journal_says("the unserved turn is said", NOT_SERVED)
            .await;
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert!(walled.reports().is_empty(), "{:#?}", walled.reports());
        assert_eq!(walled.journal_lines(REPORTED), 0);
        assert_eq!(
            walled.h.mock.requests().len(),
            3,
            "the resumed turn ran once and was answered"
        );
    });
}

/// THE RETRY OF A LOST ACKNOWLEDGMENT JUDGES THE RESUMED TURN BY THIS PROCESS'S STOP OF IT TOO:
/// the turn met an error core persisted nothing of ([`ErrorAtStop`]), so its terminal in history
/// reads clean; the consume's answer is unreadable and the stop's own `status` read fails, so
/// the stop leaves the intent to its retry. The retry reads the receipt consumed and the clean
/// terminal, and still reports nothing: the stop found the turn unserved. (POSITIVE CONTROL: the
/// same retry for a turn with no error, `a_dropped_consume_ack_recovered_by_the_retry_reports_once`.)
#[test]
fn a_lost_acks_retry_does_not_report_a_turn_its_stop_found_unserved() {
    big_stack(|| async {
        let (hook, before) = error_at_stop();
        let walled = consumed_at_stop_with("complete-unserved-retry", quick(), before).await;
        hook.bind(&walled.h.autopilot);
        hook.arm();
        walled
            .h
            .fake
            .reply_once(&walled.consume(), &format!("OK {VERB} state=unreadable\n"));
        walled.h.fake.reply_sequence(
            &walled.status(),
            vec![format!("ERR {VERB} unavailable\n"), walled.consumed()],
        );
        walled.recover(true).await;
        walled.resumed_stop().await;
        assert!(
            walled.h.shared.owed_act().is_some(),
            "the stop could not confirm the consume"
        );
        wait_until("the retry confirmed the consume", || {
            walled.h.shared.owed_act().is_none()
        })
        .await;
        assert_eq!(walled.sent(&walled.status()).len(), 2, "stop, then retry");
        walled
            .journal_says("the unserved turn is said", NOT_SERVED)
            .await;
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert!(walled.reports().is_empty(), "{:#?}", walled.reports());
        assert_eq!(walled.journal_lines(REPORTED), 0);
        assert_eq!(walled.journal_lines(NOT_SERVED), 1);
    });
}

/// A HOST THAT KEEPS ANSWERING `waiting` IS NOT ASKED ONCE THE RESUMED TURN'S END PASSES THE
/// HOLD: the resumed turn's end is aged again before every ask, so the report asks again only
/// while that end is within the (here shortened) hold, and ends then, well inside its five
/// seconds, saying why. (Without the look before each ask, it would ask on to the budget's
/// end.)
#[test]
fn a_waiting_host_is_not_asked_once_the_resumed_turns_end_passes_the_hold() {
    big_stack(|| async {
        let walled = consumed_at_stop(
            "complete-waiting-stale",
            Timing {
                canary_hold: Duration::from_secs(2),
                ..quick()
            },
        )
        .await;
        walled
            .h
            .fake
            .reply_always(&walled.complete(), &format!("OK {VERB} state=waiting\n"));
        walled.recover(true).await;
        walled.resumed_stop().await;
        walled
            .journal_says("the report's outcome is said", REPORTED)
            .await;
        let asked = walled.reports().len();
        assert!(asked >= 2, "asked again while within the hold: {asked}");
        assert_eq!(
            walled.journal_lines(STALE_ASK),
            1,
            "{:#?}",
            walled.h.autopilot.supervisor().journal()
        );
        assert_eq!(walled.journal_lines(&format!("asked {asked} time(s)")), 1);
        assert_eq!(walled.journal_lines("still waiting"), 0);
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert_eq!(walled.reports().len(), asked, "nothing after it ended");
        assert_eq!(walled.h.shared.owed_act(), None);
    });
}

/// AN OLD HOST IS ASKED ONCE AND THE MOVE GOES ON AS BEFORE: a host that knows `codex-rotation`
/// but not its report refuses it as `invalid request`. It is asked once and never again; the
/// intent the stop released, the record's write-throughs and the thread's next look are what
/// they were while the report waited at the host, and nothing is typed.
#[test]
fn an_old_host_is_asked_once_and_the_move_goes_on_as_before() {
    big_stack(|| async {
        let walled = consumed_at_stop("complete-old-host", quick()).await;
        walled
            .h
            .fake
            .reply_once(&walled.complete(), &format!("ERR {VERB} invalid request\n"));
        walled.h.fake.gate(&format!("{VERB} complete"));
        walled.recover(true).await;
        walled.resumed_stop().await;
        assert!(
            walled
                .h
                .fake
                .wait_for(|fake| fake
                    .gated_requests()
                    .iter()
                    .any(|line| *line == walled.complete()))
                .await,
            "the report reached the host"
        );
        let rt = walled.rt();
        let before = (
            walled.h.shared.owed_act(),
            walled.flushed.load(Ordering::SeqCst),
            rt.timer_due(),
            walled.h.shared.recovery(),
        );
        assert_eq!(before.0, None, "the stop released the intent");
        walled.h.fake.open_gate(&format!("{VERB} complete"));
        walled
            .journal_says("the old host's answer is said", UNSUPPORTED)
            .await;
        let after = (
            walled.h.shared.owed_act(),
            walled.flushed.load(Ordering::SeqCst),
            rt.timer_due(),
            walled.h.shared.recovery(),
        );
        assert_eq!(after, before, "the report's answer changed nothing");
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert_eq!(walled.reports(), vec![walled.complete()], "asked once");
        assert!(
            walled.h.fake.acts().is_empty(),
            "{:?}",
            walled.h.fake.acts()
        );
    });
}

/// A HOST THAT ANSWERS `waiting` IS ASKED AGAIN WITHIN THE BUDGET: a busy lock at the host
/// answers `waiting`, the report is asked again a moment later, and the second ask is
/// recorded; both within the report's five seconds, and the thread holds the receipt as
/// recorded.
#[test]
fn a_waiting_host_is_asked_again_within_the_budget() {
    big_stack(|| async {
        let walled = consumed_at_stop("complete-waiting", quick()).await;
        walled.h.fake.reply_sequence(
            &walled.complete(),
            vec![
                format!("OK {VERB} state=waiting\n"),
                format!("OK {VERB} state=completed {}\n", walled.receipt()),
            ],
        );
        walled.recover(true).await;
        walled.resumed_stop().await;
        wait_until("the report is asked", || !walled.reports().is_empty()).await;
        let first = tokio::time::Instant::now();
        walled
            .journal_says("the report's outcome is said", "recorded: the destination")
            .await;
        assert!(
            first.elapsed() < Duration::from_secs(5),
            "within the budget: {:?}",
            first.elapsed()
        );
        assert_eq!(
            walled.reports(),
            vec![walled.complete(), walled.complete()],
            "waiting, then completed"
        );
        assert_eq!(walled.journal_lines("asked 2 time(s)"), 1);
        assert_eq!(
            walled.rt().completion_reported(),
            Some((receipt_id(), true))
        );
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert_eq!(walled.reports().len(), 2);
    });
}

/// A SAME-ACCOUNT RENEWAL OR A LOST LOGIN'S MOVE REPORTS NOTHING: neither puts a canary on any
/// account, so its consumed receipt sends no report under any verb.
#[test]
fn a_renewal_or_a_lost_logins_move_reports_nothing() {
    big_stack(|| async {
        for (purpose, verb) in [
            (Purpose::Login, "codex-renewal"),
            (Purpose::LoginRotation, "codex-login-rotation"),
        ] {
            let walled = walled(
                &format!("complete-{verb}"),
                QUIET,
                quick(),
                vec![responses::sse_response(reply(
                    "recovered",
                    "Original work.",
                ))],
            )
            .await;
            walled.h.shared.set_owed_act(Some(walled.intent(purpose)));
            let consume = walled.line(&format!(
                "{verb} consume id={} turn={}",
                receipt_id(),
                walled.wall
            ));
            let consumed = match purpose {
                Purpose::Login => format!(
                    "OK {verb} state=consumed id={} thread={} wall={} account={} user={}\n",
                    receipt_id(),
                    walled.h.thread,
                    walled.wall,
                    walled.destination.account,
                    walled.destination.user
                ),
                _ => format!("OK {verb} state=consumed {}\n", walled.receipt()),
            };
            walled.h.fake.reply_once(&consume, &consumed);
            walled.recover(true).await;
            walled.resumed_stop().await;
            assert_eq!(
                walled.h.shared.owed_act(),
                None,
                "{verb}: the receipt was consumed"
            );
            assert_eq!(walled.sent(&consume).len(), 1, "{verb}");
            tokio::time::sleep(Duration::from_secs(1)).await;
            assert!(
                walled.reports().is_empty(),
                "{verb}: {:#?}",
                walled.reports()
            );
            assert_eq!(walled.journal_lines(REPORTED), 0, "{verb}");
            assert_eq!(walled.journal_lines(NOT_SERVED), 0, "{verb}");
        }
    });
}

/// A DROPPED CONSUME ACKNOWLEDGMENT, RECOVERED BY THE RETRY, REPORTS ONCE: the consume's
/// answer is unreadable and the stop's own `status` read fails, so the stop leaves the intent
/// to its retry; the retry's `status` reads the receipt consumed from the terminal history
/// holds, and the report goes once, after it, and not again.
#[test]
fn a_dropped_consume_ack_recovered_by_the_retry_reports_once() {
    big_stack(|| async {
        let walled = consumed_at_stop("complete-retry", quick()).await;
        walled
            .h
            .fake
            .reply_once(&walled.consume(), &format!("OK {VERB} state=unreadable\n"));
        walled.h.fake.reply_sequence(
            &walled.status(),
            vec![format!("ERR {VERB} unavailable\n"), walled.consumed()],
        );
        walled.recover(true).await;
        walled.resumed_stop().await;
        assert!(
            walled.h.shared.owed_act().is_some(),
            "the stop could not confirm the consume"
        );
        wait_until("the retry confirmed the consume", || {
            walled.h.shared.owed_act().is_none()
        })
        .await;
        walled
            .journal_says("the report's outcome is said", REPORTED)
            .await;
        let requests = walled.h.fake.requests();
        let confirmed = requests
            .iter()
            .rposition(|line| *line == walled.status())
            .unwrap_or_else(|| panic!("the retry read the receipt: {requests:#?}"));
        let reported = requests
            .iter()
            .position(|line| *line == walled.complete())
            .unwrap_or_else(|| panic!("the report was sent: {requests:#?}"));
        assert!(confirmed < reported, "the report follows the retry's read");
        assert_eq!(walled.sent(&walled.status()).len(), 2, "stop, then retry");
        tokio::time::sleep(Duration::from_secs(2)).await;
        assert_eq!(walled.reports(), vec![walled.complete()], "one, not two");
        assert_eq!(walled.journal_lines(REPORTED), 1);
    });
}

/// A RESUMED TURN WHOSE END IS OLDER THAN THE CANARY HOLD IS NOT REPORTED, on either path
/// that judges it: the retry of a lost acknowledgment, which ages the terminal by its durable
/// `completed_at`, and the stop itself, which ages it from the stop. The hold is shortened
/// here ([`Timing::canary_hold`]). The intent is released as without the bound; the journal
/// says why nothing went. (POSITIVE CONTROLS: the same paths under the hold's own length,
/// `a_dropped_consume_ack_recovered_by_the_retry_reports_once` and
/// `a_served_resumed_turn_reports_its_move_complete_once_after_the_consume`.)
#[test]
fn a_resumed_turn_whose_end_is_older_than_the_canary_hold_is_not_reported() {
    big_stack(|| async {
        // The retry confirms the consume only once the terminal is past a 2 s hold.
        let walled = consumed_at_stop(
            "complete-stale-retry",
            Timing {
                canary_hold: Duration::from_secs(2),
                ..quick()
            },
        )
        .await;
        walled
            .h
            .fake
            .reply_once(&walled.consume(), &format!("OK {VERB} state=unreadable\n"));
        walled
            .h
            .fake
            .reply_once(&walled.status(), &format!("ERR {VERB} unavailable\n"));
        walled.recover(true).await;
        walled.resumed_stop().await;
        assert!(walled.h.shared.owed_act().is_some());
        // Whole seconds of `completed_at` and the hold's two, with a margin.
        tokio::time::sleep(Duration::from_millis(3500)).await;
        walled
            .h
            .fake
            .reply_always(&walled.status(), &walled.consumed());
        wait_until("the retry confirmed the consume", || {
            walled.h.shared.owed_act().is_none()
        })
        .await;
        walled
            .journal_says("the stale terminal is said", STALE)
            .await;
        assert!(
            walled.journal_lines("past the host's 2 s canary hold") == 1,
            "{:#?}",
            walled.h.autopilot.supervisor().journal()
        );
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert!(walled.reports().is_empty(), "{:#?}", walled.reports());
        assert_eq!(walled.journal_lines(REPORTED), 0);

        // The stop's consume answered only once the stop is past a 1 s hold (within the
        // client's own deadline).
        let walled = consumed_at_stop(
            "complete-stale-stop",
            Timing {
                canary_hold: Duration::from_secs(1),
                ..quick()
            },
        )
        .await;
        walled.h.fake.gate(&format!("{VERB} consume"));
        walled.recover(true).await;
        assert!(
            walled
                .h
                .fake
                .wait_for(|fake| fake
                    .gated_requests()
                    .iter()
                    .any(|line| *line == walled.consume()))
                .await,
            "the stop reached its consume"
        );
        tokio::time::sleep(Duration::from_millis(2500)).await;
        walled.h.fake.open_gate(&format!("{VERB} consume"));
        walled.resumed_stop().await;
        assert_eq!(walled.h.shared.owed_act(), None, "the stop confirmed it");
        walled.journal_says("the stale stop is said", STALE).await;
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert!(walled.reports().is_empty(), "{:#?}", walled.reports());
        assert_eq!(walled.journal_lines(REPORTED), 0);
    });
}

/// The process the host starts on the destination's login after the resumed turn ended: a new
/// Autopilot, supervisor and host connection over a resume of the same rollout, seeded with
/// the bound intent as a same-pid image is (`rotation_decline.rs` `restarted` says why that
/// stands in for aterm's relaunch).
struct Restored {
    fake: FakeAterm,
    shared: &'static Shared,
    autopilot: Arc<Autopilot>,
    _test: TestCodex,
    _dir: tempfile::TempDir,
}

impl Drop for Restored {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!(
                "restored supervisor journal: {:#?}",
                self.autopilot.supervisor().journal()
            );
            eprintln!("restored fake requests: {:#?}", self.fake.requests());
        }
    }
}

/// End `walled`'s process and start the next one on its session and login, its record holding
/// `owed`, its host answering every `status` of the receipt `committed` and every `consume`
/// `consumed`. Its host's status reads wait until the thread's manager is bound
/// (`rotation_decline.rs` `restarted_under`).
async fn restored(walled: &Walled, owed: OwedAct) -> Restored {
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
    for (key, value) in ROTATION {
        fake.set_policy_key(key, value);
    }
    let line = |request: String| format!("@{} {request}", fake.sid());
    fake.reply_always(
        &line(format!("{VERB} status id={}", receipt_id())),
        &format!("OK {VERB} state=committed {}\n", walled.receipt()),
    );
    fake.reply_always(
        &line(format!(
            "{VERB} consume id={} turn={}",
            receipt_id(),
            walled.wall
        )),
        &walled.consumed(),
    );
    fake.gate("status");
    let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
    shared.set_owed_act(Some(owed.clone()));
    assert!(shared.set_inherited(Inherited {
        session_id: Some(walled.h.thread.clone()),
        owed_act: Some(owed),
        ..Default::default()
    }));
    let endpoint = fake.endpoint();
    let sup = Supervisor::new(SupervisorOptions {
        shared,
        endpoint: Arc::new(move || Some(endpoint.clone())),
        client: ClientOptions {
            timeout: Duration::from_secs(30),
            ..ClientOptions::default()
        },
    });
    let mut builder = ExtensionRegistryBuilder::<Config>::new();
    let autopilot = Autopilot::new(sup, None, None, quick(), |_| true);
    autopilot.register(&mut builder);
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
    Restored {
        fake,
        shared,
        autopilot,
        _test: test,
        _dir: dir,
    }
}

/// A RESTORED PROCESS WHOSE HISTORY ALREADY SHOWS THE RESUMED TERMINAL REPORTS ONCE: the turn
/// was recovered and served on the destination, and the process ended before its receipt was
/// consumed. The process after it, its record holding the bound intent, finds the receipt
/// `committed` and the resumed turn's clean terminal in history: it consumes the receipt
/// without recovering the turn again, and reports the move complete once, after the consume.
#[test]
fn a_restored_process_whose_history_shows_the_resumed_terminal_reports_once() {
    big_stack(|| async {
        let walled = walled(
            "complete-restored",
            QUIET,
            quick(),
            vec![responses::sse_response(reply(
                "recovered",
                "Original work.",
            ))],
        )
        .await;
        // The first process recovers and serves the turn with no intent on record of its
        // own, so nothing in it consumes the receipt.
        walled.recover(false).await;
        walled.resumed_stop().await;
        assert!(walled.reports().is_empty());
        let owed = walled.intent(Purpose::WeeklyQuota);
        let restored = restored(&walled, owed).await;
        let complete = format!(
            "@{} {VERB} complete id={} turn={}",
            restored.fake.sid(),
            receipt_id(),
            walled.wall
        );
        let consume = format!(
            "@{} {VERB} consume id={} turn={}",
            restored.fake.sid(),
            receipt_id(),
            walled.wall
        );
        assert!(
            restored
                .fake
                .wait_for(|fake| fake.requests().contains(&complete))
                .await,
            "the restored process reported the move complete"
        );
        let requests = restored.fake.requests();
        let consumed = requests
            .iter()
            .position(|line| *line == consume)
            .unwrap_or_else(|| panic!("the receipt was consumed: {requests:#?}"));
        let reported = requests
            .iter()
            .position(|line| *line == complete)
            .unwrap_or_else(|| panic!("the report was sent: {requests:#?}"));
        assert!(consumed < reported, "the report follows the consume");
        wait_until("the restored intent is released", || {
            restored.shared.owed_act().is_none()
        })
        .await;
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert_eq!(
            restored
                .fake
                .requests()
                .iter()
                .filter(|line| line.contains(" complete "))
                .count(),
            1,
            "once"
        );
        assert_eq!(
            walled.h.mock.requests().len(),
            3,
            "the served turn is not recovered again"
        );
    });
}
