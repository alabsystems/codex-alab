// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::upgrade_handoff_tests::Harness;
use super::upgrade_handoff_tests::native_test;
use super::upgrade_handoff_tests::wait_for_supervisor;
use super::*;
use codex_alab_core::session::Pace;
use codex_alab_core::session::Recovery;
use codex_alab_core::session::RecoveryWall;
use codex_alab_core::session::WallMark;
use core_test_support::responses::received_responses_requests;

fn identity(account: &str, owner: u64) -> QuotaIdentity {
    QuotaIdentity {
        account: account.into(),
        user: Some("native-user".into()),
        owner,
    }
}

/// The wall login A's record carried into the relaunch.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Carried {
    /// A's weekly usage wall, met by a person's turn: no try past it yet.
    Usage,
    /// The same wall met again by codex-alab's resume past it: the record's pace holds A's
    /// ladder on its first rung, counted from that answer a minute before the move.
    UsageRetried,
    /// A's missing-subscription refusal (`UsageNotIncluded`): an entitlement wall whose
    /// ordinary-usage denial the provider's error itself witnessed.
    NotIncluded,
}

/// A process aterm relaunched at a recorded usage or entitlement wall, after the real resume
/// scan.
struct Relaunched {
    fake: codex_alab_aterm::fake::FakeAterm,
    shared: &'static codex_alab_core::session::Shared,
    autopilot: Arc<Autopilot>,
    rt: Arc<ThreadRt>,
    store: ExtensionData,
    /// The reset of the wall the record carried, the login moved from's; for an entitlement
    /// wall, which names none, the deadline of its blind try.
    reset: Instant,
    _dir: tempfile::TempDir,
}

impl Relaunched {
    /// The waits decided for the usage resume so far (the evaluator's own notes).
    fn waits(&self) -> usize {
        let wait = format!("{}: wait ", self.rt.key());
        let rule = format!("for {} (", words::RULE_LIMIT_RESUME);
        self.autopilot
            .sup
            .journal()
            .iter()
            .filter(|note| note.starts_with(&wait) && note.contains(&rule))
            .count()
    }

    /// The restored wall's wait stands: the usage resume, at the old login's reset.
    fn waits_out_the_old_reset(&self, why: &str) {
        let owed = self.rt.owed_timer().unwrap_or_else(|| {
            panic!(
                "{why}: no wait; state={:?} journal={:#?}",
                self.rt.state(),
                self.autopilot.sup.journal()
            )
        });
        assert_eq!(owed.rule, words::RULE_LIMIT_RESUME, "{why}");
        assert!(
            owed.not_before >= self.reset,
            "{why}: the wait ends before the old reset: {owed:?}"
        );
    }

    /// Interpret a saved bound on its own clock, with the sample preceding the Instant.
    fn recorded_bound(&self, recovery: &Recovery, at_ms: u64) -> (u64, Instant) {
        let sample = self
            .autopilot
            .retry_clock_sample()
            .expect("the fixture's current retry clock");
        let now = clock();
        let anchor = recovery.clock.as_ref().expect("the saved retry clock");
        let epoch = anchor.epoch_at(&sample).expect("the same fixture clock");
        let at = anchor
            .instant_of(at_ms, &sample, now)
            .expect("the saved bound fits the fixture clock");
        (epoch, at)
    }

    async fn stop(self) {
        Autopilot::cancel_timer(&self.rt).await;
        self.autopilot
            .on_thread_stop(ThreadStopInput {
                session_store: &ExtensionData::new("cleanup"),
                thread_store: &self.store,
            })
            .await;
        drop(self.fake);
    }
}

/// aterm's `/accounts use <label>` or `/accounts auto` at a recorded usage wall (audit of
/// 2026-11-14, gap 3): the session waits at login A's usage wall, its record holding the wall's
/// `recovery` scoped to A; aterm stops it (SIGTERM) and types the record's own `resumeArgv`
/// under its lease, with only `CODEX_AUTH_PATH` changed. This is the new process: alab-aterm's
/// fake host, the dead record swept, the launch probe's witness, and the real resume scan over
/// the failed turn's history, under the login `current` the process runs with.
async fn relaunched_at_usage_wall(current: QuotaIdentity) -> Relaunched {
    relaunched_at(current, Carried::Usage).await
}

