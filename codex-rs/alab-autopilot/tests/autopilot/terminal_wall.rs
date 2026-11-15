// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A RUNNING BACKGROUND TERMINAL HOLDS THE USAGE WALL'S MOVE UNTIL IT ENDS (gap 1 of the audit
//! of 2026-11-14, as its review amended it). The move's restart ends every unified-exec
//! terminal (a dev server, a watch) the session runs, in any of its threads. aterm's rest fence
//! sees only the ones the TUI's footer counts, the shown thread's (the frame `working`, the
//! record `busy`). Before the rule a walled turn ended for a move every ask of which was
//! refused (`wall changed`) until the wall's reset, giving up its look for an earned reset;
//! and where the terminal ran in a thread the TUI did not show, the record could rest and the
//! move end it silently.
//!
//! The rule: while ANY of the session's background terminals run (core's own count, over the
//! threads the thread manager lists), the wall's handoff is not ready on any root thread and
//! the ask is not sent, so each walled turn keeps looking for an earned reset at its own
//! cadence, and, where the host's pool shows another account to move to, the person is told
//! once a wall, in the history of every root thread (whichever root thread is on screen shows
//! it; a sub-agent's is not told), why the move waits. Once the terminals end, the next look
//! finds the handoff ready, the turn ends on its usage error, and the move is asked for. A
//! `wall changed` refusal the ask meets is journaled once per ask series instead of being
//! dropped.
//!
//! Against alab-aterm's in-process fake aterm, a scratch ChatGPT login codex's own login code
//! wrote, a loopback model and a loopback usage backend. The terminals are real: the model has
//! the walled turn start `sleep` through `exec_command`, and the test ends it through core
//! (`terminate_background_terminal`). The primary and a side chat are started `Cli`-sourced,
//! as the TUI starts its own, so their walled requests really wait in quota recovery and ask
//! the handoff. The fake shows a pool with another account that can run, and its
//! `codex-rotation` requests are held at its gate and answered by the test, as aterm would from
//! the record `codex_alab_tui`'s writer wrote where it says so.

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
/// What the person asks on the primary thread: a turn that starts a dev server and then meets
/// the wall, and one that meets it with none.
const SERVES: &str = "start the dev server and do the task";
const ASKS: &str = "do the task";
/// What the person asks in the side chat: a turn that starts a dev server and then meets the
/// wall, and one that starts a dev server and ends with it running, meeting no wall.
const SIDE_SERVES: &str = "start the dev server on the side";
const SIDE_STARTS: &str = "start the dev server on the side and leave it running";
/// The `exec_command` calls that start each dev server.
const PRIMARY_CALL: &str = "primary-dev-server";
const SIDE_CALL: &str = "side-dev-server";
const SIDE_START_CALL: &str = "side-dev-server-left-running";
/// aterm's answers to an ask it judges against the session's record.
const WAITING: &str = "OK codex-rotation state=waiting\n";
const WALL_CHANGED: &str = "ERR codex-rotation wall changed\n";
/// The person's line for the held move, word for word.
const MOVE_WAITS: &str = "The usage limit is reached, and another signed-in account has room. \
                          Background terminals run in this session, and the restart would end \
                          them, so the move waits until they finish or you stop them.";
/// The journal's line for the held move, for a refused ask, and for a side turn ended for the
/// move.
const TERMINALS_HOLD: &str = "waits until the session's background terminals stop";
const REFUSED: &str = "the host refused the account handoff (wall changed)";
const LEAVING: &str = "its walled turn ends; the session is leaving this account";
/// How long quota recovery waits between two looks with no word from its host.
const POLL: Duration = Duration::from_secs(30);

/// The login's account as its usage backend reports it.
#[derive(Default)]
struct Account {
    /// The model refused a request at the usage wall: the week reads spent from here on.
    walled: AtomicBool,
    /// The primary's model requests refused at the wall, and the side chat's.
    refused: AtomicUsize,
    side_refused: AtomicUsize,
    /// Credit list reads so far, and how many there were when the side chat's request was
    /// refused: a read after that is a look for an earned reset (the side's, once the
    /// primary's turn has ended).
    lists: AtomicUsize,
    lists_at_side_wall: AtomicUsize,
}

