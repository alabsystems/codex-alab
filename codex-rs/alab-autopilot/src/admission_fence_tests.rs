// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual claim/readiness observations across a later local admission wait.

use super::*;
use codex_alab_aterm::fake::FakeAterm;
use codex_alab_aterm::fake::Mode as FakeMode;
use codex_alab_core::session::Frame;

struct Fixture {
    fake: FakeAterm,
    sup: Arc<Supervisor>,
    shared: &'static Shared,
    dir: tempfile::TempDir,
}

impl Fixture {
    async fn new() -> Self {
        codex_alab_core::activate();
        let dir = tempfile::tempdir().expect("fixture directory");
        let fake = FakeAterm::start(dir.path(), FakeMode::Current)
            .await
            .expect("fake aterm");
        fake.set_policy_key("human_grace_s", "0");
        fake.set_status("autopilot", "cruise");
        let shared = Box::leak(Box::new(Shared::new()));
        let endpoint = fake.endpoint();
        let sup = Supervisor::new(SupervisorOptions {
            shared,
            endpoint: Arc::new(move || Some(endpoint.clone())),
            client: ClientOptions::default(),
        });
        let fixture = Self {
            fake,
            sup,
            shared,
            dir,
        };
        fixture.frame(true);
        fixture.sup.start();
        wait("a proven claim", || fixture.sup.acting()).await;
        fixture
    }

    fn frame(&self, draft_empty: bool) {
        self.shared.observe(
            &Frame {
                primary_thread: Some("admission-fixture"),
                displayed_thread: Some("admission-fixture"),
                rollout_path: None,
                cwd: std::path::Path::new("/fixture"),
                model: "gpt-5",
                codex_home: self.dir.path(),
                embedded: true,
                state: ProgramState::Idle,
                draft_empty,
                queued_input: false,
                popup_open: false,
                editor_open: false,
                pending_user_turn: false,
            },
            now_ms(),
        );
    }

