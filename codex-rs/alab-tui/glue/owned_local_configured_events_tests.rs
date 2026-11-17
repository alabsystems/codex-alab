// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Original client event FIFO only. No configured receipt, Native or UI grant.
use super::*;

#[tokio::test]
async fn physically_received_deferred_events_precede_new_events_on_the_same_session() {
    let (client, _commands, events) = codex_app_server_client::CloudClient::channel(1);
    let backend = crate::backend_id::BackendId::new();
    let mut session = AppServerSession::new(
        backend,
        AppServerClient::Cloud(client),
        ThreadParamsMode::Cloud,
    );
    for text in ["first", "second"] {
        events
            .send(AppServerEvent::Disconnected {
                message: text.into(),
            })
            .unwrap();
        // The real physical/state path has already consumed this event once.
        // Required-tail waiting retains it before the next physical receive.
        let event = session.alab_next_physical_event().await.unwrap();
        session.alab_configured_events.push_back(event);
    }
    events
        .send(AppServerEvent::Disconnected {
            message: "third".into(),
        })
        .unwrap();
    for expected in ["first", "second", "third"] {
        let event = session.next_event().await.unwrap();
        assert_eq!(event.backend_id, backend);
        let AppServerEvent::Disconnected { message } = event.event else {
            panic!("original session event kind changed");
        };
        assert_eq!(message, expected);
    }
    assert!(session.alab_configured_events.is_empty());
}
