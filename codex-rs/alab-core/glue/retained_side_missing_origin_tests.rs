// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Real registered transport/processor/Core denial. No fake AcceptedOrigin.
use super::*;
use wiremock::MockServer;
#[tokio::test]
async fn actual_side_injection_without_native_a_cannot_mutate_or_poison_original_core() -> Result<()>
{
    let model = MockServer::start().await;
    let mut h = Harness::new(&model.uri()).await?;
    let plan = crate::in_process::InitialSessionPlan::new(
        crate::in_process::InitialSessionKind::Selection,
        None,
    );
    let (initial, installed) = h.processor.alab_make_initial_session(plan, h.input.clone());
    let result:Result<()>=async {
        installed?;
        let actual=h.host.control.manager.get_thread(h.parent_id).await?;
        // history_version is the replacement/reset epoch, not an append counter.
        let before=actual.conversation_history_snapshot().await;
        let before_items=before.items().cloned().collect::<Vec<_>>();
        let c1=h.c1.clone();
        let mut injected:Vec<codex_protocol::models::ResponseItem>=serde_json::from_value(json!([{"type":"message","role":"user","content":[{"type":"input_text","text":"original real injection"}]}]))?;
        // ORIGINAL media preparation also canonicalizes positional text kinds.
        // Supply that unknown classification plus identity/turn/time through the
        // existing protocol fields so the full typed append oracle is stable.
        for (index,item) in injected.iter_mut().enumerate() {
            item.set_id(Some(codex_protocol::ResponseItemId::with_suffix("msg",format!("retained-side-denial-{index}"))));
            item.set_turn_id_if_missing("original-injection-turn");
            item.set_create_time_if_missing(serde_json::Number::from(1));
            if let codex_protocol::models::ResponseItem::Message {content,internal_chat_message_metadata_passthrough,..}=item {
                // This is the exact absent-kind normalization in the ORIGINAL
                // to_annotated_content -> set_annotated_content media path.
                internal_chat_message_metadata_passthrough.get_or_insert_default().content_item_kinds=Some(content.iter().map(|_|codex_protocol::models::ContentItemKind("unknown".into())).collect());
            }
        }
        let items=serde_json::to_value(&injected)?;
        let denied=h.request::<serde_json::Value>(ConnectionId(41),&c1,json!({"id":9111,"method":"thread/inject_items","params":{
            "threadId":h.parent_id.to_string(),"items":items,"_meta":{"alab.sideInjection":{"schema":1,"claim":"11111111111111111111111111111111"}}
        }})).await;
        ensure!(denied.is_err());
        let unchanged=actual.conversation_history_snapshot().await;
        ensure!(unchanged.history_version()==before.history_version());
        let unchanged_items=unchanged.items().cloned().collect::<Vec<_>>();
        ensure!(unchanged_items==before_items,"denied selector changed actual original history: before_len={} after_len={} before_shape={:?} after_shape={:?}",before_items.len(),unchanged_items.len(),history_shape(&before_items),history_shape(&unchanged_items));
        ensure!(Arc::ptr_eq(&actual,&h.host.control.manager.get_thread(h.parent_id).await?));ensure!(!h.input.is_stopped());
        let _:codex_app_server_protocol::ThreadInjectItemsResponse=h.request(ConnectionId(41),&c1,json!({"id":9112,"method":"thread/inject_items","params":{"threadId":h.parent_id.to_string(),"items":items}})).await?;
        let after=actual.conversation_history_snapshot().await.items().cloned().collect::<Vec<_>>();
        ensure!(after.starts_with(&before_items),"ordinary injection rewrote prior actual history");
        ensure!(Arc::ptr_eq(&actual,&h.host.control.manager.get_thread(h.parent_id).await?),"ordinary RPC changed actual Core incarnation");
        // Diagnostics serialize ONLY this local fixture's supplied IDs, never
        // earlier private auto-compact/context messages in the real Core history.
        let actual_fixture=after.iter().filter(|item|injected.iter().any(|expected|item.id()==expected.id())).collect::<Vec<_>>();
        ensure!(after.len()>=before_items.len()+injected.len()&&after.ends_with(&injected),"ordinary original RPC did not append its exact supplied items: before_len={} after_len={} expected_shape={:?} after_shape={:?} expected_fixture_json={} actual_fixture_json={}",before_items.len(),after.len(),history_shape(&injected),history_shape(&after),serde_json::to_value(&injected)?,serde_json::to_value(actual_fixture)?);
        Ok(())
    }.await;
    let drained = initial.drain().await.map_err(|e| anyhow::anyhow!(e));
    let cleanup = h.shutdown().await;
    result.and(drained).and(cleanup)
}