/// [`relaunched_at_usage_wall`] at the wall `carried` names, the record's pace held in the
/// record writer's custody as its start leaves it where the record carries one.
async fn relaunched_at(current: QuotaIdentity, carried: Carried) -> Relaunched {
    use codex_alab_core::session::HandPid;
    use codex_alab_core::session::HumanSeen;
    use codex_alab_core::session::Launch;
    use codex_alab_core::session::LaunchWitness;
    use codex_alab_core::session::Predecessor;

    const DAY_MS: u64 = 86_400_000;
    const MIN_MS: u64 = 60_000;
    codex_alab_core::activate();
    let dir = tempfile::tempdir().expect("scratch aterm");
    let fake =
        codex_alab_aterm::fake::FakeAterm::start(dir.path(), codex_alab_aterm::fake::Mode::Current)
            .await
            .expect("fake aterm");
    for (key, value) in [
        ("human_grace_s", "0"),
        ("resume_limits", "true"),
        ("limit_wait", "true"),
        ("probe_api", "false"),
        ("upgrade", "false"),
    ] {
        fake.set_policy_key(key, value);
    }
    let shared: &'static codex_alab_core::session::Shared =
        Box::leak(Box::new(codex_alab_core::session::Shared::new()));
    let id = ThreadId::new();
    let key = id.to_string();
    let started = now_ms();
    let written_at = started - 1_000;
    let point_at_ms = written_at - MIN_MS;
    let line = vec!["/abs/bin/codex".to_string(), "resume".into(), key.clone()];
    shared.set_launch(Launch {
        args: line.iter().map(Into::into).collect(),
        started_at_ms: started,
        ..Default::default()
    });
    // A's scope; `ordinary`: an ordinary-usage retry whose denial was witnessed (a
    // `UsageNotIncluded` wall's, as `on_turn_error` binds it).
    let scope = |ordinary: bool| {
        Some(codex_alab_core::quota::Scope {
            account: "account-a".into(),
            bucket: "codex".into(),
            user: Some("user-a".into()),
            ordinary_retry: ordinary,
            ordinary_denied: ordinary,
        })
    };
    // The deadline the record carries: the usage wall's reset, or the entitlement wall's try.
    let (reset_ms, wall) = match carried {
        Carried::Usage | Carried::UsageRetried => {
            let reset_ms = written_at + 3 * DAY_MS;
            let wall = RecoveryWall::Usage {
                reset_at_ms: Some(reset_ms),
                reset_label: Some("the old login's reset".into()),
                quota: scope(false),
            };
            (reset_ms, wall)
        }
        // The blind try's deadline: the entitlement ladder's first rung
        // (`Timing::entitlement_backoff`) from the point.
        Carried::NotIncluded => (
            point_at_ms + 30 * MIN_MS,
            RecoveryWall::Entitlement { quota: scope(true) },
        ),
    };
    // A's ladder on its first rung: one resume typed past the wall, met again at the point.
    let pace = (carried == Carried::UsageRetried).then(|| Pace {
        wall: Some(WallMark {
            class: "usage".into(),
            attempts: 1,
            first_seen_ms: written_at - 2 * DAY_MS,
            last_try_ms: Some(point_at_ms - 5_000),
            since_ms: Some(point_at_ms),
            answer_owed: false,
        }),
        ..Pace::default()
    });
    shared.note_predecessor(Predecessor {
        pid: 77,
        session_id: key.clone(),
        exit: Some("signal:15".into()),
        resume_argv: Some(line),
        status: Some("idle".into()),
        updated_at_ms: written_at,
        pace: pace.clone(),
        recovery: Some(Recovery {
            clock: crate::recovery::test_clock(0),
            thread: key.clone(),
            turn_id: "walled-turn".into(),
            point_at_ms,
            not_before_ms: reset_ms,
            by_autopilot: carried == Carried::UsageRetried,
            by_person: carried != Carried::UsageRetried,
            wall,
        }),
        ..Predecessor::default()
    });
    if let Some(pace) = pace {
        shared.set_pace(&key, pace);
    }
    shared.set_swept();
    shared.set_launch_witness(Some(LaunchWitness {
        hand: Some("lease:aterm-harness@812".into()),
        human: HumanSeen::Never,
        read_at_ms: started + 1,
        server_pid: Some(812),
        hand_pid: HandPid::Absent,
    }));
    let endpoint = fake.endpoint();
    let sup = crate::Supervisor::new(crate::SupervisorOptions {
        shared,
        endpoint: Arc::new(move || Some(endpoint.clone())),
        client: codex_alab_aterm::Options::default(),
    });
    let autopilot = Autopilot::new(Arc::clone(&sup), None, None, Timing::default(), |_| true);
    autopilot.set_retry_clock(crate::recovery::test_clock_read());
    *lock(&autopilot.quota_test_identity) = Some(current);
    let store = ExtensionData::new(key.clone());
    store.insert(ThreadRt::new(id, None, false, Duration::ZERO));
    let rt = store.get::<ThreadRt>().expect("inserted");
    autopilot.keep_thread(&rt);
    // The quota waker, as a live process installs it.
    autopilot.watch_claims();
    sup.start();
    shared.observe(
        &codex_alab_core::session::Frame {
            primary_thread: Some(&key),
            displayed_thread: Some(&key),
            rollout_path: None,
            cwd: dir.path(),
            model: "gpt-5",
            codex_home: dir.path(),
            embedded: true,
            state: codex_alab_core::session::ProgramState::Idle,
            draft_empty: true,
            queued_input: false,
            popup_open: false,
            editor_open: false,
            pending_user_turn: false,
        },
        now_ms(),
    );
    shared.note_root_thread(&key);
    wait_for_supervisor(|| autopilot.sup.acting()).await;
    let event = |value: serde_json::Value| {
        codex_history::RolloutItem::EventMsg(
            serde_json::from_value::<EventMsg>(value).expect("native event"),
        )
    };
    autopilot.set_history(vec![
        event(serde_json::json!({
            "type": "task_started",
            "turn_id": "walled-turn",
            "model_context_window": 258400,
            "collaboration_mode_kind": "default",
            "turn_attribution": {"turn_id": "walled-turn", "turn_trigger": "user"}
        })),
        event(serde_json::json!({
            "type": "task_complete",
            "turn_id": "walled-turn",
            "last_agent_message": null,
            "error": {
                "message": "the usage limit was reached",
                "codex_error_info": "usage_limit_exceeded"
            }
        })),
    ]);
    rt.resumed.store(true, Ordering::SeqCst);
    Arc::clone(&autopilot).resume_scan(Arc::clone(&rt)).await;
    let state = rt.state();
    let reset = match (carried, state.point.as_ref().and_then(|p| p.wall.as_ref())) {
        (Carried::Usage | Carried::UsageRetried, Some(Wall::Usage { reset_at, .. })) => *reset_at,
        (Carried::NotIncluded, Some(Wall::Entitlement)) => state.recovery_not_before,
        _ => None,
    };
    let Some(reset) = reset else {
        panic!(
            "the relaunch stands at the carried wall: {state:?} {:#?}",
            autopilot.sup.journal()
        );
    };
    Relaunched {
        fake,
        shared,
        autopilot,
        rt,
        store,
        reset,
        _dir: dir,
    }
}

