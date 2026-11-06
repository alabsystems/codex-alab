// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A failed first read cannot spend custody of a native root's predecessor text.

use super::upgrade_handoff_tests::Harness;
use super::upgrade_handoff_tests::native_test;
use super::*;
use codex_alab_core::session::Predecessor;

const CARRIED: &str = "goal: the dead predecessor's task";

fn predecessor(h: &Harness) {
    h.shared.note_predecessor(Predecessor {
        pid: 41,
        session_id: h.rt.key().to_owned(),
        exit: Some("signal:9".into()),
        status: Some("idle".into()),
        updated_at_ms: now_ms().saturating_sub(1),
        ..Predecessor::default()
    });
    h.shared.set_swept();
    h.fake.describe_as_person(CARRIED);
    assert!(h.shared.predecessor(h.rt.key()).is_some());
}

async fn failed_connection(h: &Harness) {
    // Move only this fixture's listening socket name. Existing claim/event
    // connections stay open; the description's own new connection fails at once.
    let endpoint = h.fake.endpoint();
    let unavailable = endpoint.sock.with_extension("unavailable");
    std::fs::rename(&endpoint.sock, &unavailable).expect("hide the scratch socket name");
    let probe = tokio::net::UnixStream::connect(&endpoint.sock).await;
    h.autopilot.refresh_description(&h.rt).await;
    std::fs::rename(&unavailable, &endpoint.sock).expect("restore the scratch socket name");
    assert_eq!(
        probe
            .expect_err("the fault is an actual failed connection")
            .kind(),
        std::io::ErrorKind::NotFound
    );
    assert_eq!(h.fake.description().as_deref(), Some(CARRIED));
    assert!(h.autopilot.sup.acting(), "the existing claim remains held");
}

#[test]
fn a_failed_first_description_connection_preserves_carried_withdrawal() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            for fault in [false, true] {
                let h = Harness::new().await;
                h.immediate_point().await;
                predecessor(&h);
                if fault {
                    failed_connection(&h).await;
                }
                h.autopilot.refresh_description(&h.rt).await;
                let shown = h.fake.description();
                let journal = h.autopilot.sup.journal();
                h.test.codex.shutdown_and_wait().await.expect("native root shutdown");
                assert_eq!(
                    shown, None,
                    "a healthy retry withdraws predecessor text; transport_fault={fault}; journal={journal:?}"
                );
            }
        })
        .await
        .expect("carried withdrawal connection hang detector");
    });
}

#[test]
fn a_failed_first_description_connection_preserves_carried_replacement() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            for fault in [false, true] {
                let h = Harness::new().await;
                h.immediate_point().await;
                predecessor(&h);
                h.rt.set_plan_step(Some("the current native task"));
                if fault {
                    failed_connection(&h).await;
                }
                h.autopilot.refresh_description(&h.rt).await;
                let shown = h.fake.description();
                let journal = h.autopilot.sup.journal();
                h.test.codex.shutdown_and_wait().await.expect("native root shutdown");
                assert_eq!(
                    shown.as_deref(), Some("plan: the current native task"),
                    "a healthy retry replaces predecessor text; transport_fault={fault}; journal={journal:?}"
                );
            }
        })
        .await
        .expect("carried replacement connection hang detector");
    });
}

#[test]
fn a_successful_first_description_read_retires_carried_custody() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            let h = Harness::new().await;
            h.immediate_point().await;
            predecessor(&h);
            h.autopilot.refresh_description(&h.rt).await;
            assert_eq!(
                h.fake.description(),
                None,
                "the predecessor text is withdrawn"
            );
            let persons = "goal: a later description the person owns";
            h.fake.describe_as_person(persons);
            h.rt.set_plan_step(Some("the current native task"));
            h.autopilot.refresh_description(&h.rt).await;
            let shown = h.fake.description();
            h.test
                .codex
                .shutdown_and_wait()
                .await
                .expect("native root shutdown");
            assert_eq!(
                shown.as_deref(),
                Some(persons),
                "settled predecessor custody cannot claim later matching-shaped text"
            );
        })
        .await
        .expect("settled carried custody hang detector");
    });
}