#[tokio::test]
async fn actual_side_close_and_rollback_selectors_without_native_a_keep_original_core_and_subscription()
-> Result<()> {
    let model = MockServer::start().await;
    let mut h = Harness::new_persistent(&model.uri()).await?;
    let plan = crate::in_process::InitialSessionPlan::new(
        crate::in_process::InitialSessionKind::Selection,
        None,
    );
    let (initial, installed) = h.processor.alab_make_initial_session(plan, h.input.clone());
    let result:Result<()>=async {
        installed?;
        let actual=h.host.control.manager.get_thread(h.parent_id).await?;
        let c1=h.c1.clone();
        for (id, phase) in [(9126, "readResult"), (9127, "acknowledgeResult")] {
            let thread = h.parent_id.to_string();
            let query = json!({"id":id,"method":"config/read","params":{"includeLayers":false,"_meta":{
                "alab.sideView":{"phase":phase,"view":{"schema":1,"claim":"a".repeat(32),"threadId":thread},
                    "operation":"b".repeat(32),"parameters":{"threadId":thread}}
            }}});
            let error = h.request::<serde_json::Value>(ConnectionId(41), &c1, query).await.unwrap_err();
            ensure!(error.to_string().ends_with(&format!("code {}", crate::error_code::INVALID_REQUEST_ERROR_CODE)), "{error:#}");
        }
        for (id,method,key,selection) in [
            (9121,"thread/unsubscribe","alab.sideView",json!({"phase":"close","view":{"schema":1,"claim":"a".repeat(32),"threadId":h.parent_id.to_string()},"operation":"b".repeat(32)})),
            (9122,"thread/delete","alab.sideRollback",json!({"phase":"rollback","schema":1,"claim":"a".repeat(32),"operation":"c".repeat(32)})),
        ] {
            let mut params=json!({"threadId":h.parent_id.to_string(),"_meta":{}});params["_meta"][key]=selection;
            ensure!(h.request::<serde_json::Value>(ConnectionId(41),&c1,json!({"id":id,"method":method,"params":params})).await.is_err());
            ensure!(Arc::ptr_eq(&actual,&h.host.control.manager.get_thread(h.parent_id).await?));
            ensure!(!actual.has_shutdown_status()&&!h.input.is_stopped());
        }
        let reply:codex_app_server_protocol::ThreadUnsubscribeResponse=h.request(ConnectionId(41),&c1,json!({"id":9123,"method":"thread/unsubscribe","params":{"threadId":h.parent_id.to_string()}})).await?;
        ensure!(reply.status==codex_app_server_protocol::ThreadUnsubscribeStatus::Unsubscribed,"denied selector changed original subscriber state");
        Ok(())
    }.await;
    let drained = initial
        .drain()
        .await
        .map_err(|error| anyhow::anyhow!(error));
    let cleanup = h.shutdown().await;
    result.and(drained).and(cleanup)
}

// Diagnostic metadata only: never print private message/tool content on failure.
fn history_shape(
    items: &[codex_protocol::models::ResponseItem],
) -> Vec<(&'static str, Option<String>, Option<String>)> {
    items
        .iter()
        .map(|item| {
            (
                item.item_type(),
                item.id().map(|id| id.as_str().to_owned()),
                item.turn_id().map(str::to_owned),
            )
        })
        .collect()
}