/// Moved to login B at A's recorded usage wall, the relaunch waits at A's reset until B's
/// next accepted read: a read that denies B's bucket keeps that wait, and a read that permits
/// it types the usage resume at once, days before A's reset (audit of 2026-11-14, gap 3).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_move_at_a_recorded_usage_wall_resumes_on_the_new_logins_read() {
    tokio::time::timeout(Duration::from_secs(120), async {
        let moved_to = QuotaIdentity {
            account: "account-b".into(),
            user: Some("user-b".into()),
            owner: 0,
        };
        let h = relaunched_at_usage_wall(moved_to).await;
        h.waits_out_the_old_reset("no read of the login moved to yet");
        let quota = h.shared.quota();

        // B's accepted read that denies the wall's bucket: looked at again, the wait stands.
        let waits = h.waits();
        quota.start_read(1, clock());
        quota.accept_observation(
            1,
            "account-b",
            vec![],
            Some(true),
            vec!["codex".into()],
            clock(),
        );
        tokio::time::timeout(
            Duration::from_secs(30),
            wait_for_supervisor(|| h.waits() > waits),
        )
        .await
        .unwrap_or_else(|_| {
            panic!(
                "B's accepted read never woke the restored wait: binding={:?} journal={:#?}",
                lock(&h.rt.quota_wall).as_ref().map(|b| b.identity.clone()),
                h.autopilot.sup.journal()
            )
        });
        h.waits_out_the_old_reset("B's read denies the bucket");

        // B's accepted read with room: the resume is decided now, and goes to its act.
        let (entered, entry) = tokio::sync::oneshot::channel();
        let (release, blocked) = tokio::sync::oneshot::channel();
        *lock(&h.rt.before_ready) = Some((entered, blocked));
        quota.start_read(2, clock());
        quota.accept_observation(
            2,
            "account-b",
            vec!["codex".into()],
            Some(true),
            vec![],
            clock(),
        );
        tokio::time::timeout(Duration::from_secs(30), entry)
            .await
            .unwrap_or_else(|_| {
                panic!(
                    "B's read with room never brought the usage resume to its act: \
                     state={:?} journal={:#?}",
                    h.rt.state(),
                    h.autopilot.sup.journal()
                )
            })
            .expect("B's read brought the usage resume to its act");
        let recovery = h.shared.recovery().expect("the resume's wall on record");
        let RecoveryWall::Usage {
            reset_at_ms,
            quota: scope,
            ..
        } = &recovery.wall
        else {
            panic!("a usage wall on record: {recovery:?}");
        };
        let (record_now, not_before) = h.recorded_bound(&recovery, recovery.not_before_ms);
        assert!(
            recovery.not_before_ms <= record_now && not_before < h.reset,
            "usage-resume@v1 is decided now, before the old reset: {recovery:?}"
        );
        let reset_at = reset_at_ms
            .map(|at| h.recorded_bound(&recovery, at).1)
            .expect("the old reset stays on record");
        // checked_epoch_of's extra millisecond covers the fixture counter's truncation;
        // sampling pauses can extend the saved reset but cannot move it earlier.
        assert!(
            reset_at >= h.reset
                && reset_at.saturating_duration_since(h.reset) < Duration::from_secs(1),
            "the old reset stays the latest: {reset_at:?}, carried {:?}",
            h.reset
        );
        assert_eq!(
            scope.as_ref().map(|scope| scope.account.as_str()),
            Some("account-b"),
            "the record now scopes the wall to the login moved to"
        );
        let _ = release.send(());
        h.stop().await;
    })
    .await
    .expect("the moved relaunch hung (a hang detector; the waits above name their step)");
}

