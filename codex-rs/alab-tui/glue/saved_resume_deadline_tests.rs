// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;

use serde_json::Value;
use tokio::task::JoinHandle;

fn saved_reply(query: &SavedResumeQuery, outcome: Value) -> Result<Value> {
    let frozen = serde_json::to_value(query)?;
    Ok(
        json!({"result":{"_meta":{"alab.clientConstructionPrepared":{
            "phase":"savedResume", "threadId":query.child(), "offer":frozen["offer"], "outcome":outcome
        }}}}),
    )
}

// The peer accepts only fresh read IDs for the same immutable query. Any
// replayed resume or construction request fails on the actual WebSocket.
async fn saved_read_peer(
    query: &SavedResumeQuery,
    replies: Vec<Value>,
) -> Result<(AppServerSession, JoinHandle<Result<()>>)> {
    use futures::SinkExt;
    use futures::StreamExt;
    use tokio_tungstenite::tungstenite::Message;
    let frozen = serde_json::to_value(query)?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = crate::resolve_remote_addr(&format!("ws://{}", listener.local_addr()?))?;
    let peer = tokio::spawn(async move {
        let (io, _) = listener.accept().await?;
        let mut socket = tokio_tungstenite::accept_async(io).await?;
        let mut replies = replies.into_iter();
        let mut ids = Vec::new();
        while let Some(frame) = socket.next().await {
            let frame = match frame? {
                Message::Text(frame) => frame,
                Message::Close(_) => break,
                _ => continue,
            };
            let frame: Value = serde_json::from_str(&frame)?;
            let mut reply = match frame["method"].as_str() {
                Some("initialize") => json!({"result":{"userAgent":"saved-openings-test/1.0"}}),
                Some("initialized") => continue,
                Some("config/read") => {
                    assert_eq!(
                        frame["params"],
                        json!({"includeLayers":false, "_meta":{
                            "alab.clientConstructionPrepare": frozen
                        }})
                    );
                    assert!(frame["id"].is_string() && !ids.contains(&frame["id"]));
                    ids.push(frame["id"].clone());
                    replies.next().expect("unexpected extra saved-resume read")
                }
                other => panic!("unexpected request on the replacement: {other:?}"),
            };
            reply["id"] = frame["id"].clone();
            socket.send(Message::Text(reply.to_string().into())).await?;
        }
        assert!(replies.next().is_none(), "scheduled wire read missing");
        Ok(())
    });
    let server = AppServerSession::new(
        BackendId::new(),
        crate::connect_remote_app_server(endpoint).await?,
        crate::ThreadParamsMode::Remote,
    );
    Ok((server, peer))
}

async fn read_outcome(query: &SavedResumeQuery, server: &AppServerSession) -> Result<Outcome> {
    let handle = server.request_handle();
    Ok(Outcome::from(
        tokio::time::timeout(Duration::from_secs(60), query.read(&handle, query.child())).await?,
    ))
}

const OVERLOAD_MESSAGE: &str = "Server overloaded; retry later.";

#[tokio::test]
async fn pending_saved_resume_deadline_reads_the_same_original_until_terminal() -> Result<()> {
    let opening = Uuid::new_v4();
    let query = query(opening, ThreadId::new())?;
    let (server, peer) = saved_read_peer(
        &query,
        vec![
            saved_reply(&query, json!({"kind":"pending"}))?,
            json!({"error":{"code":-32001,"message":OVERLOAD_MESSAGE}}),
            saved_reply(&query, json!({"kind":"resumed"}))?,
        ],
    )
    .await?;
    // This plain WebSocket exercises read scheduling below Native ownership.
    let mut schedule = ReadSchedule::ready(TokioInstant::now());
    for (expected, delay) in [
        (Outcome::Pending, FIRST_RETRY),
        (Outcome::ReadOverloaded, FIRST_RETRY * 2),
    ] {
        schedule.started();
        let outcome = read_outcome(&query, &server).await?;
        assert_eq!(outcome, expected);
        tokio::time::pause();
        let now = TokioInstant::now();
        schedule.observed(&outcome, now, &server);
        let deadline = schedule
            .next
            .as_ref()
            .expect("same physical reader retries");
        assert_eq!(deadline.at - now, delay);
        assert!(
            deadline
                .reader
                .as_ref()
                .unwrap()
                .belongs_to_connection(&server)
        );
        let row = (deadline.at, opening);
        assert!(oldest_saved_reads(std::iter::once(row), now, 1).is_empty());
        assert_eq!(
            wait_for_saved_resume(Some((deadline.at, server.backend_id()))).await,
            server.backend_id()
        );
        assert_eq!(
            oldest_saved_reads(std::iter::once(row), TokioInstant::now(), 1),
            vec![opening]
        );
        tokio::time::resume();
    }
    schedule.started();
    let resumed = read_outcome(&query, &server).await?;
    assert!(matches!(resumed, Outcome::Resumed));
    schedule.observed(&resumed, TokioInstant::now(), &server);
    assert!(schedule.next.is_none());
    server.shutdown().await?;
    tokio::time::timeout(Duration::from_secs(60), peer).await???;
    Ok(())
}

