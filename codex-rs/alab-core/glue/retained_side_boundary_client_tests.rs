// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Lower immutable wire/checkpoint DATA only, never a Native registration.
use super::*;
use serde_json::json;
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
pub(super) fn operation() -> Result<ClientConstructionOperation> {
    let actual = ClientConstructionOperation::new(ClientConstructionSelection {
        prospective_create: codex_app_server_protocol::ThreadStartParams::default(),
        presentation: ClientPresentation::SideConversation,
        prepared: None,
    })?;
    let wire = serde_json::from_value(
        json!({"id":1,"method":"thread/fork","params":{"threadId":"original-parent","excludeTurns":true}}),
    )?;
    let frozen = actual.freeze(&wire)?;
    let offer = Offer {
        preparation: frozen.preparation,
        claim: "1".repeat(32),
        source_operation: "2".repeat(32),
        selection: "3".repeat(32),
    };
    {
        let mut p = actual.0.lock().unwrap_or_else(PoisonError::into_inner);
        p.offer = Some(offer.clone());
        p.mutation_attempted = true;
        p.result = Some(ClientConstructionResult {
            prepared: offer,
            thread_id: "original-child".into(),
            history_fallback: false,
            dynamic_tools_fallback: false,
            backend_task_tools: false,
            primary_candidate: None,
        });
    }
    Ok(actual)
}
fn wire() -> Result<codex_app_server_protocol::JSONRPCRequest> {
    Ok(serde_json::from_value(
        json!({"id":2,"method":"thread/inject_items","params":{
            "threadId":"original-child","items":[{"type":"message","role":"user","content":[{"type":"input_text","text":"exact original items"}]}],"_meta":{"foreign":{"kept":[1,2]}}
        }}),
    )?)
}
#[test]
fn exact_side_wire_freezes_before_send_and_checkpoint_replay_cannot_change_items_metadata_or_thread()
-> Result {
    let actual = operation()?;
    let original = wire()?;
    actual.freeze_side(&original)?;
    let restored = ClientConstructionOperation::restore(serde_json::from_value(
        serde_json::to_value(actual.checkpoint())?,
    )?)?;
    restored.freeze_side(&original)?;
    for field in ["threadId", "items", "_meta"] {
        let mut changed = original.clone();
        changed.params.as_mut().ok_or("params missing")?[field] = json!("changed");
        assert!(restored.freeze_side(&changed).is_err());
    }
    let complete = side_boundary::side_metadata::Completion {
        schema: 1,
        claim: "1".repeat(32),
        thread_id: "original-child".into(),
        occurrence: "4".repeat(32),
    };
    restored.record_side(complete.clone())?;
    restored.record_side(complete)?;
    let mut wrong = side_boundary::side_metadata::Completion {
        schema: 1,
        claim: "1".repeat(32),
        thread_id: "original-child".into(),
        occurrence: "5".repeat(32),
    };
    assert!(restored.record_side(wrong.clone()).is_err());
    wrong.occurrence = "4".repeat(32);
    wrong.thread_id = "changed".into();
    assert!(restored.record_side(wrong).is_err());
    assert_eq!(
        restored.side_boundary_observation_data(),
        Some(("original-child".into(), "4".repeat(32)))
    );
    Ok(())
}
#[test]
fn pre_fork_and_primary_intents_cannot_freeze_a_side_injection() -> Result {
    let start = ClientConstructionOperation::for_start();
    assert!(start.freeze_side(&wire()?).is_err());
    let actual = operation()?;
    {
        let mut p = actual.0.lock().unwrap_or_else(PoisonError::into_inner);
        if let ConstructionIntent::Projected { selection } = &mut p.intent {
            selection.presentation = ClientPresentation::Primary;
        }
    }
    assert!(actual.freeze_side(&wire()?).is_err());
    Ok(())
}