/// A session on one ChatGPT login, beside another signed-in account that has room: its
/// primary thread is the TUI's own, and a side chat is another root thread of the TUI's.
struct Session {
    h: Harness,
    account: Arc<Account>,
    /// The record's writer over the session's shared state, as the TUI installs it.
    writer: &'static Writer,
    primary: codex_core::NewThread,
    key: String,
    /// Every event of the primary thread so far.
    events: Arc<Mutex<Vec<EventMsg>>>,
    side: codex_core::NewThread,
    side_key: String,
    side_events: Arc<Mutex<Vec<EventMsg>>>,
    /// The login's hashed identity, as the ask names it.
    identity: codex_alab_aterm::renewal::Identity,
    /// How many requests held at the fake's gate were answered.
    answered: AtomicUsize,
    _backend: wiremock::MockServer,
    _refresh: wiremock::MockServer,
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

/// One row of aterm's `codex-accounts` snapshot, as the wire carries it.
fn row(id: &str, condition: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id, "label": id, "masked_email": null, "condition": condition,
        "enrolled": true, "held": false, "eligible": false,
        "quota": {"stale": true, "observed_at": null, "five_hour_used_percent": null,
            "five_hour_reset_at": null, "weekly_used_percent": null, "weekly_reset_at": null},
    })
}

/// The model's reply that starts a dev server: `exec_command` runs `sleep` past the test, and
/// yields while it runs, so core keeps it as a background terminal.
fn serve(call: &str) -> wiremock::ResponseTemplate {
    let start = serde_json::json!({"cmd": "sleep 300", "yield_time_ms": 250});
    responses::sse_response(responses::sse(vec![
        responses::ev_response_created(call),
        responses::ev_function_call(call, "exec_command", &start.to_string()),
        responses::ev_completed(call),
    ]))
}

/// The model's reply that ends a turn with a word and no tool call.
fn finish(call: &str) -> wiremock::ResponseTemplate {
    responses::sse_response(responses::sse(vec![
        responses::ev_response_created(call),
        responses::ev_assistant_message(call, "The dev server runs."),
        responses::ev_completed(call),
    ]))
}

/// This process's kernel birth, as aterm binds an account snapshot to its asker.
fn birth() -> String {
    codex_alab_tui::identity::current()
        .ok()
        .and_then(|identity| codex_alab_tui::identity::proc_start_utc(&identity))
        .unwrap_or_else(|| panic!("this process has no readable birth"))
}

/// A root thread of the TUI's own, which the autopilot keeps, with every event it sends.
async fn root_thread(h: &Harness) -> (codex_core::NewThread, String, Arc<Mutex<Vec<EventMsg>>>) {
    let thread = h
        .test
        .thread_manager
        .start_thread(codex_core::StartThreadOptions {
            session_source: Some(SessionSource::Cli),
            ..h.test.start_thread_options().await
        })
        .await
        .unwrap_or_else(|err| panic!("a root thread: {err}"));
    let key = thread.thread_id.to_string();
    wait_until("the root thread is the autopilot's", || {
        h.autopilot.thread_rt(&key).is_some()
    })
    .await;
    let events = Arc::new(Mutex::new(Vec::new()));
    let (seen, codex) = (Arc::clone(&events), Arc::clone(&thread.thread));
    tokio::spawn(async move {
        while let Ok(event) = codex.next_event().await {
            lock(&seen).push(event.msg);
        }
    });
    (thread, key, events)
}