/// CONTROL: the same relaunch on the login that met the wall, with no read since, still waits
/// out its reset (the same-identity rule: only a read started after the restart answers it).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_relaunch_on_the_walled_login_with_no_new_read_waits_out_its_reset() {
    tokio::time::timeout(Duration::from_secs(60), async {
        let h = relaunched_at_usage_wall(QuotaIdentity {
            account: "account-a".into(),
            user: Some("user-a".into()),
            owner: 0,
        })
        .await;
        h.waits_out_the_old_reset("the walled login, no read since");
        assert_eq!(
            lock(&h.rt.quota_wall)
                .as_ref()
                .map(|binding| binding.identity.account.clone()),
            Some("account-a".to_string()),
            "the walled login stays bound"
        );
        h.stop().await;
    })
    .await
    .expect("the same-login relaunch hung");
}

/// Moved to login B at A's usage wall that A's own resume met again a minute before the move
/// (the record's pace on A's first rung), B's first accepted read with room types the usage
/// resume at once: that rung counted A's tries, and B starts its own ladder at no try, which
/// the record then carries (review of gap 3's fix, finding 1: B's permitting read waited
/// 538 s "for retry pacing"). CONTROL: [`a_retried_usage_wall_on_its_own_login_waits_its_rung`].
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_move_at_a_retried_usage_wall_resumes_at_once_on_the_new_logins_read() {
    a_retried_wall_moved_to_resumes_at_once(QuotaIdentity {
        account: "account-b".into(),
        user: Some("user-b".into()),
        owner: 0,
    })
    .await;
}

/// The usual multi-subscription move: one person in two workspaces keeps one ChatGPT user id
/// under two account ids, so a move between them changes the account alone. That is another
/// login, and its ladder starts over as any other's: B's first accepted read with room types
/// the usage resume at once, not on A's rung (independent review of `1d171802e3`, minor 2:
/// every move test changed account and user together, so a move check that wanted both to
/// differ passed them all).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_move_to_another_account_of_the_same_user_starts_the_ladder_over() {
    a_retried_wall_moved_to_resumes_at_once(QuotaIdentity {
        account: "account-b".into(),
        user: Some("user-a".into()),
        owner: 0,
    })
    .await;
}

/// Another person's login in the same workspace: the account stays, the native user changes.
/// That is another login too, and its first accepted read with room resumes at once.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_move_to_another_user_of_the_same_account_starts_the_ladder_over() {
    a_retried_wall_moved_to_resumes_at_once(QuotaIdentity {
        account: "account-a".into(),
        user: Some("user-b".into()),
        owner: 0,
    })
    .await;
}

/// [`relaunched_at`] the retried usage wall under `moved_to`, a login other than the record
/// scope's (`account-a`, `user-a`) by its account, its native user or both: `moved_to`'s first
/// accepted read with room brings the usage resume to its act at once, on its own ladder at no
/// try, which the record then carries.
async fn a_retried_wall_moved_to_resumes_at_once(moved_to: QuotaIdentity) {
    tokio::time::timeout(Duration::from_secs(120), async {
        let account = moved_to.account.clone();
        let h = relaunched_at(moved_to, Carried::UsageRetried).await;
        h.waits_out_the_old_reset("no read of the login moved to yet");
        let (entered, entry) = tokio::sync::oneshot::channel();
        let (release, blocked) = tokio::sync::oneshot::channel();
        *lock(&h.rt.before_ready) = Some((entered, blocked));
        let quota = h.shared.quota();
        quota.start_read(1, clock());
        quota.accept_observation(
            1,
            &account,
            vec!["codex".into()],
            Some(true),
            vec![],
            clock(),
        );
        tokio::time::timeout(Duration::from_secs(30), entry)
            .await
            .unwrap_or_else(|_| {
                panic!(
                    "B's read with room never brought the usage resume to its act: \
                     state={:?} journal={:#?}",
                    h.rt.state(),
                    h.autopilot.sup.journal()
                )
            })
            .expect("B's read brought the usage resume to its act");
        let recovery = h.shared.recovery().expect("the resume's wall on record");
        let (record_now, _) = h.recorded_bound(&recovery, recovery.not_before_ms);
        assert!(
            recovery.not_before_ms <= record_now,
            "usage-resume@v1 is decided now, not on A's rung: {recovery:?}"
        );
        let track = h.rt.state().wall;
        assert_eq!(
            track
                .as_ref()
                .map(|track| (track.carried_class(), track.attempts)),
            Some((Some("usage"), 0)),
            "B's own ladder, at no try, which this try climbs: {track:?}"
        );
        assert_eq!(
            h.shared
                .pace(h.rt.key())
                .and_then(|pace| pace.wall)
                .map(|wall| wall.attempts),
            Some(0),
            "the record carries B's ladder, not A's rung"
        );
        let _ = release.send(());
        h.stop().await;
    })
    .await
    .expect("the moved relaunch hung (a hang detector; the waits above name their step)");
}

