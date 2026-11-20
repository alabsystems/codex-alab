// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::OwnedLocalTarget;
use crate::app_server_session::SideCloseAck;
use codex_app_server_client::SideView;
use codex_app_server_client::TypedRequestError;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadUnsubscribeParams;
use codex_app_server_protocol::ThreadUnsubscribeResponse;
use codex_protocol::ThreadId;
use futures::SinkExt;
use futures::StreamExt;
use serde_json::Value;
use serde_json::json;
use std::io;
use tokio::sync::oneshot;
use tokio_tungstenite::tungstenite::Message;

#[test]
fn only_the_first_live_side_close_can_interrupt_and_an_unissued_close_can_be_replaced()
-> io::Result<()> {
    let target = OwnedLocalTarget::new("a".repeat(32), None, None);
    let thread = ThreadId::new();
    target.retain_side_view_data(SideView {
        schema: 1,
        claim: "b".repeat(32),
        thread_id: thread.to_string(),
    })?;
    let original = target.side_close_operation(thread)?.unwrap();
    let pending = target.side_close_operation(thread)?.unwrap();
    assert!(original.is_first());
    assert!(!pending.is_first());
    assert!(original.operation.same_operation(&pending.operation));

    let successor = OwnedLocalTarget::new("a".repeat(32), None, None);
    successor.restore_side_views(target.side_view_checkpoints())?;
    let recovered = successor.side_close_operation(thread)?.unwrap();
    assert!(!recovered.is_first());

    drop(original); // Original interrupt failed before its unsubscribe was polled.
    let next = target.side_close_operation(thread)?.unwrap();
    assert!(next.is_first());
    assert!(!next.operation.same_operation(&pending.operation));
    drop(pending);
    let observer = target.side_close_operation(thread)?.unwrap();
    assert!(!observer.is_first());
    assert!(observer.operation.same_operation(&next.operation));
    Ok(())
}

#[tokio::test]
async fn unavailable_callback_cannot_acknowledge_success_received_afterwards()
-> color_eyre::Result<()> {
    tokio::time::timeout(std::time::Duration::from_secs(60), delayed_close()).await?
}

async fn delayed_close() -> color_eyre::Result<()> {
    let thread = ThreadId::new();
    let target = OwnedLocalTarget::new("a".repeat(32), None, None);
    target.retain_side_view_data(SideView {
        schema: 1,
        claim: "b".repeat(32),
        thread_id: thread.to_string(),
    })?;
    let first = target.side_close_operation(thread)?.unwrap();
    let observer = target.side_close_operation(thread)?.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = crate::resolve_remote_addr(&format!("ws://{}", listener.local_addr()?))?;
    let (entered, started) = oneshot::channel();
    let (publish, allowed) = oneshot::channel();
    let peer = tokio::spawn(async move {
        let (stream, _) = listener.accept().await?;
        let mut socket = tokio_tungstenite::accept_async(stream).await?;
        let mut original_id = None;
        let mut entered = Some(entered);
        let mut allowed = Some(allowed);
        let mut acknowledgements = 0;
        while let Some(frame) = socket.next().await {
            let Message::Text(body) = frame? else { break };
            let request: Value = serde_json::from_str(&body)?;
            let id = &request["id"];
            let response = match request["method"].as_str().unwrap() {
                "initialize" => {
                    assert_eq!(id, "initialize");
                    json!({"id":id,"result":{
                        "userAgent":"side-close-test/1","codexHome":"/test/.codex",
                        "platformFamily":"unix","platformOs":"linux"
                    }})
                }
                "initialized" => continue,
                "thread/unsubscribe" => {
                    assert_eq!(id, &json!(1));
                    assert!(original_id.replace(id.clone()).is_none());
                    entered.take().unwrap().send(()).unwrap();
                    continue;
                }
                "config/read" => {
                    let selection = &request["params"]["_meta"]["alab.sideView"];
                    match selection["phase"].as_str().unwrap() {
                        "readResult" => {
                            let absent =
                                json!({"id":id,"error":{"code":-32600,"message":"pending"}});
                            socket
                                .send(Message::Text(absent.to_string().into()))
                                .await?;
                            allowed.take().unwrap().await?;
                            json!({"id":original_id.take().unwrap(),"result":{"status":"unsubscribed"}})
                        }
                        "acknowledgeResult" => {
                            acknowledgements += 1;
                            json!({"id":id,"result":{"_meta":{"alab.sideViewObservation":{
                                "schema":1,"claim":selection["view"]["claim"],
                                "operation":selection["operation"],"threadId":thread.to_string(),"state":"observed"
                            }}}})
                        }
                        other => panic!("unexpected close query {other}"),
                    }
                }
                other => panic!("unexpected method {other}"),
            };
            socket
                .send(Message::Text(response.to_string().into()))
                .await?;
        }
        Ok::<_, color_eyre::Report>(acknowledgements)
    });
    let client = crate::connect_remote_app_server(endpoint).await?;
    let handle = client.request_handle();
    let original_handle = handle.clone();
    let operation = first.operation.clone();
    let original = tokio::spawn(async move {
        operation
            .run(original_handle.request_typed::<ThreadUnsubscribeResponse>(
                ClientRequest::ThreadUnsubscribe {
                    request_id: RequestId::Integer(1),
                    params: ThreadUnsubscribeParams {
                        thread_id: thread.to_string(),
                    },
                },
            ))
            .await
    });
    started.await?;
    let pending = observer.operation.read_result(&handle).await;
    assert!(matches!(pending, Err(TypedRequestError::Transport { .. })));
    let stale = SideCloseAck::for_reply(
        target.clone(),
        thread,
        &observer.operation,
        handle.clone(),
        &pending,
    );
    assert!(stale.is_none());
    publish.send(()).unwrap();
    let published = original.await?;
    assert!(published.is_ok());
    assert!(observer.operation.original_succeeded());
    SideCloseAck::after_application(stale).await;
    assert_eq!(target.side_view_checkpoints().len(), 1);
    let ack = SideCloseAck::for_reply(target.clone(), thread, &first.operation, handle, &published);
    assert!(ack.is_some());
    SideCloseAck::after_application(ack).await;
    assert!(target.side_view_checkpoints().is_empty());
    client.shutdown().await?;
    assert_eq!(peer.await??, 1);
    Ok(())
}
