// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use codex_app_server_protocol::JSONRPCError;
use codex_app_server_protocol::ThreadResumeParams;
use pretty_assertions::assert_eq;
use serde_json::json;

const CHILD: &str = "7bd5743b-a0bc-4819-823d-24f9eb7e8235";

#[path = "saved_opening_lookup_wire_tests.rs"]
mod lookup;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reply {
    MissingPreparationObservation,
    MissingResumeObservation,
    RejectedResume,
    Disconnected,
}

#[tokio::test]
async fn saved_resume_counts_only_the_original_retained_wire_success() {
    for reply in [
        Reply::MissingPreparationObservation,
        Reply::MissingResumeObservation,
        Reply::RejectedResume,
        Reply::Disconnected,
    ] {
        timeout(Duration::from_secs(60), exercise_saved_resume(reply))
            .await
            .expect("saved resume wire exchange stalled");
    }
}

async fn exercise_saved_resume(reply: Reply) {
    let (finished, peer_done) = oneshot::channel();
    let url = start_test_remote_server(move |mut socket| async move {
        expect_remote_initialize(&mut socket).await;
        let JSONRPCMessage::Request(request) = read_websocket_message(&mut socket).await else {
            panic!("expected retained resume preparation");
        };
        assert_eq!(request.method, "config/read");
        let params = request.params.unwrap();
        let prepare = &params["_meta"]["alab.clientConstructionPrepare"];
        assert_eq!(prepare["method"], "thread/resume");
        assert_eq!(prepare["parameters"]["threadId"], CHILD);
        let mut result = json!({"config": {}, "origins": {}, "layers": []});
        if reply != Reply::MissingPreparationObservation {
            result["_meta"] = json!({"alab.clientConstructionPrepared": {
                "phase": "prepared", "preparation": prepare["preparation"],
                "claim": "a".repeat(32), "sourceOperation": "b".repeat(32), "selection": "c".repeat(32)
            }});
        }
        let config_reply = JSONRPCMessage::Response(JSONRPCResponse { id: request.id, result });
        write_websocket_message(&mut socket, config_reply).await;
        if reply != Reply::MissingPreparationObservation {
            let JSONRPCMessage::Request(resume) = read_websocket_message(&mut socket).await else {
                panic!("expected the original retained resume");
            };
            assert_eq!(resume.method, "thread/resume");
            assert_eq!(resume.id, RequestId::Integer(99));
            let params = resume.params.unwrap();
            assert_eq!(params["threadId"], CHILD);
            let offer = &params["_meta"]["alab.clientConstruction"]["prepared"];
            assert_eq!(offer["preparation"], prepare["preparation"]);
            let response = match reply {
                Reply::MissingResumeObservation => Some(JSONRPCMessage::Response(JSONRPCResponse {
                    id: resume.id,
                    result: json!({"thread": {"id": CHILD}}),
                })),
                Reply::RejectedResume => Some(JSONRPCMessage::Error(JSONRPCError {
                    id: resume.id,
                    error: JSONRPCErrorError {
                        code: -32042,
                        message: "resume rejected".into(),
                        data: None,
                    },
                })),
                Reply::Disconnected => None,
                Reply::MissingPreparationObservation => unreachable!(),
            };
            if let Some(response) = response {
                write_websocket_message(&mut socket, response).await;
            } else {
                socket.close(None).await.unwrap();
            }
        }
        if reply != Reply::Disconnected {
            // A preparation failure must leave no original resume in the socket.
            assert!(matches!(socket.next().await, Some(Ok(Message::Close(_))) | None));
        }
        finished.send(()).unwrap();
    }).await;
    let client = AppServerClient::Remote(
        RemoteAppServerClient::connect(test_remote_connect_args(url))
            .await
            .unwrap(),
    );
    let owner = ClientConstructionOperation::for_saved_resume(
        ClientConstructionSelection {
            prospective_create: Default::default(),
            presentation: ClientPresentation::SideConversation,
            prepared: None,
        },
        "738559f2e1cb4f488802888f31317233",
        CHILD,
    )
    .unwrap();
    let (result, unreadable, retained_transport_failure) = owner
        .run(client.request_handle().request_saved_resume_observed(
            RequestId::Integer(99),
            ThreadResumeParams {
                thread_id: CHILD.into(),
                ..Default::default()
            },
        ))
        .await;
    assert_eq!(unreadable, reply == Reply::MissingResumeObservation);
    assert_eq!(retained_transport_failure, reply == Reply::Disconnected);
    let entered: ClientConstructionCheckpoint =
        serde_json::from_slice(&serde_json::to_vec(&owner.checkpoint()).unwrap()).unwrap();
    assert_eq!(
        entered.saved_resume_query().unwrap().is_some(),
        reply == Reply::Disconnected
    );
    match (reply, result.unwrap_err()) {
        (Reply::RejectedResume, TypedRequestError::Server { source, .. }) => {
            assert_eq!(source.code, -32042)
        }
        (
            Reply::MissingPreparationObservation
            | Reply::MissingResumeObservation
            | Reply::Disconnected,
            TypedRequestError::Transport { .. },
        ) => {}
        (reply, error) => panic!("unexpected retained resume error for {reply:?}: {error}"),
    }
    client.shutdown().await.unwrap();
    peer_done.await.unwrap();
}