/// CONTROL: the same retried wall relaunched on the login that met it keeps the rung the record
/// carried: that login's accepted read with room waits out its first rung, ten minutes from
/// the appearance that answered its try, before the resume (the same-login pacing unchanged).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_retried_usage_wall_on_its_own_login_waits_its_rung() {
    tokio::time::timeout(Duration::from_secs(60), async {
        let walled = QuotaIdentity {
            account: "account-a".into(),
            user: Some("user-a".into()),
            owner: 0,
        };
        let h = relaunched_at(walled, Carried::UsageRetried).await;
        h.waits_out_the_old_reset("the walled login, no read since");
        let quota = h.shared.quota();
        quota.start_read(1, clock());
        quota.accept_observation(
            1,
            "account-a",
            vec!["codex".into()],
            Some(true),
            vec![],
            clock(),
        );
        // A's read with room moves the wait off the old reset, onto the rung.
        wait_for_supervisor(|| {
            h.rt.owed_timer()
                .is_some_and(|owed| owed.not_before < h.reset)
        })
        .await;
        let owed = h.rt.owed_timer().expect("the rung's wait");
        assert_eq!(owed.rule, words::RULE_LIMIT_RESUME);
        assert!(
            owed.not_before > clock() + Duration::from_secs(8 * 60),
            "A's rung paces its resume: {owed:?}"
        );
        let journal = h.autopilot.sup.journal();
        assert!(
            journal
                .iter()
                .rev()
                .find(|note| note.contains(&format!("for {} (", words::RULE_LIMIT_RESUME)))
                .is_some_and(|note| note.contains("waiting for retry pacing")),
            "the wait names the rung: {journal:#?}"
        );
        assert_eq!(
            h.rt.state().wall.map(|track| track.attempts),
            Some(1),
            "the walled login's rung stands"
        );
        h.stop().await;
    })
    .await
    .expect("the same-login retried relaunch hung (a hang detector)");
}

/// Moved to login B at A's missing-subscription wall (`UsageNotIncluded`: an entitlement wall
/// whose ordinary-usage denial A's provider error witnessed), B's first accepted read that
/// permits ordinary usage brings the wall's try to its act, long before its blind try: the
/// denial travels with the wall, so B's permitted ordinary usage is its denied-to-allowed
/// transition though no read of B's ever showed a denial (CONTRACT §5, the quota wake). Until
/// that read the blind try's deadline on record stands.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_move_at_a_witnessed_entitlement_wall_resumes_on_the_new_logins_ordinary_usage() {
    tokio::time::timeout(Duration::from_secs(120), async {
        let moved_to = QuotaIdentity {
            account: "account-b".into(),
            user: Some("user-b".into()),
            owner: 0,
        };
        let h = relaunched_at(moved_to, Carried::NotIncluded).await;
        h.waits_out_the_old_reset("no read of the login moved to yet");
        let (entered, entry) = tokio::sync::oneshot::channel();
        let (release, blocked) = tokio::sync::oneshot::channel();
        *lock(&h.rt.before_ready) = Some((entered, blocked));
        let quota = h.shared.quota();
        quota.start_read(1, clock());
        quota.accept_observation(
            1,
            "account-b",
            vec!["codex".into()],
            Some(true),
            vec![],
            clock(),
        );
        tokio::time::timeout(Duration::from_secs(30), entry)
            .await
            .unwrap_or_else(|_| {
                panic!(
                    "B's permitted ordinary usage never brought the entitlement wall's try to \
                     its act: binding={:?} state={:?} journal={:#?}",
                    lock(&h.rt.quota_wall).as_ref().map(|b| b.wall.clone()),
                    h.rt.state(),
                    h.autopilot.sup.journal()
                )
            })
            .expect("B's read brought the entitlement wall's try to its act");
        let recovery = h.shared.recovery().expect("the try's wall on record");
        let RecoveryWall::Entitlement { quota: scope } = &recovery.wall else {
            panic!("an entitlement wall on record: {recovery:?}");
        };
        let (record_now, not_before) = h.recorded_bound(&recovery, recovery.not_before_ms);
        assert!(
            recovery.not_before_ms <= record_now && not_before < h.reset,
            "the try is decided now, before the blind try's deadline: {recovery:?}"
        );
        assert_eq!(
            scope
                .as_ref()
                .map(|scope| (scope.account.as_str(), scope.ordinary_denied)),
            Some(("account-b", true)),
            "the record scopes the wall to B, the witnessed denial with it"
        );
        let _ = release.send(());
        h.stop().await;
    })
    .await
    .expect(
        "the moved entitlement relaunch hung (a hang detector; the waits above name their step)",
    );
}

