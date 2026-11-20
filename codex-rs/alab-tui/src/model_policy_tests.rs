// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

#[tokio::test]
async fn kernel_peer_identity_thread_generation_and_replay_are_checked() {
    let home = tempfile::tempdir().unwrap();
    let dir = record::sessions_dir(home.path());
    let applied = Arc::new(AtomicUsize::new(0));
    let witness = applied.clone();
    let endpoint = listen(home.path(), 29, move |delivery| match delivery {
        Delivery::Identify(reply) => {
            let _ = reply.send(Some("thread-a".into()));
        }
        Delivery::Apply { apply, reply } => {
            assert_eq!(apply.target.thread, "thread-a");
            witness.fetch_add(1, Ordering::Relaxed);
            let _ = reply.send(Outcome::Applied {
                model: "gpt-6.2-sol".into(),
                effort: "ultra".into(),
            });
        }
    })
    .unwrap();
    let pid = std::process::id();
    let target = identify(&dir, pid).await.unwrap();
    assert_eq!(target.generation, 29);
    assert_eq!(target.pid, pid);
    let mut req = Apply {
        operation: "a39d7e3b3dfbbcb3cc60e14333e5aa81".into(),
        target,
        selection: Selection::LatestSolUltra,
    };
    let first = apply(&dir, &req).await;
    assert!(matches!(first, Outcome::Applied { .. }));
    assert_eq!(apply(&dir, &req).await, first);
    req.target.thread = "thread-b".into();
    assert_eq!(apply(&dir, &req).await, Outcome::Replaced);
    req.target.generation = 28;
    assert_eq!(apply(&dir, &req).await, Outcome::Replaced);
    assert_eq!(applied.load(Ordering::Relaxed), 1);
    drop(endpoint);
    assert!(!path(&dir, pid).exists());
}

#[tokio::test]
async fn canceled_delivery_loses_authority_before_the_application_dispatches_it() {
    let home = tempfile::tempdir().unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let _endpoint = listen(home.path(), 9, move |delivery| {
        let _ = tx.send(delivery);
    })
    .unwrap();
    let dir = record::sessions_dir(home.path());
    let pid = std::process::id();
    let mut stream = UnixStream::connect(path(&dir, pid)).await.unwrap();
    let request = Request::Apply(Apply {
        operation: "0123456789abcdeffedcba9876543210".into(),
        selection: Selection::LatestSolUltra,
        target: Target {
            pid,
            identity: (&identity::current().unwrap()).into(),
            generation: 9,
            thread: "t".into(),
        },
    });
    send(&mut stream, &request).await.unwrap();
    let Delivery::Apply { mut reply, .. } = rx.recv().await.unwrap() else {
        panic!("apply expected")
    };
    drop(stream);
    tokio::time::timeout(Duration::from_secs(2), reply.closed())
        .await
        .unwrap();
}

#[test]
fn accepts_only_canonical_ids_and_the_supported_selection() {
    assert!(operation_id("a39d7e3b3dfbbcb3cc60e14333e5aa81"));
    for value in ["", "../policy", "A39D7E3B3DFBBCB3CC60E14333E5AA81"] {
        assert!(!operation_id(value));
    }
    assert!(serde_json::from_str::<Selection>("\"shell\"").is_err());
}
