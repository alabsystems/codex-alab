// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Wire/checkpoint ownership controls only; no Native acceptance is fabricated.
use super::*;
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
use serde_json::json;
fn selection() -> ClientConstructionSelection {
    ClientConstructionSelection {
        prospective_create: codex_app_server_protocol::ThreadStartParams {
            model: Some("original-model".into()),
            ..Default::default()
        },
        presentation: ClientPresentation::Primary,
        prepared: None,
    }
}
fn wire(model: serde_json::Value) -> Result<codex_app_server_protocol::JSONRPCRequest> {
    Ok(serde_json::from_value(
        json!({"id":1,"method":"thread/resume","params":{
            "threadId":"original-target","model":model,"cwd":"/original",
            "_meta":{"foreign.policy":{"kept":[1,2,3]}}
        }}),
    )?)
}
#[test]
fn checkpoint_freezes_full_original_and_prospective_policy_without_defaulting_errors() -> Result<()>
{
    let operation = ClientConstructionOperation::new(selection())?;
    let original = wire(json!(42))?;
    let frozen = operation.freeze(&original)?;
    assert!(frozen.parameters["model"] == 42);
    assert!(
        serde_json::from_value::<ClientRequest>(serde_json::to_value(&original)?).is_err(),
        "preparation must not turn invalid original policy into defaults"
    );
    let restored = ClientConstructionOperation::restore(serde_json::from_value(
        serde_json::to_value(operation.checkpoint())?,
    )?)?;
    assert!(restored.freeze(&original)? == frozen);
    assert!(restored.freeze(&wire(json!("changed-model"))?).is_err());
    let mut changed = original;
    changed.params.as_mut().ok_or("params missing")?["_meta"]["foreign.policy"]["kept"] =
        json!([3, 2, 1]);
    assert!(
        restored.freeze(&changed).is_err(),
        "foreign full arguments were not frozen"
    );
    Ok(())
}
#[test]
fn checkpoint_rejects_unknown_schema_lost_offer_and_mismatched_completion() -> Result<()> {
    let operation = ClientConstructionOperation::new(selection())?;
    operation.freeze(&wire(json!("original-model"))?)?;
    let original = serde_json::to_value(operation.checkpoint())?;
    let mut wrong_schema = original.clone();
    wrong_schema["schema"] = json!(3);
    assert!(ClientConstructionOperation::restore(serde_json::from_value(wrong_schema)?).is_err());
    let mut lost_offer = original.clone();
    lost_offer["mutationAttempted"] = json!(true);
    assert!(ClientConstructionOperation::restore(serde_json::from_value(lost_offer)?).is_err());
    let mut changed_policy = original.clone();
    changed_policy["frozen"]["selection"]["prospectiveCreate"]["model"] =
        json!("changed-prospective");
    assert!(ClientConstructionOperation::restore(serde_json::from_value(changed_policy)?).is_err());
    let mut foreign = original;
    foreign["authority"] = json!(true);
    assert!(serde_json::from_value::<ClientConstructionCheckpoint>(foreign).is_err());
    Ok(())
}
#[tokio::test]
async fn canceling_original_observation_retains_same_frozen_operation_owner() -> Result<()> {
    let operation = ClientConstructionOperation::new(selection())?;
    let original = wire(json!("original-model"))?;
    let expected = operation.freeze(&original)?;
    let owned = operation.clone();
    let (entered, observing) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        owned
            .run(async move {
                let _ = entered.send(());
                std::future::pending::<()>().await;
            })
            .await;
    });
    let observed = observing.await;
    task.abort();
    let stopped = task.await;
    observed?;
    assert!(stopped.is_err_and(|error| error.is_cancelled()));
    assert!(operation.freeze(&original)? == expected);
    let changed = wire(json!("changed"))?;
    assert!(operation.freeze(&changed).is_err());
    // No SDK/channel/Core was constructed; this proves operation DATA custody,
    // not acceptance, Native actor retirement or cross-process UI restoration.
    Ok(())
}