#[test]
fn restored_quota_scope_binds_the_current_login_and_needs_its_new_read() {
    native_test(|| async {
        let h = tokio::time::timeout(Duration::from_secs(60), Harness::new())
            .await
            .expect("native fixture hung");
        h.immediate_point().await;
        let quota = h.shared.quota();
        *lock(&h.autopilot.quota_test_identity) = Some(identity("account", 4));
        let scope = codex_alab_core::quota::Scope {
            account: "account".into(),
            bucket: "premium".into(),
            user: Some("native-user".into()),
            ordinary_retry: true,
            ordinary_denied: true,
        };
        quota.start_read(1, clock());
        quota.accept_observation(
            1,
            "account",
            vec!["codex".into()],
            Some(true),
            vec![],
            clock(),
        );
        assert!(
            !h.autopilot.restore_quota_scope(&h.rt, Some(&scope)),
            "the scope's own login is no move: the rung its record carried is its own"
        );
        let wall = lock(&h.rt.quota_wall)
            .clone()
            .expect("matching persisted account scope");
        assert!(!quota.restored(&wall.wall, clock()));
        quota.start_read(2, clock());
        quota.accept_observation(
            2,
            "account",
            vec!["codex".into()],
            Some(true),
            vec![],
            clock(),
        );
        assert!(quota.restored(&wall.wall, clock()));
        assert!(quota.ordinary_restored(&wall.wall, clock()));
        // A restart on another login (aterm's `/accounts use` or `/accounts auto`) binds the
        // login it runs under: only that login's read, started after the restart, answers the
        // wall (audit of 2026-11-14, gap 3; before, nothing was bound and the old reset stood).
        // Another account of the same native user (one person in two workspaces) is another
        // login: the caller starts the wall's ladder over.
        *lock(&h.autopilot.quota_test_identity) = Some(identity("other", 5));
        assert!(
            h.autopilot.restore_quota_scope(&h.rt, Some(&scope)),
            "another account of the same user is another login"
        );
        let moved = lock(&h.rt.quota_wall)
            .clone()
            .expect("the login moved to is bound");
        assert_eq!(moved.identity, identity("other", 5));
        assert!(
            !quota.restored(&moved.wall, clock()),
            "no read since the move"
        );
        quota.start_read(3, clock());
        quota.accept_observation(
            3,
            "account",
            vec!["premium".into(), "codex".into()],
            Some(true),
            vec![],
            clock(),
        );
        assert!(
            !quota.restored(&moved.wall, clock()),
            "the login moved from answers nothing"
        );
        quota.start_read(4, clock());
        quota.accept_observation(
            4,
            "other",
            vec!["codex".into()],
            Some(true),
            vec!["premium".into()],
            clock(),
        );
        assert!(
            !quota.restored(&moved.wall, clock()),
            "a read that denies the wall's bucket keeps the wait"
        );
        quota.start_read(5, clock());
        quota.accept_observation(
            5,
            "other",
            vec!["premium".into()],
            Some(true),
            vec![],
            clock(),
        );
        assert!(quota.restored(&moved.wall, clock()));
        let mut other_user = identity("account", 6);
        other_user.user = Some("other-user".into());
        *lock(&h.autopilot.quota_test_identity) = Some(other_user.clone());
        assert!(
            h.autopilot.restore_quota_scope(&h.rt, Some(&scope)),
            "another user of the same account is another login"
        );
        let moved = lock(&h.rt.quota_wall)
            .clone()
            .expect("another user of the account is bound");
        assert_eq!(moved.identity, other_user);
        assert!(
            !quota.restored(&moved.wall, clock()),
            "no read since the move"
        );
        // The scope's own login under a refreshed owner generation (its token renewed) is bound
        // in that generation, and is no move.
        *lock(&h.autopilot.quota_test_identity) = Some(identity("account", 6));
        assert!(
            !h.autopilot.restore_quota_scope(&h.rt, Some(&scope)),
            "a refreshed generation of the scope's login is no move"
        );
        assert_eq!(
            lock(&h.rt.quota_wall)
                .as_ref()
                .map(|binding| binding.identity.clone()),
            Some(identity("account", 6)),
            "the refreshed generation is bound"
        );
        // Nothing bound is no move, whatever login the process runs under: a record with no
        // scope, an invalid scope (this one of another account), or no login known.
        assert!(
            !h.autopilot.restore_quota_scope(&h.rt, None),
            "a record with no scope"
        );
        assert!(lock(&h.rt.quota_wall).is_none());
        let invalid = codex_alab_core::quota::Scope {
            account: "account-elsewhere".into(),
            user: Some("bad\nuser".into()),
            ..scope.clone()
        };
        assert!(!invalid.valid());
        assert!(
            !h.autopilot.restore_quota_scope(&h.rt, Some(&invalid)),
            "an invalid scope"
        );
        assert!(lock(&h.rt.quota_wall).is_none());
        *lock(&h.autopilot.quota_test_identity) = None;
        assert!(
            h.autopilot.quota_identity().is_none(),
            "the fixture's API key names no login"
        );
        let elsewhere = codex_alab_core::quota::Scope {
            account: "account-elsewhere".into(),
            ..scope
        };
        assert!(
            !h.autopilot.restore_quota_scope(&h.rt, Some(&elsewhere)),
            "no login known"
        );
        assert!(lock(&h.rt.quota_wall).is_none());
    });
}

