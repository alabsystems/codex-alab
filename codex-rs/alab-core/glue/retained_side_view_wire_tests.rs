// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use codex_app_server_protocol::JSONRPCError;
use codex_app_server_protocol::ThreadUnsubscribeParams;
use codex_app_server_protocol::ThreadUnsubscribeResponse;
use codex_app_server_protocol::ThreadUnsubscribeStatus;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

const CHILD: &str = "7bd5743b-a0bc-4819-823d-24f9eb7e8235";
const REJECTION: i64 = -32042;
const REJECTION_MESSAGE: &str = "the original close failed";

fn restored(original: &SideViewCloseOperation) -> SideViewCloseOperation {
    let bytes = serde_json::to_vec(&original.checkpoint()).unwrap();
    SideViewCloseOperation::restore(serde_json::from_slice(&bytes).unwrap()).unwrap()
}

async fn close(
    owner: &SideViewCloseOperation,
    handle: &AppServerRequestHandle,
    id: i64,
) -> Result<ThreadUnsubscribeResponse, TypedRequestError> {
    owner
        .run(handle.request_typed(ClientRequest::ThreadUnsubscribe {
            request_id: RequestId::Integer(id),
            params: ThreadUnsubscribeParams {
                thread_id: CHILD.into(),
            },
        }))
        .await
}

fn assert_close_result(
    result: Result<ThreadUnsubscribeResponse, TypedRequestError>,
    expected: &Result<ThreadUnsubscribeStatus, Option<Value>>,
) {
    let actual = match result {
        Ok(response) => Ok(response.status),
        Err(TypedRequestError::Server { source, .. }) => {
            assert_eq!(source.code, REJECTION);
            assert_eq!(source.message, REJECTION_MESSAGE);
            Err(source.data)
        }
        Err(error) => panic!("expected the original close result: {error}"),
    };
    assert_eq!(&actual, expected);
}

#[tokio::test]
async fn restored_side_close_reads_the_original_result_without_another_unsubscribe() {
    for (reply, expected) in [
        (
            json!({"kind":"success","status":"notLoaded"}),
            Ok(ThreadUnsubscribeStatus::NotLoaded),
        ),
        (
            json!({"kind":"rejected","code":REJECTION,"message":REJECTION_MESSAGE,"hadData":false,"value":null}),
            Err(None),
        ),
        (
            json!({"kind":"rejected","code":REJECTION,"message":REJECTION_MESSAGE,"hadData":true,"value":null}),
            Err(Some(Value::Null)),
        ),
    ] {
        timeout(
            Duration::from_secs(60),
            exercise_restored_close(None, reply, expected),
        )
        .await
        .expect("saved side-close wire exchange stalled");
    }
}

#[tokio::test]
async fn malformed_original_side_close_can_recover_the_confirmed_status() {
    timeout(
        Duration::from_secs(60),
        exercise_restored_close(
            Some(json!({"status": "unknownStatus"})),
            json!({"kind":"success","status":"unsubscribed"}),
            Ok(ThreadUnsubscribeStatus::Unsubscribed),
        ),
    )
    .await
    .expect("saved side-close wire exchange stalled");
}

