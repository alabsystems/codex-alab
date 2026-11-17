// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual Cloud channel resources +native BackendSessions/input producer. These
//! local identity/queue controls create no Source/Native cross-exec grant. The
//! channel-only clients spawn no worker; every actual port is dropped locally.
use super::*;
use crate::app::backend_sessions::BackendSessions;
use crate::app::backend_sessions::alab_retained::Captured;
use crate::app::backend_sessions::alab_retained::Data;
use crate::app::backend_sessions::alab_retained::InputData;
use crate::app_server_session::AppServerSession;
use crate::app_server_session::AppServerSessionEvent;
use crate::app_server_session::ThreadParamsMode;
use codex_app_server_client::AppServerClient;
use codex_app_server_client::AppServerEvent;
use codex_app_server_client::CloudClient;
use pretty_assertions::assert_eq;
use std::io;
fn invalid(message: impl std::fmt::Display) -> io::Error {
    io::Error::other(message.to_string())
}
fn actual() -> AppServerSession {
    let (client, commands, events) = CloudClient::channel(4);
    // These are channel-only original resources, with no background worker.
    drop(commands);
    drop(events);
    AppServerSession::new(
        crate::backend_id::BackendId::new(),
        AppServerClient::Cloud(client),
        ThreadParamsMode::Cloud,
    )
}
fn decode(data: &impl serde::Serialize) -> io::Result<Data<InputData, ()>> {
    let bytes = crate::bottom_pane::alab_retained_editors::encode(data, 32 * 1024 * 1024)?;
    crate::bottom_pane::alab_retained_editors::decode(&bytes, 32 * 1024 * 1024, 100_000)
}
#[tokio::test]
async fn all_actual_backend_slots_parked_inputs_and_side_fifo_survive_one_enclosing_graph()
-> io::Result<()> {
    let mut app = make_test_app().await;
    app.chat_widget
        .apply_external_edit("original parked input".into());
    let input = app
        .chat_widget
        .capture_thread_input_state()
        .ok_or_else(|| invalid("actual widget produced no parked input"))?;
    let first = actual();
    let first_id = first.backend_id();
    let second = actual();
    let second_id = second.backend_id();
    let mut backends = BackendSessions::from_initial(first, vec![second]);
    let thread = ThreadId::new();
    let second_slot = backends
        .slot(second_id)
        .ok_or_else(|| invalid("second actual backend missing"))?;
    backends
        .parked_inputs_mut(second_slot)
        .insert(thread, input.clone());
    backends.set_preference_key(first_id, "local");
    backends.set_preference_key(second_id, "cloud.original");
    backends.defer_side_startup(AppServerSessionEvent {
        backend_id: second_id,
        event: AppServerEvent::Disconnected {
            message: "original parked side notification".into(),
        },
    });
    let Captured { data, refs, pins } = backends.alab_capture_all(|_, _| Ok(()))?;
    let first_ref = refs.reference(first_id)?;
    let second_ref = refs.reference(second_id)?;
    let saved = decode(&data)?;
    drop(data);
    let resources = backends.alab_into_original_resources(&pins, |_| ());
    let prepared = match saved.prepare(resources) {
        Ok(prepared) => prepared,
        Err(refused) => return Err(refused.error),
    };
    let (mut restored, mapping, _settings) = prepared.into_parts();
    assert_eq!(mapping.owner(first_ref)?, first_id);
    assert_eq!(mapping.owner(second_ref)?, second_id);
    let slot = restored
        .slot(second_id)
        .ok_or_else(|| invalid("restored second backend missing"))?;
    assert_eq!(restored.parked_inputs_mut(slot).get(&thread), Some(&input));
    assert_eq!(restored.preference_key(slot), Some("cloud.original"));
    // DATA capture of the actual restored collection proves side FIFO retained,
    // without invoking a server request or decoding a connection from labels.
    let again = restored.alab_capture_all(|_, _| Ok(()))?;
    let value = serde_json::to_value(&again.data).map_err(invalid)?;
    assert_eq!(
        value["entries"][1]["side_startup"][0]["payload"],
        "original parked side notification"
    );
    Ok(())
}
#[test]
fn different_actual_client_resource_is_refused_with_every_slot_still_owned() -> io::Result<()> {
    let first = BackendSessions::new(actual());
    let Captured { data, pins, .. } = first.alab_capture_all(|_, _| Ok(()))?;
    let saved = decode(&data)?;
    drop(data);
    let other = BackendSessions::new(actual());
    let resources = other.alab_into_original_resources(&pins, |_| ());
    let refused = match saved.prepare(resources) {
        Ok(_) => return Err(invalid("different actual channel was accepted")),
        Err(refused) => refused,
    };
    assert_eq!(refused.resources.len(), 1);
    assert!(
        refused
            .error
            .to_string()
            .contains("actual original or retained resource")
    );
    drop(first);
    Ok(())
}
#[test]
fn actual_postcapture_side_event_refuses_without_dropping_original_queue_resources()
-> io::Result<()> {
    let session = actual();
    let owner = session.backend_id();
    let mut backends = BackendSessions::new(session);
    let Captured { data, pins, .. } = backends.alab_capture_all(|_, _| Ok(()))?;
    let saved = decode(&data)?;
    drop(data);
    backends.defer_side_startup(AppServerSessionEvent {
        backend_id: owner,
        event: AppServerEvent::Lagged { skipped: 7 },
    });
    let resources = backends.alab_into_original_resources(&pins, |_| ());
    let refused = match saved.prepare(resources) {
        Ok(_) => return Err(invalid("postcapture actual event was dropped")),
        Err(refused) => refused,
    };
    assert_eq!(refused.error.kind(), io::ErrorKind::WouldBlock);
    assert_eq!(refused.resources.len(), 1);
    Ok(())
}
#[tokio::test]
async fn parked_input_native_options_are_required_and_unknown_event_kinds_refuse() -> io::Result<()>
{
    let mut app = make_test_app().await;
    app.chat_widget
        .apply_external_edit("required field original input".into());
    let input = app
        .chat_widget
        .capture_thread_input_state()
        .ok_or_else(|| invalid("actual widget produced no input"))?;
    let mut value = serde_json::to_value(input).map_err(invalid)?;
    value
        .as_object_mut()
        .ok_or_else(|| invalid("native input did not serialize to object"))?
        .remove("initial_user_message");
    assert!(serde_json::from_value::<crate::chatwidget::ThreadInputState>(value).is_err());
    let event: crate::app::backend_sessions::alab_retained::EventData =
        serde_json::from_value(serde_json::json!({"kind":255,"payload":null})).map_err(invalid)?;
    assert!(event.validate_data().is_err());
    Ok(())
}

