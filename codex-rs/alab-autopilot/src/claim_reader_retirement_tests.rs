// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A running old reader can outlive an abort request and poison a replacement's epoch.

use super::*;
use codex_alab_core::session::Frame;
use codex_alab_core::session::ProgramState;
use codex_alab_core::session::now_ms;
use std::sync::Condvar;

struct ReaderGate(Arc<(Mutex<bool>, Condvar)>);

impl ReaderGate {
    fn install(supervisor: &Supervisor) -> (Self, tokio::sync::oneshot::Receiver<()>) {
        let gate = Self(Arc::new((Mutex::new(false), Condvar::new())));
        let waiting = Arc::clone(&gate.0);
        let (entered, reading) = tokio::sync::oneshot::channel();
        *supervisor.before_claim_event.lock().unwrap() = Some(Box::new(move || {
            // Keep the running poll, but hand off its worker's queued LIFO work.
            tokio::task::block_in_place(move || {
                let _ = entered.send(());
                let (released, changed) = &*waiting;
                let ready = released.lock().unwrap();
                let (ready, _) = changed
                    .wait_timeout_while(ready, HANG, |released| !*released)
                    .unwrap();
                let released = *ready;
                drop(ready);
                assert!(released, "reader admission hang detector");
            });
        }));
        (gate, reading)
    }

    fn release(&self) {
        *self
            .0
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
        self.0.1.notify_all();
    }
}

impl Drop for ReaderGate {
    fn drop(&mut self) {
        self.release();
    }
}