async fn exercise_restored_close(
    malformed_response: Option<Value>,
    reply: Value,
    expected: Result<ThreadUnsubscribeStatus, Option<Value>>,
) {
    let view = SideView {
        schema: 1,
        claim: "a".repeat(32),
        thread_id: CHILD.into(),
    };
    let owner = SideViewCloseOperation::new(view.clone()).unwrap();
    let before_send = restored(&owner);
    let (sent, received) = oneshot::channel();
    let sent_malformed = malformed_response.is_some();
    let first_url = start_test_remote_server(move |mut socket| async move {
        expect_remote_initialize(&mut socket).await;
        let JSONRPCMessage::Request(request) = read_websocket_message(&mut socket).await else {
            panic!("missing initial unsubscribe");
        };
        assert_eq!(request.method, "thread/unsubscribe");
        let params = request.params.unwrap();
        assert_eq!(params["threadId"], CHILD);
        assert_eq!(params["_meta"]["alab.sideView"]["phase"], "close");
        if let Some(result) = malformed_response {
            write_websocket_message(
                &mut socket,
                JSONRPCMessage::Response(JSONRPCResponse {
                    id: request.id,
                    result,
                }),
            )
            .await;
            // A malformed success cannot be acknowledged to the original peer.
            assert!(matches!(
                socket.next().await,
                Some(Ok(Message::Close(_))) | None
            ));
        } else {
            socket.close(None).await.unwrap();
        }
        sent.send(params["_meta"]["alab.sideView"].clone()).unwrap();
    })
    .await;
    let first = AppServerClient::Remote(
        RemoteAppServerClient::connect(test_remote_connect_args(first_url))
            .await
            .unwrap(),
    );
    let result = close(&owner, &first.request_handle(), 1).await;
    if sent_malformed {
        assert!(matches!(result, Err(TypedRequestError::Deserialize { .. })));
        assert!(owner.has_frozen_request());
        assert!(!owner.original_succeeded());
        let checkpoint = serde_json::to_value(owner.checkpoint()).unwrap();
        assert_eq!(checkpoint.get("result"), Some(&Value::Null));
        assert!(
            owner
                .observe_applied(&first.request_handle())
                .await
                .is_err()
        );
    } else {
        assert!(matches!(result, Err(TypedRequestError::Transport { .. })));
    }
    first.shutdown().await.unwrap();
    let selection = received.await.unwrap();
    assert_eq!(selection["view"], json!(view));
    let operation = selection["operation"].as_str().unwrap().to_owned();
    let (finished, peer_done) = oneshot::channel();
    let second_url = start_test_remote_server(move |mut socket| async move {
        expect_remote_initialize(&mut socket).await;
        let ordinary_config = json!({"config":{}, "origins":{}, "layers":[]});
        let absent_job = JSONRPCErrorError { code: -32600, message: "no owned job".into(), data: None };
        let found = json!({"_meta":{"alab.sideViewResult":{
            "schema":1, "view":view, "operation":operation, "reply":reply
        }}});
        let mut different_view = found.clone();
        different_view["_meta"]["alab.sideViewResult"]["view"]["threadId"] = json!("another-child");
        let acknowledged = json!({"_meta":{"alab.sideViewObservation":{
            "schema":1, "claim":view.claim, "operation":operation, "threadId":CHILD, "state":"observed"
        }}});
        for (phase, result) in [
            ("readResult", Ok(ordinary_config)),
            ("readResult", Err(absent_job)),
            ("readResult", Ok(different_view)),
            ("readResult", Ok(found)),
            ("acknowledgeResult", Ok(acknowledged)),
        ] {
            let JSONRPCMessage::Request(request) = read_websocket_message(&mut socket).await else {
                panic!("expected a read-only request on the replacement");
            };
            assert_eq!(request.method, "config/read");
            assert_eq!(request.params, Some(json!({
                "includeLayers":false,
                "_meta":{"alab.sideView":{
                    "phase":phase, "view":view, "operation":operation,
                    "parameters":{"threadId":CHILD}
                }}
            })));
            let message = match result {
                Ok(result) => JSONRPCMessage::Response(JSONRPCResponse { id: request.id, result }),
                Err(error) => JSONRPCMessage::Error(JSONRPCError { id: request.id, error }),
            };
            write_websocket_message(&mut socket, message).await;
        }
        // Cached reads and a repeated acknowledgement must never reach the replacement.
        assert!(matches!(socket.next().await, Some(Ok(Message::Close(_))) | None));
        finished.send(()).unwrap();
    }).await;
    let replacement = AppServerClient::Remote(
        RemoteAppServerClient::connect(test_remote_connect_args(second_url))
            .await
            .unwrap(),
    );
    let handle = replacement.request_handle();
    assert!(matches!(
        close(&before_send, &handle, 2).await,
        Err(TypedRequestError::Transport { .. })
    ));
    assert!(!before_send.has_frozen_request());
    // A repeated original attempt sees the ordinary peer's result, which cannot confirm a close.
    assert!(matches!(
        close(&owner, &handle, 3).await,
        Err(TypedRequestError::Transport { .. })
    ));
    let recovered = restored(&owner);
    assert!(matches!(
        recovered.read_result(&handle).await,
        Err(TypedRequestError::Transport { .. })
    ));
    assert!(matches!(
        close(&recovered, &handle, 4).await,
        Err(TypedRequestError::Transport { .. })
    ));
    assert_close_result(recovered.read_result(&handle).await, &expected);
    let cached = restored(&recovered);
    assert_close_result(close(&cached, &handle, 5).await, &expected);
    cached.observe_applied(&handle).await.unwrap();
    cached.observe_applied(&handle).await.unwrap();
    replacement.shutdown().await.unwrap();
    peer_done.await.unwrap();
}