#[test]
fn accepted_post_wall_allowance_wakes_one_native_recovery_before_reset() {
    native_test(|| async {
        let h = tokio::time::timeout(Duration::from_secs(60), Harness::new())
            .await
            .expect("native fixture hung");
        let result = tokio::time::timeout(Duration::from_secs(60), async {
            h.fake.set_policy_key("upgrade", "false");
            h.fake.set_policy_key("limit_wait", "true");
            wait_for_supervisor(|| {
                h.autopilot
                    .sup
                    .policy()
                    .is_some_and(|p| !p.upgrade && p.limit_wait)
            })
            .await;
            h.immediate_point().await;
            h.hold_model_response().await;
            let quota = h.shared.quota();
            *lock(&h.autopilot.quota_test_identity) = Some(identity("account", 0));
            let owner = quota.select_owner("account", Some("native-user"), 0);
            *lock(&h.rt.quota_request) = Some((identity("account", 0), owner));
            // An error from the admitted account cannot be rebound to a new
            // owner, including switching away and back during the request.
            *lock(&h.autopilot.quota_test_identity) = Some(identity("other", 1));
            assert!(
                h.autopilot
                    .bind_quota_wall(&h.rt, "codex".into(), clock())
                    .is_none()
            );
            *lock(&h.autopilot.quota_test_identity) = Some(identity("account", 2));
            assert!(
                h.autopilot
                    .bind_quota_wall(&h.rt, "codex".into(), clock())
                    .is_none()
            );
            *lock(&h.autopilot.quota_test_identity) = Some(identity("account", 0));
            quota.start_read(1, clock());
            let wall_at = clock();
            *lock(&h.rt.quota_wall) = Some(QuotaWall {
                wall: quota
                    .wall_for("account".into(), "premium".into(), wall_at)
                    .with_ordinary_retry(true, false),
                identity: identity("account", 0),
            });
            assert!(h.autopilot.quota_scope(&h.rt).is_some());
            let mut changed = identity("account", 0);
            changed.user = Some("other-user".into());
            *lock(&h.autopilot.quota_test_identity) = Some(changed);
            assert!(
                h.autopilot.quota_scope(&h.rt).is_none(),
                "old wall cannot be persisted under a new user"
            );
            *lock(&h.autopilot.quota_test_identity) = Some(identity("account", 0));
            h.rt.with_state(|state| {
                state.point.as_mut().unwrap().wall = Some(Wall::Usage {
                    reset_at: Some(wall_at + Duration::from_secs(7 * 86400)),
                    reset_label: None,
                });
            });
            h.autopilot.watching.store(false, Ordering::SeqCst);
            h.autopilot.watch_claims();
            let before = h.rt.evaluations();
            // Even an accepted response cannot prove post-wall recovery if its
            // request began before the wall.
            quota.accept(1, "account", vec!["codex".into()], clock());
            wait_for_supervisor(|| h.rt.evaluations() > before).await;
            assert!(received_responses_requests(&h._server).await.is_empty());
            quota.start_read(2, clock());
            quota.accept_observation(2, "account", vec![], Some(false), vec![], clock());
            quota.start_read(3, clock());
            quota.accept_observation(
                3,
                "account",
                vec!["codex".into()],
                Some(true),
                vec![],
                clock(),
            );
            loop {
                let requests = received_responses_requests(&h._server).await;
                if !requests.is_empty() {
                    assert_eq!(requests.len(), 1);
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            assert!(lock(&h.rt.quota_wall).is_none());
            assert!(
                h.fake
                    .acts()
                    .iter()
                    .any(|act| act == words::RULE_LIMIT_RESUME)
            );
            Autopilot::cancel_timer(&h.rt).await;
        })
        .await;
        assert!(
            result.is_ok(),
            "native quota recovery hung: state={:?}, requests={:?}",
            h.rt.state(),
            h.fake.requests()
        );
    });
}

/// A reset that arrives before the recorded deadline must survive the network
/// preflight's second decision. Checking reachability cannot restore the old wait.
#[test]
fn restored_quota_survives_network_preflight() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            let h = Harness::new().await;
            h.fake.set_policy_key("upgrade", "false");
            h.fake.set_policy_key("limit_wait", "true");
            h.fake.set_policy_key("probe_api", "true");
            wait_for_supervisor(|| {
                h.autopilot
                    .sup
                    .policy()
                    .is_some_and(|p| !p.upgrade && p.limit_wait && p.probe_api)
            })
            .await;
            h.immediate_point().await;
            let quota = h.shared.quota();
            *lock(&h.autopilot.quota_test_identity) = Some(identity("account", 0));
            quota.select_owner("account", Some("native-user"), 0);
            let at = clock();
            *lock(&h.rt.quota_wall) = Some(QuotaWall {
                wall: quota.wall_for("account".into(), "codex".into(), at),
                identity: identity("account", 0),
            });
            h.rt.with_state(|state| {
                state.point.as_mut().unwrap().wall = Some(Wall::Usage {
                    reset_at: Some(at + Duration::from_secs(7 * 86400)),
                    reset_label: None,
                });
                state.net_asked = None;
            });
            h.rt.set_route(Some(netwatch::Route::Direct(netwatch::Dest {
                host: "127.0.0.1".into(),
                port: 9,
            })));
            quota.start_read(1, clock());
            quota.accept_observation(
                1,
                "account",
                vec!["codex".into()],
                Some(true),
                vec![],
                clock(),
            );
            Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
            assert!(h.rt.state().net_asked.is_some(), "network preflight ran");
            let owed = h.rt.owed_timer().expect("network readiness wait");
            assert!(
                owed.not_before < at + Duration::from_secs(86400),
                "restored quota was lost and the old reset deadline returned: {owed:?}"
            );
            Autopilot::cancel_timer(&h.rt).await;
        })
        .await
        .expect("quota network preflight hung");
    });
}