fn start_wire() -> Result<codex_app_server_protocol::JSONRPCRequest> {
    Ok(serde_json::from_value(
        json!({"id":11,"method":"thread/start","params":{
            "model":"original-model","modelProvider":"original-provider",
            "cwd":"/original","ephemeral":true,"historyMode":"paginated",
            "approvalPolicy":"never","config":{"model_reasoning_effort":"high"},
            "_meta":{"foreign.policy":{"kept":[1,2,3]}}
        }}),
    )?)
}
#[test]
fn start_intent_checkpoint_has_no_placeholder_and_derives_original_full_typed_policy_once()
-> Result<()> {
    let operation = ClientConstructionOperation::for_start();
    let pending = serde_json::to_value(operation.checkpoint())?;
    assert!(pending["intent"] == json!({"kind":"start"}));
    assert!(pending["frozen"].is_null());
    assert!(pending.get("selection").is_none());
    let restored = ClientConstructionOperation::restore(serde_json::from_value(pending)?)?;
    let original = start_wire()?;
    let parameters = original
        .params
        .as_ref()
        .ok_or("original parameters missing")?;
    let actual: codex_app_server_protocol::ThreadStartParams =
        serde_json::from_value(parameters.clone())?;
    let frozen = restored.freeze(&original)?;
    assert!(frozen.method == Method::Start);
    assert!(frozen.parameters == *parameters);
    assert!(frozen.selection.prospective_create == actual);
    assert!(frozen.selection.presentation == ClientPresentation::Primary);
    assert!(frozen.selection.prepared.is_none());
    let restored = ClientConstructionOperation::restore(serde_json::from_value(
        serde_json::to_value(restored.checkpoint())?,
    )?)?;
    assert!(restored.freeze(&original)? == frozen);
    let mut changed = original.clone();
    changed.params.as_mut().ok_or("params missing")?["cwd"] = json!("/changed");
    assert!(restored.freeze(&changed).is_err());
    let mut foreign = original;
    foreign.params.as_mut().ok_or("params missing")?["_meta"]["foreign.policy"]["kept"] =
        json!([3, 2, 1]);
    assert!(restored.freeze(&foreign).is_err());
    assert!(restored.freeze(&wire(json!("original-model"))?).is_err());
    Ok(())
}
#[test]
fn start_intent_and_projected_intent_cannot_change_method_or_restore_invented_policy() -> Result<()>
{
    let start = start_wire()?;
    let projected = ClientConstructionOperation::new(selection())?;
    assert!(projected.freeze(&start).is_err());
    let operation = ClientConstructionOperation::for_start();
    assert!(operation.freeze(&wire(json!("original-model"))?).is_err());
    operation.freeze(&start)?;
    let saved = serde_json::to_value(operation.checkpoint())?;
    let mut changed = saved.clone();
    changed["frozen"]["selection"]["prospectiveCreate"]["model"] = json!("invented-model");
    assert!(ClientConstructionOperation::restore(serde_json::from_value(changed)?).is_err());
    let mut side = saved.clone();
    side["frozen"]["selection"]["presentation"] = json!("sideConversation");
    assert!(ClientConstructionOperation::restore(serde_json::from_value(side)?).is_err());
    let mut method = saved;
    method["frozen"]["method"] = json!("thread/resume");
    assert!(ClientConstructionOperation::restore(serde_json::from_value(method)?).is_err());
    Ok(())
}
#[tokio::test]
async fn start_owner_survives_canceled_original_await_before_projection_exists() -> Result<()> {
    let operation = ClientConstructionOperation::for_start();
    let owned = operation.clone();
    let (entered, observing) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        owned
            .run(async move {
                let _ = entered.send(());
                std::future::pending::<()>().await;
            })
            .await;
    });
    let entered = observing.await;
    task.abort();
    let stopped = task.await;
    entered?;
    assert!(stopped.is_err_and(|error| error.is_cancelled()));
    let checkpoint = operation.checkpoint();
    assert!(checkpoint.frozen.is_none());
    assert!(checkpoint.offer.is_none());
    let restored = ClientConstructionOperation::restore(checkpoint)?;
    let original = start_wire()?;
    let expected = restored.freeze(&original)?;
    assert!(operation.freeze(&original)? == expected);
    Ok(())
}

#[test]
fn start_preflight_requires_same_actual_typed_projection_and_primary_presentation() -> Result<()> {
    let operation = ClientConstructionOperation::for_start();
    let original = start_wire()?;
    let frozen = operation.freeze(&original)?;
    assert!(frozen.valid());
    let mut side = frozen.clone();
    side.selection.presentation = ClientPresentation::SideConversation;
    assert!(!side.valid());
    let mut invented = frozen.clone();
    invented.selection.prospective_create.model = Some("invented".into());
    assert!(!invented.valid());
    let mut malformed = frozen;
    malformed.parameters["model"] = json!(42);
    assert!(!malformed.valid());
    assert!(operation.freeze(&original)?.valid());
    Ok(())
}
