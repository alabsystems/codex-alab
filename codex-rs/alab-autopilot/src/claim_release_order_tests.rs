// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual claim/proof requests and an Ours event establish the stale-cleanup order.
//! The real cleanup worker is polled directly at the proving-status gate. This is
//! a helper/transport ordering witness, not a claim that the whole event loop was
//! scheduled concurrently. No retries or elapsed-time absence oracle are used.

use super::*;
use codex_alab_aterm::Events;
use codex_alab_aterm::fake::FakeAterm;
use codex_alab_aterm::fake::Mode;
use std::future::Future;
use std::future::poll_fn;
use std::pin::Pin;
use std::task::Poll;

#[path = "claim_reader_retirement_tests.rs"]
mod claim_reader_retirement_tests;

const HANG: Duration = Duration::from_secs(60);
const CLAIM: &str = "meta set supervisor";
const RELEASE: &str = "meta unset supervisor";
type TestResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

struct Fixture {
    _dir: tempfile::TempDir,
    fake: FakeAterm,
    sup: Arc<Supervisor>,
    client: Client,
    events: Events,
}

async fn fixture() -> TestResult<Fixture> {
    let mut dir = tempfile::tempdir()?;
    dir.disable_cleanup(true);
    eprintln!("retained claim-release fixture: {}", dir.path().display());
    let fake = FakeAterm::start(dir.path(), Mode::Current).await?;
    let shared = Box::leak(Box::new(Shared::new()));
    let endpoint = fake.endpoint();
    let options = ClientOptions {
        timeout: HANG,
        ..ClientOptions::default()
    };
    let sup = Supervisor::new(SupervisorOptions {
        shared,
        endpoint: Arc::new({
            let endpoint = endpoint.clone();
            move || Some(endpoint.clone())
        }),
        client: options.clone(),
    });
    let client = Client::connect(endpoint, options).await?;
    let events = client.subscribe().await?;
    sup.lock().client = Some(client.clone());
    sup.lost_locally("the fixture has not accepted a claim");
    Ok(Fixture {
        _dir: dir,
        fake,
        sup,
        client,
        events,
    })
}

async fn poll_once<F: Future>(mut future: Pin<&mut F>) -> Poll<F::Output> {
    poll_fn(|cx| Poll::Ready(future.as_mut().poll(cx))).await
}

