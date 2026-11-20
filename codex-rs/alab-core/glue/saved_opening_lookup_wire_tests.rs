// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use pretty_assertions::assert_eq;
use serde_json::Value;

const OPENING: &str = "738559f2e1cb4f488802888f31317233";

#[tokio::test]
async fn replacement_reads_only_the_original_attempted_saved_resume() {
    timeout(Duration::from_secs(60), lost_original_reply())
        .await
        .expect("saved opening lookup exchange stalled");
}

async fn lost_original_reply() {
    let (captured, original_wire) = oneshot::channel();
    let url = start_test_remote_server(move |mut socket| async move {
        expect_remote_initialize(&mut socket).await;
        let JSONRPCMessage::Request(request) = read_websocket_message(&mut socket).await else {
            panic!("expected the saved chooser's original preparation");
        };
        assert_eq!(request.method, "config/read");
        let prepare_params = request.params.unwrap();
        let opening = &prepare_params["_meta"]["alab.savedResumeOpening"];
        let original = prepare_params["_meta"]["alab.clientConstructionPrepare"].clone();
        assert_eq!(original["phase"], "prepare");
        assert_eq!(original["method"], "thread/resume");
        assert_eq!(original["preparation"], OPENING);
        assert!(original.get("savedOpening").is_none());
        assert_eq!(opening, OPENING);
        assert_eq!(original["parameters"]["threadId"], CHILD);
        let offer = json!({"preparation":OPENING, "claim":"a".repeat(32), "sourceOperation":"b".repeat(32), "selection":"c".repeat(32)});
        let mut confirmed = offer.clone();
        confirmed["phase"] = json!("prepared");
        write_websocket_message(&mut socket, JSONRPCMessage::Response(JSONRPCResponse{
            id:request.id, result:json!({"config":{},"origins":{},"layers":[],"_meta":{"alab.clientConstructionPrepared":confirmed}})
        })).await;
        let JSONRPCMessage::Request(request) = read_websocket_message(&mut socket).await else {
            panic!("expected the original resume");
        };
        assert_eq!(request.method, "thread/resume");
        assert_eq!(request.id, RequestId::Integer(19));
        let mut parameters = request.params.unwrap();
        let meta = parameters.as_object_mut().unwrap().remove("_meta").unwrap();
        assert_eq!(meta["alab.clientConstruction"]["prepared"], offer);
        assert_eq!(original["parameters"], parameters);
        let mut original = original;
        original.as_object_mut().unwrap().remove("phase");
        original["savedOpening"] = opening.clone();
        captured.send((original,offer)).unwrap();
        socket.close(None).await.unwrap();
    }).await;
    let c1 = AppServerClient::Remote(
        RemoteAppServerClient::connect(test_remote_connect_args(url))
            .await
            .unwrap(),
    );
    let selection = ClientConstructionSelection {
        presentation: ClientPresentation::SideConversation,
        prospective_create: Default::default(),
        prepared: None,
    };
    let params = ThreadResumeParams {
        thread_id: CHILD.into(),
        model: Some("original-model".into()),
        ..Default::default()
    };
    let owner =
        ClientConstructionOperation::for_saved_resume(selection.clone(), OPENING, CHILD).unwrap();
    let before_send = owner.checkpoint();
    assert_eq!(before_send.saved_opening(), Some((OPENING, CHILD)));
    assert!(before_send.saved_resume_query().unwrap().is_none());
    let (error, unreadable, retained_transport_failure) = owner
        .run(
            c1.request_handle()
                .request_saved_resume_observed(RequestId::Integer(19), params.clone()),
        )
        .await;
    assert!(matches!(error, Err(TypedRequestError::Transport { .. })));
    assert!(!unreadable);
    assert!(retained_transport_failure);
    let (original, offer) = original_wire.await.unwrap();
    c1.shutdown().await.unwrap();

    let query = owner.saved_resume_query(CHILD).unwrap();
    let restored = serde_json::from_slice::<ClientConstructionCheckpoint>(
        &serde_json::to_vec(&owner.checkpoint()).unwrap(),
    )
    .unwrap();
    assert_eq!(restored.saved_resume_query().unwrap(), Some(query.clone()));
    let mut completed = serde_json::to_value(&restored).unwrap();
    completed["result"] = json!({
        "prepared":offer, "threadId":CHILD, "historyFallback":false,
        "dynamicToolsFallback":false, "backendTaskTools":false, "primaryCandidate":null
    });
    let completed: ClientConstructionCheckpoint = serde_json::from_value(completed).unwrap();
    assert!(completed.saved_resume_query().unwrap().is_none());
    let serialized_query = serde_json::to_value(query).unwrap();
    let query: SavedResumeQuery = serde_json::from_value(serialized_query.clone()).unwrap();
    assert_eq!(query.opening(), OPENING);
    assert_eq!(query.child(), CHILD);
    let mut other = serialized_query;
    other["offer"]["preparation"] = json!("d".repeat(32));
    assert!(serde_json::from_value::<SavedResumeQuery>(other).is_err());
    let mut changed = serde_json::to_value(&restored).unwrap();
    changed["intent"]["opening"] = json!("e".repeat(32));
    let changed: ClientConstructionCheckpoint = serde_json::from_value(changed).unwrap();
    assert!(changed.saved_resume_query().is_err());

    let (finished, peer_done) = oneshot::channel();
    let url = start_test_remote_server(move |mut socket| async move {
        expect_remote_initialize(&mut socket).await;
        let results = |outcome: Value| json!({"_meta":{"alab.clientConstructionPrepared":{
            "phase":"savedResume","threadId":CHILD,"offer":offer,"outcome":outcome
        }}});
        let mut wrong_offer = results(json!({"kind":"resumed"}));
        wrong_offer["_meta"]["alab.clientConstructionPrepared"]["offer"]["sourceOperation"] = json!("e".repeat(32));
        let mut wrong_child = results(json!({"kind":"resumed"}));
        wrong_child["_meta"]["alab.clientConstructionPrepared"]["threadId"] = json!("foreign");
        let mut extra_body = results(json!({"kind":"resumed"}));
        extra_body["config"] = json!({});
        let rejection = |had_data, value:Value| json!({"kind":"rejected","code":-32042,"message":"original rejected","hadData":had_data,"value":value});
        for result in [
            json!({"config":{},"origins":{},"layers":[]}), wrong_offer, wrong_child, extra_body,
            results(json!({"kind":"resumed","extra":1})), results(rejection(false,json!(42))),
            results(json!({"kind":"pending"})), results(json!({"kind":"unavailable"})),
            results(rejection(false,Value::Null)), results(rejection(true,Value::Null)),
            results(rejection(true,json!({"original":[1,2]}))), results(json!({"kind":"resumed"})),
        ] {
            let JSONRPCMessage::Request(request) = read_websocket_message(&mut socket).await else {
                panic!("expected replacement read-only lookup");
            };
            assert_eq!(request.method, "config/read");
            assert_eq!(request.params, Some(json!({"includeLayers":false,"_meta":{"alab.clientConstructionPrepare":{
                "phase":"readSavedResume","original":original,"offer":offer
            }}})));
            write_websocket_message(&mut socket, JSONRPCMessage::Response(JSONRPCResponse {id:request.id,result})).await;
        }
        assert!(matches!(socket.next().await, Some(Ok(Message::Close(_))) | None));
        finished.send(()).unwrap();
    }).await;
    let c2 = AppServerClient::Remote(
        RemoteAppServerClient::connect(test_remote_connect_args(url))
            .await
            .unwrap(),
    );
    let handle = c2.request_handle();
    assert!(query.read(&handle, "wrong-child").await.is_err());
    for owner in [
        owner,
        ClientConstructionOperation::restore(before_send).unwrap(),
        ClientConstructionOperation::restore(restored).unwrap(),
    ] {
        let (result, unreadable, retained_transport_failure) = owner
            .run(handle.request_saved_resume_observed(RequestId::Integer(20), params.clone()))
            .await;
        assert!(result.is_err() && !unreadable && !retained_transport_failure);
    }
    assert!(
        ClientConstructionOperation::new(selection)
            .unwrap()
            .saved_resume_query(CHILD)
            .is_err()
    );
    for _ in 0..6 {
        assert!(query.read(&handle, CHILD).await.is_err());
    }
    assert_eq!(
        query.read(&handle, CHILD).await.unwrap(),
        SavedResumeObservation::Pending
    );
    assert_eq!(
        query.read(&handle, CHILD).await.unwrap(),
        SavedResumeObservation::Unavailable
    );
    for data in [None, Some(Value::Null), Some(json!({"original":[1,2]}))] {
        assert_eq!(
            query.read(&handle, CHILD).await.unwrap(),
            SavedResumeObservation::Rejected(JSONRPCErrorError {
                code: -32042,
                message: "original rejected".into(),
                data
            })
        );
    }
    assert_eq!(
        query.read(&handle, CHILD).await.unwrap(),
        SavedResumeObservation::Resumed
    );
    c2.shutdown().await.unwrap();
    peer_done.await.unwrap();
}
