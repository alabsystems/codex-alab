// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! An HTTPS fallback ends, through real core turns: which request returns to the
//! WebSocket, what it sends, what a failed return costs, and which fallback never ends.
//!
//! This binary shortens the fallback interval to nothing for every session it creates,
//! so its tests are the only ones in it. The interval's arithmetic is tested without a
//! clock in `codex_alab_core::https_fallback`.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;

use codex_protocol::protocol::EventMsg;
use codex_protocol::user_input::UserInput;
use core_test_support::responses;
use core_test_support::test_codex::test_codex;
use pretty_assertions::assert_eq;
use serde_json::json;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use wiremock::Mock;
use wiremock::ResponseTemplate;
use wiremock::http::Method;
use wiremock::matchers::method;
use wiremock::matchers::path_regex;

fn plan_call(response: &str, call: &str) -> Vec<serde_json::Value> {
    vec![
        responses::ev_response_created(response),
        responses::ev_function_call(
            call,
            "update_plan",
            &json!({"plan":[{"step":call,"status":"completed"}]}).to_string(),
        ),
        responses::ev_completed(response),
    ]
}

/// One origin for both fixtures, as upstream's withdrawn recovery tests had (openai/codex
/// #58950): the WebSocket upgrade goes to the WebSocket fixture, everything else to the
/// HTTP one. From the upgrade numbered `refuse_from` on (the first is 0), the origin
/// answers 403 itself, as a middlebox that refuses upgrades does.
async fn serve_both(
    listener: TcpListener,
    http: SocketAddr,
    websocket: SocketAddr,
    upgrades: Arc<AtomicUsize>,
    refuse_from: usize,
) {
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            return;
        };
        let upgrades = Arc::clone(&upgrades);
        tokio::spawn(async move {
            let mut incoming = BufReader::new(stream);
            let mut line = String::new();
            if incoming.read_line(&mut line).await.is_err() {
                return;
            }
            let upstream = if line.starts_with("GET /v1/responses ") {
                if upgrades.fetch_add(1, Ordering::SeqCst) >= refuse_from {
                    let refusal =
                        b"HTTP/1.1 403 Forbidden\r\ncontent-length: 0\r\nconnection: close\r\n\r\n";
                    let _ = incoming.get_mut().write_all(refusal).await;
                    return;
                }
                websocket
            } else {
                http
            };
            let Ok(mut upstream) = TcpStream::connect(upstream).await else {
                return;
            };
            if upstream.write_all(line.as_bytes()).await.is_err() {
                return;
            }
            let _ = tokio::io::copy_bidirectional(&mut incoming, &mut upstream).await;
        });
    }
}

const TURN_STATE: &str = "x-codex-turn-state";

fn answer(response: &str, text: &str) -> Vec<serde_json::Value> {
    vec![
        responses::ev_response_created(response),
        responses::ev_assistant_message("answer", text),
        responses::ev_completed(response),
    ]
}

/// A socket that accepts one request and closes before completing it.
fn dies(response: &str) -> Vec<Vec<serde_json::Value>> {
    vec![vec![responses::ev_response_created(response)]]
}

/// A WebSocket fixture and an HTTP fixture behind one origin, and a session whose stream
/// budget is one retry. The first connection also answers the startup warmup.
struct OneOrigin {
    test: core_test_support::test_codex::TestCodex,
    websocket: responses::WebSocketTestServer,
    /// Upgrades the origin was asked for, refused ones included.
    upgrades: Arc<AtomicUsize>,
    front: tokio::task::JoinHandle<()>,
}

async fn session_behind_one_origin(
    http: &wiremock::MockServer,
    mut connections: Vec<Vec<Vec<serde_json::Value>>>,
    refuse_upgrades_from: usize,
) -> OneOrigin {
    connections[0].insert(
        0,
        vec![
            responses::ev_response_created("warmup"),
            responses::ev_completed("warmup"),
        ],
    );
    let websocket = responses::start_websocket_server(connections).await;
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|err| panic!("bind the shared origin: {err}"));
    let origin = listener
        .local_addr()
        .unwrap_or_else(|err| panic!("the shared origin's address: {err}"));
    let websocket_addr = websocket
        .uri()
        .trim_start_matches("ws://")
        .parse()
        .unwrap_or_else(|err| panic!("the WebSocket fixture's address: {err}"));
    let upgrades = Arc::new(AtomicUsize::new(0));
    let front = tokio::spawn(serve_both(
        listener,
        *http.address(),
        websocket_addr,
        Arc::clone(&upgrades),
        refuse_upgrades_from,
    ));
    let test = test_codex()
        .with_config(move |config| {
            config.update_plan_enabled = true;
            config.model_provider.base_url = Some(format!("http://{origin}/v1"));
            config.model_provider.supports_websockets = true;
            config.model_provider.stream_max_retries = Some(1);
            config.model_provider.request_max_retries = Some(0);
        })
        .build(http)
        .await
        .unwrap_or_else(|err| panic!("build the session: {err:#}"));
    OneOrigin {
        test,
        websocket,
        upgrades,
        front,
    }
}

