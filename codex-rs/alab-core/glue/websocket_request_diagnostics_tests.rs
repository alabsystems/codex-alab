// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Select request failures after cancellation cleanup has preserved any completion.

use super::*;
use crate::common::ResponseCreateWsRequest;
use crate::common::ResponsesApiRequest;
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Mutex as StdMutex;

#[derive(Clone, Default)]
struct Events(Arc<StdMutex<Vec<BTreeMap<String, String>>>>);

#[derive(Default)]
struct Fields(BTreeMap<String, String>);

impl tracing::field::Visit for Fields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0.insert(field.name().into(), format!("{value:?}"));
    }
}

impl tracing::Subscriber for Events {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        if fields
            .0
            .get("message")
            .is_some_and(|message| message.starts_with("Responses request"))
        {
            self.0.lock().unwrap().push(fields.0);
        }
    }
}

impl Events {
    fn take_request(&self, failure: bool) {
        let mut records = self.0.lock().unwrap();
        assert_eq!(records.len(), if failure { 2 } else { 1 }, "{records:?}");
        let shape = &records[0];
        assert_eq!(shape["message"], "Responses request shape");
        assert_eq!(shape["transport"], "\"responses_websocket\"");
        assert_eq!(shape["input_items"], "1");
        if failure {
            let error = &records[1];
            assert_eq!(error["message"], "Responses request failed");
            assert_eq!(shape["local_attempt"], error["local_attempt"]);
            assert_eq!(error["failure_kind"], "\"stream\"");
        }
        let text = format!("{records:?}");
        assert!(!text.contains("never-log-request-content"), "{text}");
        records.clear();
    }
}

fn api_request() -> ResponsesApiRequest {
    ResponsesApiRequest {
        model: "gpt-fixture".into(),
        stream: true,
        service_tier: None,
        input: vec![
            serde_json::from_value(json!({
                "type":"message", "role":"user",
                "content":[{"type":"input_text", "text":"never-log-request-content"}]
            }))
            .unwrap(),
        ],
        tools: None,
        tool_choice: "auto".into(),
        parallel_tool_calls: true,
        reasoning: None,
        store: false,
        stream_options: None,
        include: vec![],
        prompt_cache_key: None,
        prompt_cache_options: None,
        text: None,
        client_metadata: None,
        access_programs: None,
    }
}

async fn request_stream(connection: &ResponsesWebsocketConnection) -> ResponseStream {
    let request = api_request();
    connection
        .stream_request(
            ResponsesWsRequest::ResponseCreate(ResponseCreateWsRequest::from(&request)),
            false,
            None,
        )
        .await
        .unwrap()
}

fn empty_connection() -> ResponsesWebsocketConnection {
    ResponsesWebsocketConnection {
        routing_affinity: None,
        stream: Arc::new(Mutex::new(None)),
        idle_timeout: Duration::from_secs(60),
        server_reasoning_included: false,
        server_model: None,
        telemetry: None,
    }
}