async fn drive_until<F: Future>(
    mut future: Pin<&mut F>,
    ready: impl Fn() -> bool,
) -> TestResult<Option<F::Output>> {
    tokio::time::timeout(HANG, async {
        loop {
            let polled = poll_once(future.as_mut()).await;
            if let Poll::Ready(value) = polled {
                return Some(value);
            }
            if ready() {
                return None;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(Into::into)
}

async fn actual_ours(events: &mut Events, holder: &str) -> TestResult<()> {
    tokio::time::timeout(HANG, async {
        while let Some(event) = events.next().await {
            if matches!(claim_signal(&event, holder), Signal::Ours) {
                return Ok(());
            }
        }
        Err(std::io::Error::other("actual Ours event stream ended"))
    })
    .await??;
    Ok(())
}

async fn finish(fixture: &Fixture) -> TestResult<()> {
    tokio::time::timeout(HANG, async {
        while fixture.sup.badge_busy.load(Ordering::SeqCst)
            || fixture.sup.badge_again.load(Ordering::SeqCst)
        {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    fixture.client.release().await?;
    fixture.client.close();
    tokio::time::timeout(HANG, fixture.client.closed()).await?;
    assert!(!fixture.client.is_open() && fixture.fake.claim().is_none());
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn stale_ours_cleanup_cannot_release_a_claim_its_refresh_has_not_published_yet()
-> TestResult<()> {
    tokio::time::timeout(HANG, async {
        let mut fixture = fixture().await?;
        fixture.fake.gate(CLAIM);
        let mut refresh = Box::pin(tokio::task::unconstrained(fixture.sup.refresh(&fixture.client)));
        assert!(drive_until(refresh.as_mut(), || {
            fixture.fake.gated_requests().iter().any(|request| request.contains(CLAIM))
        }).await?.is_none(), "fresh refresh reaches its actual claim request");
        // The first status (the mode read) already answered before the claim.
        // Gate the second one: the server has emitted Ours, but local taken()
        // cannot run until this exact proving read answers.
        let status_before = fixture.fake.gated_requests().len();
        fixture.fake.gate("status");
        fixture.fake.open_gate(CLAIM);
        assert!(drive_until(refresh.as_mut(), || {
            fixture.fake.gated_requests().iter().skip(status_before).any(|request| request.contains(" status"))
        }).await?.is_none(), "actual proving status is held");
        actual_ours(&mut fixture.events, fixture.client.holder()).await?;
        assert!(!fixture.sup.held());
        assert_eq!(fixture.fake.claim().map(|claim| claim.holder).as_deref(), Some(fixture.client.holder()));
        fixture.fake.gate(RELEASE);
        let before = fixture.sup.released.load(Ordering::SeqCst);
        let echoes_before = fixture.sup.lock().echoes_due;
        let mut stale = Box::pin(tokio::task::unconstrained(
            fixture.sup.give_back_if_unheld_on(fixture.client.clone())
        ));
        let pending = poll_once(stale.as_mut()).await.is_pending();
        let early_intents = fixture.sup.released.load(Ordering::SeqCst) - before;
        let early_echoes = fixture.sup.lock().echoes_due - echoes_before;
        fixture.fake.open_gate("status");
        let finished = drive_until(refresh.as_mut(), || fixture.sup.held()).await?;
        let freshly_published = fixture.sup.acting()
            && fixture.sup.shared().autopilot() == AutopilotState::Active
            && fixture.sup.shared().claim().as_deref() == Some(fixture.client.holder());
        // An unfenced worker has queued its real unset behind the proving read.
        // Let it finish, rather than cancelling the semantic failure's cleanup.
        fixture.fake.open_gate(RELEASE);
        if finished.is_none() {
            tokio::time::timeout(HANG, async { tokio::join!(refresh.as_mut(), stale.as_mut()); }).await?;
        } else {
            tokio::time::timeout(HANG, stale.as_mut()).await?;
        }
        let remote_proven = fixture.client.status().await?.proven_holder(fixture.client.holder());
        let release_requests = fixture.fake.requests().iter().filter(|request| request.contains(RELEASE)).count();
        let final_held = fixture.sup.held();
        let final_active = fixture.sup.acting();
        finish(&fixture).await?;
        eprintln!("stale Ours: early intents={early_intents}, echoes={early_echoes}, fresh={freshly_published}, remote={remote_proven}, held={final_held}, active={final_active}, unsets={release_requests}");
        assert!(pending && freshly_published, "the actual proof and take completed before cleanup settled");
        assert_eq!(early_intents, 0, "an unheld snapshot cannot authorize release while a fresh refresh owns the claim transaction");
        assert_eq!(early_echoes, 0, "skipped stale cleanup owes no release echo");
        assert_eq!(release_requests, 0, "stale Ours cleanup must not unset the fresh actual claim");
        assert!(remote_proven && final_held && final_active);
        Ok(())
    }).await?
}

#[tokio::test(flavor = "current_thread")]
async fn an_ours_claim_that_was_never_accepted_is_released() -> TestResult<()> {
    tokio::time::timeout(HANG, async {
        let mut fixture = fixture().await?;
        assert!(matches!(
            fixture.client.harness_policy().await?,
            PolicyAnswer::Supported(_)
        ));
        assert!(matches!(
            fixture.client.claim().await?,
            ClaimOutcome::Held { .. }
        ));
        actual_ours(&mut fixture.events, fixture.client.holder()).await?;
        let unaccepted = !fixture.sup.held() && fixture.client.holds_claim();
        fixture.sup.give_back_if_unheld(&fixture.client).await;
        let remote_empty = fixture.fake.claim().is_none();
        let client_released = !fixture.client.holds_claim();
        let intents = fixture.sup.released.load(Ordering::SeqCst);
        let echoes = fixture.sup.lock().echoes_due;
        let unsets = fixture
            .fake
            .requests()
            .iter()
            .filter(|request| request.contains(RELEASE))
            .count();
        finish(&fixture).await?;
        assert!(unaccepted && remote_empty && client_released);
        assert_eq!(
            (intents, echoes, unsets),
            (1, 1, 1),
            "a legitimate passive cleanup really releases and accounts its echo"
        );
        Ok(())
    })
    .await?
}

#[tokio::test(flavor = "current_thread")]
async fn cancelling_the_cleanup_caller_keeps_its_owned_release_ahead_of_a_new_claim()
-> TestResult<()> {
    tokio::time::timeout(HANG, async {
        let mut fixture = fixture().await?;
        assert!(matches!(
            fixture.client.harness_policy().await?,
            PolicyAnswer::Supported(_)
        ));
        assert!(matches!(
            fixture.client.claim().await?,
            ClaimOutcome::Held { .. }
        ));
        actual_ours(&mut fixture.events, fixture.client.holder()).await?;
        let boundary = fixture.fake.requests().len();
        fixture.fake.gate(RELEASE);
        let caller = tokio::spawn({
            let sup = Arc::clone(&fixture.sup);
            let client = fixture.client.clone();
            async move { sup.give_back_if_unheld(&client).await }
        });
        assert!(
            fixture
                .fake
                .wait_for(|fake| fake
                    .gated_requests()
                    .iter()
                    .any(|request| request.contains(RELEASE)))
                .await,
            "the owned worker has entered its actual unanswered release"
        );
        caller.abort();
        let cancelled = caller.await.is_err_and(|error| error.is_cancelled());
        // This fixture has not called Supervisor::refresh yet. The worker owns
        // the policy fence while its actual release remains unanswered. Polling
        // the later refresh must not even enter its policy-read bookkeeping.
        assert!(fixture.sup.lock().policy_read.is_none());
        let mut refresh = std::pin::pin!(tokio::task::unconstrained(
            fixture.sup.refresh(&fixture.client),
        ));
        let waiting = poll_once(refresh.as_mut()).await.is_pending();
        let did_not_enter_policy = fixture.sup.lock().policy_read.is_none();
        fixture.fake.open_gate(RELEASE);
        tokio::time::timeout(HANG, refresh.as_mut()).await?;
        let owned_completed = fixture
            .sup
            .journal()
            .iter()
            .any(|line| line == "gave back a claim taken while passive");
        let proven = fixture
            .client
            .status()
            .await?
            .proven_holder(fixture.client.holder());
        let active = fixture.sup.acting();
        let requests = fixture.fake.requests();
        let tail = &requests[boundary..];
        let unset = tail.iter().position(|request| request.contains(RELEASE));
        let take = tail.iter().position(|request| request.contains(CLAIM));
        let unset_count = tail
            .iter()
            .filter(|request| request.contains(RELEASE))
            .count();
        let take_count = tail
            .iter()
            .filter(|request| request.contains(CLAIM))
            .count();
        finish(&fixture).await?;
        assert!(
            cancelled && waiting && did_not_enter_policy,
            "aborting the wrapper cannot release the worker's transaction fence"
        );
        assert!(owned_completed && proven && active);
        assert!(
            matches!((unset, take), (Some(unset), Some(take)) if unset < take),
            "actual release precedes actual fresh take: {tail:?}"
        );
        assert_eq!((unset_count, take_count), (1, 1));
        Ok(())
    })
    .await?
}

#[tokio::test(flavor = "current_thread")]
async fn a_persons_clear_stays_given_up_until_an_actual_policy_change() -> TestResult<()> {
    tokio::time::timeout(HANG, async {
        let fixture = fixture().await?;
        fixture.sup.set_policy_recheck(Duration::from_secs(3600));
        let (tui, mut changes) = watch::channel(0);
        let mut tries = 0;
        let mut session = std::pin::pin!(fixture.sup.session(
            fixture.fake.endpoint(),
            &mut changes,
            &mut tries,
            Some(fixture.client.clone()),
        ));
        assert!(
            drive_until(session.as_mut(), || {
                let state = fixture.sup.lock();
                state.acting() && !state.attention_unknown() && !state.needs_unknown()
            })
            .await?
            .is_none(),
            "the actual session settles its first proven claim"
        );
        let first_claims = fixture.sup.lock().claims_taken;
        let sets_before = fixture
            .fake
            .requests()
            .iter()
            .filter(|request| request.contains(CLAIM))
            .count();
        fixture.fake.clear_claim();
        assert!(
            drive_until(session.as_mut(), || {
                let cleared = { fixture.sup.lock().cleared };
                cleared
                    && fixture.sup.journal().iter().any(|line| {
                        line == "the claim was cleared: not taken again until the policy moves"
                    })
            })
            .await?
            .is_none(),
            "the actual clear event completes its passive cleanup"
        );
        fixture.sup.refresh(&fixture.client).await;
        let stayed_passive = !fixture.sup.held()
            && !fixture.sup.acting()
            && fixture.sup.shared().claim().is_none()
            && fixture.sup.shared().autopilot() == AutopilotState::Passive
            && fixture.fake.claim().is_none();
        let sets_after_refresh = fixture
            .fake
            .requests()
            .iter()
            .filter(|request| request.contains(CLAIM))
            .count();
        // A positive protocol barrier replaces any timed absence check: the
        // explicit same-policy refresh above completed, and this real new policy
        // event must be what authorizes the next actual take.
        fixture
            .fake
            .set_policy_key("continue_text", "the policy moved after the clear");
        assert!(
            drive_until(session.as_mut(), || {
                let state = fixture.sup.lock();
                state.acting()
                    && state.claims_taken > first_claims
                    && !state.attention_unknown()
                    && !state.needs_unknown()
            })
            .await?
            .is_none(),
            "a real policy event permits a fresh claim"
        );
        let recovered = fixture.sup.acting()
            && fixture
                .client
                .status()
                .await?
                .proven_holder(fixture.client.holder());
        tokio::time::timeout(HANG, async {
            while fixture.sup.badge_busy.load(Ordering::SeqCst)
                || fixture.sup.badge_again.load(Ordering::SeqCst)
            {
                tokio::task::yield_now().await;
            }
        })
        .await?;
        drop(tui);
        let ended = tokio::time::timeout(HANG, session.as_mut()).await?;
        let closed = !fixture.client.is_open() && fixture.fake.claim().is_none();
        assert!(
            matches!(ended, SessionEnd::Closing) && closed,
            "actual session retirement releases and closes its client"
        );
        assert!(stayed_passive && recovered);
        assert_eq!(
            sets_before, sets_after_refresh,
            "a person's clear is not mistaken for an own-release echo"
        );
        Ok(())
    })
    .await?
}

/// The real session admits the old clear while its new-claim cleanup is held.
/// Drive the same renewal helper a TUI change calls, then provide no further TUI
/// publication: classifying that clear must retry the still-unacknowledged line.
#[tokio::test(flavor = "current_thread")]
async fn a_queued_claim_change_defers_and_then_retries_the_resume_line_without_a_new_frame()
-> TestResult<()> {
    tokio::time::timeout(HANG, async {
        let fixture = fixture().await?;
        fixture.sup.set_policy_recheck(Duration::from_secs(3600));
        fixture.fake.set_policy_key("human_grace_s", "0");
        fixture.fake.set_status("autopilot", "cruise");
        let (tui, mut changes) = watch::channel(0);
        let mut tries = 0;
        let mut session = std::pin::pin!(fixture.sup.session(
            fixture.fake.endpoint(),
            &mut changes,
            &mut tries,
            Some(fixture.client.clone()),
        ));
        assert!(
            drive_until(session.as_mut(), || {
                let state = fixture.sup.lock();
                state.acting() && !state.attention_unknown() && !state.needs_unknown()
            })
            .await?
            .is_none()
        );
        let claims_taken = fixture.sup.lock().claims_taken;
        fixture.fake.delay_clear_event();
        fixture.fake.set_status("autopilot", "off");
        fixture.fake.set_effective(false, "autopilot-off");
        assert!(
            drive_until(session.as_mut(), || {
                !fixture.sup.held()
                    && fixture.fake.claim().is_none()
                    && fixture
                        .sup
                        .journal()
                        .iter()
                        .any(|line| line.starts_with("gave the claim back ("))
            })
            .await?
            .is_none()
        );
        let gated = fixture.fake.gated_requests().len();
        fixture.fake.gate("meta unset attention");
        fixture.fake.set_status("autopilot", "cruise");
        fixture.fake.set_effective(true, "enabled");
        assert!(
            drive_until(session.as_mut(), || {
                let state = fixture.sup.lock();
                state.claims_taken > claims_taken
                    && state.queued_claim_events.pending.load(Ordering::SeqCst) > 0
                    && fixture
                        .fake
                        .gated_requests()
                        .iter()
                        .skip(gated)
                        .any(|request| request.contains("meta unset attention"))
            })
            .await?
            .is_none(),
            "the reader really queued the older clear after a new proof"
        );
        assert!(fixture.sup.acting() && !fixture.client.reconfirmed_since_claim());
        let line_before = fixture.sup.lock().confirmed_line;
        fixture.sup.shared().note_resume_line();
        let line = fixture.sup.shared().resume_lines();
        assert_ne!(line, line_before);
        let sets = || {
            fixture
                .fake
                .requests()
                .iter()
                .filter(|request| request.contains(CLAIM))
                .count()
        };
        let before = sets();
        let mut renewal = Box::pin(fixture.sup.renew_for_record(&fixture.client));
        let deferred = poll_once(renewal.as_mut()).await.is_ready();
        let kept_line = fixture.sup.lock().confirmed_line == line_before;
        let did_not_reconfirm = !fixture.client.reconfirmed_since_claim() && sets() == before;
        let mut claims = fixture.sup.claims();
        drop(claims.borrow_and_update());
        fixture.fake.open_gate("meta unset attention");
        if !deferred {
            tokio::time::timeout(HANG, renewal.as_mut()).await?;
        }
        drop(renewal);
        assert!(
            drive_until(session.as_mut(), || {
                let state = fixture.sup.lock();
                state.confirmed_line == line || !state.held
            })
            .await?
            .is_none()
        );
        let renewed = fixture.sup.lock().confirmed_line == line;
        let woke = claims.has_changed()?;
        let proven = fixture
            .client
            .status()
            .await?
            .proven_holder(fixture.client.holder());
        let recovered = fixture.sup.acting() && fixture.sup.held() && proven;
        drop(tui);
        let ended = tokio::time::timeout(HANG, session.as_mut()).await?;
        assert!(matches!(ended, SessionEnd::Closing));
        assert!(
            deferred && kept_line && did_not_reconfirm,
            "queued clear defers the actual renewal without acknowledging its line"
        );
        assert!(
            renewed && woke && recovered,
            "the classified clear retries its line and wakes decisions without another frame"
        );
        Ok(())
    })
    .await?
}

#[tokio::test(flavor = "current_thread")]
async fn a_claim_lost_during_an_off_hold_status_read_stays_passive_and_unclaimed() -> TestResult<()>
{
    tokio::time::timeout(HANG, async {
        let fixture = fixture().await?;
        fixture.fake.set_status("autopilot", "cruise");
        fixture.sup.refresh(&fixture.client).await;
        assert!(fixture.sup.acting() && fixture.sup.held());
        assert_eq!(fixture.sup.shared().harness_word(), HarnessWord::Cruise);

        // Lifting standby into Off::Hold reconciles unsettled needs and attention.
        fixture
            .sup
            .stand_by(&fixture.client, "why=suspended".into())
            .await;
        assert_eq!(fixture.sup.standby().as_deref(), Some("why=suspended"));
        assert!(fixture.sup.want_needs(&["grant prod key".to_string()], 77));
        fixture.fake.set_effective(false, "hold");
        fixture.sup.refresh(&fixture.client).await;
        assert!(fixture.sup.standby().is_none() && fixture.sup.held());
        assert_eq!(
            fixture.sup.needs_shown().as_deref(),
            Some("Codex needs you: grant prod key \u{2014} carrying on")
        );

        // Now lose the claim while a subsequent Off::Hold refresh is reading status.
        let gated_before = fixture.fake.gated_requests().len();
        fixture.fake.gate("status");
        let mut hold_refresh = std::pin::pin!(tokio::task::unconstrained(
            fixture.sup.refresh(&fixture.client),
        ));
        assert!(
            drive_until(hold_refresh.as_mut(), || {
                fixture
                    .fake
                    .gated_requests()
                    .iter()
                    .skip(gated_before)
                    .any(|request| request.contains(" status"))
            })
            .await?
            .is_none(),
            "Off::Hold refresh is gated on its autopilot_word_said status read"
        );
        fixture.fake.clear_claim();
        fixture.client.note_claim_lost();
        fixture
            .sup
            .lost_locally("status shows no proven claim of ours");
        assert!(!fixture.sup.held());
        assert_eq!(fixture.sup.shared().harness_word(), HarnessWord::Unknown);

        fixture.fake.open_gate("status");
        tokio::time::timeout(HANG, hold_refresh.as_mut()).await?;

        let held_after = fixture.sup.held();
        let acting_after = fixture.sup.acting();
        let policy_after = fixture.sup.lock().policy.clone();
        let hold_after = fixture.sup.lock().hold;
        let autopilot_after = fixture.sup.shared().autopilot();
        let word_after = fixture.sup.shared().harness_word();
        let journal = fixture.sup.journal();
        finish(&fixture).await?;

        assert!(!held_after && !acting_after);
        assert!(
            policy_after.is_none(),
            "unheld Off::Hold completion must not republish a held policy: {policy_after:?}"
        );
        assert!(hold_after, "passive clock remains armed for why=hold");
        assert_eq!(autopilot_after, AutopilotState::Passive);
        assert_eq!(
            word_after,
            HarnessWord::Unknown,
            "unheld Off::Hold completion must not restore an acting chrome word"
        );
        assert_eq!(
            journal.last().map(String::as_str),
            Some("policy effective=off why=hold: passive"),
            "journal: {journal:?}"
        );
        Ok(())
    })
    .await?
}