    async fn fence(&self) -> AdmissionFence {
        tokio::time::timeout(Duration::from_secs(60), async {
            loop {
                if let Ok(fence) = self.sup.ready_for_admission().await {
                    return fence;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("fresh admission readiness")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.sup.closing.store(true, Ordering::SeqCst);
        if let Some(client) = self.sup.client() {
            client.close();
        }
        self.fake.end_subscriptions();
    }
}

async fn wait(label: &str, predicate: impl Fn() -> bool) {
    tokio::time::timeout(Duration::from_secs(60), async {
        while !predicate() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("{label}"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queued_and_observed_hold_revoke_a_previously_ready_admission() {
    let fixture = Fixture::new().await;
    let before = fixture.fence().await;
    assert!(fixture.sup.admission_fence_current(&before));
    fixture.fake.gate("harness policy");
    fixture.fake.set_status("hold", "1");
    fixture.fake.set_effective(false, "hold");
    wait("actual hold waits at policy transport", || {
        fixture
            .fake
            .gated_requests()
            .iter()
            .any(|request| request.contains("harness policy"))
    })
    .await;
    assert!(!fixture.sup.lock().hold, "the policy reply is still gated");
    assert!(
        !fixture.sup.admission_fence_current(&before),
        "queued hold cannot borrow cached readiness"
    );
    fixture.fake.open_gate("harness policy");
    wait("the native hold is observed", || fixture.sup.lock().hold).await;
    assert!(fixture.sup.acting(), "a hold keeps the proven claim");
    assert!(!fixture.sup.admission_fence_current(&before));
    fixture.fake.set_status("hold", "0");
    fixture.fake.set_effective(true, "enabled");
    let after = fixture.fence().await;
    assert!(fixture.sup.admission_fence_current(&after));
    assert!(
        !fixture.sup.admission_fence_current(&before),
        "lifting the hold needs fresh readiness"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn manual_resume_skips_only_the_matching_input_grace() {
    let fixture = Fixture::new().await;
    fixture.fake.set_policy_key("human_grace_s", "120");
    wait("human grace policy", || {
        fixture
            .sup
            .policy()
            .is_some_and(|p| p.human_grace == Duration::from_secs(120))
    })
    .await;
    // This is an established claim; only the person's recent input is under test.
    fixture.sup.lock().since = Some(clock() - Duration::from_secs(121));
    fixture.shared.human_input(now_ms());
    fixture.frame(true);
    fixture.fake.set_status("human_ms", "0");
    fixture.fake.set_status("human_seq", "7");
    assert!(matches!(fixture.sup.ready().await, Ready::Wait { .. }));
    fixture.fake.set_status("resume_seq", "7");
    fixture.fake.set_status("resume_at", &now_ms().to_string());
    assert_eq!(fixture.sup.ready().await, Ready::Go);
    fixture.frame(false);
    assert!(matches!(fixture.sup.ready().await, Ready::Wait { why, .. } if why.contains("draft")));
    fixture.frame(true);
    fixture.fake.set_status("human_seq", "8");
    assert!(matches!(fixture.sup.ready().await, Ready::Wait { why, .. } if why.contains("person")));
    fixture.fake.set_status("human_seq", "7");
    assert_eq!(fixture.sup.ready().await, Ready::Go);
    fixture.fake.set_status("hold", "1");
    assert!(matches!(fixture.sup.ready().await, Ready::Wait { why, .. } if why.contains("Hold")));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn claim_loss_and_reacquisition_never_regrant_an_old_admission() {
    let fixture = Fixture::new().await;
    let before = fixture.fence().await;
    fixture.fake.set_status("autopilot", "off");
    fixture.fake.set_effective(false, "autopilot-off");
    wait("the actual claim is released", || {
        !fixture.sup.held() && fixture.fake.claim().is_none()
    })
    .await;
    assert!(!fixture.sup.admission_fence_current(&before));
    fixture.fake.set_status("autopilot", "cruise");
    fixture.fake.set_effective(true, "enabled");
    let after = fixture.fence().await;
    assert!(fixture.sup.admission_fence_current(&after));
    assert!(
        !fixture.sup.admission_fence_current(&before),
        "same holder on a new claim is different authority"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_later_draft_or_clearing_it_cannot_reuse_an_old_ready_fence() {
    let fixture = Fixture::new().await;
    let before = fixture.fence().await;
    fixture.frame(false);
    assert!(
        !fixture.sup.admission_fence_current(&before),
        "a real TUI frame has a new draft"
    );
    fixture.frame(true);
    assert!(
        !fixture.sup.admission_fence_current(&before),
        "clearing the draft does not resurrect old readiness"
    );
    assert!(fixture.sup.admission_fence_current(&fixture.fence().await));
}

#[test]
fn only_authority_relevant_reader_events_invalidate_readiness() {
    let events = Arc::new(ClaimEvents::default());
    let holder = "codex-alab@123";
    for event in [
        Event::Meta {
            field: "supervisor".into(),
            value: Some(holder.into()),
        },
        Event::Meta {
            field: "attention".into(),
            value: None,
        },
        Event::Meta {
            field: "description".into(),
            value: Some("working".into()),
        },
    ] {
        assert!(AdmissionEvent::for_event(&events, &event, holder).is_none());
    }
    assert_eq!(events.admission_epoch.load(Ordering::SeqCst), 0);
    let pending = AdmissionEvent::for_event(
        &events,
        &Event::Other("EVENT 1 hold 1 reason=fixture".into()),
        holder,
    )
    .expect("hold event");
    assert_eq!(events.admission_epoch.load(Ordering::SeqCst), 1);
    assert_eq!(events.admission_pending.load(Ordering::SeqCst), 1);
    drop(pending);
    assert_eq!(events.admission_pending.load(Ordering::SeqCst), 0);
    assert_eq!(
        events.admission_epoch.load(Ordering::SeqCst),
        1,
        "settling an event cannot restore the old epoch"
    );
}