async fn session() -> Session {
    let (login, identity, refresh) = subscription_fixture("terminal-wall").await;
    // The week's reset, inside the day an earned reset may be redeemed for.
    let reset = i64::try_from(now_ms() / 1000).unwrap_or(i64::MAX) + 13 * 3600;
    let account = Arc::new(Account::default());
    let backend = wiremock::MockServer::start().await;
    let read = Arc::clone(&account);
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/api/codex/usage"))
        .respond_with(move |_: &wiremock::Request| {
            let spent = read.walled.load(Ordering::SeqCst);
            wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "account_id": "terminal-wall-account", "user_id": "terminal-wall-user",
                "plan_type": "pro",
                "rate_limit": {"allowed": !spent, "limit_reached": spent,
                    "secondary_window": {"used_percent": if spent { 100 } else { 0 },
                        "limit_window_seconds": 604_800, "reset_at": reset,
                        "reset_after_seconds": 46_800}},
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
            listed.lists.fetch_add(1, Ordering::SeqCst);
            wiremock::ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"available_count": 0, "credits": []}))
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
    // The model: a turn that serves starts its dev server, and every request after that, and
    // every request of a turn that does not serve, stands at the spent week.
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
            SERVES if !request.has_function_call(PRIMARY_CALL) => serve(PRIMARY_CALL),
            SERVES | ASKS => {
                model.refused.fetch_add(1, Ordering::SeqCst);
                refused()
            }
            SIDE_SERVES if !request.has_function_call(SIDE_CALL) => serve(SIDE_CALL),
            SIDE_SERVES => {
                if model.side_refused.fetch_add(1, Ordering::SeqCst) == 0 {
                    model
                        .lists_at_side_wall
                        .store(model.lists.load(Ordering::SeqCst), Ordering::SeqCst);
                }
                refused()
            }
            SIDE_STARTS if !request.has_function_call(SIDE_START_CALL) => serve(SIDE_START_CALL),
            SIDE_STARTS => finish(SIDE_START_CALL),
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
    // Another signed-in account can run: the move would follow the walled turn's end.
    h.fake.set_account_pool(Some((
        &birth(),
        None,
        serde_json::json!([row("a", "usable"), row("b", "unseen")]),
    )));
    // Every request of the handoff protocol waits for the test's answer.
    h.fake.gate(ROTATION);

    let (primary, key, events) = root_thread(&h).await;
    let (side, side_key, side_events) = root_thread(&h).await;
    let session = Session {
        h,
        account,
        writer,
        primary,
        key,
        events,
        side,
        side_key,
        side_events,
        identity,
        answered: AtomicUsize::new(0),
        _backend: backend,
        _refresh: refresh,
    };
    // The TUI registers each at the frame that first shows it, and then shows its primary.
    session.shows(&session.side_key);
    session.shows(&session.key);
    assert_eq!(
        session.h.shared.view().primary_thread.as_deref(),
        Some(session.key.as_str())
    );
    assert!(session.h.shared.is_root_thread(&session.side_key));
    assert_eq!(session.status(), record::Status::Idle, "the session rests");
    session
}

