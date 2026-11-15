// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! THE LOST LOGIN'S MOVE end to end (`alab/design/accounts-lost-login.md`): a turn whose login
//! codex could not refresh because its refresh token simply ended (`refresh_token_expired`)
//! asks a host that advertises `codex_rotation_login=1` to move the session to another account
//! already signed in (`codex-login-rotation prepare`), once per wall. Only a definite no, a
//! retired receipt or [`Timing::login_rotation_ask`] without a receipt declines it, and the
//! login wall then goes the way it goes without a move: the same account's renewal
//! (`codex-renewal`) where the host renews logins, else a person's sign-in. Any other refusal
//! of the refresh (invalidated, reused, a 400 `invalid_grant`) vetoes the move, and the veto
//! outlives the process; a host without the word (aterm 0.199.0) is never asked.
//!
//! Against alab-aterm's in-process fake aterm and scratch ChatGPT logins codex's own login code
//! wrote, with fake tokens only; the token authority and the model are loopback mocks. Never a
//! real credential, provider or aterm.

use super::login_fixtures as fixtures;
use super::*;
use codex_alab_aterm::renewal::Identity;
use codex_alab_core::account_handoff::Destination;
use codex_alab_core::account_handoff::Purpose;
use codex_login::AuthCredentialsStoreMode;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

const VERB: &str = "codex-login-rotation";
const WORD: &str = codex_alab_aterm::account_recovery::LOGIN_ROTATION;

/// A host that moves a lost login (`codex_rotation_login`, `accounts`; `relaunch` is the
/// fake's default) and does not renew one in place. `continue` is off so nothing is typed after
/// a recovered turn.
const POLICY: &[(&str, &str)] = &[
    ("human_grace_s", "0"),
    (WORD, "1"),
    ("accounts", "wall"),
    ("resume_limits", "true"),
    ("continue", "false"),
];

/// [`POLICY`], on a host that also renews a lost login in place (`logins=renew`).
const POLICY_RENEW: &[(&str, &str)] = &[
    ("human_grace_s", "0"),
    (WORD, "1"),
    ("accounts", "wall"),
    ("logins", "renew"),
    ("resume_limits", "true"),
    ("continue", "false"),
];

/// aterm 0.199.0's policy: every switch [`POLICY`] sets but the word for the move.
const POLICY_0199: &[(&str, &str)] = &[
    ("human_grace_s", "0"),
    ("codex_rotation", "1"),
    ("accounts", "wall"),
    ("resume_limits", "true"),
    ("continue", "false"),
];

/// [`POLICY_0199`] with `logins=renew`.
const POLICY_0199_RENEW: &[(&str, &str)] = &[
    ("human_grace_s", "0"),
    ("codex_rotation", "1"),
    ("accounts", "wall"),
    ("logins", "renew"),
    ("resume_limits", "true"),
    ("continue", "false"),
];

/// The login wall's own ask of a person.
const SIGN_IN: &str = "sign-in was refused or expired";
/// The journal's line when the move is declined.
const DECLINED: &str = "the lost login is not moved to another account";

/// The claim's re-confirm (`meta set supervisor`). aterm seals a handoff to the claim's revision
/// when it answers `ready`, and each re-confirm moves that revision, so none may go out from
/// `commit` on (`rotation_decline.rs`, `a_committed_handoff_never_reconfirms_the_claim_it_sealed`).
const RECONFIRM: &str = " meta set supervisor ";

/// The longest aterm answers `waiting` to the move while it settles the lost login's refusal on
/// its own record (its `ROTATION_HANG`). The refusal of a token aterm presents itself settles a
/// minute later (`alab/design/accounts-lost-login.md`).
const SETTLEMENT: Duration = Duration::from_secs(60);

/// The token authority's refusal of the refresh: `(status, body)`.
type Refusal = (u16, serde_json::Value);

fn refusal(code: &str) -> Refusal {
    (401, serde_json::json!({"error": {"code": code}}))
}

fn expired() -> Refusal {
    refusal("refresh_token_expired")
}

/// The account the host moves the session to: another fake login, already signed in.
fn destination() -> Identity {
    Identity::from_native("moved-account", "moved-user")
        .unwrap_or_else(|| panic!("the destination's identity"))
}

/// The model's refusal of the access token, which sends codex to refresh it.
fn unauthorized() -> wiremock::ResponseTemplate {
    wiremock::ResponseTemplate::new(401).set_body_json(serde_json::json!({
        "error": {
            "message": "fixture sign-in expired",
            "type": "invalid_request_error",
            "code": "invalid_api_key"
        }
    }))
}

/// A session whose turn ended at a lost login.
struct Lost {
    h: Harness,
    home: Arc<tempfile::TempDir>,
    /// The walled turn's login, hashed as the host names it.
    identity: Identity,
    turn: String,
    /// The exact move request for the walled turn.
    prepare: String,
    /// The model refuses the access token while this is set.
    refuse: Arc<AtomicBool>,
    model: responses::ResponseMock,
    /// The token authority every scratch login here names: no refresh leaves this machine.
    authority: wiremock::MockServer,
}

impl Lost {
    /// Every line the host was sent, or holds at its gate, under the move's verb.
    fn move_lines(&self) -> Vec<String> {
        self.h
            .fake
            .requests()
            .into_iter()
            .chain(self.h.fake.gated_requests())
            .filter(|line| line.contains(&format!(" {VERB} ")))
            .collect()
    }