#[tokio::test]
async fn changed_omitted_or_reordered_input_data_refuses_and_exact_scene_can_retry_same_resources()
-> io::Result<()> {
    let mut app = make_test_app().await;
    app.chat_widget
        .apply_external_edit("actual immutable parked input".into());
    let input = app
        .chat_widget
        .capture_thread_input_state()
        .ok_or_else(|| invalid("actual widget produced no input"))?;
    for change in 0..3 {
        let session = actual();
        let owner = session.backend_id();
        let mut backends = BackendSessions::new(session);
        let slot = backends
            .slot(owner)
            .ok_or_else(|| invalid("actual original slot missing"))?;
        backends
            .parked_inputs_mut(slot)
            .insert(ThreadId::new(), input.clone());
        backends
            .parked_inputs_mut(slot)
            .insert(ThreadId::new(), input.clone());
        let Captured { data, pins, .. } = backends.alab_capture_all(|_, _| Ok(()))?;
        let clean = decode(&data)?;
        let mut changed = serde_json::to_value(&data).map_err(invalid)?;
        match change {
            0 => {
                changed["entries"][0]["parked_inputs"][0][1]["native"]["composer"]["text"] =
                    serde_json::Value::String("different DTO input".into())
            }
            1 => changed["entries"][0]["parked_inputs"] = serde_json::json!([]),
            _ => changed["entries"][0]["parked_inputs"]
                .as_array_mut()
                .ok_or_else(|| invalid("parked input DATA absent"))?
                .reverse(),
        }
        let changed = decode(&changed)?;
        let resources = backends.alab_into_original_resources(&pins, |_| ());
        let refused = match changed.prepare(resources) {
            Ok(_) => {
                return Err(invalid(
                    "changed/omitted/reordered input DATA was silently accepted",
                ));
            }
            Err(refused) => refused,
        };
        assert_eq!(refused.resources.len(), 1);
        assert_eq!(refused.error.kind(), io::ErrorKind::WouldBlock);
        let restored = match clean.prepare(refused.resources) {
            Ok(prepared) => prepared,
            Err(refused) => return Err(refused.error),
        };
        let (mut restored, _, _) = restored.into_parts();
        let slot = restored
            .slot(owner)
            .ok_or_else(|| invalid("actual retained slot lost"))?;
        assert_eq!(restored.parked_inputs_mut(slot).len(), 2);
    }
    Ok(())
}
