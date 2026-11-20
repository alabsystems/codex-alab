// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! THE SUCCESSOR'S RECOVERED TURN (happy-successor diagnosis, 2026-11-13). The host renewed a
//! subscription login for this session and started this process on it; the record holds the
//! intent bound to the host's receipt, and the host answers `committed` for it. The follow
//! recovers the walled turn under that receipt, and core runs the admission's checks before it
//! consumes any input. Those checks wait on the host (its status, the claim's re-confirms),
//! and the TUI moves the upgrade's scan counter on every event it dispatches meanwhile (the
//! usage poll's reply, a redraw). Fenced on that counter, every admission was refused, nothing
//! logged why, and the next look came a minute later: the original request never went again.
//!
//! Against alab-aterm's in-process fake aterm and a scratch ChatGPT login codex's own login code
//! wrote (fake keys only, never a real credential); the model is a loopback mock.

use super::*;

/// How soon a recovered turn goes once the host has answered, and how soon the pass after a
/// refusal comes: seconds, where a login's point waited a minute for its next look before.
const WITHIN: Duration = Duration::from_secs(15);

/// How long the first admission may take to reach the host once renewal is let: the suite's
/// own bound for a wait (`wait_until`), not a bound in seconds. That look queues behind
/// whatever evaluation of the thread still runs, and the walled turn's own sets up the lost
/// login's folder watch (FSEvents) while it holds the thread's evaluation permit: with four
/// copies of this binary running, that alone took over 30 s (review of the admission retry,
/// finding 2). No recheck is waited for before it, so nothing a seconds bound guards is in it.
const FIRST: Duration = Duration::from_secs(60);

/// The journal's line once the walled turn's evaluation has the lost login's file watched.
const WATCHED: &str = "the lost login's file is watched while the wall stands";

/// The journal's line for a recovered turn's refused admission.
const REFUSED: &str = "the recovered turn's admission was refused";

/// A claim's re-confirm, which each of the admission's readiness checks sends.
const RECONFIRM: &str = "meta set supervisor";

fn refused() -> wiremock::ResponseTemplate {
    wiremock::ResponseTemplate::new(401).set_body_json(serde_json::json!({
        "error": {"message": "fixture sign-in expired",
            "type": "invalid_request_error", "code": "invalid_api_key"}
    }))
}

/// The process the host started on the renewed login.
struct Successor {
    h: Harness,
    identity: codex_alab_aterm::renewal::Identity,
    /// The policy setting's next value ([`Self::renewal_on`]).
    looks: std::sync::atomic::AtomicU32,
    /// The login's refresh endpoint, which refuses: kept up for the harness's life.
    _refresh: wiremock::MockServer,
}

impl Successor {
    /// A successor on `shared` whose model answers `walls` login walls, each refused twice
    /// and then answered once.
    async fn start(label: &str, shared: &'static Shared, walls: usize) -> Successor {
        let (login, identity, refresh) = subscription_fixture(label).await;
        let mut replies = Vec::new();
        for wall in 0..walls {
            replies.push(refused());
            replies.push(refused());
            replies.push(responses::sse_response(reply(
                &format!("recovered-{wall}"),
                "Recovered original work.",
            )));
        }
        let h = harness_login_on(
            shared,
            Mode::Current,
            replies,
            &[
                ("continue", "false"),
                ("human_grace_s", "0"),
                ("logins", "off"),
            ],
            &[],
            Some(0),
            Some(login),
            Timing {
                min_work: Duration::ZERO,
                ..Timing::default()
            },
            |_| {},
            None,
            ErrorHooks::default(),
        )
        .await;
        wait_until("proven claim", || h.autopilot.supervisor().acting()).await;
        Successor {
            h,
            identity,
            looks: std::sync::atomic::AtomicU32::new(1),
            _refresh: refresh,
        }
    }

