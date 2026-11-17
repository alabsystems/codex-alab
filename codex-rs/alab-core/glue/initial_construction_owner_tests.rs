// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Closed recovery DATA controls, never positive Native/Core admission.
use super::*;
use serde_json::json;
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
fn owner() -> IoResult<InitialConstructionOperation> {
    InitialConstructionOperation::new(
        InitialSessionSelection {
            claim: "11111111111111111111111111111111".into(),
            prospective_create: None,
        },
        InitialAction::Start,
    )
}
#[test]
fn recovery_preserves_absent_and_explicit_null_original_params_without_guessing() -> Result<()> {
    for had_params in [false, true] {
        let mut data = serde_json::to_value(owner()?.checkpoint())?;
        data["request"] = json!({"method":"thread/start","hadParams":had_params,"params":null});
        let restored =
            InitialConstructionOperation::restore(serde_json::from_value(data.clone())?)?;
        assert!(serde_json::to_value(restored.checkpoint())? == data);
        assert!(restored.prepared_offer().is_none());
        assert!(restored.observed_result().is_none());
    }
    let mut malformed = serde_json::to_value(owner()?.checkpoint())?;
    malformed["request"] =
        json!({"method":"thread/start","hadParams":false,"params":{"changed":true}});
    assert!(InitialConstructionOperation::restore(serde_json::from_value(malformed)?).is_err());
    Ok(())
}
#[test]
fn recovery_refuses_changed_original_action_unknown_schema_and_foreign_fields() -> Result<()> {
    let mut wrong_action = serde_json::to_value(owner()?.checkpoint())?;
    wrong_action["request"] =
        json!({"method":"thread/resume","hadParams":true,"params":{"threadId":"different"}});
    assert!(InitialConstructionOperation::restore(serde_json::from_value(wrong_action)?).is_err());
    let mut wrong_schema = serde_json::to_value(owner()?.checkpoint())?;
    wrong_schema["schema"] = json!(2);
    assert!(InitialConstructionOperation::restore(serde_json::from_value(wrong_schema)?).is_err());
    let mut foreign = serde_json::to_value(owner()?.checkpoint())?;
    foreign["nativeCurrent"] = json!(true);
    assert!(serde_json::from_value::<InitialConstructionCheckpoint>(foreign).is_err());
    Ok(())
}
#[tokio::test]
async fn initial_waiter_loss_keeps_same_explicit_owner_and_choice() -> Result<()> {
    let operation = owner()?;
    let before = serde_json::to_value(operation.checkpoint())?;
    let held = operation.clone();
    let (entered, observing) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        held.run(async move {
            let _ = entered.send(());
            std::future::pending::<()>().await
        })
        .await;
    });
    let observed = observing.await;
    task.abort();
    let stopped = task.await;
    observed?;
    assert!(stopped.is_err_and(|error| error.is_cancelled()));
    assert!(serde_json::to_value(operation.checkpoint())? == before);
    assert!(InitialConstructionOperation::restore(operation.checkpoint()).is_ok());
    // No network/Native constructor/Core was manufactured by this ownership control.
    Ok(())
}