    /// Every move request the host answered under `verb` (`prepare`, `status`, `commit`).
    fn answered(&self, verb: &str) -> usize {
        let needle = format!(" {VERB} {verb} ");
        self.h
            .fake
            .requests()
            .iter()
            .filter(|line| line.contains(&needle))
            .count()
    }

    fn asked_a_person(&self) -> bool {
        self.h
            .fake
            .attention()
            .values()
            .any(|text| text.contains(SIGN_IN))
    }

    /// The same account's renewal, as the login wall's own path asks the host for it.
    fn renewal_prepare(&self) -> String {
        format!(
            "@{} codex-renewal prepare thread={} wall={} account={} user={}",
            self.h.fake.sid(),
            self.h.thread,
            self.turn,
            self.identity.account,
            self.identity.user
        )
    }

    fn intent(&self) -> Option<codex_alab_core::session::OwedAct> {
        self.h
            .shared
            .owed_act()
            .filter(|owed| owed.renewal.is_some())
    }

    /// A receipt for the walled turn's move to [`destination`]: its id, its fields, and the
    /// exact `status` request for it.
    fn receipt(&self) -> (String, String, String) {
        let id = "1".repeat(32);
        let target = destination();
        let origin = self
            .prepare
            .split_once(" prepare ")
            .map(|(_, origin)| origin.to_owned())
            .unwrap_or_else(|| panic!("the ask names no origin: {}", self.prepare));
        let receipt = format!(
            "id={id} {origin} target_account={} target_user={}",
            target.account, target.user
        );
        let status = format!("@{} {VERB} status id={id}", self.h.fake.sid());
        (id, receipt, status)
    }

    /// The held ask is answered `waiting`, the next `ready`, and every `status` of the receipt
    /// `waiting`: codex-alab binds the receipt and sends `commit` once.
    async fn bound(&self) -> (String, String, String) {
        let (id, receipt, status) = self.receipt();
        self.h.fake.reply_sequence(
            &self.prepare,
            vec![
                format!("OK {VERB} state=waiting\n"),
                format!("OK {VERB} state=ready {receipt}\n"),
            ],
        );
        self.h
            .fake
            .reply_always(&status, &format!("OK {VERB} state=waiting {receipt}\n"));
        self.h.fake.open_gate(VERB);
        wait_until("the move's receipt is bound and asked about", || {
            self.intent()
                .and_then(|owed| owed.renewal)
                .and_then(|renewal| renewal.receipt)
                .as_ref()
                == Some(&id)
                && self.answered("status") >= 1
        })
        .await;
        (id, receipt, status)
    }
}

/// A turn on a scratch ChatGPT login that the model refuses and whose refresh the authority
/// refuses with `refused`, under the host policy `policy`; the host holds any move request at
/// its gate. `ask` is [`Timing::login_rotation_ask`].
async fn lost(policy: &[(&str, &str)], refused: Refusal, ask: Duration) -> Lost {
    lost_in(
        policy,
        refused,
        ask,
        Arc::new(fixtures::home()),
        Mode::Current,
    )
    .await
}

async fn lost_in(
    policy: &[(&str, &str)],
    refused: Refusal,
    ask: Duration,
    home: Arc<tempfile::TempDir>,
    mode: Mode,
) -> Lost {
    lost_gated(policy, refused, ask, home, mode, &[VERB]).await
}

/// [`lost_in`], the host holding at its gate every request under each of `gates`.
async fn lost_gated(
    policy: &[(&str, &str)],
    (status, body): Refusal,
    ask: Duration,
    home: Arc<tempfile::TempDir>,
    mode: Mode,
    gates: &[&str],
) -> Lost {
    assert!(
        std::env::var_os(codex_login::REFRESH_TOKEN_URL_OVERRIDE_ENV_VAR).is_none(),
        "CODEX_REFRESH_TOKEN_URL_OVERRIDE is set: the refresh would go past the mock authority"
    );
    assert!(
        std::env::var_os("CODEX_AUTH_PATH").is_none(),
        "CODEX_AUTH_PATH is set: the scratch login would not be the one read"
    );
    let authority = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .respond_with(wiremock::ResponseTemplate::new(status).set_body_json(body))
        .mount(&authority)
        .await;
    fixtures::chatgpt(
        home.path(),
        "lost-user",
        "lost-account",
        "lost-refresh",
        Some(&authority.uri()),
    );
    let manager = fixtures::manager(home.path(), AuthCredentialsStoreMode::File).await;
    let identity = Identity::from_native("lost-account", "lost-user")
        .unwrap_or_else(|| panic!("the lost login's identity"));
    let h = harness_login(
        mode,
        Vec::new(),
        policy,
        &[],
        Some(0),
        Some(TestLogin::Manager {
            manager,
            home: Arc::clone(&home),
        }),
        Timing {
            min_work: Duration::ZERO,
            login_rotation_ask: ask,
            ..Timing::default()
        },
        |_| {},
        None,
    )
    .await;
    let refuse = Arc::new(AtomicBool::new(true));
    let refusing = Arc::clone(&refuse);
    let model = responses::mount_response_handler(&h._server, move |_| {
        if refusing.load(Ordering::SeqCst) {
            unauthorized()
        } else {
            responses::sse_response(reply("recovered", "Recovered the original work."))
        }
    })
    .await;
    if mode == Mode::Current {
        assert!(
            h.fake
                .wait_for(|f| f.claim().is_some_and(|c| c.proven_pid.is_some()))
                .await
        );
    }
    for gate in gates {
        h.fake.gate(gate);
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
        "@{} {VERB} prepare thread={} turn={turn} source_account={} source_user={}",
        h.fake.sid(),
        h.thread,
        identity.account,
        identity.user
    );
    Lost {
        h,
        home,
        identity,
        turn,
        prepare,
        refuse,
        model,
        authority,
    }
}

