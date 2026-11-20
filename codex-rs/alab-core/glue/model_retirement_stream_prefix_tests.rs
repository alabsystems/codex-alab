// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(super) async fn exercise() {
    let created = json!({"type":"response.created","response":{"id":"one"}}).to_string();
    let failed = |id: &str, code: &str| {
        json!({
            "type":"response.failed","response":{"id":id,"error":{"code":code}}
        })
    };
    let retired = failed("one", "model_not_found");
    let unknown = json!({"type":"future.response.work"}).to_string();
    let delta = json!({"type":"response.output_text.delta","delta":"already shown"}).to_string();
    let mut embedded = retired.clone();
    embedded["response"]["output"] = json!([{"type":"function_call","call_id":"prior"}]);
    let mut billed = retired.clone();
    billed["response"]["usage"] = json!({"output_tokens":5});
    let mut conflicting = retired.clone();
    conflicting["response_id"] = json!("different");
    let mut matching = retired.clone();
    matching["response_id"] = json!("one");
    let mut malformed = retired.clone();
    malformed["response"]["id"] = json!(false);
    let mut addressed_item = retired.clone();
    addressed_item["item_id"] = json!("already-started");
    let mut addressed_call = retired.clone();
    addressed_call["call_id"] = json!("already-started");
    for (case, prefix, refusal, accepted) in [
        ("first", vec![], retired.clone(), Some(true)),
        (
            "created",
            vec![created.clone()],
            retired.clone(),
            Some(true),
        ),
        ("unknown frame", vec![unknown], retired.clone(), Some(false)),
        (
            "malformed frame",
            vec!["{".into()],
            retired.clone(),
            Some(false),
        ),
        (
            "delivered output",
            vec![created.clone(), delta],
            retired.clone(),
            Some(false),
        ),
        ("embedded output", vec![], embedded, Some(false)),
        ("billed response", vec![], billed, Some(false)),
        ("matching response IDs", vec![], matching, Some(true)),
        ("conflicting response IDs", vec![], conflicting, Some(false)),
        ("malformed response ID", vec![], malformed, Some(false)),
        ("addressed item", vec![], addressed_item, Some(false)),
        ("addressed call", vec![], addressed_call, Some(false)),
        (
            "other response",
            vec![created.clone()],
            failed("two", "model_not_found"),
            Some(false),
        ),
        (
            "duplicate creation",
            vec![created.clone(), created],
            retired,
            Some(false),
        ),
        (
            "generic failure",
            vec![],
            failed("one", "resource_not_found"),
            None,
        ),
    ] {
        let frames = prefix
            .into_iter()
            .chain([refusal.to_string()])
            .map(|frame| Ok(Bytes::from(format!("data: {frame}\n\n"))));
        let mut stream = spawn_response_stream(
            StreamResponse {
                status: StatusCode::OK,
                headers: HeaderMap::new(),
                bytes: Box::pin(futures::stream::iter(frames)),
            },
            Duration::from_secs(5),
            None,
            None,
        );
        let error = loop {
            match stream.next().await {
                Some(Ok(_)) => {}
                Some(Err(error)) => break error,
                None => panic!("{case}: no terminal provider error"),
            }
        };
        assert_eq!(error.alab_streamed_model_retirement(), accepted, "{case}");
    }
}