impl Session {
    /// The TUI draws a frame that shows `displayed`, at rest as far as the frame knows. No
    /// footer count is handed unless a test hands one.
    fn shows(&self, displayed: &str) {
        self.h.shared.observe(
            &Frame {
                primary_thread: Some(&self.key),
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

    /// The person asks `text` on `thread`; the turn is left running.
    async fn asks(&self, thread: &codex_core::NewThread, text: &str) {
        thread
            .thread
            .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
                text: text.into(),
                text_elements: Vec::new(),
            }]))
            .await
            .unwrap_or_else(|err| panic!("the person's turn: {err}"));
    }

    /// The dev server `thread`'s turn started runs in core: its process id.
    async fn serving(&self, thread: &codex_core::NewThread) -> i32 {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
        loop {
            if let [terminal] = thread.thread.list_background_terminals().await.as_slice() {
                return terminal
                    .process_id
                    .parse()
                    .unwrap_or_else(|err| panic!("a process id: {err}"));
            }
            if tokio::time::Instant::now() >= deadline {
                let requests = responses::received_responses_requests(&self.h._server).await;
                let outputs: Vec<_> = requests
                    .iter()
                    .flat_map(|request| request.inputs_of_type("function_call_output"))
                    .collect();
                let primary_events = lock(&self.events).clone();
                let side_events = lock(&self.side_events).clone();
                panic!(
                    "never: the dev server runs on {}\nprimary events ({}): {primary_events:#?}\nside events ({}): {side_events:#?}\nactual provider tool outputs: {outputs:#?}",
                    thread.thread_id, self.key, self.side_key
                );
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// The person stops `thread`'s dev server.
    async fn stops(&self, thread: &codex_core::NewThread, process: i32) {
        assert!(
            thread.thread.terminate_background_terminal(process).await,
            "the dev server was not running"
        );
        let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
        while !thread.thread.list_background_terminals().await.is_empty() {
            assert!(
                tokio::time::Instant::now() < deadline,
                "never: the dev server ends"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    fn runs(&self, key: &str) -> bool {
        self.h.shared.turn_running_on(key)
    }

    /// The host's account pool shows this process `rows`, naming no row as the session's.
    fn pool(&self, rows: serde_json::Value) {
        self.h.fake.set_account_pool(Some((&birth(), None, rows)));
    }

    /// The usage errors a thread's turns ended on.
    fn usage_errors(events: &Mutex<Vec<EventMsg>>) -> usize {
        lock(events)
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

    /// The primary's turn ended on its own usage error.
    fn ended_at_the_wall(&self) -> bool {
        !self.runs(&self.key) && Self::usage_errors(&self.events) == 1
    }

    /// The side chat's turn ended on its own usage error.
    fn side_ended_at_the_wall(&self) -> bool {
        !self.runs(&self.side_key) && Self::usage_errors(&self.side_events) == 1
    }

    fn lists(&self) -> usize {
        self.account.lists.load(Ordering::SeqCst)
    }

    /// Credit list reads since the side chat's request was refused.
    fn side_looks(&self) -> usize {
        self.lists()
            .saturating_sub(self.account.lists_at_side_wall.load(Ordering::SeqCst))
    }

    /// The handoff requests the host was sent so far, held at its gate.
    fn asked(&self) -> usize {
        self.h.fake.gated_requests().len()
    }

    /// The next handoff request the host holds unanswered.
    async fn held(&self) -> String {
        let next = self.answered.load(Ordering::SeqCst);
        wait_until("the host is sent a handoff request", || self.asked() > next).await;
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

    /// The exact ask the Autopilot sends for the primary's walled turn.
    fn prepare(&self) -> String {
        let turn = self
            .h
            .shared
            .latest_turn(&self.key)
            .unwrap_or_else(|| panic!("the walled turn has no id"));
        format!(
            "@{} {ROTATION} prepare thread={} turn={turn} source_account={} source_user={}",
            self.h.fake.sid(),
            self.key,
            self.identity.account,
            self.identity.user
        )
    }

    fn journal_count(&self, needle: &str) -> usize {
        self.h
            .autopilot
            .supervisor()
            .journal()
            .iter()
            .filter(|line| line.contains(needle))
            .count()
    }

    /// The threads the person was told in so far, each once.
    fn told_in(&self) -> Vec<String> {
        let said = self.h.warnings.said();
        assert!(
            said.iter()
                .all(|warning| warning.message == MOVE_WAITS && warning.turn_id.is_none()),
            "{said:?}"
        );
        let mut threads: Vec<String> = said.into_iter().map(|warning| warning.thread_id).collect();
        threads.sort();
        threads
    }

    /// Every root thread of the session, as [`Self::told_in`] lists them: the primary, the
    /// side chat, and the harness's own first thread, which the TUI showed first and the
    /// autopilot keeps. Whichever the person views shows the line.
    fn roots(&self) -> Vec<String> {
        let mut roots = vec![
            self.h.thread.clone(),
            self.key.clone(),
            self.side_key.clone(),
        ];
        roots.sort();
        roots
    }

    /// For `held`, the primary's walled turn neither ends nor errors, nothing is asked of the
    /// host, and the record does not rest.
    async fn primary_parks(&self, held: Duration) {
        let until = tokio::time::Instant::now() + held;
        while tokio::time::Instant::now() < until {
            assert!(
                self.runs(&self.key),
                "the walled turn ended while the terminal ran"
            );
            assert_eq!(Self::usage_errors(&self.events), 0);
            assert_eq!(self.asked(), 0, "{:?}", self.h.fake.requests());
            assert!(
                !self.record_admits(),
                "the record rests under a walled turn"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

/// THE PRIMARY'S TERMINAL RUNS AT THE WALL, IN A SESSION WHOSE FRAMES HAND NO FOOTER COUNT (a
/// host that draws none: the count the hold read before the review, which held nothing here).
/// The walled turn looks for an earned reset, finds none, and does not end: the handoff is not
/// ready while core counts the terminal, nothing is asked of the host, and the person is told
/// once, in each root thread, why the move waits. The person stops the dev server; the turn's
/// next look, at its own cadence, ends it on its usage error, and the move is asked for. aterm
/// refuses the first two asks `wall changed` (the record left rest between the turn's end and
/// the ask: its return to rest is written coalesced, CONTRACT §4); the journal says so once,
/// and the point is kept. The ask after that, under a record at rest, is granted and committed.
#[test]
fn a_background_terminal_holds_the_move_until_it_ends() {
    big_stack(|| async {
        let s = session().await;
        assert_eq!(s.h.shared.background_terminals(), None, "a footer count");

        s.asks(&s.primary, SERVES).await;
        let server = s.serving(&s.primary).await;
        wait_until("the request is refused at the wall", || {
            s.account.refused.load(Ordering::SeqCst) >= 1
        })
        .await;
        wait_until("its recovery looked for an earned reset", || s.lists() >= 1).await;
        // THE RULE: the turn keeps waiting while the terminal runs, and nothing is asked.
        s.primary_parks(Duration::from_secs(2)).await;
        wait_until("the person is told why the move waits", || {
            !s.h.warnings.said().is_empty()
        })
        .await;
        let looks = s.lists();
        assert_eq!(s.told_in(), s.roots());
        assert_eq!(s.journal_count(TERMINALS_HOLD), 1);
        assert_eq!(s.account.refused.load(Ordering::SeqCst), 1);

        // The dev server ends. The turn's next look, within its own poll, ends it.
        s.stops(&s.primary, server).await;
        within(
            POLL + Duration::from_secs(15),
            "the walled turn ends",
            || s.ended_at_the_wall(),
        )
        .await;
        assert!(s.lists() > looks, "it ended with no further look");
        assert_eq!(s.told_in(), s.roots(), "told more than once");
        assert_eq!(s.journal_count(TERMINALS_HOLD), 1);

        // The ask, refused twice: journaled once, and the point kept for the next ask.
        for _ in 0..2 {
            let asked = s.held().await;
            assert_eq!(asked, s.prepare());
            s.answer(&asked, WALL_CHANGED);
        }
        let asked = s.held().await;
        assert_eq!(asked, s.prepare());
        assert_eq!(
            s.journal_count(REFUSED),
            1,
            "{:#?}",
            s.h.autopilot.supervisor().journal()
        );

        // Under a record at rest the host grants the move and its commit.
        wait_until("the session rests", || s.h.shared.unrest().is_none()).await;
        assert!(s.record_admits(), "the record does not rest");
        let id = "1".repeat(32);
        let origin = asked
            .split_once(" prepare ")
            .map(|(_, origin)| origin.to_owned())
            .unwrap_or_else(|| panic!("the ask names no origin: {asked}"));
        let fields = format!(
            "id={id} {origin} target_account={} target_user={}",
            "c".repeat(64),
            "d".repeat(64)
        );
        s.answer(&asked, &format!("OK {ROTATION} state=ready {fields}\n"));
        let commit = format!("@{} {ROTATION} commit id={id}", s.h.fake.sid());
        assert_eq!(s.held().await, commit);
        assert!(
            s.record_admits(),
            "the commit met a record that does not rest"
        );
        s.answer(
            &commit,
            &format!("OK {ROTATION} state=committed {fields}\n"),
        );
    });
}

/// THE PRIMARY'S TERMINAL RUNS WHILE THE PERSON VIEWS THE SIDE CHAT, whose footer counts none
/// (the count the hold read before the review, which held nothing here, and the turn's end
/// would have left the record at rest for a move that ends the server). The walled turn waits
/// all the same, and the person is told in the side chat they view as well as in the primary.
/// Once the dev server ends the turn ends at its next look, and the ask, under a record at
/// rest, is admitted.
#[test]
fn the_primarys_terminal_holds_the_move_while_another_thread_is_shown() {
    big_stack(|| async {
        let s = session().await;
        s.shows(&s.side_key);
        s.h.shared
            .set_background_terminals(0, std::time::Instant::now());

        s.asks(&s.primary, SERVES).await;
        let server = s.serving(&s.primary).await;
        wait_until("the request is refused at the wall", || {
            s.account.refused.load(Ordering::SeqCst) >= 1
        })
        .await;
        wait_until("its recovery looked for an earned reset", || s.lists() >= 1).await;
        s.primary_parks(Duration::from_secs(2)).await;
        wait_until("the person is told why the move waits", || {
            !s.h.warnings.said().is_empty()
        })
        .await;
        assert!(
            s.told_in().contains(&s.side_key),
            "the thread on screen was not told: {:?}",
            s.h.warnings.said()
        );
        assert_eq!(s.told_in(), s.roots());
        assert_eq!(s.journal_count(TERMINALS_HOLD), 1);

        s.stops(&s.primary, server).await;
        within(
            POLL + Duration::from_secs(15),
            "the walled turn ends",
            || s.ended_at_the_wall(),
        )
        .await;
        assert_eq!(s.told_in(), s.roots(), "told more than once");
        wait_until("the session rests", || s.h.shared.unrest().is_none()).await;
        assert!(
            s.answers_the_ask_as_aterm().await,
            "the ask after the dev server ended was refused"
        );
    });
}

/// THE REVIEW'S SCENARIO: THE SIDE CHAT STARTS A DEV SERVER AFTER THE PRIMARY ASKED. The
/// primary walls with no terminal running, its turn ends, and the move is asked for; aterm
/// refuses the ask `wall changed`, since the side chat is working. That turn starts a dev
/// server and its next request meets the same wall. Before the review its handoff said ready
/// (the move was live), so it gave up its look for an earned reset, every ask was refused while
/// the server ran, and the person was told nothing. Now its handoff says no while the server
/// runs, the person is told once (in each root thread), and the primary's passes ask the host
/// nothing more: an ask under a record at rest would end the server. The person stops it; the
/// next pass asks, the side turn's first look after that ends it on its usage error, and the
/// ask under a record at rest is admitted.
#[test]
fn a_side_chats_terminal_holds_its_walled_turn_and_the_ask() {
    big_stack(|| async {
        let s = session().await;
        s.asks(&s.primary, ASKS).await;
        within(Duration::from_secs(30), "the primary's turn ends", || {
            s.ended_at_the_wall()
        })
        .await;
        let first = s.held().await;
        assert_eq!(first, s.prepare());
        assert!(s.h.warnings.said().is_empty(), "{:?}", s.h.warnings.said());

        // The side chat works while the ask stands, starts its dev server, and walls.
        s.shows(&s.side_key);
        s.asks(&s.side, SIDE_SERVES).await;
        let server = s.serving(&s.side).await;
        wait_until("the side chat's request is refused at the wall", || {
            s.account.side_refused.load(Ordering::SeqCst) >= 1
        })
        .await;
        wait_until("its recovery looked for an earned reset", || {
            s.side_looks() >= 1
        })
        .await;
        // aterm refuses the ask under the busy record: the move is live, the side turn is
        // woken, and its handoff says no while the server runs.
        assert!(
            !s.answers_the_ask_as_aterm().await,
            "aterm admitted an ask under a busy record"
        );
        wait_until("the person is told why the move waits", || {
            !s.h.warnings.said().is_empty()
        })
        .await;
        assert_eq!(s.told_in(), s.roots());
        assert_eq!(s.journal_count(TERMINALS_HOLD), 1);
        // THE RULE, for longer than a pass's recheck: the side turn waits, nothing more is
        // asked, and nobody is told again.
        let until = tokio::time::Instant::now() + Duration::from_secs(8);
        while tokio::time::Instant::now() < until {
            assert!(
                s.runs(&s.side_key),
                "the side turn ended while its server ran"
            );
            assert_eq!(Session::usage_errors(&s.side_events), 0);
            assert_eq!(s.asked(), 1, "asked again while the server ran");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert_eq!(s.told_in(), s.roots(), "told more than once");
        assert_eq!(s.journal_count(TERMINALS_HOLD), 1);
        assert_eq!(s.journal_count(LEAVING), 0);
        assert_eq!(s.account.side_refused.load(Ordering::SeqCst), 1);

        // The dev server ends: the next pass asks, and the side turn's next look ends it.
        s.stops(&s.side, server).await;
        let looks = s.side_looks();
        assert!(
            !s.answers_the_ask_as_aterm().await,
            "aterm admitted an ask while the side turn ran"
        );
        within(Duration::from_secs(15), "the side chat's turn ends", || {
            s.side_ended_at_the_wall()
        })
        .await;
        assert!(s.side_looks() > looks, "it ended with no further look");
        assert_eq!(s.journal_count(LEAVING), 1);
        assert_eq!(s.told_in(), s.roots(), "told more than once");
        wait_until("the session rests", || s.h.shared.unrest().is_none()).await;
        assert!(
            s.answers_the_ask_as_aterm().await,
            "the ask after the side turn's end was refused"
        );
    });
}

/// THE FOLLOW PASS TELLS OF THE HOLD ONLY WHERE ANOTHER ACCOUNT HAS ROOM (the review of
/// `af5f4fcc54`, its first finding). The pass's intent can stand on the usage wall alone (a
/// turn whose 429 named no reset ends on its own, with no pool read), and the room the
/// primary's question saw can be gone by the pass (another session took it). The pass told the
/// person "another signed-in account has room" with no read of the pool; they stopped their
/// dev server, and the ask was answered `not-needed`. Here the primary walls with room and is
/// asked for; the room goes; the side chat starts a dev server in a turn that meets no wall, so
/// only the primary's pass is held by it. For longer than a pass's recheck the pass sends no
/// ask and tells nobody. Once another account has room the next pass tells, once, in each root
/// thread, and still asks nothing; once the server ends the next pass asks.
#[test]
fn the_follow_pass_tells_of_the_hold_only_where_another_account_has_room() {
    big_stack(|| async {
        let s = session().await;
        s.asks(&s.primary, ASKS).await;
        within(Duration::from_secs(30), "the primary's turn ends", || {
            s.ended_at_the_wall()
        })
        .await;
        let first = s.held().await;
        assert_eq!(first, s.prepare());

        // The other account's room goes, and the side chat leaves a dev server running.
        s.pool(serde_json::json!([row("a", "usable"), row("b", "limited")]));
        s.asks(&s.side, SIDE_STARTS).await;
        let server = s.serving(&s.side).await;
        within(Duration::from_secs(30), "the side chat's turn ends", || {
            !s.runs(&s.side_key)
        })
        .await;
        assert_eq!(Session::usage_errors(&s.side_events), 0);
        s.answers_the_ask_as_aterm().await;

        // THE RULE, for longer than a pass's recheck: held, and nobody is told.
        let until = tokio::time::Instant::now() + Duration::from_secs(8);
        while tokio::time::Instant::now() < until {
            assert_eq!(s.asked(), 1, "asked again while the server ran");
            assert!(s.h.warnings.said().is_empty(), "{:?}", s.h.warnings.said());
            assert_eq!(
                s.journal_count(TERMINALS_HOLD),
                0,
                "{:#?}",
                s.h.autopilot.supervisor().journal()
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        // Another account has room again: the next pass tells, once, and asks nothing.
        s.pool(serde_json::json!([row("a", "usable"), row("b", "unseen")]));
        within(
            Duration::from_secs(15),
            "the person is told why the move waits",
            || !s.h.warnings.said().is_empty(),
        )
        .await;
        let until = tokio::time::Instant::now() + Duration::from_secs(6);
        while tokio::time::Instant::now() < until {
            assert_eq!(s.asked(), 1, "asked again while the server ran");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert_eq!(s.told_in(), s.roots(), "told more than once");
        assert_eq!(s.journal_count(TERMINALS_HOLD), 1);

        // The dev server ends: the next pass asks.
        s.stops(&s.side, server).await;
        let asked = s.held().await;
        assert_eq!(asked, s.prepare());
        s.answer(&asked, WAITING);
        assert_eq!(s.told_in(), s.roots(), "told more than once");
    });
}
