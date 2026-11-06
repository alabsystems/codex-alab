// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Deterministic publication-order witnesses through the existing endpoint callback.
//! The client really proves its claim and acknowledges a description. The callback
//! invokes the actual take/resume helper during withdrawal; it models another task's
//! local continuation, rather than claiming two concurrent server exchanges occurred.

use super::*;
use codex_alab_aterm::fake::FakeAterm;
use codex_alab_aterm::fake::Mode;
use std::sync::OnceLock;

const HANG: Duration = Duration::from_secs(60);
const DESCRIPTION: &str = "plan: the acknowledged claim-publication fixture";
type TestResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

enum Reenter {
    Take(Policy),
    Resume(Policy),
}

#[derive(Debug)]
enum Reentered {
    Took(Taken),
    Resumed(Option<String>),
}

#[derive(Debug)]
struct CallbackObservation {
    before: Option<(bool, Option<String>)>,
    outcome: Option<Reentered>,
    active_after: bool,
    claim_after: Option<String>,
}

#[derive(Default)]
struct Callback {
    supervisor: OnceLock<Weak<Supervisor>>,
    next: Mutex<Option<Reenter>>,
    observed: Mutex<Option<CallbackObservation>>,
}

impl Callback {
    fn visit(&self) {
        let next = self
            .next
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        let Some(next) = next else { return };
        let Some(supervisor) = self.supervisor.get().and_then(Weak::upgrade) else {
            return;
        };
        // A misplaced production change that holds State across withdrawal must
        // fail the witness, rather than deadlock this synchronous callback.
        let before = {
            match supervisor.state.try_lock() {
                Ok(state) => Some((state.held, state.standby.clone())),
                Err(_) => None,
            }
        };
        let outcome = before.as_ref().map(|_| match next {
            Reenter::Take(policy) => Reentered::Took(supervisor.taken(policy)),
            Reenter::Resume(policy) => {
                Reentered::Resumed(supervisor.resume(policy, HarnessWord::Cruise))
            }
        });
        *self
            .observed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(CallbackObservation {
            before,
            outcome,
            active_after: supervisor.shared().autopilot() == AutopilotState::Active,
            claim_after: supervisor.shared().claim(),
        });
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    fake: FakeAterm,
    supervisor: Arc<Supervisor>,
    client: Client,
    policy: Policy,
    callback: Arc<Callback>,
}

async fn fixture() -> TestResult<Fixture> {
    let mut dir = tempfile::tempdir()?;
    dir.disable_cleanup(true);
    eprintln!(
        "retained claim-publication fixture: {}",
        dir.path().display()
    );
    let fake = FakeAterm::start(dir.path(), Mode::Current).await?;
    let shared = Box::leak(Box::new(Shared::new()));
    let endpoint = fake.endpoint();
    let callback = Arc::new(Callback::default());
    let supervisor = Supervisor::new(SupervisorOptions {
        shared,
        endpoint: Arc::new({
            let endpoint = endpoint.clone();
            let callback = Arc::clone(&callback);
            move || {
                callback.visit();
                Some(endpoint.clone())
            }
        }),
        client: ClientOptions {
            timeout: HANG,
            ..ClientOptions::default()
        },
    });
    assert!(callback.supervisor.set(Arc::downgrade(&supervisor)).is_ok());
    let client = Client::connect(
        endpoint,
        ClientOptions {
            timeout: HANG,
            ..ClientOptions::default()
        },
    )
    .await?;
    let policy = match client.harness_policy().await? {
        PolicyAnswer::Supported(policy) => Policy::from_harness(&policy),
        other => {
            return Err(
                std::io::Error::other(format!("supported fixture policy: {other:?}")).into(),
            );
        }
    };
    assert!(matches!(client.claim().await?, ClaimOutcome::Held { .. }));
    assert!(client.status().await?.proven_holder(client.holder()));
    supervisor.lock().client = Some(client.clone());
    assert_eq!(supervisor.taken(policy.clone()), Taken::Held);
    supervisor.describe(Some(DESCRIPTION.into())).await;
    assert_eq!(fake.description().as_deref(), Some(DESCRIPTION));
    assert!(DescriptionReceipt::of(&supervisor.lock()).is_some());
    Ok(Fixture {
        _dir: dir,
        fake,
        supervisor,
        client,
        policy,
        callback,
    })
}

#[derive(Debug)]
struct Publication {
    held: bool,
    client_proven: bool,
    acting: bool,
    claim: Option<String>,
    autopilot: AutopilotState,
    standby: Option<String>,
}

fn publication(fixture: &Fixture) -> Publication {
    Publication {
        held: fixture.supervisor.held(),
        client_proven: fixture.client.holds_claim(),
        acting: fixture.supervisor.acting(),
        claim: fixture.supervisor.shared().claim(),
        autopilot: fixture.supervisor.shared().autopilot(),
        standby: fixture.supervisor.standby(),
    }
}

fn observe_withdrawal(fixture: &Fixture) -> tokio::sync::oneshot::Receiver<Client> {
    let (observer, observed) = tokio::sync::oneshot::channel();
    *fixture
        .supervisor
        .description_connection
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(observer);
    observed
}

async fn finish(
    fixture: &Fixture,
    observed: tokio::sync::oneshot::Receiver<Client>,
) -> TestResult<()> {
    let auxiliary = tokio::time::timeout(HANG, observed).await??;
    tokio::time::timeout(HANG, auxiliary.closed()).await?;
    fixture.supervisor.descriptions_settled().await;
    // resume() may have started the independent badge read. Its actual worker
    // closes its client before badge_busy is cleared; do not abandon that work.
    tokio::time::timeout(HANG, async {
        while fixture.supervisor.badge_busy.load(Ordering::SeqCst)
            || fixture.supervisor.badge_again.load(Ordering::SeqCst)
        {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    fixture.client.release().await?;
    fixture.client.close();
    tokio::time::timeout(HANG, fixture.client.closed()).await?;
    assert!(!auxiliary.is_open() && !fixture.client.is_open());
    assert!(
        fixture.fake.claim().is_none(),
        "fixture released its actual remote claim"
    );
    Ok(())
}

fn assert_active(actual: &Publication, holder: &str) {
    assert!(
        actual.held && actual.client_proven,
        "actual proven claim: {actual:?}"
    );
    assert!(
        actual.acting,
        "a stale passive publication stranded the fresh claim: {actual:?}"
    );
    assert_eq!(actual.claim.as_deref(), Some(holder));
    assert_eq!(actual.autopilot, AutopilotState::Active);
    assert!(actual.standby.is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn a_local_loss_cannot_unpublish_a_take_from_its_description_withdrawal() -> TestResult<()> {
    tokio::time::timeout(HANG, async {
        let fixture = fixture().await?;
        let observed = observe_withdrawal(&fixture);
        *fixture.callback.next.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Reenter::Take(fixture.policy.clone()));
        fixture.supervisor.lost_locally("the prior claim was lost");
        let after_loss = publication(&fixture);
        // The normal kept-claim refresh must neither be needed to repair a stale
        // loss nor turn an already inconsistent snapshot into a false pass.
        fixture.supervisor.refresh(&fixture.client).await;
        let after_refresh = publication(&fixture);
        let callback = fixture.callback.observed.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take();
        let remote_proven = fixture.client.status().await?.proven_holder(fixture.client.holder());
        finish(&fixture, observed).await?;
        eprintln!("loss publication: {after_loss:?}; repeated refresh: {after_refresh:?}; callback: {callback:?}");
        let callback = callback.ok_or_else(|| std::io::Error::other("withdrawal callback did not run"))?;
        assert_eq!(callback.before, Some((false, None)));
        assert!(matches!(callback.outcome, Some(Reentered::Took(Taken::Held))));
        assert!(callback.active_after && callback.claim_after.as_deref() == Some(fixture.client.holder()));
        assert!(remote_proven);
        assert_active(&after_loss, fixture.client.holder());
        assert_active(&after_refresh, fixture.client.holder());
        Ok(())
    }).await?
}

#[tokio::test(flavor = "current_thread")]
async fn a_standby_cannot_unpublish_a_resume_from_its_description_withdrawal() -> TestResult<()> {
    tokio::time::timeout(HANG, async {
        let fixture = fixture().await?;
        let observed = observe_withdrawal(&fixture);
        *fixture.callback.next.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Reenter::Resume(fixture.policy.clone()));
        fixture.supervisor.stand_by(&fixture.client, "the prior policy was unreadable".into()).await;
        let after_standby = publication(&fixture);
        fixture.supervisor.refresh(&fixture.client).await;
        let after_refresh = publication(&fixture);
        let callback = fixture.callback.observed.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take();
        let remote_proven = fixture.client.status().await?.proven_holder(fixture.client.holder());
        finish(&fixture, observed).await?;
        eprintln!("standby publication: {after_standby:?}; repeated refresh: {after_refresh:?}; callback: {callback:?}");
        let callback = callback.ok_or_else(|| std::io::Error::other("withdrawal callback did not run"))?;
        assert_eq!(callback.before, Some((true, Some("the prior policy was unreadable".into()))));
        assert!(matches!(callback.outcome, Some(Reentered::Resumed(Some(_)))));
        assert!(callback.active_after && callback.claim_after.as_deref() == Some(fixture.client.holder()));
        assert!(remote_proven);
        assert_active(&after_standby, fixture.client.holder());
        assert_active(&after_refresh, fixture.client.holder());
        Ok(())
    }).await?
}

#[tokio::test(flavor = "current_thread")]
async fn a_loss_or_standby_without_a_retake_remains_passive() -> TestResult<()> {
    tokio::time::timeout(HANG, async {
        for standby in [false, true] {
            let fixture = fixture().await?;
            let observed = observe_withdrawal(&fixture);
            if standby {
                fixture
                    .supervisor
                    .stand_by(&fixture.client, "an ordinary standby".into())
                    .await;
            } else {
                fixture.supervisor.lost_locally("an ordinary loss");
            }
            let passive = publication(&fixture);
            let callback = fixture
                .callback
                .observed
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            finish(&fixture, observed).await?;
            assert!(callback.is_none(), "no retake was armed");
            assert!(!passive.acting && passive.client_proven);
            assert_eq!(passive.autopilot, AutopilotState::Passive);
            assert_eq!(passive.held, standby);
            assert_eq!(
                passive.claim.as_deref(),
                standby.then(|| fixture.client.holder())
            );
            assert_eq!(passive.standby.is_some(), standby);
        }
        Ok(())
    })
    .await?
}

#[tokio::test(flavor = "current_thread")]
async fn a_fresh_take_after_a_completed_loss_stays_active_through_normal_refresh() -> TestResult<()>
{
    tokio::time::timeout(HANG, async {
        let fixture = fixture().await?;
        let observed = observe_withdrawal(&fixture);
        fixture.supervisor.lost_locally("a completed prior loss");
        let lost = publication(&fixture);
        let retaken = fixture.client.claim().await?;
        let remote_proven = fixture
            .client
            .status()
            .await?
            .proven_holder(fixture.client.holder());
        let taken = fixture.supervisor.taken(fixture.policy.clone());
        let after_take = publication(&fixture);
        fixture.supervisor.refresh(&fixture.client).await;
        let after_refresh = publication(&fixture);
        finish(&fixture, observed).await?;
        assert!(!lost.held && !lost.acting && lost.claim.is_none());
        assert_eq!(lost.autopilot, AutopilotState::Passive);
        assert!(matches!(retaken, ClaimOutcome::Held { .. }) && remote_proven);
        assert_eq!(taken, Taken::Held);
        assert_active(&after_take, fixture.client.holder());
        assert_active(&after_refresh, fixture.client.holder());
        Ok(())
    })
    .await?
}