fn output_call_ids(request: &serde_json::Value) -> Vec<String> {
    request["input"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|item| item["type"] == "function_call_output")
        .filter_map(|item| item["call_id"].as_str().map(str::to_owned))
        .collect()
}

async fn run_turn(test: &core_test_support::test_codex::TestCodex, text: &str) -> EventMsg {
    test.codex
        .start_or_steer_turn(codex_core::TurnInputRequest::user_input(vec![
            UserInput::Text {
                text: text.into(),
                text_elements: Vec::new(),
            },
        ]))
        .await
        .unwrap_or_else(|err| panic!("start the turn: {err:?}"));
    core_test_support::wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await
}

/// Two WebSocket streams die, so upstream's exhausted budget moves the turn to HTTPS.
/// The failed request's retry stays there. The next request of the same turn, which is
/// not a retry, opens a socket and sends what a turn's first request sends: its full
/// input and no routing token. The one after continues on that socket.
#[test]
fn a_fallback_after_failed_requests_ends_at_the_next_request_that_is_not_a_retry() {
    core_test_support::test_codex::run_test_with_large_stack(
        "websocket return after fallback",
        || async move {
            codex_alab_core::activate();
            codex_alab_core::https_fallback::set_base(Duration::ZERO);

            let http = responses::start_mock_server().await;
            let http_requests = responses::mount_response_sequence(
                &http,
                vec![
                    responses::sse_response(responses::sse(plan_call(
                        "replayed-over-https",
                        "https-call",
                    )))
                    .insert_header(TURN_STATE, "issued-over-https"),
                ],
            )
            .await;
            let OneOrigin {
                test,
                websocket,
                front,
                ..
            } = session_behind_one_origin(
                &http,
                vec![
                    dies("failed"),
                    dies("failed-again"),
                    vec![
                        plan_call("returned", "websocket-call"),
                        answer("final", "RETURNED"),
                    ],
                ],
                usize::MAX,
            )
            .await;
            let EventMsg::TurnComplete(completed) = run_turn(&test, "Plan twice.").await else {
                unreachable!()
            };
            assert!(completed.error.is_none(), "{completed:?}");
            assert_eq!(completed.last_agent_message.as_deref(), Some("RETURNED"));

            assert_eq!(
                http_requests.requests().len(),
                1,
                "only the failed request's retry goes over HTTPS"
            );
            let connections = websocket.connections();
            let [_, _, returned] = connections.as_slice() else {
                panic!(
                    "two failed sockets and the returned one, got {}",
                    connections.len()
                );
            };
            let [full, delta] = returned.as_slice() else {
                panic!(
                    "two requests on the returned socket, got {}",
                    returned.len()
                );
            };
            let full = full.body_json();
            assert!(
                full.get("previous_response_id").is_none(),
                "the returning request continues nothing"
            );
            assert!(
                full["client_metadata"].get(TURN_STATE).is_none(),
                "the token HTTPS issued is not replayed on the socket: {}",
                full["client_metadata"]
            );
            assert!(
                full["input"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|item| item["content"][0]["text"] == "Plan twice."),
                "the returning request carries the turn's history"
            );
            assert_eq!(output_call_ids(&full), ["https-call"]);
            let delta = delta.body_json();
            assert_eq!(delta["previous_response_id"], "returned");
            assert_eq!(output_call_ids(&delta), ["websocket-call"]);

            test.codex.shutdown_and_wait().await?;
            front.abort();
            Ok(())
        },
    )
    .expect("websocket return after fallback");
}

/// The request that returns is a trial. Its socket dies, and although the stream budget
/// has a retry left, the request is replayed over HTTPS at once instead of on another
/// socket. With no interval to wait in this binary, the request after it returns again.
#[test]
fn a_returned_socket_that_fails_its_first_request_gets_no_retry_budget() {
    core_test_support::test_codex::run_test_with_large_stack(
        "failed websocket return",
        || async move {
            codex_alab_core::activate();
            codex_alab_core::https_fallback::set_base(Duration::ZERO);

            let http = responses::start_mock_server().await;
            let http_requests = responses::mount_sse_sequence(
                &http,
                vec![
                    responses::sse(plan_call("first-over-https", "first-https-call")),
                    responses::sse(plan_call("second-over-https", "second-https-call")),
                ],
            )
            .await;
            let OneOrigin {
                test,
                websocket,
                front,
                ..
            } = session_behind_one_origin(
                &http,
                vec![
                    dies("failed"),
                    dies("failed-again"),
                    dies("failed-return"),
                    vec![answer("final", "RETURNED AGAIN")],
                ],
                usize::MAX,
            )
            .await;
            let EventMsg::TurnComplete(completed) = run_turn(&test, "Plan twice.").await else {
                unreachable!()
            };
            assert!(completed.error.is_none(), "{completed:?}");
            assert_eq!(
                completed.last_agent_message.as_deref(),
                Some("RETURNED AGAIN")
            );

            let posts = http_requests.requests();
            let [_, replayed] = posts.as_slice() else {
                panic!(
                    "the first fallback's retry and the failed return's, got {}",
                    posts.len()
                );
            };
            assert_eq!(
                output_call_ids(&replayed.body_json()),
                ["first-https-call"],
                "the request the returned socket failed is the one replayed over HTTPS"
            );
            let requests_per_socket = websocket
                .connections()
                .iter()
                .map(Vec::len)
                .collect::<Vec<_>>();
            // The warmup shares the first socket. The failed return had one request and
            // no second socket; the fourth socket is the next return.
            assert_eq!(requests_per_socket, [2, 1, 1, 1]);

            test.codex.shutdown_and_wait().await?;
            front.abort();
            Ok(())
        },
    )
    .expect("failed websocket return");
}