/// Wait until the move is asked for, and check the ask is the walled turn's, alone.
async fn asked(lost: &Lost) {
    wait_until("the move is asked for", || {
        !lost.h.fake.gated_requests().is_empty()
    })
    .await;
    assert_eq!(
        lost.h.fake.gated_requests(),
        std::slice::from_ref(&lost.prepare),
        "the ask names the walled turn and the login's hashed identity"
    );
}

/// AN EXPIRED REFRESH TOKEN UNDER A HOST THAT MOVES ONE IS ASKED ABOUT, AND A RECEIPT BINDS:
/// the exact `codex-login-rotation prepare` line, `waiting` asked again, then `ready`; the
/// record binds the receipt and its destination under `rotation` by `login-rotate@v1`, and
/// `commit` goes once. From `commit` on the claim the host sealed is never re-confirmed, over
/// two more passes that ask about the receipt; the second unbound ask, after `waiting`,
/// re-confirmed it before (POSITIVE CONTROL). Nothing is typed and the person is not asked
/// meanwhile.
#[test]
fn an_expired_login_is_moved_to_the_account_the_host_selects() {
    big_stack(|| async {
        let lost = lost(POLICY, expired(), Duration::from_secs(240)).await;
        asked(&lost).await;
        let (id, _, _) = lost.bound().await;
        assert_eq!(lost.answered("prepare"), 2, "waiting, then ready");
        let owed = lost.intent().unwrap_or_else(|| panic!("the bound intent"));
        assert_eq!(owed.rule.as_deref(), Some("login-rotate@v1"));
        assert_eq!(owed.text.as_deref(), Some("Continue the original task."));
        let renewal = owed.renewal.unwrap_or_else(|| panic!("the handoff"));
        let target = destination();
        assert_eq!(renewal.purpose, Purpose::LoginRotation);
        assert_eq!(renewal.receipt.as_deref(), Some(id.as_str()));
        assert_eq!(
            renewal.destination,
            Some(Destination {
                account: target.account.clone(),
                user: target.user.clone(),
            })
        );
        assert_eq!(
            (renewal.account.as_str(), renewal.user.as_str()),
            (lost.identity.account.as_str(), lost.identity.user.as_str())
        );
        let polled = lost.answered("status");
        wait_until("two more passes ask about the bound receipt", || {
            lost.answered("status") >= polled + 2
        })
        .await;
        assert_eq!(lost.answered("commit"), 1, "{:#?}", lost.h.fake.requests());
        assert_eq!(lost.answered("prepare"), 2);
        let requests = lost.h.fake.requests();
        let commit = format!("@{} {VERB} commit id={id}", lost.h.fake.sid());
        let committed = requests
            .iter()
            .position(|request| *request == commit)
            .unwrap_or_else(|| panic!("commit was sent: {requests:#?}"));
        let first_ask = requests
            .iter()
            .position(|request| *request == lost.prepare)
            .unwrap_or_else(|| panic!("the move was asked for: {requests:#?}"));
        assert!(
            requests[first_ask..committed]
                .iter()
                .any(|request| request.contains(RECONFIRM)),
            "POSITIVE CONTROL: the second unbound ask re-confirmed the claim: {requests:#?}"
        );
        assert!(
            requests[committed..]
                .iter()
                .all(|request| !request.contains(RECONFIRM)),
            "the sealed claim was re-confirmed after commit: {:#?}",
            &requests[committed..]
        );
        assert!(!lost.asked_a_person(), "{:?}", lost.h.fake.attention());
        assert!(lost.h.fake.acts().is_empty(), "{:?}", lost.h.fake.acts());
        assert!(
            lost.h
                .autopilot
                .supervisor()
                .journal()
                .iter()
                .any(|line| line.contains("the host is asked to move it")),
            "the journal says the move is asked for"
        );
    });
}