#[test]
fn subscription_transition_wakes_one_native_continuation() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            let h = Harness::new().await;
            h.fake.set_policy_key("upgrade", "false");
            wait_for_supervisor(|| h.autopilot.sup.policy().is_some_and(|p| !p.upgrade)).await;
            h.immediate_point().await;
            h.hold_model_response().await;
            let quota = h.shared.quota();
            *lock(&h.autopilot.quota_test_identity) = Some(identity("account", 0));
            quota.select_owner("account", Some("native-user"), 0);
            *lock(&h.rt.quota_wall) = Some(QuotaWall {
                wall: quota.wall_for("account".into(), "codex".into(), clock()),
                identity: identity("account", 0),
            });
            h.rt.with_state(|state| state.point.as_mut().unwrap().wall = Some(Wall::Entitlement));
            h.autopilot.watching.store(false, Ordering::SeqCst);
            h.autopilot.watch_claims();

            // Ordinary access that already existed is not proof that a missing
            // subscription or premium entitlement changed.
            let before = h.rt.evaluations();
            quota.start_read(1, clock());
            quota.accept_observation(
                1,
                "account",
                vec!["codex".into()],
                Some(true),
                vec![],
                clock(),
            );
            wait_for_supervisor(|| h.rt.evaluations() > before).await;
            assert!(received_responses_requests(&h._server).await.is_empty());

            quota.start_read(2, clock());
            quota.accept_observation(2, "account", vec![], Some(false), vec![], clock());
            quota.start_read(3, clock());
            quota.accept_observation(
                3,
                "account",
                vec!["codex".into()],
                Some(true),
                vec![],
                clock(),
            );
            loop {
                let requests = received_responses_requests(&h._server).await;
                if !requests.is_empty() {
                    assert_eq!(requests.len(), 1);
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            assert!(lock(&h.rt.quota_wall).is_none());
            // A slow resumed response must not turn each later account poll
            // into another model request. Drive each evaluation to completion,
            // rather than relying on a quiet-time delay to prove no duplicate.
            for id in 4..=8 {
                quota.start_read(id, clock());
                quota.accept_observation(
                    id,
                    "account",
                    vec!["codex".into()],
                    Some(true),
                    vec![],
                    clock(),
                );
                Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
                assert_eq!(received_responses_requests(&h._server).await.len(), 1);
                assert_eq!(
                    h.fake
                        .acts()
                        .iter()
                        .filter(|act| *act == words::RULE_LIMIT_RESUME)
                        .count(),
                    1
                );
                assert!(h.rt.owed_timer().is_none());
            }
            Autopilot::cancel_timer(&h.rt).await;
        })
        .await
        .expect("subscription watcher hung");
    });
}
