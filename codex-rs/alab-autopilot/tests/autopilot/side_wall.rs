// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A SECOND ROOT THREAD'S WALLED TURN DOES NOT HOLD THE PRIMARY THREAD'S ACCOUNT MOVE
//! (2026-11-13). A side chat is a root thread of the TUI's own (`Cli`), so its turn at a
//! creditless usage wall waits in quota recovery, looking for an earned reset every 30
//! seconds, and its handoff says no: ending it starts no move. But a running turn makes the
//! session's record say `busy`, and aterm moves no account under a busy record: it refuses
//! the primary thread's ask at `prepare`, in its preparation job, at `commit` and before its
//! signal. One side chat left open at the wall held the primary's handoff off until the
//! wall's reset.
//!
//! The rule: while the primary thread's usage-wall move is live (its intent from this login
//! on record, codex-alab acting, no upgrade under way, its last follow pass having kept its
//! point and either sent the ask or kept the receipt the host granted), such a turn ends on
//! its own usage error after one more look for an earned reset. A turn that waits is woken
//! by the next pass that leaves the move live, whether its last look was told to wait or
//! ended before it asked, so it does not wait for its own 30 s poll. An intent that only
//! lingers on record ends nothing, and neither does an unbound point its pass keeps with no
//! word to the host (a person's hold).
//!
//! Against alab-aterm's in-process fake aterm, a scratch ChatGPT login codex's own login
//! code wrote, a loopback model and a loopback usage backend. The second thread is started
//! `Cli`-sourced, as the TUI starts a side chat, so its walled request really waits in
//! quota recovery. Every test but two first shows it parked ([`Roots::side_parks`]): the
//! record reads `busy` and the turn neither ends nor errors. The two that cannot are the
//! ones whose turn meets its wall while the move is already live
//! (`a_side_turn_walled_after_the_ask_does_not_park`,
//! `a_bound_receipt_under_a_host_that_cannot_be_read_ends_a_side_turn`): under the rule that
//! turn never parks. What says it would have is the same turn parked in the other tests,
//! and a run made by hand with the rule taken out (not in the tree), in which both fail on
//! a turn that does not end. The fake's `codex-rotation` requests are held at its gate and
//! answered by aterm's own rule, read from the record `codex_alab_tui`'s writer wrote: an
//! ask or a `commit` is refused while the record's status is not `idle` or `waiting`.

use super::renewal_admission::subscription_fixture;
use super::*;
use codex_alab_tui::record;
use codex_alab_tui::session::Writer;
use codex_protocol::protocol::CodexErrorInfo;
use codex_protocol::protocol::SessionSource;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

/// The host policy under which a usage wall asks for another account and, failing one,
/// waits the wall out. `continue` is off, so nothing is typed after a turn's end.
const POLICY: &[(&str, &str)] = &[
    ("human_grace_s", "0"),
    ("codex_rotation", "1"),
    ("accounts", "wall"),
    ("resume_limits", "true"),
    ("limit_wait", "true"),
    ("continue", "false"),
];

const ROTATION: &str = "codex-rotation";
/// What the person asks on the session's primary thread, and in the side chat.
const PRIMARY_ASKS: &str = "do the task";
const SIDE_ASKS: &str = "a question on the side";
/// aterm's answers to a request it judges against the session's record.
const WAITING: &str = "OK codex-rotation state=waiting\n";
const WALL_CHANGED: &str = "ERR codex-renewal wall changed\n";
const RECEIPT_CHANGED: &str = "ERR codex-renewal receipt changed\n";
/// The journal's line for a side turn the rule ended.
const LEAVING: &str = "its walled turn ends; the session is leaving this account";
/// The journal's line for a usage wall's own decision while its reset is ahead.
const WAITS: &str = "s for usage-resume@v1";
/// How long quota recovery waits between two looks with no word from its host.
const POLL: Duration = Duration::from_secs(30);

/// The login's account as its usage backend reports it.
#[derive(Default)]
struct Account {
    /// The model refused a request at the usage wall: the week reads spent from here on.
    walled: AtomicBool,
    /// The credit list offers an earned reset.
    credit: AtomicBool,
    /// The credit list cannot be read: it answers 503.
    list_down: AtomicBool,
    /// An earned reset was redeemed: the week reads usable again.
    redeemed: AtomicBool,
    /// Credit list reads so far, and how many there were when the side chat's request was
    /// refused: a list read after that is its recovery's look for an earned reset.
    lists: AtomicUsize,
    lists_at_side_wall: AtomicUsize,
    /// Model requests of the side chat that were refused at the wall, and answered.
    side_refused: AtomicUsize,
    side_answered: AtomicUsize,
    redemptions: AtomicUsize,
}

/// A session with two root threads on one ChatGPT login: the primary (the harness's own)
/// and a side chat started as the TUI starts one.
struct Roots {
    h: Harness,
    account: Arc<Account>,
    /// The record's writer over the session's shared state, as the TUI installs it.
    writer: &'static Writer,
    side: codex_core::NewThread,
    side_key: String,
    /// Every event of the side chat so far.
    side_events: Arc<Mutex<Vec<EventMsg>>>,
    /// The login's hashed identity, as the ask names it.
    identity: codex_alab_aterm::renewal::Identity,
    /// How many requests held at the fake's gate were answered.
    answered: AtomicUsize,
    _backend: wiremock::MockServer,
    _refresh: wiremock::MockServer,
}