fn frame(fixture: &Fixture) {
    let root = "reader-retirement-root";
    fixture.sup.shared().observe(
        &Frame {
            primary_thread: Some(root),
            displayed_thread: Some(root),
            rollout_path: None,
            cwd: std::path::Path::new("/work"),
            model: "gpt-5",
            codex_home: fixture._dir.path(),
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
    fixture.sup.shared().note_root_thread(root);
}

fn settled(supervisor: &Supervisor) -> bool {
    let state = supervisor.lock();
    state.acting() && !state.attention_unknown() && !state.needs_unknown()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_retired_reader_cannot_clear_its_proven_replacement() -> TestResult<()> {
    tokio::time::timeout(HANG, async {
        codex_alab_core::activate();
        let mut outcomes = Vec::new();
        for case in ["ordinary-reconnect", "person-clear", "retired-reader"] {
            let fixture = fixture().await?;
            frame(&fixture);
            fixture.sup.set_policy_recheck(Duration::from_secs(3600));
            fixture.fake.set_policy_key("human_grace_s", "0");
            fixture.fake.set_status("autopilot", "cruise");
            let (gate, reading) = if case == "retired-reader" {
                let (gate, reading) = ReaderGate::install(&fixture.sup);
                (Some(gate), Some(reading))
            } else {
                (None, None)
            };
            let (tui, mut changes) = watch::channel(0);
            let mut tries = 0;
            let mut old_session = Box::pin(fixture.sup.session(
                fixture.fake.endpoint(),
                &mut changes,
                &mut tries,
                Some(fixture.client.clone()),
            ));
            assert!(
                drive_until(old_session.as_mut(), || settled(&fixture.sup))
                    .await?
                    .is_none()
            );
            assert_eq!(
                fixture.sup.ready().await,
                Ready::Go,
                "{case}: initial readiness"
            );
            if case == "person-clear" {
                fixture.fake.clear_claim();
                assert!(
                    drive_until(old_session.as_mut(), || {
                        let state = fixture.sup.lock();
                        state.cleared
                            && !state.held
                            && state.queued_claim_events.pending.load(Ordering::SeqCst) == 0
                    })
                    .await?
                    .is_none()
                );
                fixture.sup.refresh(&fixture.client).await;
                let cleared = fixture.sup.lock().cleared;
                let preserved = !fixture.sup.held()
                    && !fixture.sup.acting()
                    && cleared
                    && fixture.fake.claim().is_none();
                assert!(matches!(fixture.sup.ready().await, Ready::Passive(_)));
                drop(tui);
                let ended = tokio::time::timeout(HANG, old_session.as_mut()).await?;
                assert!(matches!(ended, SessionEnd::Closing));
                assert!(!fixture.client.is_open());
                assert!(
                    preserved,
                    "the actual current person's clear stays authoritative"
                );
                eprintln!("{case}: actual clear remains given up after same-policy refresh");
                continue;
            }

            let mut returned = if let Some(reading) = reading {
                fixture.fake.clear_claim();
                tokio::time::timeout(HANG, reading).await??;
                let retiring = Arc::new(AtomicBool::new(false));
                *fixture.sup.reader_retiring.lock().unwrap() = Some(Box::new({
                    let retiring = Arc::clone(&retiring);
                    move || retiring.store(true, Ordering::SeqCst)
                }));
                // Retire the real request socket before asking whether the old
                // session can finish. No cleanup request remains to explain Pending.
                fixture.client.close();
                tokio::time::timeout(HANG, fixture.client.closed()).await?;
                let returned =
                    drive_until(old_session.as_mut(), || retiring.load(Ordering::SeqCst)).await?;
                assert!(
                    retiring.load(Ordering::SeqCst),
                    "actual reader retirement was entered"
                );
                returned
            } else {
                // An ordinary push-stream end lets the actual reader finish
                // normally, before the owning session retires its request client.
                fixture.fake.end_subscriptions();
                Some(tokio::time::timeout(HANG, old_session.as_mut()).await?)
            };
            let late = returned.is_some() && gate.is_some();
            if returned.is_none() {
                // A correct abort-and-join may legitimately hold retirement here.
                // Release the old synchronous poll before starting any replacement.
                gate.as_ref()
                    .expect("only the held reader can defer retirement")
                    .release();
                returned = Some(tokio::time::timeout(HANG, old_session.as_mut()).await?);
            }
            assert!(matches!(returned, Some(SessionEnd::Retry(_))));
            drop(old_session);
            drop(tui);
            fixture.sup.lost_locally("the aterm connection ended");
            let client =
                Client::connect(fixture.fake.endpoint(), fixture.sup.opts.client.clone()).await?;
            let (next_tui, mut next_changes) = watch::channel(0);
            let mut next_tries = 0;
            let mut replacement = std::pin::pin!(fixture.sup.session(
                fixture.fake.endpoint(),
                &mut next_changes,
                &mut next_tries,
                Some(client.clone()),
            ));
            assert!(
                drive_until(replacement.as_mut(), || settled(&fixture.sup))
                    .await?
                    .is_none()
            );
            assert!(client.status().await?.proven_holder(client.holder()));
            assert_eq!(
                fixture.sup.ready().await,
                Ready::Go,
                "{case}: replacement readiness"
            );

            let ready = if late {
                let met = fixture.fake.gated_requests().len();
                fixture.fake.gate(CLAIM);
                let mut checks = std::pin::pin!(fixture.sup.ready());
                assert!(
                    drive_until(checks.as_mut(), || {
                        fixture
                            .fake
                            .gated_requests()
                            .iter()
                            .skip(met)
                            .any(|request| request.contains(CLAIM))
                    })
                    .await?
                    .is_none(),
                    "the replacement passed status and entered its real reconfirm"
                );
                let epoch = fixture.sup.lost_epoch.load(Ordering::SeqCst);
                gate.as_ref().unwrap().release();
                tokio::time::timeout(HANG, async {
                    while fixture.sup.lost_epoch.load(Ordering::SeqCst) == epoch {
                        tokio::task::yield_now().await;
                    }
                })
                .await?;
                assert_eq!(
                    fixture
                        .sup
                        .lock()
                        .queued_claim_events
                        .pending
                        .load(Ordering::SeqCst),
                    0,
                    "only the retired reader published: the replacement has no queued clear"
                );
                fixture.fake.open_gate(CLAIM);
                tokio::time::timeout(HANG, checks.as_mut()).await?
            } else {
                Ready::Go
            };
            let stayed_active = ready == Ready::Go && fixture.sup.held() && fixture.sup.acting();
            let cleared = fixture.sup.lock().cleared;
            eprintln!(
                "{case}: late={late} ready={ready:?} held={} active={} cleared={cleared}; {:?}",
                fixture.sup.held(),
                fixture.sup.acting(),
                fixture.sup.journal()
            );
            outcomes.push((case, stayed_active));
            drop(next_tui);
            let ended = tokio::time::timeout(HANG, replacement.as_mut()).await?;
            assert!(matches!(ended, SessionEnd::Closing));
            assert!(!client.is_open());
        }
        assert!(
            outcomes.iter().all(|(_, active)| *active),
            "a retired reader must not revoke a proven replacement: {outcomes:?}"
        );
        Ok(())
    })
    .await?
}