    /// A turn meets the refused login while renewal is off, so this process asks the host
    /// nothing; the record then holds what the process before it left there: the intent for
    /// that turn, bound to receipt `id`, which the host answers `committed` and consumes.
    async fn walled(&self, prompt: &str, id: &str) -> String {
        let h = &self.h;
        let watched = || {
            h.autopilot
                .supervisor()
                .journal()
                .iter()
                .filter(|line| line.contains(WATCHED))
                .count()
        };
        let watches = watched();
        h.test
            .submit_turn(prompt)
            .await
            .unwrap_or_else(|err| panic!("the refused login's turn: {err}"));
        // The walled turn's own evaluation arms the login's watch for this wall: waited for
        // here, so the look that lets renewal does not start its seconds behind it.
        wait_until(
            "the walled turn's evaluation watches the lost login",
            || watched() > watches,
        )
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
                .unwrap_or_else(|err| panic!("standalone proof: {err}")),
            "the walled turn's standalone native proof"
        );
        h.shared.set_owed_act(Some(OwedAct {
            thread: h.thread.clone(),
            native_origin: Some(NativeOrigin {
                turn_id: wall.clone(),
                phase: NativePhase::Complete,
            }),
            renewal: Some(AccountHandoff {
                purpose: codex_alab_core::account_handoff::Purpose::Login,
                destination: None,
                limit: None,
                openai_scope: true,
                local: false,
                account: self.identity.account.clone(),
                user: self.identity.user.clone(),
                receipt: Some(id.into()),
                turn: None,
                accepted: false,
                starts: 1,
                goal_id: None,
            }),
            rule: Some("login-back@v1".into()),
            text: Some("Restored native custody.".into()),
            mode: Some("cruise".into()),
            ..Default::default()
        }));
        let receipt = format!(
            "id={id} thread={} wall={wall} account={} user={}",
            h.thread, self.identity.account, self.identity.user
        );
        let sid = h.fake.sid();
        h.fake.reply_always(
            &format!("@{sid} codex-renewal status id={id}"),
            &format!("OK codex-renewal state=committed {receipt}\n"),
        );
        h.fake.reply_always(
            &format!("@{sid} codex-renewal consume id={id} turn={wall}"),
            &format!("OK codex-renewal state=consumed {receipt}\n"),
        );
        wall
    }

    /// The person lets the host renew logins. `logins` moves no decision's policy, so a
    /// setting changed beside it has the thread look again, as the resumed thread's scan has
    /// the successor look in aterm.
    fn renewal_on(&self) {
        let look = self.looks.fetch_add(1, Ordering::SeqCst);
        self.h.fake.set_policy_key("logins", "renew");
        self.h
            .fake
            .set_policy_key("continue_per_hour", &look.to_string());
    }

    fn renewal_off(&self) {
        self.h.fake.set_policy_key("logins", "off");
    }

    /// The re-confirm the gate holds, once one more than `before` met it, and how long that
    /// took: within `within` ([`FIRST`] for the first admission; [`WITHIN`] after a refusal,
    /// which no pass a minute away meets).
    async fn held(&self, before: usize, within: Duration) -> Duration {
        let asked = tokio::time::Instant::now();
        while self.h.fake.gated_requests().len() <= before {
            assert!(
                asked.elapsed() < within,
                "no admission's readiness check reached the host within {} s: {:#?}",
                within.as_secs(),
                self.h.autopilot.supervisor().journal()
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let held = &self.h.fake.gated_requests()[before];
        assert!(
            held.contains(&format!(" {RECONFIRM} ")),
            "the gate holds a re-confirm: {held}"
        );
        asked.elapsed()
    }

    /// The claim's re-confirms the host answered from its `since`-th request to the consume of
    /// receipt `id` for `wall`: each of an admission's three readiness checks sends one.
    fn reconfirms(&self, since: usize, wall: &str, id: &str) -> usize {
        let consume = format!(" codex-renewal consume id={id} turn={wall}");
        self.h.fake.requests()[since..]
            .iter()
            .take_while(|request| !request.ends_with(&consume))
            .filter(|request| request.contains(&format!(" {RECONFIRM} ")))
            .count()
    }

    /// The bound intent's reservation of the recovered turn, which the admission writes before
    /// its second readiness check.
    fn reserved(&self) -> Option<String> {
        self.h
            .shared
            .owed_act()
            .and_then(|owed| owed.renewal)
            .and_then(|renewal| renewal.turn)
    }

    /// The gate opens, and the walled turn's request goes again within [`WITHIN`] as the
    /// `requests`-th model request; the host is then told it was consumed, and the intent goes.
    async fn admitted(&self, wall: &str, id: &str, requests: usize) {
        let h = &self.h;
        let opened = tokio::time::Instant::now();
        h.fake.open_gate(RECONFIRM);
        let consume = format!(
            "@{} codex-renewal consume id={id} turn={wall}",
            h.fake.sid()
        );
        let done = || {
            h.mock.requests().len() == requests
                && h.fake.requests().contains(&consume)
                && h.shared.owed_act().is_none()
        };
        while !done() {
            assert!(
                opened.elapsed() < WITHIN,
                "the recovered turn was not admitted and consumed within {} s ({} model \
                 requests): {:#?}",
                WITHIN.as_secs(),
                h.mock.requests().len(),
                h.autopilot.supervisor().journal()
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let requests = h.mock.requests();
        assert_eq!(
            requests[requests.len() - 3].body_json()["input"],
            requests[requests.len() - 1].body_json()["input"],
            "the walled request's own input goes again"
        );
        assert_eq!(h.shared.latest_turn(&h.thread).as_deref(), Some(wall));
        // The autopilot started this turn, so its end is still queued: taken here, it is not
        // the end the next walled turn's submit waits for.
        wait_for_event(
            &h.test.codex,
            |event| matches!(event, EventMsg::TurnComplete(done) if done.turn_id == wall),
        )
        .await;
        wait_until("the recovered turn ended", || {
            !h.shared.turn_running_on(&h.thread)
        })
        .await;
    }
}

/// TUI EVENTS DURING THE ADMISSION'S HOST READS DO NOT REFUSE IT (the refuter's test). The fake
/// host holds a readiness check's re-confirm while the TUI's own hook,
/// `codex_alab_tui::upgrade::native_event` (what every App and app-server event the TUI
/// dispatches calls), runs: once at the admission's first check, and once at its last, after
/// the recovered turn's reservation was written. The admission the event came during is the
/// one that starts the turn: the host is sent its three re-confirms and no fourth before the
/// consume (a look queued behind a refused one would send its own and hide the refusal), and
/// no refusal is journaled. The hook acts on the process's one shared state, so this test runs
/// on it.
#[test]
fn tui_events_during_the_successors_admission_do_not_refuse_it() {
    big_stack(|| async {
        let successor = Successor::start("tui-event", codex_alab_core::session::shared(), 2).await;
        let h = &successor.h;
        for (index, passes) in [0, 2].into_iter().enumerate() {
            let id = format!("{:032x}", 0xa000 + index);
            let wall = successor
                .walled(&format!("Recover original request {index}."), &id)
                .await;
            assert_eq!(h.mock.requests().len(), index * 3 + 2);
            let before = h.fake.gated_requests().len();
            let since = h.fake.requests().len();
            h.fake.gate_after(RECONFIRM, passes);
            successor.renewal_on();
            successor.held(before, FIRST).await;
            if passes == 0 {
                assert_eq!(successor.reserved(), None, "the admission's first check");
            } else {
                assert_eq!(
                    successor.reserved().as_deref(),
                    Some(wall.as_str()),
                    "the admission's last check, after the reservation was written"
                );
            }
            let (scan, admission) = (
                h.shared.upgrade_generation(),
                h.shared.admission_generation(),
            );
            codex_alab_tui::upgrade::native_event();
            assert_ne!(
                h.shared.upgrade_generation(),
                scan,
                "POSITIVE CONTROL: the TUI's hook moved this process's upgrade scan"
            );
            assert_eq!(
                h.shared.admission_generation(),
                admission,
                "and left the native point the admission is fenced on"
            );
            successor.admitted(&wall, &id, index * 3 + 3).await;
            assert_eq!(
                successor.reconfirms(since, &wall, &id),
                3,
                "the admission the event came during went, and no second one: {:#?}",
                h.fake.requests()
            );
            assert!(
                !journal_has(h, REFUSED),
                "{:#?}",
                h.autopilot.supervisor().journal()
            );
            successor.renewal_off();
        }
        h.test.codex.shutdown_and_wait().await.expect("shutdown");
    });
}

/// A pass that comes this soon after a refusal was queued behind the refused one (a policy
/// change's look, the login watch's); the retry's own timer takes a second at least.
const QUEUED: Duration = Duration::from_millis(500);

/// The wait the first refusal of a series schedules (`admission_retry`'s first step).
const FIRST_RETRY: Duration = Duration::from_secs(1);

/// A REFUSED ADMISSION SAYS WHY AND GOES AGAIN WITHIN SECONDS. Another loaded thread starts
/// while the host holds the admission's first re-confirm (`on_thread_start`'s
/// `invalidate_native`): a native fact the admission must not act across, so it is refused.
/// Each look queued behind the refused pass is refused the same way, until a pass comes that
/// only the retry's timer can have asked for: seconds after the refusal, not at the login's
/// minute. That one's turn goes, and is consumed. The journal names core's refusal with the
/// admission's own reason, once for the whole series.
///
/// THE TIMER THE FIRST REFUSAL LEAVES IS THE RETRY'S OWN, a second away, and not the bound
/// receipt's 5 s recheck (review of the admission retry, finding 5): a pass that dropped the
/// retry and fell through to that recheck still came inside [`WITHIN`], and the journal still
/// named the second, so nothing failed. The refusal schedules its retry before it journals
/// it; the timer pending once the line is there, and still a moment later, is due within the
/// retry's first step. That bound holds however late this test reads it, since time only
/// brings a deadline nearer.
#[test]
fn a_refused_successor_admission_is_logged_and_retried_within_seconds() {
    big_stack(|| async {
        let successor =
            Successor::start("refused-fact", Box::leak(Box::new(Shared::new())), 1).await;
        let h = &successor.h;
        let id = format!("{:032x}", 0xb000);
        let wall = successor.walled("Recover the original request.", &id).await;
        let rt = h
            .autopilot
            .thread_rt(&h.thread)
            .unwrap_or_else(|| panic!("the thread's runtime"));
        let before = h.fake.gated_requests().len();
        h.fake.gate(RECONFIRM);
        successor.renewal_on();
        let mut refusals = 0;
        loop {
            let within = if refusals == 0 { FIRST } else { WITHIN };
            let waited = successor.held(before + refusals, within).await;
            if refusals > 0 && waited >= QUEUED {
                break;
            }
            assert!(
                refusals < 8,
                "looks kept coming at once: {:#?}",
                h.autopilot.supervisor().journal()
            );
            assert_eq!(successor.reserved(), None, "the admission's first check");
            h.shared.invalidate_native();
            h.fake.release_held(RECONFIRM);
            refusals += 1;
            if refusals == 1 {
                wait_until("the first refusal is journaled", || journal_has(h, REFUSED)).await;
                for _ in 0..2 {
                    if let Some(due) = rt.timer_due() {
                        let left = due
                            .saturating_duration_since(codex_alab_autopilot::supervisor::clock());
                        assert!(
                            left <= FIRST_RETRY,
                            "the timer the first refusal left is due in {left:?}, not within \
                             the retry's {FIRST_RETRY:?}: {:#?}",
                            h.autopilot.supervisor().journal()
                        );
                    }
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            }
        }
        successor.admitted(&wall, &id, 3).await;
        let journal = h.autopilot.supervisor().journal();
        let said: Vec<_> = journal
            .iter()
            .filter(|line| line.contains(REFUSED))
            .collect();
        assert_eq!(
            said.len(),
            1,
            "{refusals} refusals, one reason: {journal:#?}"
        );
        assert!(
            said[0].contains("not durably admitted: renewal admission changed")
                && said[0].contains("the next pass comes in 1000 ms"),
            "core's refusal and the admission's own reason: {}",
            said[0]
        );
        h.test.codex.shutdown_and_wait().await.expect("shutdown");
    });
}

/// The journal's line for a recovered turn core is not asked for while the TUI itself waits.
const WAITS: &str =
    "the recovered turn waits (a draft or an unrendered key stands in the composer)";

/// A PERSON'S DRAFT HAS CORE ASKED FOR NO RECOVERED TURN (review of the admission retry,
/// finding 3). The successor holds a bound, committed receipt and a person's key stands
/// undrawn in the composer. The admission's readiness waits on that, and it took core to find
/// out: each look had core reserve the turn and write its settings into the rollout before the
/// admission refused, and with looks seconds apart that went on for as long as the draft
/// stood. The TUI's own wait is read first now: four looks read the receipt, the journal says
/// once what the turn waits for and that the next pass is a second away (the look is paced as
/// a refusal is, since no frame's change wakes the point), nothing is reserved and no refusal
/// is journaled. POSITIVE CONTROL: once a frame has drawn the composer empty, the turn goes
/// within seconds with no look asked for.
#[test]
fn a_persons_draft_has_core_asked_for_no_recovered_turn() {
    big_stack(|| async {
        let successor = Successor::start("draft", Box::leak(Box::new(Shared::new())), 1).await;
        let h = &successor.h;
        let id = format!("{:032x}", 0xd000);
        let wall = successor.walled("Recover the original request.", &id).await;
        h.shared.human_input(now_ms());
        let status = format!("@{} codex-renewal status id={id}", h.fake.sid());
        let reads = || {
            h.fake
                .requests()
                .iter()
                .filter(|request| **request == status)
                .count()
        };
        // Each look starts only once the one before it is over, whatever that one asked of
        // core: the fourth's read says the first three are.
        for _ in 0..4 {
            let before = reads();
            successor.renewal_on();
            wait_until("the bound receipt is read again", || reads() > before).await;
        }
        let journal = h.autopilot.supervisor().journal();
        assert!(
            !journal
                .iter()
                .any(|line| line.contains(REFUSED) || line.contains("was not submitted")),
            "core was asked for the recovered turn under a person's draft: {journal:#?}"
        );
        let said: Vec<_> = journal.iter().filter(|line| line.contains(WAITS)).collect();
        assert_eq!(
            said.len(),
            1,
            "what the turn waits for, said once: {journal:#?}"
        );
        assert!(
            said[0].contains("core is not asked, and the next pass comes in 1000 ms"),
            "the first look the wait stopped is followed a second later: {}",
            said[0]
        );
        assert_eq!(successor.reserved(), None, "no recovered turn was reserved");
        assert_eq!(h.mock.requests().len(), 2, "and none started");
        h.shared.observe(
            &Frame {
                primary_thread: Some(&h.thread),
                displayed_thread: Some(&h.thread),
                rollout_path: None,
                cwd: h.test.cwd_path(),
                model: "gpt-5",
                codex_home: h.test.codex_home_path(),
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
        // Nothing asks for a look from here: the wait's own pacing brings the pass that finds
        // the composer empty, seconds later and not at a draft's minute.
        successor.admitted(&wall, &id, 3).await;
        h.test.codex.shutdown_and_wait().await.expect("shutdown");
    });
}

/// The reason an act's checks give while a handoff has sealed the claim.
const SEALED: &str = "the claim stands sealed for an account handoff";

/// THE PROCESS WHOSE CLAIM STANDS SEALED ASKS CORE FOR NO RECOVERED TURN. A turn at a refused
/// login asks the host to renew it: the host answers `ready`, the claim is sealed to that
/// receipt and `commit` sent, and the host then answers `committed` while this process still
/// runs (it ends it and starts the successor a moment later). A same-account renewal's
/// destination is this very login, so each look at the bound receipt, now seconds apart and a
/// second after a refusal, would ask core for the recovered turn: core reserved it and wrote
/// its settings into the rollout the successor resumes from, and only then did the admission
/// wait on the seal and refuse. No recovery is asked while the seal stands: two looks read
/// `committed`, nothing is reserved, and no refusal is journaled.
#[test]
fn a_sealed_source_asks_core_for_no_recovered_turn() {
    big_stack(|| async {
        let (login, identity, _refresh) = subscription_fixture("sealed-source").await;
        let h = harness_login(
            Mode::Current,
            vec![refused(), refused()],
            &[
                ("continue", "false"),
                ("human_grace_s", "0"),
                ("logins", "renew"),
            ],
            &[],
            Some(0),
            Some(login),
            Timing {
                min_work: Duration::ZERO,
                ..Timing::default()
            },
            |_| {},
            None,
        )
        .await;
        h.fake.gate("codex-renewal");
        h.test
            .submit_turn("Recover this original request.")
            .await
            .unwrap_or_else(|err| panic!("the refused login's turn: {err}"));
        let wall = h
            .shared
            .latest_turn(&h.thread)
            .unwrap_or_else(|| panic!("the walled turn has no id"));
        let id = format!("{:032x}", 0xc000);
        let receipt = format!(
            "id={id} thread={} wall={wall} account={} user={}",
            h.thread, identity.account, identity.user
        );
        let sid = h.fake.sid();
        let prepare = format!(
            "@{sid} codex-renewal prepare thread={} wall={wall} account={} user={}",
            h.thread, identity.account, identity.user
        );
        wait_until("the renewal is asked for", || {
            !h.fake.gated_requests().is_empty()
        })
        .await;
        h.fake.reply_once(
            &prepare,
            &format!("OK codex-renewal state=ready {receipt}\n"),
        );
        let status = format!("@{sid} codex-renewal status id={id}");
        h.fake.reply_always(
            &status,
            &format!("OK codex-renewal state=committed {receipt}\n"),
        );
        h.fake.open_gate("codex-renewal");
        let commit = format!("@{sid} codex-renewal commit id={id}");
        wait_until("the bound receipt is committed", || {
            h.fake.requests().contains(&commit)
        })
        .await;
        let sup = h.autopilot.supervisor();
        let ready = sup.ready().await;
        assert!(
            matches!(&ready, codex_alab_autopilot::supervisor::Ready::Wait { why, .. }
                if why == SEALED),
            "POSITIVE CONTROL: the committed renewal has sealed the claim: {ready:?}"
        );
        // Two looks at the bound receipt, each asked for by a setting changed beside it: the
        // second starts only once the first is over, whatever the first asked of core.
        let reads = || {
            h.fake
                .requests()
                .iter()
                .filter(|request| **request == status)
                .count()
        };
        for look in 1..=2 {
            let before = reads();
            h.fake
                .set_policy_key("continue_per_hour", &look.to_string());
            wait_until("the bound receipt is read again", || reads() > before).await;
        }
        let before = reads();
        h.fake.set_policy_key("continue_per_hour", "3");
        wait_until("the looks before it are over", || reads() > before).await;
        let journal = sup.journal();
        assert!(
            !journal
                .iter()
                .any(|line| line.contains(REFUSED) || line.contains("was not submitted")),
            "the sealed source asked core for the recovered turn: {journal:#?}"
        );
        assert_eq!(
            h.shared
                .owed_act()
                .and_then(|owed| owed.renewal)
                .and_then(|renewal| renewal.turn),
            None,
            "no recovered turn was reserved"
        );
        assert_eq!(h.mock.requests().len(), 2, "and none started");
        assert!(
            matches!(
                sup.ready().await,
                codex_alab_autopilot::supervisor::Ready::Wait { why, .. } if why == SEALED
            ),
            "the seal still stands"
        );
        h.test.codex.shutdown_and_wait().await.expect("shutdown");
    });
}