#[tokio::test]
async fn first_saved_read_errors_and_an_original_rejection_do_not_arm_a_retry() -> Result<()> {
    let query = query(Uuid::new_v4(), ThreadId::new())?;
    for (reply, expected) in [
        (
            json!({"error":{"code":-32001,"message":OVERLOAD_MESSAGE}}),
            Outcome::ReadOverloaded,
        ),
        (
            json!({"result":{"config":{}, "origins":{}}}),
            Outcome::ReadFailed,
        ),
        (
            json!({"error":{"code":-32603,"message":OVERLOAD_MESSAGE}}),
            Outcome::ReadFailed,
        ),
        (
            json!({"error":{"code":-32601,"message":"Method not found"}}),
            Outcome::ReadFailed,
        ),
        (
            json!({"error":{"code":-32001,"message":"Unauthorized"}}),
            Outcome::ReadFailed,
        ),
        (
            json!({"error":{"code":-32001,"message":"Three feedback uploads are already in progress; try again after one finishes"}}),
            Outcome::ReadFailed,
        ),
        (
            json!({"error":{"code":-32001,"message":OVERLOAD_MESSAGE,"data":{"retry":true}}}),
            Outcome::ReadFailed,
        ),
        (
            saved_reply(
                &query,
                json!({"kind":"rejected","code":-32001,"message":OVERLOAD_MESSAGE,"hadData":false,"value":null}),
            )?,
            Outcome::Rejected(-32001),
        ),
    ] {
        let (server, peer) = saved_read_peer(&query, vec![reply]).await?;
        let mut schedule = ReadSchedule::ready(TokioInstant::now());
        schedule.started();
        let outcome = read_outcome(&query, &server).await?;
        assert_eq!(outcome, expected);
        schedule.observed(&outcome, TokioInstant::now(), &server);
        assert!(schedule.next.is_none());
        server.shutdown().await?;
        tokio::time::timeout(Duration::from_secs(60), peer).await???;
    }
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn due_saved_openings_leave_capacity_for_older_waiting_reads() {
    let mut now = tokio::time::Instant::now();
    let (owner, _wire) = session(BackendId::new());
    let mut schedules: Vec<_> = (1..=5)
        .map(|n| {
            let mut schedule = ReadSchedule::ready(now);
            schedule.started();
            schedule.observed(&Outcome::Pending, now, &owner);
            (Uuid::from_u128(n), schedule)
        })
        .collect();
    now = schedules[0].1.next.as_ref().expect("first Pending").at;
    tokio::time::sleep_until(now).await;
    let first = Uuid::from_u128(1);
    let pair = [(now, Uuid::from_u128(2)), (now, first)];
    assert!(oldest_saved_reads(pair.iter().copied(), now, 0).is_empty());
    assert_eq!(oldest_saved_reads(pair.into_iter(), now, 1), vec![first]);
    let mut dispatched = Vec::new();
    for remaining in [2, 2, 1] {
        let selected = oldest_saved_reads(
            schedules.iter().rev().filter_map(|(opening, schedule)| {
                schedule
                    .next
                    .as_ref()
                    .map(|deadline| (deadline.at, *opening))
            }),
            tokio::time::Instant::now(),
            MAX_READS,
        );
        assert_eq!(selected.len(), remaining);
        for opening in selected {
            let schedule = &mut schedules
                .iter_mut()
                .find(|(id, _)| *id == opening)
                .unwrap()
                .1;
            schedule.started();
            schedule.observed(&Outcome::Pending, tokio::time::Instant::now(), &owner);
            assert!(schedule.next.as_ref().unwrap().at > tokio::time::Instant::now());
            dispatched.push(opening);
        }
    }
    assert_eq!(dispatched, (1..=5).map(Uuid::from_u128).collect::<Vec<_>>());
}

#[tokio::test(start_paused = true)]
async fn saved_resume_read_failure_and_terminal_replies_disarm_the_deadline() {
    let (server, _wire) = session(BackendId::new());
    let (replacement, _other_wire) = session(server.backend_id());
    let now = tokio::time::Instant::now();
    let mut schedule = ReadSchedule::ready(now);
    assert!(
        schedule
            .next
            .as_ref()
            .is_some_and(|deadline| deadline.at == now && deadline.reader.is_none())
    );
    for outcome in [
        Outcome::ReadFailed,
        Outcome::Unavailable,
        Outcome::Rejected(-32001),
        Outcome::Resumed,
    ] {
        schedule.started();
        schedule.observed(&Outcome::Pending, now, &server);
        let bound = schedule.next.as_ref().unwrap().reader.as_ref().unwrap();
        assert!(bound.belongs_to_connection(&server));
        assert!(!bound.belongs_to_connection(&replacement));
        schedule.observed(&outcome, now, &server);
        assert!(schedule.next.is_none());
        schedule.rearm(now);
        schedule.started();
        schedule.observed(&Outcome::ReadOverloaded, now, &server);
        assert!(schedule.next.is_none(), "cleared reader evidence came back");
    }
    schedule.rearm(now);
    schedule.started();
    schedule.observed(&Outcome::Pending, now, &server);
    schedule.rearm(now);
    schedule.started();
    schedule.observed(&Outcome::ReadOverloaded, now, &replacement);
    assert!(
        schedule.next.is_none(),
        "a new connection inherited Pending"
    );
}

#[tokio::test(start_paused = true)]
async fn pending_saved_resume_backoff_is_bounded_and_explicit_rearm_is_immediate() {
    let (server, _wire) = session(BackendId::new());
    let mut schedule = ReadSchedule::ready(tokio::time::Instant::now());
    let mut observed = FIRST_RETRY;
    for index in 0..10 {
        let now = tokio::time::Instant::now();
        schedule.started();
        let outcome = if index == 0 {
            Outcome::Pending
        } else {
            Outcome::ReadOverloaded
        };
        schedule.observed(&outcome, now, &server);
        let at = schedule.next.as_ref().unwrap().at;
        assert_eq!(at - now, observed);
        observed = (observed * 2).min(MAX_RETRY);
        tokio::time::sleep_until(at).await;
    }
    assert_eq!(observed, MAX_RETRY);
    let now = tokio::time::Instant::now();
    schedule.rearm(now);
    assert!(
        schedule
            .next
            .as_ref()
            .is_some_and(|deadline| deadline.at == now && deadline.reader.is_none())
    );
    schedule.started();
    schedule.observed(&Outcome::ReadOverloaded, now, &server);
    assert_eq!(schedule.next.as_ref().unwrap().at - now, MAX_RETRY);
}

#[tokio::test(start_paused = true)]
async fn immediate_read_can_use_a_slot_while_older_timed_rows_wait_for_the_timer() {
    let (server, _wire) = session(BackendId::new());
    let (other, _other_wire) = session(server.backend_id());
    let started = TokioInstant::now();
    let mut pending = ReadSchedule::ready(started);
    pending.started();
    pending.observed(&Outcome::Pending, started, &server);
    let timed = pending.next.as_ref().unwrap();
    tokio::time::advance(timed.at - started + MIN_BATCH_INTERVAL).await;
    let now = TokioInstant::now();
    let immediate = ReadSchedule::ready(now);
    let fresh = immediate.next.as_ref().unwrap();
    let rows = [
        (timed, Uuid::from_u128(1)),
        (timed, Uuid::from_u128(2)),
        (fresh, Uuid::from_u128(3)),
    ];
    let selected = |timer, next_batch| {
        oldest_saved_reads(
            rows.iter().filter_map(|(deadline, id)| {
                deadline
                    .eligible_on(&server, timer, next_batch, now)
                    .then_some((deadline.at, *id))
            }),
            now,
            MAX_READS,
        )
    };
    assert_eq!(selected(false, now), vec![rows[2].1]);
    assert_eq!(selected(true, now), vec![rows[0].1, rows[1].1]);
    assert!(!timed.eligible_on(&server, true, now + MIN_BATCH_INTERVAL, now));
    assert!(!timed.eligible_on(&other, true, now, now));
}