/// AN ASK OUTLASTS THE HOST'S SETTLEMENT: aterm answers `waiting` to the move for as long as it
/// settles the lost login's refusal on its own record, up to a minute ([`SETTLEMENT`]), and only
/// then `ready`. Under the bound codex-alab ships ([`Timing::login_rotation_ask`], from the first
/// ask) the move is asked for again on each pass through that minute and past it. It is never
/// declined and no person is asked meanwhile. The `ready` that follows binds the receipt, and
/// `commit` goes once. The minute is real: this test takes a little over it.
#[test]
fn an_ask_outlasts_the_hosts_minute_of_waiting() {
    big_stack(|| async {
        let lost = lost(POLICY, expired(), Timing::default().login_rotation_ask).await;
        asked(&lost).await;
        let (id, receipt, status) = lost.receipt();
        lost.h
            .fake
            .reply_always(&lost.prepare, &format!("OK {VERB} state=waiting\n"));
        lost.h
            .fake
            .reply_always(&status, &format!("OK {VERB} state=waiting {receipt}\n"));
        let asking = tokio::time::Instant::now();
        lost.h.fake.open_gate(VERB);
        let stands = |when: &str| {
            assert!(!journal_has(&lost.h, DECLINED), "declined {when}");
            assert!(
                !lost.asked_a_person(),
                "a person was asked {when}: {:?}",
                lost.h.fake.attention()
            );
            assert!(
                lost.intent()
                    .and_then(|owed| owed.renewal)
                    .is_some_and(|renewal| renewal.purpose == Purpose::LoginRotation
                        && renewal.receipt.is_none()),
                "the unbound ask stands {when}"
            );
        };
        while asking.elapsed() < SETTLEMENT {
            stands("within the minute");
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        // A pass comes every five seconds; the count is no latency budget.
        let within = lost.answered("prepare");
        assert!(within >= 4, "asked again through the minute: {within}");
        wait_until("the move is asked for past the minute", || {
            lost.answered("prepare") > within
        })
        .await;
        stands("past the minute");
        lost.h
            .fake
            .reply_once(&lost.prepare, &format!("OK {VERB} state=ready {receipt}\n"));
        wait_until("the move's receipt is bound and asked about", || {
            lost.intent()
                .and_then(|owed| owed.renewal)
                .and_then(|renewal| renewal.receipt)
                .as_ref()
                == Some(&id)
                && lost.answered("status") >= 1
        })
        .await;
        assert_eq!(lost.answered("commit"), 1, "{:#?}", lost.h.fake.requests());
        assert!(!journal_has(&lost.h, DECLINED));
        assert!(!lost.asked_a_person(), "{:?}", lost.h.fake.attention());
        assert!(lost.h.fake.acts().is_empty(), "{:?}", lost.h.fake.acts());
    });
}

/// The process the host starts on the destination's own login: a new Autopilot, supervisor and
/// host connection over a resume of the same rollout, seeded with the record the walled process
/// left (its bound intent), as a same-pid image is (`rotation_decline.rs` `restarted` says why
/// that stands in for aterm's relaunch). Its first `status` read waits at its host's gate until
/// the test opens it.
struct Moved {
    fake: FakeAterm,
    shared: &'static Shared,
    autopilot: Arc<Autopilot>,
    _test: TestCodex,
    _dir: tempfile::TempDir,
}

impl Drop for Moved {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!(
                "moved supervisor journal: {:#?}",
                self.autopilot.supervisor().journal()
            );
            eprintln!("moved fake requests: {:#?}", self.fake.requests());
        }
    }
}

