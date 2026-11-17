// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Real START/listener/idle-unload controls. A Source Core lease grants no
//! Native admission, current origin, Go, frontend delivery or approval right.
use super::*;
use codex_app_server_protocol::ThreadResumeResponse;
use codex_app_server_protocol::ThreadSetNameResponse;
use codex_app_server_protocol::ThreadUnsubscribeResponse;
use codex_app_server_protocol::ThreadUnsubscribeStatus;

/// This is the original per-thread configuration input, not a timer or a
/// subscriber fabricated by the test. Existing Harness roots keep 3600s.
async fn idle_core(h: &mut Harness, request: i64) -> Result<(ThreadId, Arc<CodexThread>)> {
    let c1 = h.c1.clone();
    let started: ThreadStartResponse = h
        .request(
            ConnectionId(41),
            &c1,
            json!({
                "id": request, "method": "thread/start", "params": {
                    "ephemeral": false, "config": {"thread_unload_delay_secs": 1}
                }
            }),
        )
        .await?;
    let id = ThreadId::from_string(&started.thread.id)?;
    let actual = h.host.control.manager.get_thread(id).await?;
    ensure!(
        actual.config().await.thread_unload_delay == Duration::from_secs(1),
        "real Core did not receive the requested idle policy"
    );
    Ok((id, actual))
}

async fn current(h: &Harness, id: ThreadId, actual: &Arc<CodexThread>) -> Result<()> {
    let registered = h.host.control.manager.get_thread(id).await?;
    ensure!(Arc::ptr_eq(&registered, actual), "actual Core was replaced");
    ensure!(!actual.has_shutdown_status(), "actual Core was shut down");
    Ok(())
}

