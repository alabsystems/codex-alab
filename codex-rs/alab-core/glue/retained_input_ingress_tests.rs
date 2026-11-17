// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual Source connection gate/tracker scheduling only. No Native grant,
//! endpoint response, original Value acceptance or Core effects are fabricated.
use super::*;
use futures::FutureExt;
use tokio::sync::oneshot;

#[tokio::test]
async fn eof_without_actual_joined_reader_is_not_an_ingress_cut() -> io::Result<()> {
    let gate = Arc::new(ConnectionRpcGate::new());
    let cut = Arc::new(ConnectionCut::new(gate.clone()));
    gate.close().await;
    cut.close();
    if cut.drain().now_or_never().is_some() {
        return Err(invalid("EOF alone fabricated an ingress cut"));
    }
    cut.reader_joined();
    cut.drain().await;
    Ok(())
}

#[tokio::test]
async fn actual_prior_input_frame_holds_drain_until_original_queued_dispatch_finishes()
-> io::Result<()> {
    let gate = Arc::new(ConnectionRpcGate::new());
    let cut = Arc::new(ConnectionCut::new(gate.clone()));
    let (started, observed) = oneshot::channel();
    let (release, held) = oneshot::channel();
    let frame = cut.frame()?; // Actual read-to-queued-dispatch lease, before enqueue.
    let run = gate.run(async move {
        let _frame = frame;
        let _ = started.send(());
        let _ = held.await;
    });
    let body = async {
        let outcome = async {
            observed
                .await
                .map_err(|_| invalid("original RPC did not enter"))?;
            gate.close().await;
            cut.close();
            cut.reader_joined();
            if cut.drain().now_or_never().is_some() {
                return Err(invalid("joined reader skipped an actual accepted RPC"));
            }
            Ok(())
        }
        .await;
        let _ = release.send(()); // All Result paths release the SAME original body.
        outcome
    };
    let ((), outcome) = tokio::join!(run, body);
    gate.close().await;
    cut.close();
    cut.reader_joined();
    cut.drain().await;
    outcome
}

#[tokio::test]
async fn actual_spawned_dispatch_lease_survives_rpc_return_and_waiter_loss() -> io::Result<()> {
    let gate = Arc::new(ConnectionRpcGate::new());
    let cut = Arc::new(ConnectionCut::new(gate.clone()));
    let token = cut.dispatch()?; // Actual lease acquired before the worker is scheduled.
    let (started, observed) = oneshot::channel();
    let (release, held) = oneshot::channel();
    let original = async move {
        let _ = started.send(());
        let _ = held.await;
        drop(token);
    };
    let body = async {
        let outcome = async {
            observed
                .await
                .map_err(|_| invalid("original dispatch did not enter"))?;
            gate.close().await;
            cut.close();
            cut.reader_joined();
            if cut.dispatch().is_ok() {
                return Err(invalid("closed connection admitted another dispatch"));
            }
            let mut observation = Box::pin(cut.drain());
            if observation.as_mut().now_or_never().is_some() {
                return Err(invalid("RPC return forgot spawned dispatch custody"));
            }
            drop(observation); // Cancelling observation cannot cancel/join the original worker.
            if cut.drain().now_or_never().is_some() {
                return Err(invalid("cancelled drain forgot original dispatch"));
            }
            Ok(())
        }
        .await;
        let _ = release.send(());
        outcome
    };
    let ((), outcome) = tokio::join!(original, body);
    gate.close().await;
    cut.close();
    cut.reader_joined();
    cut.drain().await;
    outcome
}

#[tokio::test]
async fn reader_join_alone_cannot_close_current_connection_or_admit_absence() -> io::Result<()> {
    let gate = Arc::new(ConnectionRpcGate::new());
    let cut = Arc::new(ConnectionCut::new(gate.clone()));
    cut.reader_joined();
    if cut.drain().now_or_never().is_some() {
        return Err(invalid(
            "reader signal fabricated original connection closure",
        ));
    }
    let token = cut.dispatch()?;
    drop(token);
    gate.close().await;
    cut.close();
    cut.drain().await;
    Ok(())
}

#[tokio::test]
async fn unrelated_long_lived_rpc_is_not_an_idle_requirement_for_input_ingress_cut()
-> io::Result<()> {
    let gate = Arc::new(ConnectionRpcGate::new());
    let cut = Arc::new(ConnectionCut::new(gate.clone()));
    let (started, observed) = oneshot::channel();
    let (release, held) = oneshot::channel();
    let unrelated = gate.run(async move {
        let _ = started.send(());
        let _ = held.await;
    });
    let body = async {
        let outcome = async {
            observed
                .await
                .map_err(|_| invalid("unrelated RPC did not enter"))?;
            gate.close().await;
            cut.close();
            cut.reader_joined();
            if cut.drain().now_or_never().is_none() {
                return Err(invalid(
                    "unrelated RPC became an input-publication idle barrier",
                ));
            }
            Ok(())
        }
        .await;
        let _ = release.send(());
        outcome
    };
    let ((), outcome) = tokio::join!(unrelated, body);
    outcome
}