async fn request_task_finished(connection: &ResponsesWebsocketConnection) {
    tokio::time::timeout(Duration::from_secs(1), async {
        while Arc::strong_count(&connection.stream) > 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("request task releases its connection ownership");
}

async fn acknowledge_command(commands: &mut mpsc::Receiver<WsCommand>, expected: &str) {
    let Some(WsCommand::Send { message, tx_result }) = commands.recv().await else {
        panic!("expected outgoing {expected}");
    };
    let Message::Text(text) = message else {
        panic!("expected a text request");
    };
    let payload: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(payload["type"], expected);
    assert!(tx_result.send(Ok(())).is_ok());
}

async fn completion_survives_failed_cancel_ack(events: &Events) {
    let (tx_command, mut rx_command) = mpsc::channel(2);
    let (tx_message, rx_message) = mpsc::unbounded_channel();
    let pump_task = tokio::spawn(async move {
        acknowledge_command(&mut rx_command, "response.create").await;
        tx_message
            .send(Ok(Message::Text(
                json!({
                    "type":"response.created", "response":{"id":"resp-cancel"}
                })
                .to_string()
                .into(),
            )))
            .unwrap();
        acknowledge_command(&mut rx_command, "response.interrupt").await;
        // The real parser receives completion before the failed cancellation ACK.
        // Cancellation cleanup must preserve that completion and its token usage.
        tx_message
            .send(Ok(Message::Text(
                json!({
                    "type":"response.completed", "response":{
                        "id":"resp-cancel",
                        "usage":{"input_tokens":3,"output_tokens":2,"total_tokens":5}
                    }
                })
                .to_string()
                .into(),
            )))
            .unwrap();
        tx_message
            .send(Ok(Message::Text(
                json!({
                    "type":"response.interrupt.failed", "response_id":"resp-cancel"
                })
                .to_string()
                .into(),
            )))
            .unwrap();
    });
    let socket = WsStream {
        max_message_bytes: None,
        routing_affinity: None,
        tx_command,
        rx_message,
        pump_task,
    };
    let connection =
        ResponsesWebsocketConnection::new(socket, Duration::from_secs(60), false, None, None);
    let mut stream = request_stream(&connection).await;
    assert!(
        matches!(stream.next().await, Some(Ok(ResponseEvent::Created { response_id: Some(id) })) if id == "resp-cancel")
    );
    assert!(
        stream
            .interrupt
            .take()
            .unwrap()
            .send(ResponseInterrupt::Cancel {
                deadline: Instant::now() + Duration::from_secs(60),
            })
            .is_ok()
    );
    match stream.next().await {
        Some(Ok(ResponseEvent::Completed {
            response_id,
            token_usage: Some(usage),
            ..
        })) => {
            assert_eq!(response_id, "resp-cancel");
            assert_eq!(
                (usage.input_tokens, usage.output_tokens, usage.total_tokens),
                (3, 2, 5)
            );
        }
        event => panic!("cleanup must preserve completion: {event:?}"),
    }
    assert!(stream.next().await.is_none());
    request_task_finished(&connection).await;
    assert!(
        connection.stream.lock().await.is_none(),
        "failed cancellation retires the socket"
    );
    events.take_request(false);
}

async fn closed_socket_selects_error(events: &Events) {
    let connection = empty_connection();
    let mut stream = request_stream(&connection).await;
    assert!(
        matches!(stream.next().await, Some(Err(ApiError::Stream(message))) if message == "websocket connection is closed")
    );
    assert!(stream.next().await.is_none());
    request_task_finished(&connection).await;
    events.take_request(true);
}

async fn queued_cancellation_deadline_selects_error(events: &Events) {
    let connection = empty_connection();
    let lock = Arc::clone(&connection.stream);
    let (ready, ready_rx) = oneshot::channel();
    let (release, release_rx) = oneshot::channel();
    let blocker = tokio::spawn(
        #[expect(
            clippy::await_holding_invalid_type,
            reason = "this fixture intentionally holds the socket mutex until the queued request selects its cancellation deadline"
        )]
        async move {
            let _guard = lock.lock().await;
            ready.send(()).unwrap();
            release_rx.await.unwrap();
        },
    );
    ready_rx.await.unwrap();
    let mut stream = request_stream(&connection).await;
    assert!(
        stream
            .interrupt
            .take()
            .unwrap()
            .send(ResponseInterrupt::Cancel {
                deadline: Instant::now(),
            })
            .is_ok()
    );
    assert!(
        matches!(stream.next().await, Some(Err(ApiError::Stream(message))) if message == "timed out draining interrupted response")
    );
    assert!(stream.next().await.is_none());
    release.send(()).unwrap();
    blocker.await.unwrap();
    request_task_finished(&connection).await;
    events.take_request(true);
}

async fn dropped_consumer_selects_no_error(events: &Events) {
    let connection = empty_connection();
    let stream = request_stream(&connection).await;
    // Drop before the spawned request runs. The closed receiver branch has
    // priority even though a closed socket would otherwise select an error.
    drop(stream);
    request_task_finished(&connection).await;
    events.take_request(false);
}

#[tokio::test(flavor = "current_thread")]
async fn websocket_diagnostics_follow_the_selected_terminal_result() {
    const CHILD: &str = "ALAB_WS_TERMINAL_DIAGNOSTIC_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let name = format!(
            "{}::websocket_diagnostics_follow_the_selected_terminal_result",
            module_path!().split_once("::").unwrap().1
        );
        let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", &name, "--nocapture"])
            .env(CHILD, "1");
        crate::test_child_process::run(command, std::time::Duration::from_secs(60))
            .await
            .unwrap_or_else(|failure| panic!("{name}: {failure}"))
            .assert_test_success(&name);
        return;
    }
    codex_alab_core::activate();
    let events = Events::default();
    tracing::subscriber::set_global_default(events.clone()).unwrap();
    completion_survives_failed_cancel_ack(&events).await;
    closed_socket_selects_error(&events).await;
    queued_cancellation_deadline_selects_error(&events).await;
    dropped_consumer_selects_no_error(&events).await;
}