/// The returning upgrade is refused with a status core calls terminal. The refusal is
/// the socket's, not the request's: the request is replayed over HTTPS and the turn
/// completes, as it would have had the session stayed there.
#[test]
fn a_refused_return_is_replayed_over_https_instead_of_ending_the_turn() {
    core_test_support::test_codex::run_test_with_large_stack(
        "refused websocket return",
        || async move {
            codex_alab_core::activate();
            codex_alab_core::https_fallback::set_base(Duration::ZERO);

            let http = responses::start_mock_server().await;
            let http_requests = responses::mount_sse_sequence(
                &http,
                vec![
                    responses::sse(plan_call("over-https", "https-call")),
                    responses::sse(answer("final", "REPLAYED")),
                ],
            )
            .await;
            let OneOrigin {
                test,
                websocket,
                upgrades,
                front,
            } = session_behind_one_origin(
                &http,
                vec![dies("failed"), dies("failed-again")],
                /*refuse_upgrades_from*/ 2,
            )
            .await;
            let EventMsg::TurnComplete(completed) = run_turn(&test, "Plan once.").await else {
                unreachable!()
            };
            assert!(completed.error.is_none(), "{completed:?}");
            assert_eq!(completed.last_agent_message.as_deref(), Some("REPLAYED"));

            let posts = http_requests.requests();
            let [_, replayed] = posts.as_slice() else {
                panic!(
                    "the fallback's retry and the refused return's replay, got {}",
                    posts.len()
                );
            };
            assert_eq!(output_call_ids(&replayed.body_json()), ["https-call"]);
            assert_eq!(
                upgrades.load(Ordering::SeqCst),
                3,
                "the refused upgrade is asked for once"
            );
            assert_eq!(websocket.connections().len(), 2);

            test.codex.shutdown_and_wait().await?;
            front.abort();
            Ok(())
        },
    )
    .expect("refused websocket return");
}

/// HTTP 426 is the server's answer, not a failure of this client's requests: upstream's
/// recovery kept that fallback for the session, and so does this one, however short the
/// interval.
#[test]
fn a_fallback_the_server_asked_for_stays() {
    core_test_support::test_codex::run_test_with_large_stack(
        "upgrade-required fallback stays",
        || async move {
            codex_alab_core::activate();
            codex_alab_core::https_fallback::set_base(Duration::ZERO);

            let server = responses::start_mock_server().await;
            Mock::given(method("GET"))
                .and(path_regex(".*/responses$"))
                .respond_with(ResponseTemplate::new(426))
                .mount(&server)
                .await;
            let posts = responses::mount_sse_sequence(
                &server,
                vec![
                    responses::sse(plan_call("first", "first-call")),
                    responses::sse(plan_call("second", "second-call")),
                    responses::sse(answer("final", "STAYED")),
                    responses::sse(answer("again", "STAYED AGAIN")),
                ],
            )
            .await;
            let test = test_codex()
                .with_config(|config| {
                    config.update_plan_enabled = true;
                    config.model_provider.supports_websockets = true;
                    config.model_provider.stream_max_retries = Some(2);
                    config.model_provider.request_max_retries = Some(0);
                })
                .build(&server)
                .await?;
            for (turn, expected) in [("Plan twice.", "STAYED"), ("Answer again.", "STAYED AGAIN")] {
                let EventMsg::TurnComplete(completed) = run_turn(&test, turn).await else {
                    unreachable!()
                };
                assert!(completed.error.is_none(), "{completed:?}");
                assert_eq!(completed.last_agent_message.as_deref(), Some(expected));
            }
            assert_eq!(
                posts.requests().len(),
                4,
                "three requests, then the second turn"
            );
            let upgrades = server
                .received_requests()
                .await
                .unwrap_or_default()
                .iter()
                .filter(|request| {
                    request.method == Method::GET && request.url.path().ends_with("/responses")
                })
                .count();
            assert_eq!(upgrades, 1, "the refused upgrade is not asked for again");

            test.codex.shutdown_and_wait().await?;
            Ok(())
        },
    )
    .expect("upgrade-required fallback stays");
}