async fn moved(lost: &Lost, owed: codex_alab_core::session::OwedAct) -> Moved {
    let rollout = lost
        .h
        .test
        .codex
        .rollout_path()
        .unwrap_or_else(|| panic!("the walled turn's rollout"));
    lost.h
        .test
        .codex
        .shutdown_and_wait()
        .await
        .unwrap_or_else(|err| panic!("the walled process's thread: {err}"));
    // The host restarts the session on the destination's own login.
    fixtures::chatgpt(
        lost.home.path(),
        "moved-user",
        "moved-account",
        "moved-refresh",
        Some(&lost.authority.uri()),
    );
    let dir = tempfile::tempdir().unwrap_or_else(|err| panic!("scratch dir: {err}"));
    let fake = FakeAterm::start(dir.path(), Mode::Current)
        .await
        .unwrap_or_else(|err| panic!("fake aterm: {err}"));
    for (key, value) in POLICY {
        fake.set_policy_key(key, value);
    }
    // Held until the thread's manager is bound (`rotation_decline.rs` `restarted_under`).
    fake.gate("status");
    let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
    shared.set_owed_act(Some(owed.clone()));
    assert!(shared.set_inherited(codex_alab_core::session::Inherited {
        session_id: Some(lost.h.thread.clone()),
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
    let autopilot = Autopilot::new(
        sup,
        None,
        None,
        Timing {
            min_work: Duration::ZERO,
            ..Timing::default()
        },
        |_| true,
    );
    autopilot.register(&mut builder);
    let manager = fixtures::manager(lost.home.path(), AuthCredentialsStoreMode::File).await;
    let test = test_codex()
        .with_extensions(Arc::new(builder.build()))
        .with_config(|config| {
            config.model_provider.request_max_retries = Some(0);
            config.model_provider.stream_max_retries = Some(0);
        })
        .with_auth_manager(manager)
        .resume(&lost.h._server, Arc::clone(&lost.home), rollout)
        .await
        .unwrap_or_else(|err| panic!("the resumed thread: {err}"));
    autopilot.bind_thread_manager(Arc::downgrade(&test.thread_manager));
    let thread = test.session_configured.thread_id.to_string();
    assert_eq!(thread, lost.h.thread, "the same session");
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
    Moved {
        fake,
        shared,
        autopilot,
        _test: test,
        _dir: dir,
    }
}

/// THE SESSION ON THE ACCOUNT IT WAS MOVED TO RECOVERS THE ORIGINAL TURN: the walled process
/// binds the receipt and commits; the process the host starts on the destination's login, its
/// record holding the bound intent, finds the receipt `committed` and recovers the walled turn
/// under its own id with its own words, once, with nothing typed; `consumed` releases the
/// intent.
#[test]
fn the_moved_session_recovers_the_original_turn_once() {
    big_stack(|| async {
        let lost = lost(POLICY, expired(), Duration::from_secs(240)).await;
        asked(&lost).await;
        let (id, receipt, _) = lost.bound().await;
        let owed = lost.intent().unwrap_or_else(|| panic!("the bound intent"));
        let walled = lost.model.requests().len();
        lost.refuse.store(false, Ordering::SeqCst);
        let moved = moved(&lost, owed).await;
        let status = format!("@{} {VERB} status id={id}", moved.fake.sid());
        let consume = format!(
            "@{} {VERB} consume id={id} turn={}",
            moved.fake.sid(),
            lost.turn
        );
        moved
            .fake
            .reply_always(&status, &format!("OK {VERB} state=committed {receipt}\n"));
        moved
            .fake
            .reply_always(&consume, &format!("OK {VERB} state=consumed {receipt}\n"));
        moved.fake.open_gate("status");
        assert!(
            moved
                .fake
                .wait_for(|f| f.claim().is_some_and(|c| c.proven_pid.is_some()))
                .await
        );
        wait_until("the original turn is recovered", || {
            lost.model.requests().len() > walled
        })
        .await;
        wait_until("the recovered turn's receipt is consumed", || {
            moved
                .shared
                .owed_act()
                .and_then(|owed| owed.renewal)
                .is_none()
        })
        .await;
        let requests = lost.model.requests();
        assert_eq!(requests.len(), walled + 1, "recovered once");
        // The resumed process sends the original turn's own words, and nothing typed: no
        // continuation and no harness line among the user's messages. (Its environment context
        // is its own: a resumed process states its working folder again.)
        let words = |request: &responses::ResponsesRequest| {
            request
                .message_input_texts("user")
                .into_iter()
                .filter(|text| !text.starts_with('<'))
                .collect::<Vec<_>>()
        };
        let recovered = words(&requests[walled]);
        assert_eq!(
            recovered,
            words(&requests[0]),
            "the walled request's own words"
        );
        assert_eq!(
            recovered.last().map(String::as_str),
            Some("do the task"),
            "{recovered:?}"
        );
        assert!(
            recovered
                .iter()
                .all(|text| !text.contains("Continue the original task")
                    && !text.contains("[aterm harness]")),
            "{recovered:?}"
        );
        assert_eq!(
            moved.shared.latest_turn(&lost.h.thread).as_deref(),
            Some(lost.turn.as_str()),
            "under the original turn's id"
        );
        assert!(
            moved.fake.requests().iter().any(|line| line == &consume),
            "{:#?}",
            moved.fake.requests()
        );
        assert!(moved.fake.acts().is_empty(), "{:?}", moved.fake.acts());
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert_eq!(lost.model.requests().len(), walled + 1, "never twice");
    });
}

/// A HOST WITH NO ACCOUNT TO MOVE TO (`not-needed`) DECLINES THE MOVE, and the login wall goes
/// its own way on the next pass: the same account's renewal where the host renews logins, with
/// no ask of a person between; a person's sign-in where it does not. The move is not asked for
/// again.
#[test]
fn a_host_with_nowhere_to_move_leaves_the_login_wall_its_own_path() {
    big_stack(|| async {
        for renews in [true, false] {
            let policy = if renews { POLICY_RENEW } else { POLICY };
            let lost = lost(policy, expired(), Duration::from_secs(240)).await;
            asked(&lost).await;
            lost.h
                .fake
                .reply_once(&lost.prepare, &format!("OK {VERB} state=not-needed\n"));
            lost.h.fake.open_gate(VERB);
            if renews {
                let renewal = lost.renewal_prepare();
                wait_until("the same account's renewal is asked for", || {
                    lost.h.fake.requests().contains(&renewal)
                })
                .await;
                assert!(!lost.asked_a_person(), "{:?}", lost.h.fake.attention());
            } else {
                wait_until("the person is asked to sign in", || lost.asked_a_person()).await;
            }
            assert!(journal_has(&lost.h, DECLINED));
            assert_eq!(lost.answered("prepare"), 1, "renews={renews}");
            // The decline left the walled turn's sign-in recovery standing for the login wall's
            // own path: it retired nothing.
            let db = lost
                .h
                .test
                .codex
                .state_db()
                .unwrap_or_else(|| panic!("the custody DB"));
            assert_eq!(
                db.thread_goals()
                    .alab_auth_goal_context(lost.h.test.session_configured.thread_id, &lost.turn)
                    .await
                    .unwrap_or_else(|err| panic!("the walled turn's custody: {err}")),
                Some(None),
                "renews={renews}"
            );
            assert!(
                lost.intent()
                    .and_then(|owed| owed.renewal)
                    .is_none_or(|renewal| renewal.purpose == Purpose::Login),
                "the move's intent was released (renews={renews})"
            );
        }
    });
}

/// A HOST WITHOUT THE WORD IS NEVER ASKED (aterm 0.199.0): the login wall goes exactly as it
/// did before the move existed, to the same account's renewal or to a person, and no line under
/// the move's verb is sent.
#[test]
fn a_host_that_does_not_advertise_the_move_is_never_asked() {
    big_stack(|| async {
        for renews in [true, false] {
            let policy = if renews {
                POLICY_0199_RENEW
            } else {
                POLICY_0199
            };
            let lost = lost(policy, expired(), Duration::from_secs(240)).await;
            if renews {
                let renewal = lost.renewal_prepare();
                wait_until("the same account's renewal is asked for", || {
                    lost.h.fake.requests().contains(&renewal)
                })
                .await;
            } else {
                wait_until("the person is asked to sign in", || lost.asked_a_person()).await;
            }
            assert_eq!(lost.move_lines(), Vec::<String>::new(), "renews={renews}");
            assert!(!journal_has(&lost.h, DECLINED));
        }
    });
}

/// A HOST THAT KNOWS NO SUCH VERB DECLINES THE MOVE ONCE: `ERR unknown verb` is a definite no,
/// the person is asked, and no ask follows on the passes after it.
#[test]
fn a_host_that_knows_no_such_verb_is_asked_once() {
    big_stack(|| async {
        let lost = lost(POLICY, expired(), Duration::from_secs(240)).await;
        asked(&lost).await;
        // The fake knows no such verb, as aterm before it.
        lost.h.fake.open_gate(VERB);
        wait_until("the person is asked to sign in", || lost.asked_a_person()).await;
        assert!(journal_has(&lost.h, "the host refused: unknown verb"));
        tokio::time::sleep(Duration::from_secs(6)).await;
        assert_eq!(lost.answered("prepare"), 1, "{:#?}", lost.h.fake.requests());
        assert!(lost.intent().is_none());
    });
}

/// A REFUSAL THAT IS NO DEFINITE NO IS ASKED ABOUT AGAIN, BUT ONLY WITHIN THE ASK'S BOUND:
/// `denied` (a hold) is asked again on the next pass, and once the bound passes with no
/// receipt the move is declined and the person asked.
#[test]
fn a_denied_move_is_asked_again_until_the_ask_lapses() {
    big_stack(|| async {
        let lost = lost(POLICY, expired(), Duration::from_secs(8)).await;
        asked(&lost).await;
        lost.h
            .fake
            .reply_always(&lost.prepare, &format!("ERR {VERB} denied\n"));
        lost.h.fake.open_gate(VERB);
        wait_until("the denied move is asked again", || {
            lost.answered("prepare") >= 2
        })
        .await;
        assert!(!lost.asked_a_person(), "not declined by a hold");
        wait_until("the lapsed ask leaves the wall to a person", || {
            lost.asked_a_person()
        })
        .await;
        assert!(journal_has(&lost.h, "no receipt in 8 s"));
        let asks = lost.answered("prepare");
        assert!((2..=3).contains(&asks), "{asks}");
        tokio::time::sleep(Duration::from_secs(6)).await;
        assert_eq!(lost.answered("prepare"), asks, "never asked again");
    });
}

/// A RECEIPT THE HOST RETIRES DECLINES THE MOVE: the person is asked, and no new ask is made.
/// Nothing else declines a bound receipt, whose `commit` may have gone: an unknown verb or an
/// unreadable answer about it is asked about again, the intent still bound.
#[test]
fn a_retired_receipt_declines_the_move() {
    big_stack(|| async {
        let lost = lost(POLICY, expired(), Duration::from_secs(240)).await;
        asked(&lost).await;
        let (id, receipt, status) = lost.bound().await;
        for answer in [
            "ERR unknown verb (try: help)\n".to_owned(),
            format!("OK codex-rotation state=committed {receipt}\n"),
        ] {
            lost.h.fake.reply_always(&status, &answer);
            let polled = lost.answered("status");
            wait_until("the bound receipt is asked about twice more", || {
                lost.answered("status") >= polled + 2
            })
            .await;
            assert_eq!(
                lost.intent()
                    .and_then(|owed| owed.renewal)
                    .and_then(|renewal| renewal.receipt),
                Some(id.clone()),
                "{answer}"
            );
            assert!(!lost.asked_a_person(), "{answer}");
        }
        lost.h
            .fake
            .reply_always(&status, &format!("OK {VERB} state=retired {receipt}\n"));
        wait_until("the person is asked to sign in", || lost.asked_a_person()).await;
        assert!(journal_has(&lost.h, "the host retired its receipt"));
        assert!(lost.intent().is_none());
        assert_eq!(lost.answered("prepare"), 2);
        assert_eq!(lost.answered("commit"), 1);
    });
}

/// ANY REFUSAL BUT AN EXPIRED TOKEN VETOES THE MOVE (codex-alab's own reading): invalidated,
/// reused, a 400 `invalid_grant` and an unknown code are never asked about; the person is.
/// The veto is kept for the refused token, and a process after a restart that sees the same
/// token refused as expired still does not ask.
#[test]
fn any_refusal_but_an_expired_token_vetoes_the_move_across_a_restart() {
    big_stack(|| async {
        for refused in [
            refusal("refresh_token_invalidated"),
            refusal("refresh_token_reused"),
            (400, serde_json::json!({"error": "invalid_grant"})),
            refusal("account_deactivated"),
        ] {
            let what = refused.1.to_string();
            let home = Arc::new(fixtures::home());
            {
                let lost = lost_in(
                    POLICY,
                    refused,
                    Duration::from_secs(240),
                    Arc::clone(&home),
                    Mode::Current,
                )
                .await;
                wait_until("the person is asked to sign in", || lost.asked_a_person()).await;
                assert_eq!(lost.move_lines(), Vec::<String>::new(), "{what}");
                assert!(journal_has(&lost.h, "refused for good"), "{what}");
                lost.h
                    .test
                    .codex
                    .shutdown_and_wait()
                    .await
                    .unwrap_or_else(|err| panic!("the first process's thread: {err}"));
            }
            let vetoes = std::fs::read_dir(home.path().join("alab").join("login-refusals"))
                .map(Iterator::count)
                .unwrap_or_default();
            assert_eq!(vetoes, 1, "{what}");
            // The next process, the same token refused as expired this time.
            let restarted = lost_in(
                POLICY,
                expired(),
                Duration::from_secs(240),
                Arc::clone(&home),
                Mode::Current,
            )
            .await;
            wait_until("the person is asked to sign in", || {
                restarted.asked_a_person()
            })
            .await;
            assert_eq!(
                restarted.move_lines(),
                Vec::<String>::new(),
                "{what} after a restart"
            );
        }
    });
}

/// A USAGE WALL IS NEVER A LOST LOGIN: under a host that moves both, a spent window asks for
/// the usage wall's own move (`codex-rotation`) and never for a lost login's.
#[test]
fn a_usage_wall_never_asks_for_the_lost_logins_move() {
    big_stack(|| async {
        let authority = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .respond_with(
                wiremock::ResponseTemplate::new(400)
                    .set_body_json(serde_json::json!({"error": "invalid_grant"})),
            )
            .mount(&authority)
            .await;
        let home = fixtures::home();
        fixtures::chatgpt(
            home.path(),
            "usage-user",
            "usage-account",
            "usage-refresh",
            Some(&authority.uri()),
        );
        let manager = fixtures::manager(home.path(), AuthCredentialsStoreMode::File).await;
        let limited = wiremock::ResponseTemplate::new(429).set_body_json(serde_json::json!({
            "error": {
                "type": "usage_limit_reached",
                "message": "limit reached",
                "resets_at": now_ms() / 1000 + 3600,
                "plan_type": "pro"
            }
        }));
        let policy: Vec<(&str, &str)> = POLICY
            .iter()
            .copied()
            .chain([("codex_rotation", "1")])
            .collect();
        let h = harness_login(
            Mode::Current,
            vec![limited],
            &policy,
            &[],
            Some(0),
            Some(TestLogin::Manager {
                manager,
                home: Arc::new(home),
            }),
            Timing {
                min_work: Duration::ZERO,
                ..Timing::default()
            },
            |_| {},
            None,
        )
        .await;
        assert!(
            h.fake
                .wait_for(|f| f.claim().is_some_and(|c| c.proven_pid.is_some()))
                .await
        );
        h.test
            .submit_turn("do the task")
            .await
            .unwrap_or_else(|err| panic!("the walled turn: {err}"));
        wait_until("the usage wall's move is asked for", || {
            h.fake
                .requests()
                .iter()
                .any(|line| line.contains(" codex-rotation prepare "))
        })
        .await;
        assert!(
            h.fake
                .requests()
                .iter()
                .all(|line| !line.contains(&format!(" {VERB} "))),
            "{:#?}",
            h.fake.requests()
        );
    });
}

/// The same account's renewal, as the login wall's own path asks the host for it.
const RENEWAL: &str = "codex-renewal";

/// The wall's first ask, `ask` under `verb`, which the host holds at its gate until now: the
/// moment the host answered it, and when each ask the host answered in the second from then came
/// (the first ask's own `0`, then every other's). The host answers each `waiting`, as aterm does
/// while it settles a lost login's refusal or renews one.
async fn asks_of_the_first_second(
    lost: &Lost,
    verb: &str,
    ask: &str,
) -> (tokio::time::Instant, Vec<Duration>) {
    wait_until("the wall's first ask reaches the host", || {
        !lost.h.fake.gated_requests().is_empty()
    })
    .await;
    assert_eq!(
        lost.h.fake.gated_requests(),
        [ask.to_owned()],
        "the first ask is the walled turn's"
    );
    lost.h
        .fake
        .reply_always(ask, &format!("OK {verb} state=waiting\n"));
    let answered = |fake: &FakeAterm| fake.requests().iter().filter(|line| *line == ask).count();
    let mut at = Vec::new();
    lost.h.fake.open_gate(verb);
    let second = async {
        loop {
            let seen = at.len();
            if !lost.h.fake.wait_for(|fake| answered(fake) > seen).await {
                return;
            }
            at.push(tokio::time::Instant::now());
        }
    };
    // The second runs from the gate's opening, which answers the held ask.
    let _ = tokio::time::timeout(Duration::from_secs(1), second).await;
    let first = *at
        .first()
        .unwrap_or_else(|| panic!("the held ask was never answered"));
    (
        first,
        at.iter().map(|at| at.duration_since(first)).collect(),
    )
}

/// ONE ASK A PASS, AND ONE PASS UNTIL ITS OWN RECHECK (`alab/CONTRACT.md` §5, "A sealed
/// claim"): a lost login's move is asked for once at the wall, and the next ask is that pass's
/// recheck five seconds later. The pass that first read the lost login armed its file's watch,
/// whose first wake (the login file is there) decided the point again behind it as though the
/// profile had never been known, so the host was asked twice within milliseconds (aterm's
/// fixture smoke, 2026-11-13: 12 to 23 ms apart). The first ask is held at the host's gate, so
/// any wake that comes while it waits queues a pass behind it.
#[test]
fn a_lost_logins_move_is_asked_for_once_at_the_wall() {
    big_stack(|| async {
        let lost = lost(POLICY, expired(), Duration::from_secs(240)).await;
        let (first, asks) = asks_of_the_first_second(&lost, VERB, &lost.prepare).await;
        assert_eq!(asks.len(), 1, "asked at {asks:?} after the first ask");
        // The pass's own recheck is the next ask: no sooner than its five seconds from the
        // answer it read, which a timer cannot run ahead of (a floor, never a latency budget).
        wait_until("the move is asked for again", || {
            lost.answered("prepare") >= 2
        })
        .await;
        let again = first.elapsed();
        assert!(
            again >= Duration::from_millis(4900),
            "asked again {again:?}"
        );
        assert!(!lost.asked_a_person(), "{:?}", lost.h.fake.attention());
    });
}

/// The same for the same account's renewal (`codex-renewal prepare`), on a host that renews a
/// lost login and moves none: one ask at the wall, its recheck a minute later.
#[test]
fn a_lost_logins_renewal_is_asked_for_once_at_the_wall() {
    big_stack(|| async {
        let lost = lost_gated(
            POLICY_0199_RENEW,
            expired(),
            Duration::from_secs(240),
            Arc::new(fixtures::home()),
            Mode::Current,
            &[RENEWAL],
        )
        .await;
        let (_, asks) = asks_of_the_first_second(&lost, RENEWAL, &lost.renewal_prepare()).await;
        assert_eq!(asks.len(), 1, "asked at {asks:?} after the first ask");
        assert_eq!(lost.move_lines(), Vec::<String>::new());
        assert!(!lost.asked_a_person(), "{:?}", lost.h.fake.attention());
    });
}

/// THE WATCH THE FIRST ASK ARMED STILL DELIVERS THE LOGIN'S RETURN. It owes the profile that
/// ask already follows no second pass, and nothing else: the walled identity signed in again in
/// its own file is reloaded at the watch's next wake (its folder's change, else its look), the
/// thread looks again, the wall's follow lets the point go, and the walled turn goes again on
/// the renewed login, the host asked no more.
#[test]
fn the_watch_the_first_ask_armed_delivers_the_logins_return() {
    big_stack(|| async {
        let lost = lost_gated(
            POLICY_0199_RENEW,
            expired(),
            Duration::from_secs(240),
            Arc::new(fixtures::home()),
            Mode::Current,
            &[RENEWAL],
        )
        .await;
        let ask = lost.renewal_prepare();
        let (_, asks) = asks_of_the_first_second(&lost, RENEWAL, &ask).await;
        assert_eq!(asks.len(), 1, "asked at {asks:?} after the first ask");
        let walled = lost.model.requests().len();
        lost.refuse.store(false, Ordering::SeqCst);
        fixtures::chatgpt(
            lost.home.path(),
            "lost-user",
            "lost-account",
            "renewed-refresh",
            Some(&lost.authority.uri()),
        );
        wait_until("the walled turn goes again on the renewed login", || {
            lost.model.requests().len() > walled
        })
        .await;
        wait_until("the watch says it looked again", || {
            journal_has(&lost.h, "looked again at the lost login's return")
        })
        .await;
        let asked = lost
            .h
            .fake
            .requests()
            .iter()
            .filter(|line| **line == ask)
            .count();
        assert_eq!(asked, 1, "{:#?}", lost.h.fake.requests());
        assert!(!lost.asked_a_person(), "{:?}", lost.h.fake.attention());
    });
}

/// NO CLAIM, NO MOVE: a session aterm does not hand codex-alab (an aterm before the harness, or
/// Autopilot off) never asks for the move, however the refresh was refused.
#[test]
fn a_session_without_a_claim_never_asks_for_the_move() {
    big_stack(|| async {
        // Passive: an aterm before the harness is never claimed.
        let passive = lost_in(
            POLICY,
            expired(),
            Duration::from_secs(240),
            Arc::new(fixtures::home()),
            Mode::Old,
        )
        .await;
        tokio::time::sleep(Duration::from_secs(2)).await;
        assert_eq!(passive.move_lines(), Vec::<String>::new());
        assert!(passive.intent().is_none());
        drop(passive);

        // Autopilot off: the claim is given back before the turn.
        let home = Arc::new(fixtures::home());
        let authority = wiremock::MockServer::start().await;
        let (status, body) = expired();
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .respond_with(wiremock::ResponseTemplate::new(status).set_body_json(body))
            .mount(&authority)
            .await;
        fixtures::chatgpt(
            home.path(),
            "off-user",
            "off-account",
            "off-refresh",
            Some(&authority.uri()),
        );
        let manager = fixtures::manager(home.path(), AuthCredentialsStoreMode::File).await;
        let h = harness_login(
            Mode::Current,
            vec![unauthorized(), unauthorized()],
            POLICY,
            &[],
            Some(0),
            Some(TestLogin::Manager { manager, home }),
            Timing {
                min_work: Duration::ZERO,
                ..Timing::default()
            },
            |_| {},
            None,
        )
        .await;
        assert!(
            h.fake
                .wait_for(|f| f.claim().is_some_and(|c| c.proven_pid.is_some()))
                .await
        );
        h.fake.set_status("autopilot", "off");
        h.fake.set_effective(false, "autopilot-off");
        wait_until("the claim is given back", || h.fake.claim().is_none()).await;
        h.test
            .submit_turn("do the task")
            .await
            .unwrap_or_else(|err| panic!("the walled turn: {err}"));
        tokio::time::sleep(Duration::from_secs(2)).await;
        assert!(
            h.fake
                .requests()
                .iter()
                .all(|line| !line.contains(&format!(" {VERB} "))),
            "{:#?}",
            h.fake.requests()
        );
        assert!(h.shared.owed_act().and_then(|owed| owed.renewal).is_none());
    });
}