/// A receipt the host binds the primary's handoff to.
struct Receipt {
    /// The receipt's fields, as every answer about it carries them.
    fields: String,
    /// The exact `status` and `commit` requests for it.
    status: String,
    commit: String,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Wait until `done` holds, at most `limit`: a bound on how soon, where [`wait_until`] is
/// only a hang detector.
async fn within(limit: Duration, what: &str, done: impl Fn() -> bool) {
    let deadline = tokio::time::Instant::now() + limit;
    while !done() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "not within {limit:?}: {what}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn roots() -> Roots {
    let (login, identity, refresh) = subscription_fixture("side-wall").await;
    // The week's reset is beyond the 24-hour earned-reset spending boundary.
    let reset = i64::try_from(now_ms() / 1000).unwrap_or(i64::MAX) + 25 * 3600;
    let account = Arc::new(Account::default());
    let backend = wiremock::MockServer::start().await;
    let read = Arc::clone(&account);
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/api/codex/usage"))
        .respond_with(move |_: &wiremock::Request| {
            let spent = read.walled.load(Ordering::SeqCst) && !read.redeemed.load(Ordering::SeqCst);
            wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "account_id": "side-wall-account", "user_id": "side-wall-user",
                "plan_type": "pro",
                "rate_limit": {"allowed": !spent, "limit_reached": spent,
                    "secondary_window": {"used_percent": if spent { 100 } else { 0 },
                        "limit_window_seconds": 604_800, "reset_at": reset,
                        "reset_after_seconds": 90_000}},
            }))
        })
        .mount(&backend)
        .await;
    let listed = Arc::clone(&account);
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path(
            "/api/codex/rate-limit-reset-credits",
        ))
        .respond_with(move |_: &wiremock::Request| {
            // Read before the count: a test that sees a read counted knows how it was
            // answered.
            let down = listed.list_down.load(Ordering::SeqCst);
            listed.lists.fetch_add(1, Ordering::SeqCst);
            if down {
                return wiremock::ResponseTemplate::new(503);
            }
            let credits = if listed.credit.load(Ordering::SeqCst) {
                serde_json::json!([{"id": "A", "reset_type": "codex_rate_limits",
                    "status": "available", "granted_at": "2026-01-01T00:00:00Z",
                    "expires_at": "2030-01-01T00:00:00Z"}])
            } else {
                serde_json::json!([])
            };
            let count = credits.as_array().map_or(0, Vec::len);
            wiremock::ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"available_count": count, "credits": credits}))
        })
        .mount(&backend)
        .await;
    let redeemed = Arc::clone(&account);
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path(
            "/api/codex/rate-limit-reset-credits/consume",
        ))
        .respond_with(move |_: &wiremock::Request| {
            redeemed.redemptions.fetch_add(1, Ordering::SeqCst);
            redeemed.redeemed.store(true, Ordering::SeqCst);
            wiremock::ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"code": "reset", "windows_reset": 2}))
        })
        .mount(&backend)
        .await;
    let usage_base = backend.uri();
    let h = harness_login(
        Mode::Current,
        Vec::new(),
        POLICY,
        &[],
        Some(0),
        Some(login),
        Timing {
            reset_grace: Duration::ZERO,
            reset_jitter: Duration::ZERO,
            ..Timing::default()
        },
        move |config| config.chatgpt_base_url = usage_base,
        None,
    )
    .await;
    // The record's writer, installed before anything is written through.
    let writer: &'static Writer = Box::leak(Box::new(Writer::new(
        h.shared,
        h._dir.path().join("sessions"),
        Duration::ZERO,
        None,
    )));
    writer.install_write_through();
    // The model: every request of a turn that stands at the spent week is refused, the
    // side chat's own once an earned reset was redeemed is answered.
    let model = Arc::clone(&account);
    responses::mount_response_handler(&h._server, move |request| {
        let asked = request
            .message_input_texts("user")
            .last()
            .cloned()
            .unwrap_or_default();
        let refused = || {
            model.walled.store(true, Ordering::SeqCst);
            wiremock::ResponseTemplate::new(429).set_body_json(serde_json::json!({
                "error": {
                    "type": "usage_limit_reached",
                    "message": "limit reached",
                    "resets_at": reset,
                    "plan_type": "pro"
                }
            }))
        };
        match asked.as_str() {
            PRIMARY_ASKS => refused(),
            SIDE_ASKS if !model.redeemed.load(Ordering::SeqCst) => {
                if model.side_refused.fetch_add(1, Ordering::SeqCst) == 0 {
                    model
                        .lists_at_side_wall
                        .store(model.lists.load(Ordering::SeqCst), Ordering::SeqCst);
                }
                refused()
            }
            SIDE_ASKS => {
                model.side_answered.fetch_add(1, Ordering::SeqCst);
                responses::sse_response(reply("side", "The side chat is answered."))
            }
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
    // The host's sleep is the test's to say, whatever this Mac does meanwhile.
    h.autopilot.read_host_sleep_as(|| Duration::from_secs(5000));
    // Every request of the handoff protocol waits for the test's answer.
    h.fake.gate(ROTATION);

    // The side chat: a root thread of the TUI's own, which the autopilot keeps and the TUI
    // registers at the frame that first shows it.
    let side = h
        .test
        .thread_manager
        .start_thread(codex_core::StartThreadOptions {
            session_source: Some(SessionSource::Cli),
            ..h.test.start_thread_options().await
        })
        .await
        .unwrap_or_else(|err| panic!("the side chat: {err}"));
    let side_key = side.thread_id.to_string();
    wait_until("the side chat is the autopilot's", || {
        h.autopilot.thread_rt(&side_key).is_some()
    })
    .await;
    let side_events = Arc::new(Mutex::new(Vec::new()));
    let (events, thread) = (Arc::clone(&side_events), Arc::clone(&side.thread));
    tokio::spawn(async move {
        while let Ok(event) = thread.next_event().await {
            lock(&events).push(event.msg);
        }
    });
    let roots = Roots {
        h,
        account,
        writer,
        side,
        side_key,
        side_events,
        identity,
        answered: AtomicUsize::new(0),
        _backend: backend,
        _refresh: refresh,
    };
    roots.shows(&roots.side_key);
    roots.shows(&roots.h.thread);
    assert!(roots.h.shared.is_root_thread(&roots.side_key));
    assert_eq!(roots.status(), record::Status::Idle, "the session rests");
    roots
}

impl Roots {
    /// The TUI draws a frame that shows `displayed`, at rest as far as the frame knows.
    fn shows(&self, displayed: &str) {
        self.h.shared.observe(
            &Frame {
                primary_thread: Some(&self.h.thread),
                displayed_thread: Some(displayed),
                rollout_path: None,
                cwd: self.h.test.cwd_path(),
                model: "gpt-5",
                codex_home: self.h.test.codex_home_path(),
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
    }

    /// The status the session's record says now, as aterm reads it: written by the TUI's
    /// writer and read back from its file.
    fn status(&self) -> record::Status {
        self.writer
            .write_now()
            .unwrap_or_else(|err| panic!("the record's write: {err}"));
        record::read(&self.writer.path())
            .unwrap_or_else(|| panic!("no record was written"))
            .status
    }

    /// Whether aterm would act on the record as it is: only at `idle` or `waiting`.
    fn record_admits(&self) -> bool {
        matches!(
            self.status(),
            record::Status::Idle | record::Status::Waiting
        )
    }

    /// The person asks in the side chat; the turn is left running.
    async fn asks_on_the_side(&self) {
        self.side
            .thread
            .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
                text: SIDE_ASKS.into(),
                text_elements: Vec::new(),
            }]))
            .await
            .unwrap_or_else(|err| panic!("the side chat's turn: {err}"));
    }

    fn side_runs(&self) -> bool {
        self.h.shared.turn_running_on(&self.side_key)
    }

    /// The usage errors the side chat's turn ended on.
    fn side_usage_errors(&self) -> usize {
        lock(&self.side_events)
            .iter()
            .filter(|event| {
                matches!(
                    event,
                    EventMsg::Error(error)
                        if error.codex_error_info == Some(CodexErrorInfo::UsageLimitExceeded)
                )
            })
            .count()
    }

    /// The side chat's turn ended on its own usage error.
    fn side_ended_at_the_wall(&self) -> bool {
        !self.side_runs() && self.side_usage_errors() == 1
    }

    /// Credit list reads since the side chat's request was refused: its recovery's looks
    /// for an earned reset.
    fn side_looks(&self) -> usize {
        self.account
            .lists
            .load(Ordering::SeqCst)
            .saturating_sub(self.account.lists_at_side_wall.load(Ordering::SeqCst))
    }

    /// THE SIDE CHAT'S TURN IS PARKED IN QUOTA RECOVERY: the model refused its request at
    /// the wall, its recovery looked for an earned reset and found none, and for `held` more
    /// the turn neither ends nor errors. The record says `busy` for it though the frame
    /// reads at rest, which is what aterm refuses a move under.
    async fn side_parks(&self, held: Duration) {
        wait_until("the side chat's request is refused at the wall", || {
            self.account.side_refused.load(Ordering::SeqCst) >= 1
        })
        .await;
        wait_until("its recovery looked for an earned reset", || {
            self.side_looks() >= 1
        })
        .await;
        let until = tokio::time::Instant::now() + held;
        loop {
            assert!(self.side_runs(), "the side chat's walled turn ended");
            assert_eq!(self.side_usage_errors(), 0);
            assert!(!self.record_admits(), "the record reads at rest");
            if tokio::time::Instant::now() >= until {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert_eq!(self.status(), record::Status::Busy);
        assert_eq!(
            self.account.side_refused.load(Ordering::SeqCst),
            1,
            "the walled request was sent again"
        );
    }

    /// The session rests again: no turn runs and nothing else holds it.
    async fn rests(&self) {
        wait_until("the session rests", || self.h.shared.unrest().is_none()).await;
        assert_eq!(self.status(), record::Status::Idle);
    }

    /// The primary thread's turn meets the same wall and ends on its usage error (the
    /// harness's primary thread is an exec-sourced one, whose turn never waits a wall out).
    async fn primary_walls(&self) {
        self.h
            .test
            .submit_turn(PRIMARY_ASKS)
            .await
            .unwrap_or_else(|err| panic!("the primary's walled turn: {err}"));
    }

    /// The exact ask the Autopilot sends for the primary's walled turn.
    fn prepare(&self) -> String {
        let turn = self
            .h
            .shared
            .latest_turn(&self.h.thread)
            .unwrap_or_else(|| panic!("the primary's walled turn has no id"));
        format!(
            "@{} {ROTATION} prepare thread={} turn={turn} source_account={} source_user={}",
            self.h.fake.sid(),
            self.h.thread,
            self.identity.account,
            self.identity.user
        )
    }

    /// The receipt a host would bind the primary's handoff to.
    fn receipt(&self) -> Receipt {
        let id = "1".repeat(32);
        let prepare = self.prepare();
        let fields = prepare
            .split_once(" prepare ")
            .map(|(_, origin)| {
                format!(
                    "id={id} {origin} target_account={} target_user={}",
                    "c".repeat(64),
                    "d".repeat(64)
                )
            })
            .unwrap_or_else(|| panic!("the ask names no origin: {prepare}"));
        let sid = self.h.fake.sid();
        Receipt {
            fields,
            status: format!("@{sid} {ROTATION} status id={id}"),
            commit: format!("@{sid} {ROTATION} commit id={id}"),
        }
    }

    /// The handoff requests the host was sent so far, held at its gate.
    fn asks(&self) -> usize {
        self.h.fake.gated_requests().len()
    }

    /// The next handoff request the host holds unanswered.
    async fn held(&self) -> String {
        let next = self.answered.load(Ordering::SeqCst);
        wait_until("the host is sent a handoff request", || self.asks() > next).await;
        self.h.fake.gated_requests()[next].clone()
    }

    /// The host answers the request it holds with `reply`, and holds the next one.
    fn answer(&self, request: &str, reply: &str) {
        self.h.fake.reply_once(request, reply);
        self.answered.fetch_add(1, Ordering::SeqCst);
        self.h.fake.release_held(ROTATION);
    }

    /// The host answers the ask it holds as aterm does: `waiting` (it has the ask, and no
    /// account yet) under a record at rest, refused under any other. Whether it was admitted.
    async fn answers_the_ask_as_aterm(&self) -> bool {
        let request = self.held().await;
        assert_eq!(
            request,
            self.prepare(),
            "the ask for the primary's walled turn"
        );
        let admitted = self.record_admits();
        self.answer(&request, if admitted { WAITING } else { WALL_CHANGED });
        admitted
    }

    fn intent_stands(&self) -> bool {
        self.h
            .shared
            .owed_act()
            .is_some_and(|owed| owed.thread == self.h.thread && owed.renewal.is_some())
    }
}

/// THE SIDE CHAT WALLED FIRST. Its turn is parked, and the primary thread idle: nothing
/// moves, and it keeps looking for an earned reset. Then the primary's own turn meets the
/// wall and asks for another account. aterm refuses that ask under the busy record; the
/// pass that asked keeps the point, the side turn's recovery is woken, looks once more for
/// an earned reset, and ends the turn on its usage error. The record reads `idle` and the
/// next ask is admitted. Before the rule the side turn stayed parked until the wall's
/// reset and every ask was refused.
#[test]
fn a_side_turn_parked_first_ends_when_the_primary_asks() {
    big_stack(|| async {
        let roots = roots().await;
        // The TUI shows the side chat, where the person asks.
        roots.shows(&roots.side_key);
        roots.asks_on_the_side().await;
        roots.side_parks(Duration::from_secs(2)).await;
        // With the primary idle no move is asked for, and none is on record to end it for.
        assert_eq!(roots.asks(), 0, "{:?}", roots.h.fake.requests());
        assert!(!roots.intent_stands());
        assert!(!journal_has(&roots.h, LEAVING));

        roots.primary_walls().await;
        // The ask is at the host, and the side turn still holds the record busy: aterm's
        // refusal, which was every answer the primary got until the wall's reset.
        let asked = roots.held().await;
        assert_eq!(asked, roots.prepare());
        assert!(roots.side_runs());
        assert_eq!(roots.status(), record::Status::Busy);
        let looks = roots.side_looks();
        assert!(
            !roots.answers_the_ask_as_aterm().await,
            "aterm admitted an ask under a busy record"
        );

        // THE RULE: the session is leaving this account, so the side turn ends.
        within(Duration::from_secs(15), "the side chat's turn ends", || {
            roots.side_ended_at_the_wall()
        })
        .await;
        assert!(journal_has(&roots.h, LEAVING));
        assert!(
            roots.side_looks() > looks,
            "it ended with no further look for an earned reset"
        );
        assert_eq!(
            roots.account.redemptions.load(Ordering::SeqCst),
            0,
            "there was no credit to redeem"
        );
        assert_eq!(roots.account.side_refused.load(Ordering::SeqCst), 1);
        roots.rests().await;
        assert!(roots.intent_stands(), "the primary's move stands");
        assert!(
            roots.answers_the_ask_as_aterm().await,
            "the next ask is refused though the record rests"
        );
    });
}

/// THE PRIMARY ASKED FIRST. Its move is live (aterm has the ask and answered `waiting`),
/// then the person asks in the side chat and that turn meets the same wall: it looks for an
/// earned reset, ends, and the primary's next ask is admitted. Before the rule it parked
/// there and held the record busy against every later ask, the `commit` and the restart
/// for the whole wall. This test cannot show the turn parked first, since under the rule it
/// does not park; with the rule taken out it fails on a turn that never ends.
#[test]
fn a_side_turn_walled_after_the_ask_does_not_park() {
    big_stack(|| async {
        let roots = roots().await;
        roots.primary_walls().await;
        assert!(
            roots.answers_the_ask_as_aterm().await,
            "the first ask, under a record at rest"
        );
        assert!(roots.intent_stands());

        roots.shows(&roots.side_key);
        roots.asks_on_the_side().await;
        within(Duration::from_secs(15), "the side chat's turn ends", || {
            roots.side_ended_at_the_wall()
        })
        .await;
        assert!(journal_has(&roots.h, LEAVING));
        assert!(
            roots.side_looks() >= 1,
            "it ended without looking for an earned reset"
        );
        assert_eq!(roots.account.side_refused.load(Ordering::SeqCst), 1);
        roots.rests().await;
        assert!(
            roots.answers_the_ask_as_aterm().await,
            "the ask after the side turn's end"
        );
    });
}

/// A SIDE TURN THAT STARTS UNDER A BOUND RECEIPT DOES NOT HOLD ITS COMMIT. The host
/// answered `ready`, the receipt is bound and `commit` is on its way when the person asks
/// in the side chat. aterm refuses a `commit` under a busy record (`receipt changed`);
/// the side turn ends for the move, and the `commit` sent again is taken.
#[test]
fn a_side_turn_started_under_a_bound_receipt_does_not_hold_the_commit() {
    big_stack(|| async {
        let roots = roots().await;
        roots.primary_walls().await;
        let receipt = roots.receipt();
        let asked = roots.held().await;
        assert_eq!(asked, roots.prepare());
        assert!(roots.record_admits());
        roots.answer(
            &asked,
            &format!("OK {ROTATION} state=ready {}\n", receipt.fields),
        );
        // `commit` is at the host, not yet judged, when the side chat's turn starts.
        assert_eq!(roots.held().await, receipt.commit);
        roots.shows(&roots.side_key);
        roots.asks_on_the_side().await;
        // It parks: the pass that sent `commit` has not come back to say it keeps the
        // point. The host's call deadline bounds how long this may be watched.
        roots.side_parks(Duration::from_millis(500)).await;
        // aterm judges the commit against the record: busy.
        roots.answer(&receipt.commit, RECEIPT_CHANGED);

        within(Duration::from_secs(15), "the side chat's turn ends", || {
            roots.side_ended_at_the_wall()
        })
        .await;
        assert!(journal_has(&roots.h, LEAVING));
        roots.rests().await;
        // The bound receipt is asked about again, read `ready` again and committed again;
        // aterm takes a commit under a record at rest.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
        loop {
            assert!(
                tokio::time::Instant::now() < deadline,
                "no commit followed the side turn's end: {:?}",
                roots.h.fake.requests()
            );
            let request = roots.held().await;
            if request == receipt.commit {
                assert!(
                    roots.record_admits(),
                    "the commit after the side turn's end met a busy record"
                );
                roots.answer(
                    &request,
                    &format!("OK {ROTATION} state=committed {}\n", receipt.fields),
                );
                break;
            }
            assert_eq!(request, receipt.status, "{:?}", roots.h.fake.requests());
            roots.answer(
                &request,
                &format!("OK {ROTATION} state=ready {}\n", receipt.fields),
            );
        }
    });
}

/// AN EARNED RESET FOUND AT THE WAKE IS REDEEMED, AND THE TURN GOES ON. The wake only has
/// the parked turn look again; the look comes before the question. A credit earned while
/// it waited is spent, the side chat's request is sent again and answered, and no usage
/// error ends the turn. The wake is what brings that look now: without it the credit is
/// found at the recovery's next poll, 30 seconds after its last.
#[test]
fn an_earned_reset_found_at_the_wake_is_redeemed_and_the_turn_goes_on() {
    big_stack(|| async {
        let roots = roots().await;
        roots.shows(&roots.side_key);
        roots.asks_on_the_side().await;
        roots.side_parks(Duration::from_secs(1)).await;
        let parked = tokio::time::Instant::now();
        roots.account.credit.store(true, Ordering::SeqCst);

        roots.primary_walls().await;
        // The host answers each ask as aterm does until the side chat's turn is over.
        let over = || !roots.side_runs();
        while !over() {
            assert!(
                parked.elapsed() < POLL - Duration::from_secs(10),
                "the credit was not redeemed before the recovery's own poll"
            );
            if roots.asks() > roots.answered.load(Ordering::SeqCst) {
                roots.answers_the_ask_as_aterm().await;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(roots.account.redemptions.load(Ordering::SeqCst), 1);
        wait_until("the side chat's request is answered", || {
            roots.account.side_answered.load(Ordering::SeqCst) == 1
        })
        .await;
        assert_eq!(
            roots.side_usage_errors(),
            0,
            "the turn ended on a usage error with a credit to redeem"
        );
        assert!(!journal_has(&roots.h, LEAVING));
        wait_until("the side chat's turn completes", || {
            lock(&roots.side_events)
                .iter()
                .any(|event| matches!(event, EventMsg::TurnComplete(_)))
        })
        .await;
    });
}

/// How the host stops the primary's ask while its intent is on record.
#[derive(Clone, Copy)]
enum Stops {
    /// Its policy cannot be read.
    Unread,
    /// Its policy reads, and no longer permits rotation.
    Refuses,
}

/// AN INTENT THAT ONLY LINGERS ON RECORD ENDS NO SIDE TURN. The primary's ask let its point
/// go, because the host `stops` it, and left its intent on record, as it does. No move is
/// being asked for, so a side turn that meets the wall waits it out and keeps its looks for
/// an earned reset; ending it would give those up for nothing. An Autopilot Off leaves the
/// intent there too, and ends nothing either. Once the host permits rotation again and
/// codex-alab acts, the ask goes out and keeps its point, and the turn that waited is woken
/// and ends.
async fn an_intent_that_lingers_ends_no_side_turn(stops: Stops) {
    let roots = roots().await;
    let sup = Arc::clone(roots.h.autopilot.supervisor());
    roots.primary_walls().await;
    assert!(
        roots.answers_the_ask_as_aterm().await,
        "the move is live: aterm has the ask"
    );

    // The next pass of the ask lets the point go, and the wall's own decision (its wait
    // for the reset) takes it. The intent stays.
    match stops {
        Stops::Unread => roots
            .h
            .fake
            .answer_policy_with(Some("ERR harness policy unavailable")),
        Stops::Refuses => roots.h.fake.set_policy_key("codex_rotation", "0"),
    }
    wait_until("the ask lets its point go", || journal_has(&roots.h, WAITS)).await;
    assert!(roots.intent_stands(), "the intent stays on record");
    assert!(sup.acting(), "codex-alab still acts under its claim");
    let asks = roots.asks();

    roots.shows(&roots.side_key);
    roots.asks_on_the_side().await;
    roots.side_parks(Duration::from_secs(3)).await;
    assert!(roots.intent_stands() && sup.acting());
    assert_eq!(roots.asks(), asks, "a host that stops the ask was asked");
    assert!(!journal_has(&roots.h, LEAVING));

    // The host permits rotation again. Nothing looks at the primary's point for that (its
    // wall waits for its reset), so the intent still only lingers.
    match stops {
        Stops::Unread => roots.h.fake.answer_policy_with(None),
        Stops::Refuses => roots.h.fake.set_policy_key("codex_rotation", "1"),
    }
    roots.side_parks(Duration::from_secs(1)).await;
    // AUTOPILOT OFF: the intent is still there, and nobody asks for the move.
    roots.h.fake.set_status("autopilot", "off");
    roots.h.fake.set_effective(false, "autopilot-off");
    wait_until("the claim is given back", || {
        !sup.held() && roots.h.fake.claim().is_none()
    })
    .await;
    assert!(
        roots.intent_stands(),
        "the intent stays on record through an Off"
    );
    roots.side_parks(Duration::from_secs(1)).await;
    assert_eq!(roots.asks(), asks);

    // ACTING AGAIN under a host that permits rotation: the ask goes out, refused under the
    // busy record, its pass keeps the point, and the side turn ends for the live move.
    roots.h.fake.set_status("autopilot", "cruise");
    roots.h.fake.set_effective(true, "enabled");
    wait_until("the claim is taken again", || sup.acting()).await;
    assert!(
        !roots.answers_the_ask_as_aterm().await,
        "aterm admitted an ask under a busy record"
    );
    within(Duration::from_secs(15), "the side chat's turn ends", || {
        roots.side_ended_at_the_wall()
    })
    .await;
    assert!(journal_has(&roots.h, LEAVING));
    roots.rests().await;
}

#[test]
fn an_intent_left_by_a_host_that_cannot_be_read_ends_no_side_turn() {
    big_stack(|| an_intent_that_lingers_ends_no_side_turn(Stops::Unread));
}

#[test]
fn an_intent_left_by_a_host_that_stopped_permitting_ends_no_side_turn() {
    big_stack(|| an_intent_that_lingers_ends_no_side_turn(Stops::Refuses));
}

/// A PASS THAT KEEPS ITS POINT WITHOUT ASKING ENDS NO SIDE TURN. The primary's move was
/// live: aterm had the ask. Then `status` shows a person's hold while the policy still
/// permits rotation. Every later pass of the ask reads that policy, meets the hold at its
/// checks before an act, keeps its point and sends the host nothing, for as long as the
/// hold stands; a draft, a popup or a person at the terminal keep it the same way. The
/// intent is still on record and codex-alab still acts under its claim, but nobody is
/// asking for the move: a side turn that meets the wall keeps its looks for an earned
/// reset. When the hold lifts the ask goes out again, and the turn that waited is woken and
/// ends. The first build of the rule took any kept point for a live move, and ended this
/// side turn as it met the wall (measured in review: 51 ms after it began). A hold aterm
/// also folds into its policy (`effective=off why=hold`) has the pass let its point go
/// instead, as a host that stopped permitting does
/// (`an_intent_left_by_a_host_that_stopped_permitting_ends_no_side_turn`).
#[test]
fn an_ask_a_persons_hold_stops_ends_no_side_turn() {
    big_stack(|| async {
        let roots = roots().await;
        let sup = Arc::clone(roots.h.autopilot.supervisor());
        let primary = roots
            .h
            .autopilot
            .thread_rt(&roots.h.thread)
            .unwrap_or_else(|| panic!("the primary thread is not the autopilot's"));
        roots.primary_walls().await;
        assert!(
            roots.answers_the_ask_as_aterm().await,
            "the move is live: aterm has the ask"
        );
        within(
            Duration::from_secs(15),
            "the pass that asked leaves the move live",
            || primary.move_live(),
        )
        .await;

        // THE HOLD: `status hold=1`, and nothing else. The next pass of the ask, within its
        // 5 s recheck, meets it and asks nothing.
        roots.h.fake.set_status("hold", "1");
        within(
            Duration::from_secs(15),
            "a pass that meets the hold leaves the move not live",
            || !primary.move_live(),
        )
        .await;
        assert_eq!(
            roots.asks(),
            1,
            "an ask went out under a person's hold: {:?}",
            roots.h.fake.requests()
        );
        assert!(roots.intent_stands(), "the intent stays on record");
        assert!(sup.acting(), "codex-alab still acts under its claim");

        roots.shows(&roots.side_key);
        roots.asks_on_the_side().await;
        roots.side_parks(Duration::from_secs(3)).await;
        assert!(roots.intent_stands() && sup.acting());
        assert_eq!(roots.asks(), 1, "the host was asked under the hold");
        assert!(!journal_has(&roots.h, LEAVING));

        // THE HOLD LIFTS, with the hold's own event: the policy read it brings has every
        // thread look again, the ask goes out, refused under the busy record, and the pass
        // that sent it wakes the turn that was told to wait.
        roots.h.fake.set_status("hold", "0");
        roots.h.fake.emit("hold 0 reason=owner origin=cli");
        assert!(
            !roots.answers_the_ask_as_aterm().await,
            "aterm admitted an ask under a busy record"
        );
        within(Duration::from_secs(15), "the side chat's turn ends", || {
            roots.side_ended_at_the_wall()
        })
        .await;
        assert!(journal_has(&roots.h, LEAVING));
        roots.rests().await;
        assert!(
            roots.answers_the_ask_as_aterm().await,
            "the ask after the side turn's end"
        );
    });
}

/// A TURN TOLD TO WAIT IS WOKEN BY THE NEXT ASK, WHATEVER MADE THE ANSWER YES. The primary's
/// move was live when the person turned Autopilot off. A side turn that meets the wall then
/// is told no (codex-alab does not act) and waits. Autopilot on again: the primary asks
/// again, and the pass that asks wakes the waiting turn. The first build woke a turn only
/// when the primary's mark turned from no to yes; the mark the last ask left before the Off
/// was still set, so no wake came, and the turn waited out its own 30 s poll while aterm
/// refused every ask under the busy record (measured in review: 28 s, six asks refused).
#[test]
fn a_side_turn_that_waited_through_an_off_is_woken_by_the_next_ask() {
    big_stack(|| async {
        let roots = roots().await;
        let sup = Arc::clone(roots.h.autopilot.supervisor());
        roots.primary_walls().await;
        assert!(
            roots.answers_the_ask_as_aterm().await,
            "the move is live: aterm has the ask"
        );

        // AUTOPILOT OFF: the intent stays, and nobody asks for the move.
        roots.h.fake.set_status("autopilot", "off");
        roots.h.fake.set_effective(false, "autopilot-off");
        wait_until("the claim is given back", || {
            !sup.held() && roots.h.fake.claim().is_none()
        })
        .await;
        assert!(roots.intent_stands());
        let asks = roots.asks();
        assert_eq!(asks, roots.answered.load(Ordering::SeqCst));

        roots.shows(&roots.side_key);
        roots.asks_on_the_side().await;
        let parked = tokio::time::Instant::now();
        roots.side_parks(Duration::from_secs(2)).await;
        assert_eq!(roots.asks(), asks, "a host was asked under an Off");
        assert!(!journal_has(&roots.h, LEAVING));

        // ACTING AGAIN. The host answers each ask as aterm does (refused, under the busy
        // record) until the side chat's turn is over, which must be well before that
        // turn's own poll.
        roots.h.fake.set_status("autopilot", "cruise");
        roots.h.fake.set_effective(true, "enabled");
        wait_until("the claim is taken again", || sup.acting()).await;
        let mut refused = 0;
        while roots.side_runs() {
            assert!(
                parked.elapsed() < POLL - Duration::from_secs(10),
                "the side turn was not woken: {refused} asks refused under its busy record"
            );
            if roots.asks() > roots.answered.load(Ordering::SeqCst)
                && !roots.answers_the_ask_as_aterm().await
            {
                refused += 1;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(roots.side_ended_at_the_wall());
        assert!(journal_has(&roots.h, LEAVING));
        assert!(refused >= 1, "the turn ended before the move was asked for");
        roots.rests().await;
    });
}

/// A TURN WHOSE LOOKS ENDED BEFORE THEY ASKED IS WOKEN BY THE NEXT ASK. The side chat meets
/// the wall while the credit list cannot be read. One unread list may hide an earned reset,
/// so a look that finds it unread ends before the handoff question (the host is asked only
/// at a look a whole poll after the run's first unread list): so does the side turn's first
/// look, and the one the primary's intent brings when the primary meets the wall. Then the
/// list reads again, and the primary's ask, refused under the busy record, leaves the move
/// live. The build before left word that a turn waits only at the question, so that pass
/// woke nobody, and the turn waited out its own 30 s poll while aterm refused every ask
/// (measured in review: 30.0 s after the list came back, six asks refused; 1.05 s and one
/// under the wake on the mark's flip before it). Every look leaves that word now.
#[test]
fn a_side_turn_whose_looks_ended_before_they_asked_is_woken_by_the_next_ask() {
    big_stack(|| async {
        let roots = roots().await;
        roots.account.list_down.store(true, Ordering::SeqCst);
        roots.shows(&roots.side_key);
        roots.asks_on_the_side().await;
        roots.side_parks(Duration::from_secs(2)).await;
        assert_eq!(
            roots.side_looks(),
            1,
            "the side turn looked again before the primary met the wall"
        );

        // The primary meets the wall. Its intent's arrival brings the side turn's second
        // look, the list unread again a few seconds after the first; its ask is at the host.
        roots.primary_walls().await;
        assert_eq!(roots.held().await, roots.prepare());
        within(
            Duration::from_secs(15),
            "the intent's arrival brings a look",
            || roots.side_looks() >= 2,
        )
        .await;
        assert!(roots.side_runs());
        assert!(!journal_has(&roots.h, LEAVING));

        // THE LIST READS AGAIN, and nothing tells the waiting turn so. The host answers
        // each ask as aterm does (refused, under the busy record) until the side chat's
        // turn is over, which must be well before that turn's own poll.
        roots.account.list_down.store(false, Ordering::SeqCst);
        let back = tokio::time::Instant::now();
        assert_eq!(
            roots.side_looks(),
            2,
            "a look came before the list read again"
        );
        let mut refused = 0;
        while roots.side_runs() {
            assert!(
                back.elapsed() < POLL - Duration::from_secs(10),
                "the side turn was not woken: {refused} asks refused under its busy record"
            );
            if roots.asks() > roots.answered.load(Ordering::SeqCst)
                && !roots.answers_the_ask_as_aterm().await
            {
                refused += 1;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(roots.side_ended_at_the_wall());
        assert!(journal_has(&roots.h, LEAVING));
        assert!(refused >= 1, "the turn ended before the move was asked for");
        assert!(
            roots.side_looks() >= 3,
            "it ended with no look at the list that reads again"
        );
        assert_eq!(roots.account.redemptions.load(Ordering::SeqCst), 0);
        roots.rests().await;
    });
}

/// A BOUND RECEIPT UNDER A HOST THAT CANNOT BE READ ENDS A SIDE TURN. The host answered
/// `ready`, the receipt is bound and its `commit` was taken: the host is about to restart
/// the session on the other account, and does so only under a record at rest. Then its
/// policy cannot be read. The pass keeps the point (one failed read proves nothing about a
/// handoff whose `commit` went) and sends the host nothing, and the move is live all the
/// same: the receipt the host granted stands. A side turn that meets the wall then must not
/// hold the record busy against the restart: it ends. An intent no receipt binds is the
/// other case, and ends nothing under a host that cannot be read
/// (`an_intent_left_by_a_host_that_cannot_be_read_ends_no_side_turn`).
#[test]
fn a_bound_receipt_under_a_host_that_cannot_be_read_ends_a_side_turn() {
    big_stack(|| async {
        let roots = roots().await;
        roots.primary_walls().await;
        let receipt = roots.receipt();
        let asked = roots.held().await;
        assert_eq!(asked, roots.prepare());
        roots.answer(
            &asked,
            &format!("OK {ROTATION} state=ready {}\n", receipt.fields),
        );
        assert_eq!(roots.held().await, receipt.commit);
        // The host's policy cannot be read from here on; the `commit` it holds is taken.
        roots
            .h
            .fake
            .answer_policy_with(Some("ERR harness policy unavailable"));
        assert!(roots.record_admits());
        roots.answer(
            &receipt.commit,
            &format!("OK {ROTATION} state=committed {}\n", receipt.fields),
        );
        wait_until(
            "the pass keeps the bound point under the unread host",
            || {
                journal_has(
                    &roots.h,
                    "the host cannot be read while a bound handoff stands",
                )
            },
        )
        .await;
        let asks = roots.asks();

        roots.shows(&roots.side_key);
        roots.asks_on_the_side().await;
        within(Duration::from_secs(10), "the side chat's turn ends", || {
            roots.side_ended_at_the_wall()
        })
        .await;
        assert!(journal_has(&roots.h, LEAVING));
        assert!(
            roots.side_looks() >= 1,
            "it ended without looking for an earned reset"
        );
        assert_eq!(roots.account.side_refused.load(Ordering::SeqCst), 1);
        assert_eq!(
            roots.asks(),
            asks,
            "a host that cannot be read was asked: {:?}",
            roots.h.fake.requests()
        );
        // The record the host's restart reads is at rest.
        roots.rests().await;
    });
}