async fn idle_removed(h: &Harness, id: ThreadId, actual: &Arc<CodexThread>) -> Result<()> {
    bounded(async {
        loop {
            if h.host.control.manager.get_thread(id).await.is_err() {
                ensure!(
                    actual.has_shutdown_status(),
                    "Core removal preceded actual shutdown"
                );
                return Ok::<(), anyhow::Error>(());
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await??;
    Ok(())
}

#[tokio::test]
async fn primary_survives_real_c1_gap_and_c2_resume_then_last_drop_unloads() -> Result<()> {
    let model = wiremock::MockServer::start().await;
    let mut h = Harness::new_persistent(&model.uri()).await?;
    let mut lease = None;
    let mut same_lease = None;
    let mut c2_cleanup = None;
    let result: Result<()> = async {
        let (id, actual) = idle_core(&mut h, 701).await?;
        let owner = h
            .processor
            .thread_processor
            .alab_acquire_primary(id, actual.clone(), &h.input)
            .await
            .map_err(|error| anyhow::anyhow!("primary acquisition failed, code {}", error.code))?;
        same_lease = Some(owner.clone());
        lease = Some(owner);
        // Materialize metadata through the original live mutation so a real
        // loaded resume has its original saved-thread precondition.
        let c1 = h.c1.clone();
        let _: ThreadSetNameResponse = h
            .request(
                ConnectionId(41),
                &c1,
                json!({
                    "id":702,"method":"thread/name/set","params":{
                        "threadId":id.to_string(),"name":"Retained primary idle control"
                    }
                }),
            )
            .await?;
        let (ordinary_id, ordinary) = idle_core(&mut h, 703).await?;
        bounded(h.processor.connection_closed(ConnectionId(41), &h.c1)).await?;
        // The ordinary Core proves the original idle deadline and shared event
        // loop actually ran while the selected exact primary remained owned.
        idle_removed(&h, ordinary_id, &ordinary).await?;
        current(&h, id, &actual).await?;
        ensure!(
            !h.input.is_stopped(),
            "frontend loss withdrew the runtime owner"
        );

        let c2 = Arc::new(ConnectionSessionState::new(ConnectionOrigin::WebSocket));
        c2_cleanup = Some(c2.clone());
        let _: InitializeResponse = h
            .request(
                ConnectionId(42),
                &c2,
                json!({
                    "id":704,"method":"initialize","params":{
                        "clientInfo":{"name":"retained-http-c1","version":"1"},
                        "capabilities":{"experimentalApi":true,"requiredHumanApproval":true}
                    }
                }),
            )
            .await?;
        // No new override on rejoin: the actual loaded Core keeps its1s
        // policy, and original explicit-override replacement stays unchanged.
        let resumed: ThreadResumeResponse = h
            .request(
                ConnectionId(42),
                &c2,
                json!({
                    "id":705,"method":"thread/resume","params":{
                        "threadId":id.to_string()
                    }
                }),
            )
            .await?;
        ensure!(
            resumed.thread.id == id.to_string(),
            "C2 resumed another thread"
        );
        current(&h, id, &actual).await?;
        let unsubscribed: ThreadUnsubscribeResponse = h
            .request(
                ConnectionId(42),
                &c2,
                json!({
                    "id":706,"method":"thread/unsubscribe","params":{"threadId":id.to_string()}
                }),
            )
            .await?;
        ensure!(
            unsubscribed.status == ThreadUnsubscribeStatus::Unsubscribed,
            "C2 did not install a real subscription on the retained listener"
        );
        drop(lease.take());
        // One clone is one continuing owner. Wait past the genuine original
        // no-subscriber deadline; the test adds no keepalive or subscriber.
        tokio::time::sleep(Duration::from_millis(1200)).await;
        current(&h, id, &actual).await?;
        drop(same_lease.take());
        // The final Drop must wake the already-waiting original idle branch.
        idle_removed(&h, id, &actual).await?;
        ensure!(
            h.server_requests.load(Ordering::Relaxed) == 0,
            "primary lifetime depended on a frontend acknowledgement"
        );
        Ok(())
    }
    .await;
    // Lease release and physical client teardown occur on every Result path,
    // before the actual processor/host drain.
    drop(lease.take());
    drop(same_lease.take());
    let c2_closed = match c2_cleanup {
        Some(c2) => bounded(h.processor.connection_closed(ConnectionId(42), &c2))
            .await
            .map(|_| ()),
        None => Ok(()),
    };
    let cleanup = h.shutdown().await;
    result.and(c2_closed).and(cleanup)
}

#[tokio::test]
async fn primary_acquire_rejects_wrong_exact_core_and_already_idle_removed_core() -> Result<()> {
    let model = wiremock::MockServer::start().await;
    let mut h = Harness::new_persistent(&model.uri()).await?;
    let mut retained = None;
    let result: Result<()> = async {
        let (id, actual) = idle_core(&mut h, 711).await?;
        let (other_id, other) = idle_core(&mut h, 712).await?;
        let wrong = h
            .processor
            .thread_processor
            .alab_acquire_primary(id, other.clone(), &h.input)
            .await;
        ensure!(
            wrong.is_err(),
            "an ID selected a different actual Core for ownership"
        );
        current(&h, id, &actual).await?;
        current(&h, other_id, &other).await?;
        retained = Some(
            h.processor
                .thread_processor
                .alab_acquire_primary(id, actual.clone(), &h.input)
                .await
                .map_err(|error| {
                    anyhow::anyhow!("primary acquisition failed, code {}", error.code)
                })?,
        );
        bounded(h.processor.connection_closed(ConnectionId(41), &h.c1)).await?;
        idle_removed(&h, other_id, &other).await?;
        let closed = h
            .processor
            .thread_processor
            .alab_acquire_primary(other_id, other.clone(), &h.input)
            .await;
        ensure!(
            closed.is_err(),
            "ownership resurrected an actually unloaded Core"
        );
        ensure!(
            h.host.control.manager.get_thread(other_id).await.is_err(),
            "late ownership published a replacement under the old ID"
        );
        current(&h, id, &actual).await?;
        // Owner withdrawal must deny a new lease even for the current exact
        // Core. Existing retained custody is released during actual cleanup.
        h.input.stop();
        let withdrawn = h
            .processor
            .thread_processor
            .alab_acquire_primary(id, actual.clone(), &h.input)
            .await;
        ensure!(
            withdrawn.is_err(),
            "withdrawn owner acquired a fresh primary lease"
        );
        Ok(())
    }
    .await;
    drop(retained.take());
    result.and(h.shutdown().await)
}
