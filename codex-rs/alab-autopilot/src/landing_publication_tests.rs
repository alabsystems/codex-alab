// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Landing publication against the in-process fake: submitted work, refusal and a
//! cancelled evaluator keep the point provisional. Only an acknowledged current point
//! ends its carry and hold. No provider, installed host or native application is used.

use super::*;
use crate::decide::Mode as Word;
use crate::repo::RepoRead;
use crate::words;

async fn ready(before: impl FnOnce(&FakeAterm, &Supervisor)) -> Harness {
    ready_within(before, ClientOptions::default().timeout, true).await
}

async fn ready_within(
    before: impl FnOnce(&FakeAterm, &Supervisor),
    timeout: Duration,
    wire_hold: bool,
) -> Harness {
    let h = started_with(
        &[],
        |fake, sup| {
            fake.set_status("autopilot", "land");
            before(fake, sup);
        },
        false,
        ClientOptions {
            timeout,
            ..ClientOptions::default()
        },
    )
    .await;
    wait_until("the claim is proven", || h.sup.acting()).await;
    seed(&h, wire_hold).await;
    h
}

async fn seed(h: &Harness, wire_hold: bool) {
    land_status_tests::land_act(h, words::RULE_LAND, 1_000);
    h.sup.show_land().await;
    let hold = h.rt.state().land_held().expect("the local hold is active");
    if wire_hold {
        assert_eq!(
            h.fake.land(),
            Some(hold),
            "the capable host acknowledged its hold"
        );
    } else {
        assert!(h.fake.land().is_none());
        let answer = h
            .sup
            .client()
            .expect("claim connection")
            .harness_land(Some(&hold))
            .await
            .expect("capability response");
        assert!(
            matches!(answer, codex_alab_aterm::LandOutcome::Unsupported(_)),
            "the legacy host actually refuses the optional hold field: {answer:?}"
        );
        assert!(h.fake.land().is_none());
    }
    h.rt.with_state(|state| {
        state.mode = Some(Word::Land);
        state.landed = false;
        state.reland = false;
        let end = TurnEnd {
            by_autopilot: true,
            by_person: false,
            ..ended(
                Some("Nothing left: all merged and pushed to origin/main."),
                None,
            )
        };
        let at = Some(end.at);
        state.point = Some(end);
        state.probed_for(at, RepoRead::Read(None));
    });
    h.shared.set_carried_on(3);
}

struct Before {
    state: crate::decide::ThreadState,
    wire_hold: Option<codex_alab_aterm::LandReport>,
}

async fn before(h: &Harness) -> Before {
    // Settle the canonical desire too: a retained landing awaiting a reland has no
    // standing wire hold, while its local landing memory remains intact.
    h.autopilot.tell_land(&h.rt);
    h.sup.show_land().await;
    Before {
        state: h.rt.state(),
        wire_hold: h.fake.land(),
    }
}

fn input(h: &Harness, pending: bool, queued: bool) {
    h.shared.observe(
        &Frame {
            primary_thread: Some(h.rt.key()),
            displayed_thread: Some(h.rt.key()),
            rollout_path: None,
            cwd: std::path::Path::new("/work"),
            model: "gpt-5",
            codex_home: h._dir.path(),
            embedded: true,
            state: if pending {
                ProgramState::Working
            } else {
                ProgramState::Idle
            },
            draft_empty: true,
            queued_input: queued,
            popup_open: false,
            editor_open: false,
            pending_user_turn: pending,
        },
        now_ms(),
    );
}

async fn evaluate(h: &Harness) {
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
}

async fn provisional(h: &Harness, before: &Before) {
    let after = h.rt.state();
    assert_eq!(after.point_at(), before.state.point_at());
    assert_eq!(after.landed, before.state.landed);
    assert_eq!(after.reland, before.state.reland);
    assert_eq!(after.landing, before.state.landing);
    assert_eq!(after.land.hold, before.state.land.hold);
    assert_eq!(h.shared.carried_on(), 3);
    assert_eq!(
        h.shared.task_face(),
        codex_alab_core::session::TaskFace::Open
    );
    wait_until(
        "the provisional point keeps its existing hold desire",
        || h.fake.land() == before.wire_hold,
    )
    .await;
}

async fn landed(h: &Harness) {
    assert!(h.rt.state().landed, "{:?}", h.sup.journal());
    assert!(!h.rt.state().reland);
    assert_eq!(h.shared.carried_on(), 0);
    assert_eq!(
        h.shared.task_face(),
        codex_alab_core::session::TaskFace::Landed
    );
    assert!(h.fake.acts().iter().any(|act| act == words::RULE_LANDED));
    wait_until("the acknowledged landing ends its hold", || {
        h.fake.land().is_none()
    })
    .await;
}

