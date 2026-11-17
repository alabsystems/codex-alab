// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual original store/cell controls; no Source/Native receipt is fabricated.
use super::*;
use crate::app::side_companion::SideCompanion;
use codex_utils_absolute_path::AbsolutePathBuf;
fn start(output: &str) -> Result<ServerNotification> {
    Ok(serde_json::from_value(
        serde_json::json!({"method":"item/started","params":{
            "threadId":"actual","turnId":"actual-turn","startedAtMs":1,"item":{"type":"commandExecution","id":"actual-call","command":"printf actual","cwd":"/tmp","processId":null,"source":"agent","status":"inProgress","commandActions":[],"aggregatedOutput":output,"exitCode":null,"durationMs":null}
        }}),
    )?)
}
fn delta() -> ServerNotification {
    ServerNotification::CommandExecutionOutputDelta(
        codex_app_server_protocol::CommandExecutionOutputDeltaNotification {
            thread_id: "actual".into(),
            turn_id: "actual-turn".into(),
            item_id: "actual-call".into(),
            delta: "same".into(),
        },
    )
}
fn rendered(pane: &SideCompanion) -> String {
    pane.display
        .active
        .as_ref()
        .map(|cell| {
            cell.transcript_lines(120)
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}
#[test]
fn actual_identical_output_deliveries_apply_once_each_while_old_prefix_replay_is_idempotent()
-> Result<()> {
    let mut store = ThreadEventStore::new(32);
    let mut pane = SideCompanion::empty(AbsolutePathBuf::from_absolute_path("/tmp")?);
    let initial = start("prefix-")?;
    store.push_notification_ref(&initial);
    let initial_receipt = store
        .alab_side_delivery
        .current(&initial)?
        .ok_or_else(|| eyre!("actual delivery missing"))?;
    pane.alab_apply_command(&initial, &initial_receipt)?;
    let draft = "original pending composer";
    pane.display.draft.set_text_clearing_elements(draft);
    let active = std::ptr::from_ref(
        pane.display
            .active
            .as_ref()
            .ok_or_else(|| eyre!("actual active cell missing"))?
            .as_ref(),
    ) as *const ();
    for _ in 0..2 {
        let next = delta();
        store.push_notification_ref(&next);
        let receipt = store
            .alab_side_delivery
            .current(&next)?
            .ok_or_else(|| eyre!("actual delta missing"))?;
        pane.alab_apply_command(&next, &receipt)?;
    }
    let before = rendered(&pane);
    assert!(before.contains("prefix-samesame"), "{before}");
    let captured = store
        .buffer
        .iter()
        .map(|event| Ok((event.clone(), store.alab_side_delivery.stored(event)?)))
        .collect::<Result<Vec<_>>>()?;
    for (event, receipt) in captured {
        if let (ThreadBufferedEvent::Notification(notification), Some(receipt)) = (event, receipt) {
            pane.alab_apply_command(&notification, &receipt)?;
        }
    }
    assert_eq!(before, rendered(&pane));
    assert_eq!(pane.display.draft.text(), draft);
    assert_eq!(
        active,
        std::ptr::from_ref(
            pane.display
                .active
                .as_ref()
                .ok_or_else(|| eyre!("actual active cell lost"))?
                .as_ref()
        ) as *const ()
    );
    Ok(())
}
#[test]
fn actual_rebase_preserves_delivery_and_different_store_cannot_rebind_existing_output() -> Result<()>
{
    let mut store = ThreadEventStore::new(32);
    let mut pane = SideCompanion::empty(AbsolutePathBuf::from_absolute_path("/tmp")?);
    let initial = start("actual prefix")?;
    store.push_notification_ref(&initial);
    let receipt = store
        .alab_side_delivery
        .current(&initial)?
        .ok_or_else(|| eyre!("actual delivery missing"))?;
    pane.alab_apply_command(&initial, &receipt)?;
    store.attachment_prefix = Some(0);
    store.rebase_buffer_after_session_refresh();
    let rebased = store
        .alab_side_delivery
        .stored(
            store
                .buffer
                .front()
                .ok_or_else(|| eyre!("actual buffer lost"))?,
        )?
        .ok_or_else(|| eyre!("actual receipt lost"))?;
    assert!(Arc::ptr_eq(&receipt, &rebased));
    let mut other = ThreadEventStore::new(32);
    let next = delta();
    other.push_notification_ref(&next);
    let foreign = other
        .alab_side_delivery
        .current(&next)?
        .ok_or_else(|| eyre!("other actual delivery missing"))?;
    assert!(pane.alab_apply_command(&next, &foreign).is_err());
    Ok(())
}
