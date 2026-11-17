// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual processor/Unix-WS controls for the required construction inhibitor.
//! This proves Source scheduling installation, not Native activation or Go.
use super::*;
use codex_alab_core::retained_goal_gate::Owner;

#[tokio::test]
async fn actual_prepared_processor_has_same_gate_before_dispatch_and_rejects_second_install() {
    let (_home, args) = arguments().await;
    let (incoming, _queue, _withdrawn) = input();
    let (_owner, gate, _hold) = Owner::new_held();
    let prepared = bounded(super::super::super::prepare_with_goal_gate(
        args,
        incoming.clone(),
        gate.clone(),
    ))
    .await
    .unwrap()
    .unwrap();
    let (_, foreign, _) = Owner::new_held();
    let foreign_refused = prepared
        .processor
        .alab_bind_retained_goal_gate(foreign)
        .is_err();
    let remains_closed = gate.admit().await.is_none();
    clean_prepared(prepared, incoming).await;
    assert!(foreign_refused);
    assert!(remains_closed);
}

#[tokio::test]
async fn actual_legacy_root_seals_registration_before_late_retained_install() {
    let (_home, args) = arguments().await;
    let (incoming, queue, _withdrawn) = input();
    let prepared = bounded(prepare(args, incoming.clone()))
        .await
        .unwrap()
        .unwrap();
    let processor = prepared.processor.clone();
    let stop = CancellationToken::new();
    let worker = tokio::spawn(run(prepared, incoming.clone(), stop.clone()));
    let result = std::panic::AssertUnwindSafe(async {
        let mut wire = connect(&queue).await;
        let started: ThreadStartResponse = serde_json::from_value(
            request(
                &mut wire,
                json!(2),
                "thread/start",
                json!({"ephemeral": true}),
            )
            .await,
        )
        .unwrap();
        let (_, gate, _) = Owner::new_held();
        let refused = processor.alab_bind_retained_goal_gate(gate).is_err();
        wire.close(None).await.unwrap();
        (started.thread.ephemeral, refused)
    })
    .catch_unwind()
    .await;
    incoming.stop();
    stop.cancel();
    let cleanup = bounded(worker).await.unwrap();
    match result {
        Ok((ephemeral, refused)) => {
            cleanup.unwrap();
            assert!(ephemeral);
            assert!(refused);
        }
        Err(panic) => std::panic::resume_unwind(panic),
    }
}