/// The journal lines that say a landing waited for a person's turn on its way.
fn landing_waits(h: &Harness) -> usize {
    h.sup
        .journal()
        .iter()
        .filter(|line| line.contains("the landing waits: a person's turn is on its way"))
        .count()
}

/// A person's turn submitted or queued defers the landing, which journals that it waits exactly
/// once and looks again (the look cancelled here, so no second look can journal again), the point
/// kept; the next look with neither lands. CONTROL: with neither it lands at once and journals no
/// wait.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn submitted_or_queued_work_defers_landing_without_consuming_the_point() {
    for (pending, queued) in [(true, false), (false, true), (false, false)] {
        let on_its_way = pending || queued;
        let h = ready(|_, _| {}).await;
        let before = before(&h).await;
        input(&h, pending, queued);
        evaluate(&h).await;
        if on_its_way {
            assert!(h.rt.timer_pending(), "looked at again: {:?}", h.sup.journal());
            Autopilot::cancel_timer(&h.rt).await;
            assert_eq!(landing_waits(&h), 1, "{:?}", h.sup.journal());
            provisional(&h, &before).await;
            assert!(h.fake.acts().is_empty());
            input(&h, false, false);
            evaluate(&h).await;
        }
        landed(&h).await;
        assert_eq!(h.fake.acts(), vec![words::RULE_LANDED.to_string()]);
        assert_eq!(
            landing_waits(&h),
            usize::from(on_its_way),
            "{:?}",
            h.sup.journal()
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refused_landing_keeps_hold_carry_and_reland_until_acknowledged() {
    for reland in [false, true] {
        let h = ready(|fake, _| fake.refuse_harness_acts(Some("ERR busy harness_act"))).await;
        if reland {
            h.rt.with_state(|state| {
                state.landed = true;
                state.reland = true;
            });
        }
        let before = before(&h).await;
        evaluate(&h).await;
        provisional(&h, &before).await;
        assert!(h.fake.acts().is_empty());
        h.fake.refuse_harness_acts(None);
        evaluate(&h).await;
        landed(&h).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_submission_during_publication_keeps_the_local_landing_provisional() {
    let h = ready_within(
        |fake, _| {
            fake.set_version("0.109.0");
            fake.support_act_deadlines(false);
        },
        Duration::from_secs(60),
        false,
    )
    .await;
    let before = before(&h).await;
    h.fake.gate("meta set harness_act");
    let publication = tokio::spawn(Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)));
    assert!(
        h.fake
            .wait_for(|fake| fake
                .gated_requests()
                .iter()
                .any(|line| { line.ends_with("meta set harness_act landed@v1") }))
            .await
    );
    provisional(&h, &before).await;
    assert!(h.fake.acts().is_empty(), "no ACK has arrived");
    input(&h, true, false);
    h.fake.open_gate("meta set harness_act");
    tokio::time::timeout(Duration::from_secs(60), publication)
        .await
        .expect("publication hang detector")
        .expect("publication task");
    assert_eq!(
        h.fake.acts(),
        vec![words::RULE_LANDED.to_string()],
        "the raced write was actually acknowledged"
    );
    provisional(&h, &before).await;
    // This supported legacy host has no at-rest check. Requiring an actual ACK makes
    // the post-publication input guard discriminating: lease expiry cannot pass it.
    // The paired host test separately covers refusal after an optimistic TUI begin.
    input(&h, false, false);
    evaluate(&h).await;
    landed(&h).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cancelled_landing_publisher_retains_native_and_upgrade_custody() {
    use codex_extension_api::TurnLifecycleContributor;
    use codex_extension_api::TurnStartInput;
    use codex_protocol::config_types::CollaborationMode;
    use codex_protocol::config_types::ModeKind;
    use codex_protocol::config_types::Settings;

    let h = ready_within(|_, _| {}, Duration::from_secs(60), true).await;
    let before = before(&h).await;
    h.fake.gate("meta set harness_act");
    let evaluation = tokio::spawn(Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)));
    assert!(
        h.fake
            .wait_for(|fake| fake
                .gated_requests()
                .iter()
                .any(|line| { line.contains("meta set harness_act landed@v1 until_us=") }))
            .await
    );
    evaluation.abort();
    assert!(
        evaluation
            .await
            .expect_err("cancelled evaluator")
            .is_cancelled()
    );
    let mut eval_permit = Box::pin(h.rt.hold_eval_when_free());
    let custody = std::future::poll_fn(|cx| {
        std::task::Poll::Ready(std::future::Future::poll(eval_permit.as_mut(), cx))
    })
    .await;
    assert!(
        custody.is_pending(),
        "the owned publisher retains the evaluation permit"
    );
    drop(eval_permit);
    provisional(&h, &before).await;
    let policy = h.sup.policy().expect("claim policy");
    assert!(!h.sup.commit_upgrade(&policy, h.shared.upgrade_generation()));

    let session = ExtensionData::new("session");
    let turn = ExtensionData::new("new-native-turn");
    let mode = CollaborationMode {
        mode: ModeKind::Default,
        settings: Settings {
            model: "gpt-5".to_string(),
            reasoning_effort: None,
            developer_instructions: None,
        },
    };
    let mut start = h.autopilot.on_turn_start(TurnStartInput {
        turn_id: turn.level_id(),
        collaboration_mode: &mode,
        token_usage_at_turn_start: None,
        session_store: &session,
        thread_store: &h._store,
        turn_store: &turn,
    });
    let polled = std::future::poll_fn(|cx| {
        std::task::Poll::Ready(std::future::Future::poll(start.as_mut(), cx))
    })
    .await;
    assert!(
        polled.is_pending(),
        "native admission waits for the owned publication"
    );
    assert!(!h.shared.turn_running());
    assert!(h.fake.acts().is_empty());
    h.fake.open_gate("meta set harness_act");
    tokio::time::timeout(Duration::from_secs(60), start)
        .await
        .expect("native admission hang detector");
    let permit = tokio::time::timeout(Duration::from_secs(60), h.rt.hold_eval_when_free())
        .await
        .expect("publication custody hang detector");
    drop(permit);
    assert!(h.shared.turn_running_on(h.rt.key()));
    assert_eq!(
        h.shared.task_face(),
        codex_alab_core::session::TaskFace::Open,
        "native acceptance opens the face while landing memory awaits classification"
    );
    // If the explicit five-second host lease expired while the barrier was held,
    // the request was refused instead. Either outcome settles before native admission.
    assert!(h.fake.acts().len() <= 1);
    let acts = h.fake.acts();
    evaluate(&h).await;
    assert_eq!(
        h.fake.acts(),
        acts,
        "a running point records no new landing"
    );
    assert_eq!(
        h.shared.task_face(),
        codex_alab_core::session::TaskFace::Open
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn old_landing_hosts_keep_the_supported_unbounded_spelling() {
    for version in ["0.108.0", "0.109.0"] {
        let h = ready_within(
            |fake, _| {
                fake.set_version(version);
                fake.support_act_deadlines(false);
            },
            ClientOptions::default().timeout,
            false,
        )
        .await;
        evaluate(&h).await;
        landed(&h).await;
        assert!(
            h.fake
                .requests()
                .iter()
                .any(|line| { line.ends_with("meta set harness_act landed@v1") })
        );
        assert!(
            !h.fake
                .requests()
                .iter()
                .any(|line| { line.contains("meta set harness_act landed@v1 until_us=") })
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unsupported_landing_is_rejudged_on_the_hosts_real_reconnect_event() {
    for version in ["0.107.0", "unknown"] {
        let h = started_with_claim_watch(
            &[],
            |fake, _| {
                fake.set_status("autopilot", "land");
                fake.set_version(version);
                fake.support_act_deadlines(false);
            },
            false,
        )
        .await;
        wait_until("the old claim is proven", || h.sup.acting()).await;
        seed(&h, false).await;
        let before = before(&h).await;
        evaluate(&h).await;
        provisional(&h, &before).await;
        assert!(h.fake.acts().is_empty());
        assert!(
            !h.rt.timer_pending(),
            "unsupported hosts are not polled forever"
        );
        assert!(
            !h.rt.undecided(),
            "the first evaluation really decided this point"
        );

        evaluate(&h).await;

        assert_eq!(
            h.sup
                .journal()
                .iter()
                .filter(|line| line.contains("predates landing records"))
                .count(),
            1
        );

        h.autopilot.watch_claims();
        h.fake.set_version("0.110.0");
        h.fake.support_act_deadlines(true);
        h.sup.client().expect("old claim connection").close();
        wait_until("the real reconnect adopts the upgraded host", || {
            h.sup
                .client()
                .is_some_and(|client| client.server_release() == Some((0, 110)))
        })
        .await;
        // No direct evaluate/ask_again call: the actual claim watcher must undo
        // decided-point memoization and publish this same retained point.
        wait_until("the claim event rejudges the retained landing", || {
            h.rt.state().landed
        })
        .await;
        assert_eq!(h.rt.state().point_at(), before.state.point_at());
        landed(&h).await;
    }
}
