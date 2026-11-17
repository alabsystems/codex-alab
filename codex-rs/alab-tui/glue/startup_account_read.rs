// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Retry the narrowly classified transient failure in the TUI's read-only account request.
//!
//! App-server starts workspace routing in the background, then the TUI immediately reads the
//! account. Both calls share a discovery semaphore while each one's fifteen-second deadline also
//! counts time waiting for that semaphore. A slow leader can therefore make both deadlines expire
//! together. Bounded fresh reads give discovery independent deadlines, which is what relaunching
//! the TUI did, while every authentication, configuration, and routing-integrity failure remains
//! fatal.

use super::*;

const INTERNAL_ERROR_CODE: i64 = -32603;
const ROUTING_TIMEOUT_MESSAGE: &str = "workspace routing discovery timed out";
const ROUTING_TIMEOUT_REASON: &str = codex_alab_core::workspace_routing::DISCOVERY_TIMEOUT_REASON;
const RETRY_DELAYS: [Duration; 2] = [Duration::from_secs(1), Duration::from_secs(4)];

fn is_workspace_routing_timeout(error: &TypedRequestError) -> bool {
    let TypedRequestError::Server { method, source } = error else {
        return false;
    };
    if method != "account/read" || source.code != INTERNAL_ERROR_CODE {
        return false;
    }
    match &source.data {
        Some(data) => {
            data.get("reason").and_then(serde_json::Value::as_str) == Some(ROUTING_TIMEOUT_REASON)
        }
        None => source.message == ROUTING_TIMEOUT_MESSAGE,
    }
}

fn without_workspace_routing_timeout_reason(mut error: TypedRequestError) -> TypedRequestError {
    if is_workspace_routing_timeout(&error)
        && let TypedRequestError::Server { source, .. } = &mut error
        && let Some(serde_json::Value::Object(mut data)) = source.data.take()
    {
        data.remove("reason");
        source.data = (!data.is_empty()).then_some(serde_json::Value::Object(data));
    }
    error
}

pub(super) async fn read(session: &mut AppServerSession) -> Result<GetAccountResponse> {
    let mut retry_delays = RETRY_DELAYS.into_iter();
    let mut attempt = 1;
    loop {
        let account_request_id = session.next_request_id();
        let result = session
            .request_handle
            .request_typed(ClientRequest::GetAccount {
                request_id: account_request_id,
                params: GetAccountParams {
                    refresh_token: false,
                },
            })
            .await;
        match result {
            Ok(account) => return Ok(account),
            Err(error) => {
                if is_workspace_routing_timeout(&error)
                    && let Some(delay) = retry_delays.next()
                {
                    attempt += 1;
                    tracing::warn!(
                        attempt,
                        "workspace routing discovery timed out; retrying account/read"
                    );
                    tokio::time::sleep(delay).await;
                    continue;
                }
                // TypedRequestError already includes the method and failure phase. Wrapping it
                // with the same account/read context produced "account/read failed" twice in
                // the terminal error shown to the person. The structured reason is an internal
                // retry discriminator, not another person-facing diagnostic; retain any other
                // server data while rendering the timeout without it.
                return Err(color_eyre::Report::new(
                    without_workspace_routing_timeout_reason(error),
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_app_server_client::CloudOperation;
    use codex_app_server_protocol::JSONRPCErrorError;
    use codex_app_server_protocol::RequestId;

    fn session() -> (
        AppServerSession,
        tokio::sync::mpsc::Receiver<codex_app_server_client::CloudCommand>,
    ) {
        let (client, commands, _events) =
            codex_app_server_client::CloudClient::channel(/*capacity*/ 1);
        (
            AppServerSession::new(
                crate::backend_id::BackendId::new(),
                AppServerClient::Cloud(client),
                ThreadParamsMode::Cloud,
            ),
            commands,
        )
    }

    fn timeout_error() -> JSONRPCErrorError {
        JSONRPCErrorError {
            code: INTERNAL_ERROR_CODE,
            data: Some(serde_json::json!({"reason": ROUTING_TIMEOUT_REASON})),
            message: ROUTING_TIMEOUT_MESSAGE.to_string(),
        }
    }

    fn timeout_error_with_diagnostic() -> JSONRPCErrorError {
        JSONRPCErrorError {
            data: Some(serde_json::json!({
                "reason": ROUTING_TIMEOUT_REASON,
                "requestId": "keep-this-diagnostic",
            })),
            ..timeout_error()
        }
    }

    fn legacy_timeout_error() -> JSONRPCErrorError {
        JSONRPCErrorError {
            code: INTERNAL_ERROR_CODE,
            data: None,
            message: ROUTING_TIMEOUT_MESSAGE.to_string(),
        }
    }

    fn account() -> GetAccountResponse {
        GetAccountResponse {
            account: None,
            user_id: None,
            requires_openai_auth: true,
            workspace_routing: None,
        }
    }

    fn ready_request(
        commands: &mut tokio::sync::mpsc::Receiver<codex_app_server_client::CloudCommand>,
    ) -> (RequestId, codex_app_server_client::CloudCommand) {
        let command = commands
            .try_recv()
            .expect("account/read request at the retry boundary");
        let request_id = account_read_request_id(&command);
        (request_id, command)
    }

    fn account_read_request_id(command: &codex_app_server_client::CloudCommand) -> RequestId {
        let CloudOperation::Request(request) = &command.operation else {
            panic!("expected account/read request")
        };
        let ClientRequest::GetAccount { request_id, params } = request.as_ref() else {
            panic!("expected account/read request")
        };
        assert!(!params.refresh_token);
        request_id.clone()
    }

    fn keep_paused_runtime_runnable() -> tokio::task::JoinHandle<()> {
        tokio::spawn(async {
            loop {
                tokio::task::yield_now().await;
            }
        })
    }

    #[tokio::test(start_paused = true)]
    async fn routing_timeout_gets_a_fresh_startup_read() {
        let (mut session, mut commands) = session();
        let expected = account();
        let started_at = tokio::time::Instant::now();
        let keep_runtime_runnable = keep_paused_runtime_runnable();
        let account_read = read(&mut session);
        tokio::pin!(account_read);

        assert!(futures::poll!(account_read.as_mut()).is_pending());
        let (first_id, first) = ready_request(&mut commands);
        first
            .response
            .send(Ok(Err(legacy_timeout_error())))
            .unwrap();
        assert!(futures::poll!(account_read.as_mut()).is_pending());
        assert!(matches!(
            commands.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));

        tokio::time::advance(Duration::from_secs(1)).await;
        assert!(futures::poll!(account_read.as_mut()).is_pending());
        let (second_id, second) = ready_request(&mut commands);
        second
            .response
            .send(Ok(Ok(serde_json::to_value(&expected).unwrap())))
            .unwrap();

        let actual = match futures::poll!(account_read.as_mut()) {
            std::task::Poll::Ready(Ok(actual)) => actual,
            std::task::Poll::Ready(Err(error)) => panic!("second account/read failed: {error}"),
            std::task::Poll::Pending => panic!("successful second account/read did not finish"),
        };
        assert_eq!(actual, expected);
        assert_ne!(first_id, second_id);
        assert_eq!(
            tokio::time::Instant::now().duration_since(started_at),
            Duration::from_secs(1)
        );

        // Let any detached retry register its timer at the success boundary.
        for _ in 0..4 {
            tokio::task::yield_now().await;
        }
        assert!(
            matches!(
                commands.try_recv(),
                Err(tokio::sync::mpsc::error::TryRecvError::Empty)
            ),
            "successful account/read issued a third request"
        );

        tokio::time::advance(Duration::from_secs(4)).await;
        for _ in 0..4 {
            tokio::task::yield_now().await;
        }
        assert!(
            matches!(
                commands.try_recv(),
                Err(tokio::sync::mpsc::error::TryRecvError::Empty)
            ),
            "successful account/read left a delayed third request"
        );
        assert_eq!(
            tokio::time::Instant::now().duration_since(started_at),
            Duration::from_secs(5)
        );

        keep_runtime_runnable.abort();
        let _ = keep_runtime_runnable.await;
    }

    #[tokio::test(start_paused = true)]
    async fn same_session_reads_have_fresh_retry_budgets() {
        let (mut session, mut commands) = session();
        let expected = account();
        let started_at = tokio::time::Instant::now();
        let keep_runtime_runnable = keep_paused_runtime_runnable();

        let (first_id, second_id) = {
            let account_read = read(&mut session);
            tokio::pin!(account_read);

            assert!(futures::poll!(account_read.as_mut()).is_pending());
            let (first_id, first) = ready_request(&mut commands);
            first.response.send(Ok(Err(timeout_error()))).unwrap();
            assert!(futures::poll!(account_read.as_mut()).is_pending());
            assert!(matches!(
                commands.try_recv(),
                Err(tokio::sync::mpsc::error::TryRecvError::Empty)
            ));

            tokio::time::advance(Duration::from_secs(1)).await;
            assert!(futures::poll!(account_read.as_mut()).is_pending());
            let (second_id, second) = ready_request(&mut commands);
            second
                .response
                .send(Ok(Ok(serde_json::to_value(&expected).unwrap())))
                .unwrap();
            let actual = match futures::poll!(account_read.as_mut()) {
                std::task::Poll::Ready(Ok(actual)) => actual,
                std::task::Poll::Ready(Err(error)) => panic!("first startup read failed: {error}"),
                std::task::Poll::Pending => {
                    panic!("first startup read did not finish at one second")
                }
            };
            assert_eq!(actual, expected);
            assert_eq!(
                tokio::time::Instant::now().duration_since(started_at),
                Duration::from_secs(1)
            );
            for _ in 0..4 {
                tokio::task::yield_now().await;
            }
            assert!(
                matches!(
                    commands.try_recv(),
                    Err(tokio::sync::mpsc::error::TryRecvError::Empty)
                ),
                "first startup read left an extra request"
            );
            (first_id, second_id)
        };

        let second_started_at = tokio::time::Instant::now();
        assert_eq!(
            second_started_at.duration_since(started_at),
            Duration::from_secs(1)
        );
        let account_read = read(&mut session);
        tokio::pin!(account_read);

        assert!(futures::poll!(account_read.as_mut()).is_pending());
        let (third_id, third) = ready_request(&mut commands);
        third.response.send(Ok(Err(timeout_error()))).unwrap();
        assert!(futures::poll!(account_read.as_mut()).is_pending());
        assert!(matches!(
            commands.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));

        tokio::time::advance(Duration::from_millis(999)).await;
        assert!(futures::poll!(account_read.as_mut()).is_pending());
        assert!(
            matches!(
                commands.try_recv(),
                Err(tokio::sync::mpsc::error::TryRecvError::Empty)
            ),
            "second startup read retried before its own one-second delay"
        );
        assert_eq!(
            tokio::time::Instant::now().duration_since(second_started_at),
            Duration::from_millis(999)
        );

        tokio::time::advance(Duration::from_millis(1)).await;
        assert!(futures::poll!(account_read.as_mut()).is_pending());
        let (fourth_id, fourth) = ready_request(&mut commands);
        fourth
            .response
            .send(Ok(Ok(serde_json::to_value(&expected).unwrap())))
            .unwrap();
        let actual = match futures::poll!(account_read.as_mut()) {
            std::task::Poll::Ready(Ok(actual)) => actual,
            std::task::Poll::Ready(Err(error)) => panic!("second startup read failed: {error}"),
            std::task::Poll::Pending => {
                panic!("second startup read did not get a fresh retry budget")
            }
        };
        assert_eq!(actual, expected);
        assert_eq!(
            tokio::time::Instant::now().duration_since(second_started_at),
            Duration::from_secs(1)
        );
        assert_eq!(
            tokio::time::Instant::now().duration_since(started_at),
            Duration::from_secs(2)
        );
        assert_ne!(first_id, second_id);
        assert_ne!(first_id, third_id);
        assert_ne!(first_id, fourth_id);
        assert_ne!(second_id, third_id);
        assert_ne!(second_id, fourth_id);
        assert_ne!(third_id, fourth_id);

        for _ in 0..4 {
            tokio::task::yield_now().await;
        }
        assert!(
            matches!(
                commands.try_recv(),
                Err(tokio::sync::mpsc::error::TryRecvError::Empty)
            ),
            "second startup read left an extra request"
        );

        keep_runtime_runnable.abort();
        let _ = keep_runtime_runnable.await;
    }

    #[tokio::test(start_paused = true)]
    async fn same_session_read_succeeds_immediately_after_retry_exhaustion() {
        let (mut session, mut commands) = session();
        let expected = account();
        let started_at = tokio::time::Instant::now();
        let keep_runtime_runnable = keep_paused_runtime_runnable();

        let (first_id, second_id, third_id) = {
            let account_read = read(&mut session);
            tokio::pin!(account_read);

            assert!(futures::poll!(account_read.as_mut()).is_pending());
            let (first_id, first) = ready_request(&mut commands);
            first.response.send(Ok(Err(timeout_error()))).unwrap();
            assert!(futures::poll!(account_read.as_mut()).is_pending());
            assert_eq!(
                tokio::time::Instant::now().duration_since(started_at),
                Duration::ZERO
            );

            tokio::time::advance(Duration::from_secs(1)).await;
            assert!(futures::poll!(account_read.as_mut()).is_pending());
            let (second_id, second) = ready_request(&mut commands);
            second.response.send(Ok(Err(timeout_error()))).unwrap();
            assert!(futures::poll!(account_read.as_mut()).is_pending());
            assert_eq!(
                tokio::time::Instant::now().duration_since(started_at),
                Duration::from_secs(1)
            );

            tokio::time::advance(Duration::from_secs(4)).await;
            assert!(futures::poll!(account_read.as_mut()).is_pending());
            let (third_id, third) = ready_request(&mut commands);
            third.response.send(Ok(Err(timeout_error()))).unwrap();
            let error = match futures::poll!(account_read.as_mut()) {
                std::task::Poll::Ready(Err(error)) => error,
                std::task::Poll::Ready(Ok(_)) => {
                    panic!("exhausted account/read unexpectedly succeeded")
                }
                std::task::Poll::Pending => {
                    panic!("exhausted account/read did not finish at five seconds")
                }
            };
            assert!(
                error.to_string().contains(ROUTING_TIMEOUT_MESSAGE),
                "{error}"
            );
            assert_eq!(
                tokio::time::Instant::now().duration_since(started_at),
                Duration::from_secs(5)
            );
            assert_eq!(
                commands.try_recv().err(),
                Some(tokio::sync::mpsc::error::TryRecvError::Empty),
                "exhausted account/read must leave the session's channel open and idle"
            );
            (first_id, second_id, third_id)
        };
        // The scope drops the exhausted backing future before borrowing the session again.

        let second_started_at = tokio::time::Instant::now();
        let account_read = read(&mut session);
        tokio::pin!(account_read);

        assert!(futures::poll!(account_read.as_mut()).is_pending());
        let (fourth_id, fourth) = ready_request(&mut commands);
        assert_eq!(
            tokio::time::Instant::now().duration_since(second_started_at),
            Duration::ZERO,
            "same-session account/read delayed its initial request after exhaustion"
        );
        fourth
            .response
            .send(Ok(Ok(serde_json::to_value(&expected).unwrap())))
            .unwrap();
        let actual = match futures::poll!(account_read.as_mut()) {
            std::task::Poll::Ready(Ok(actual)) => actual,
            std::task::Poll::Ready(Err(error)) => {
                panic!("same-session account/read failed after exhaustion: {error}")
            }
            std::task::Poll::Pending => {
                panic!("same-session account/read did not succeed immediately after exhaustion")
            }
        };
        assert_eq!(actual, expected);
        assert_ne!(first_id, second_id);
        assert_ne!(first_id, third_id);
        assert_ne!(first_id, fourth_id);
        assert_ne!(second_id, third_id);
        assert_ne!(second_id, fourth_id);
        assert_ne!(third_id, fourth_id);

        for _ in 0..4 {
            tokio::task::yield_now().await;
        }
        assert_eq!(
            commands.try_recv().err(),
            Some(tokio::sync::mpsc::error::TryRecvError::Empty),
            "successful account/read after exhaustion must leave an open, idle channel"
        );
        assert_eq!(
            tokio::time::Instant::now().duration_since(second_started_at),
            Duration::ZERO
        );
        assert_eq!(
            tokio::time::Instant::now().duration_since(started_at),
            Duration::from_secs(5)
        );

        keep_runtime_runnable.abort();
        let _ = keep_runtime_runnable.await;
    }

    #[tokio::test(start_paused = true)]
    async fn routing_timeout_can_succeed_on_the_final_retry() {
        let (mut session, mut commands) = session();
        let expected = account();
        let started_at = tokio::time::Instant::now();
        let keep_runtime_runnable = keep_paused_runtime_runnable();
        let account_read = read(&mut session);
        tokio::pin!(account_read);

        assert!(futures::poll!(account_read.as_mut()).is_pending());
        let (first_id, first) = ready_request(&mut commands);
        first.response.send(Ok(Err(timeout_error()))).unwrap();
        assert!(futures::poll!(account_read.as_mut()).is_pending());
        assert_eq!(
            tokio::time::Instant::now().duration_since(started_at),
            Duration::ZERO
        );

        tokio::time::advance(Duration::from_millis(999)).await;
        assert!(futures::poll!(account_read.as_mut()).is_pending());
        assert!(
            matches!(
                commands.try_recv(),
                Err(tokio::sync::mpsc::error::TryRecvError::Empty)
            ),
            "first retry ignored its one-second delay"
        );
        tokio::time::advance(Duration::from_millis(1)).await;
        assert!(futures::poll!(account_read.as_mut()).is_pending());
        let (second_id, second) = ready_request(&mut commands);
        second.response.send(Ok(Err(timeout_error()))).unwrap();
        assert!(futures::poll!(account_read.as_mut()).is_pending());
        assert_eq!(
            tokio::time::Instant::now().duration_since(started_at),
            Duration::from_secs(1)
        );

        tokio::time::advance(Duration::from_millis(3_999)).await;
        assert!(futures::poll!(account_read.as_mut()).is_pending());
        assert!(
            matches!(
                commands.try_recv(),
                Err(tokio::sync::mpsc::error::TryRecvError::Empty)
            ),
            "second retry ignored its four-second delay"
        );
        tokio::time::advance(Duration::from_millis(1)).await;
        assert!(futures::poll!(account_read.as_mut()).is_pending());
        let (third_id, third) = ready_request(&mut commands);
        third
            .response
            .send(Ok(Ok(serde_json::to_value(&expected).unwrap())))
            .unwrap();
        let actual = match futures::poll!(account_read.as_mut()) {
            std::task::Poll::Ready(Ok(actual)) => actual,
            std::task::Poll::Ready(Err(error)) => panic!("final startup retry failed: {error}"),
            std::task::Poll::Pending => {
                panic!("successful final retry did not finish at five seconds")
            }
        };
        assert_eq!(actual, expected);
        assert_eq!(
            tokio::time::Instant::now().duration_since(started_at),
            Duration::from_secs(5)
        );
        assert_ne!(first_id, second_id);
        assert_ne!(first_id, third_id);
        assert_ne!(second_id, third_id);

        for _ in 0..4 {
            tokio::task::yield_now().await;
        }
        assert!(
            matches!(
                commands.try_recv(),
                Err(tokio::sync::mpsc::error::TryRecvError::Empty)
            ),
            "successful final retry issued a fourth request"
        );

        keep_runtime_runnable.abort();
        let _ = keep_runtime_runnable.await;
    }

    #[tokio::test(start_paused = true)]
    async fn routing_timeout_retries_only_on_the_bounded_schedule() {
        let (mut session, mut commands) = session();
        let started_at = tokio::time::Instant::now();
        // `start_paused` otherwise advances to the next timer while the request driver waits,
        // which would make the test observe runtime idling rather than the retry boundaries.
        let keep_runtime_runnable = keep_paused_runtime_runnable();
        let account_read = read(&mut session);
        tokio::pin!(account_read);

        assert!(futures::poll!(account_read.as_mut()).is_pending());
        let (first_id, first) = ready_request(&mut commands);
        first.response.send(Ok(Err(timeout_error()))).unwrap();
        assert!(futures::poll!(account_read.as_mut()).is_pending());

        tokio::time::advance(Duration::from_millis(999)).await;
        assert!(futures::poll!(account_read.as_mut()).is_pending());
        assert!(
            matches!(
                commands.try_recv(),
                Err(tokio::sync::mpsc::error::TryRecvError::Empty)
            ),
            "first retry ignored its one-second delay"
        );
        tokio::time::advance(Duration::from_millis(1)).await;
        assert!(futures::poll!(account_read.as_mut()).is_pending());
        let (second_id, second) = ready_request(&mut commands);
        second.response.send(Ok(Err(timeout_error()))).unwrap();
        assert!(futures::poll!(account_read.as_mut()).is_pending());

        tokio::time::advance(Duration::from_millis(3_999)).await;
        assert!(futures::poll!(account_read.as_mut()).is_pending());
        assert!(
            matches!(
                commands.try_recv(),
                Err(tokio::sync::mpsc::error::TryRecvError::Empty)
            ),
            "second retry ignored its four-second delay"
        );
        tokio::time::advance(Duration::from_millis(1)).await;
        assert!(futures::poll!(account_read.as_mut()).is_pending());
        let (third_id, third) = ready_request(&mut commands);
        third
            .response
            .send(Ok(Err(timeout_error_with_diagnostic())))
            .unwrap();
        let actual = match futures::poll!(account_read.as_mut()) {
            std::task::Poll::Ready(actual) => actual,
            std::task::Poll::Pending => panic!("third routing timeout did not finish the read"),
        };
        keep_runtime_runnable.abort();
        let _ = keep_runtime_runnable.await;

        let error = actual.unwrap_err();
        let typed = error
            .downcast_ref::<TypedRequestError>()
            .expect("typed account/read error");
        let TypedRequestError::Server { method, source } = typed else {
            panic!("expected server error")
        };
        assert_eq!(method, "account/read");
        assert_eq!(source.code, INTERNAL_ERROR_CODE);
        assert_eq!(source.message, ROUTING_TIMEOUT_MESSAGE);
        assert_eq!(
            source.data,
            Some(serde_json::json!({"requestId": "keep-this-diagnostic"}))
        );

        let error = error.to_string();
        assert!(error.contains(ROUTING_TIMEOUT_MESSAGE), "{error}");
        assert_eq!(error.matches("account/read failed").count(), 1, "{error}");
        assert!(error.contains("(code -32603)"), "{error}");
        assert!(!error.contains(ROUTING_TIMEOUT_REASON), "{error}");
        assert!(error.contains("requestId"), "{error}");
        assert!(error.contains("keep-this-diagnostic"), "{error}");
        assert_eq!(
            tokio::time::Instant::now().duration_since(started_at),
            Duration::from_secs(5)
        );
        assert_ne!(first_id, second_id);
        assert_ne!(second_id, third_id);
        assert_ne!(first_id, third_id);
        assert_eq!(
            commands.try_recv().err(),
            Some(tokio::sync::mpsc::error::TryRecvError::Empty),
            "exhausted account/read must leave an open, idle channel without a fourth request"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn non_timeout_error_after_transient_timeout_stops_immediately() {
        const MESSAGE: &str = "selected workspace missing from routing discovery";
        let (mut session, mut commands) = session();
        let started_at = tokio::time::Instant::now();
        let keep_runtime_runnable = keep_paused_runtime_runnable();
        let account_read = read(&mut session);
        tokio::pin!(account_read);

        assert!(futures::poll!(account_read.as_mut()).is_pending());
        let (first_id, first) = ready_request(&mut commands);
        first.response.send(Ok(Err(timeout_error()))).unwrap();
        assert!(futures::poll!(account_read.as_mut()).is_pending());
        assert!(matches!(
            commands.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));

        tokio::time::advance(Duration::from_secs(1)).await;
        assert!(futures::poll!(account_read.as_mut()).is_pending());
        let (second_id, second) = ready_request(&mut commands);
        second
            .response
            .send(Ok(Err(JSONRPCErrorError {
                code: INTERNAL_ERROR_CODE,
                data: None,
                message: MESSAGE.to_string(),
            })))
            .unwrap();

        let error = match futures::poll!(account_read.as_mut()) {
            std::task::Poll::Ready(Err(error)) => error,
            std::task::Poll::Ready(Ok(_)) => panic!("non-timeout server error returned an account"),
            std::task::Poll::Pending => {
                panic!("non-timeout server error was retried after a timeout")
            }
        };
        // Let any detached retry register its timer at the one-second boundary.
        for _ in 0..4 {
            tokio::task::yield_now().await;
        }
        assert!(
            matches!(
                commands.try_recv(),
                Err(tokio::sync::mpsc::error::TryRecvError::Empty)
            ),
            "non-timeout server error issued a third request"
        );
        assert_eq!(
            tokio::time::Instant::now().duration_since(started_at),
            Duration::from_secs(1)
        );
        assert_ne!(first_id, second_id);

        let typed = error
            .downcast_ref::<TypedRequestError>()
            .expect("typed account/read error");
        let TypedRequestError::Server { method, source } = typed else {
            panic!("expected server error")
        };
        assert_eq!(method, "account/read");
        assert_eq!(source.code, INTERNAL_ERROR_CODE);
        assert_eq!(source.message, MESSAGE);
        assert_eq!(source.data, None);

        let rendered = error.to_string();
        assert_eq!(
            rendered.matches("account/read failed").count(),
            1,
            "{rendered}"
        );
        assert!(rendered.contains(MESSAGE), "{rendered}");

        tokio::time::advance(Duration::from_secs(4)).await;
        for _ in 0..4 {
            tokio::task::yield_now().await;
        }
        assert!(
            matches!(
                commands.try_recv(),
                Err(tokio::sync::mpsc::error::TryRecvError::Empty)
            ),
            "non-timeout server error left a delayed third request"
        );

        keep_runtime_runnable.abort();
        let _ = keep_runtime_runnable.await;
    }

    #[tokio::test(start_paused = true)]
    async fn cancellation_during_backoff_issues_no_later_request() {
        let (mut session, mut commands) = session();
        let started_at = tokio::time::Instant::now();
        let keep_runtime_runnable = keep_paused_runtime_runnable();

        {
            let mut account_read = std::pin::pin!(read(&mut session));
            assert!(futures::poll!(account_read.as_mut()).is_pending());
            let (_, first) = ready_request(&mut commands);
            first.response.send(Ok(Err(timeout_error()))).unwrap();
            assert!(futures::poll!(account_read.as_mut()).is_pending());
        }

        tokio::time::advance(Duration::from_secs(1)).await;
        for _ in 0..4 {
            tokio::task::yield_now().await;
        }
        assert!(
            matches!(
                commands.try_recv(),
                Err(tokio::sync::mpsc::error::TryRecvError::Empty)
            ),
            "cancelled startup issued its first retry"
        );

        tokio::time::advance(Duration::from_secs(4)).await;
        for _ in 0..4 {
            tokio::task::yield_now().await;
        }
        assert!(
            matches!(
                commands.try_recv(),
                Err(tokio::sync::mpsc::error::TryRecvError::Empty)
            ),
            "cancelled startup issued a later retry"
        );
        assert_eq!(
            tokio::time::Instant::now().duration_since(started_at),
            Duration::from_secs(5)
        );

        keep_runtime_runnable.abort();
        let _ = keep_runtime_runnable.await;
    }

    #[tokio::test(start_paused = true)]
    async fn in_flight_cancellation_does_not_poison_later_same_session_read() {
        let (mut session, mut commands) = session();
        let expected = account();
        let started_at = tokio::time::Instant::now();
        let keep_runtime_runnable = keep_paused_runtime_runnable();

        let (first_id, first) = {
            let account_read = read(&mut session);
            tokio::pin!(account_read);

            assert!(futures::poll!(account_read.as_mut()).is_pending());
            let (first_id, first) = ready_request(&mut commands);
            assert!(
                !first.response.is_closed(),
                "in-flight account/read must still own its response receiver"
            );
            (first_id, first)
        };
        assert!(
            first.response.is_closed(),
            "dropping the pending read must close its own response receiver"
        );

        let recovery_started_at = tokio::time::Instant::now();
        assert_eq!(
            recovery_started_at.duration_since(started_at),
            Duration::ZERO
        );
        let account_read = read(&mut session);
        tokio::pin!(account_read);

        assert!(futures::poll!(account_read.as_mut()).is_pending());
        let (second_id, second) = ready_request(&mut commands);
        assert_ne!(first_id, second_id);

        let expected_timeout = timeout_error();
        let undelivered = first
            .response
            .send(Ok(Err(expected_timeout.clone())))
            .expect_err("canceled account/read accepted its late timeout");
        let actual_timeout = match undelivered {
            Ok(Err(error)) => error,
            other => panic!("late timeout returned a different response shape: {other:?}"),
        };
        assert_eq!(actual_timeout, expected_timeout);
        assert!(
            !second.response.is_closed(),
            "canceling the first read closed the second response receiver"
        );
        for _ in 0..4 {
            tokio::task::yield_now().await;
        }
        assert!(
            futures::poll!(account_read.as_mut()).is_pending(),
            "the canceled read's late timeout completed the second read"
        );
        assert_eq!(
            commands.try_recv().err(),
            Some(tokio::sync::mpsc::error::TryRecvError::Empty),
            "the canceled read's late timeout issued a retry or extra command"
        );

        second
            .response
            .send(Ok(Ok(serde_json::to_value(&expected).unwrap())))
            .expect("the second read must still accept its own response");
        let actual = match futures::poll!(account_read.as_mut()) {
            std::task::Poll::Ready(Ok(actual)) => actual,
            std::task::Poll::Ready(Err(error)) => {
                panic!("same-session read failed after in-flight cancellation: {error}")
            }
            std::task::Poll::Pending => {
                panic!("same-session read did not succeed immediately after in-flight cancellation")
            }
        };
        assert_eq!(actual, expected);
        for _ in 0..4 {
            tokio::task::yield_now().await;
        }
        assert_eq!(
            commands.try_recv().err(),
            Some(tokio::sync::mpsc::error::TryRecvError::Empty),
            "successful recovery after in-flight cancellation left an extra command"
        );
        assert_eq!(
            tokio::time::Instant::now().duration_since(recovery_started_at),
            Duration::ZERO
        );
        assert_eq!(
            tokio::time::Instant::now().duration_since(started_at),
            Duration::ZERO
        );

        tokio::time::advance(Duration::from_secs(1)).await;
        for _ in 0..4 {
            tokio::task::yield_now().await;
        }
        assert_eq!(
            commands.try_recv().err(),
            Some(tokio::sync::mpsc::error::TryRecvError::Empty),
            "canceled in-flight read issued a detached first retry"
        );
        tokio::time::advance(Duration::from_secs(4)).await;
        for _ in 0..4 {
            tokio::task::yield_now().await;
        }
        assert_eq!(
            commands.try_recv().err(),
            Some(tokio::sync::mpsc::error::TryRecvError::Empty),
            "canceled in-flight read issued a detached later retry"
        );
        assert_eq!(
            tokio::time::Instant::now().duration_since(started_at),
            Duration::from_secs(5)
        );

        keep_runtime_runnable.abort();
        let _ = keep_runtime_runnable.await;
    }

    #[tokio::test]
    async fn government_workspace_required_is_not_retried_or_redacted() {
        const MESSAGE: &str =
            "Sign in to a government workspace to use the configured government backend.";
        let expected_data = serde_json::json!({
            "reason": "governmentWorkspaceRequired",
            "action": "relogin",
        });
        let (mut session, mut commands) = session();
        let mut account_read = std::pin::pin!(read(&mut session));

        assert!(futures::poll!(account_read.as_mut()).is_pending());
        let (_, command) = ready_request(&mut commands);
        command
            .response
            .send(Ok(Err(JSONRPCErrorError {
                code: -32600,
                data: Some(expected_data.clone()),
                message: MESSAGE.to_string(),
            })))
            .unwrap();

        let actual = match futures::poll!(account_read.as_mut()) {
            std::task::Poll::Ready(actual) => actual,
            std::task::Poll::Pending => panic!("government workspace error was retried"),
        };
        assert!(
            matches!(
                commands.try_recv(),
                Err(tokio::sync::mpsc::error::TryRecvError::Empty)
            ),
            "government workspace error issued a retry"
        );

        let error = actual.unwrap_err();
        let typed = error
            .downcast_ref::<TypedRequestError>()
            .expect("typed account/read error");
        let TypedRequestError::Server { method, source } = typed else {
            panic!("expected server error")
        };
        assert_eq!(method, "account/read");
        assert_eq!(source.code, -32600);
        assert_eq!(source.message, MESSAGE);
        assert_eq!(source.data.as_ref(), Some(&expected_data));

        let rendered = error.to_string();
        assert_eq!(
            rendered.matches("account/read failed").count(),
            1,
            "{rendered}"
        );
        assert!(rendered.contains(MESSAGE), "{rendered}");
        assert!(rendered.contains("(code -32600)"), "{rendered}");
        assert!(
            rendered.contains("governmentWorkspaceRequired"),
            "{rendered}"
        );
        assert!(rendered.contains("relogin"), "{rendered}");
    }

    #[tokio::test]
    async fn transport_failure_preserves_its_phase_without_retry() {
        const MARKER: &str = "startup-transport-marker";
        let (mut session, mut commands) = session();
        let mut account_read = std::pin::pin!(read(&mut session));

        assert!(futures::poll!(account_read.as_mut()).is_pending());
        let (_, command) = ready_request(&mut commands);
        command
            .response
            .send(Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionReset,
                MARKER,
            )))
            .unwrap();

        let actual = match futures::poll!(account_read.as_mut()) {
            std::task::Poll::Ready(actual) => actual,
            std::task::Poll::Pending => panic!("account/read transport failure was retried"),
        };
        assert!(
            matches!(
                commands.try_recv(),
                Err(tokio::sync::mpsc::error::TryRecvError::Empty)
            ),
            "account/read transport failure issued a retry"
        );

        let error = actual.unwrap_err();
        let typed = error
            .downcast_ref::<TypedRequestError>()
            .expect("typed account/read error");
        let TypedRequestError::Transport { method, source } = typed else {
            panic!("expected transport error")
        };
        assert_eq!(method, "account/read");
        assert_eq!(source.kind(), std::io::ErrorKind::ConnectionReset);
        assert_eq!(source.to_string(), MARKER);

        let rendered = error.to_string();
        assert_eq!(rendered.matches("account/read").count(), 1, "{rendered}");
        assert!(rendered.contains("transport error"), "{rendered}");
        assert!(rendered.contains(MARKER), "{rendered}");
    }

    #[tokio::test]
    async fn response_decode_failure_preserves_its_phase_without_retry() {
        const MARKER: &str = "startup-decode-marker";
        let (mut session, mut commands) = session();
        let mut account_read = std::pin::pin!(read(&mut session));

        assert!(futures::poll!(account_read.as_mut()).is_pending());
        let (_, command) = ready_request(&mut commands);
        command
            .response
            .send(Ok(Ok(serde_json::json!({
                "account": null,
                "requiresOpenaiAuth": MARKER,
                "workspaceRouting": null,
            }))))
            .unwrap();

        let actual = match futures::poll!(account_read.as_mut()) {
            std::task::Poll::Ready(actual) => actual,
            std::task::Poll::Pending => panic!("account/read decode failure was retried"),
        };
        assert!(
            matches!(
                commands.try_recv(),
                Err(tokio::sync::mpsc::error::TryRecvError::Empty)
            ),
            "account/read decode failure issued a retry"
        );

        let error = actual.unwrap_err();
        let typed = error
            .downcast_ref::<TypedRequestError>()
            .expect("typed account/read error");
        let TypedRequestError::Deserialize { method, source } = typed else {
            panic!("expected response decode error")
        };
        assert_eq!(method, "account/read");
        assert!(source.to_string().contains(MARKER), "{source}");

        let rendered = error.to_string();
        assert_eq!(rendered.matches("account/read").count(), 1, "{rendered}");
        assert!(rendered.contains("response decode error"), "{rendered}");
        assert!(rendered.contains(MARKER), "{rendered}");
    }

    #[tokio::test(start_paused = true)]
    async fn fatal_account_error_does_not_poison_later_same_session_read() {
        const MESSAGE: &str = "selected workspace missing from routing discovery";
        let (mut session, mut commands) = session();
        let expected = account();
        let started_at = tokio::time::Instant::now();
        let keep_runtime_runnable = keep_paused_runtime_runnable();

        let first_id = {
            let account_read = read(&mut session);
            tokio::pin!(account_read);

            assert!(futures::poll!(account_read.as_mut()).is_pending());
            let (first_id, command) = ready_request(&mut commands);
            command
                .response
                .send(Ok(Err(JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    data: None,
                    message: MESSAGE.to_string(),
                })))
                .unwrap();
            let error = match futures::poll!(account_read.as_mut()) {
                std::task::Poll::Ready(Err(error)) => error,
                std::task::Poll::Ready(Ok(_)) => {
                    panic!("fatal account/read unexpectedly succeeded")
                }
                std::task::Poll::Pending => {
                    panic!("fatal account/read did not terminate immediately")
                }
            };
            let error = error.to_string();
            assert!(error.contains(MESSAGE), "{error}");
            assert_eq!(error.matches("account/read failed").count(), 1, "{error}");
            assert_eq!(
                tokio::time::Instant::now().duration_since(started_at),
                Duration::ZERO
            );
            assert_eq!(
                commands.try_recv().err(),
                Some(tokio::sync::mpsc::error::TryRecvError::Empty),
                "fatal account/read must leave an open, idle channel without a retry"
            );
            first_id
        };
        // The scope drops the failed backing future before borrowing the session again.

        let recovery_started_at = tokio::time::Instant::now();
        let account_read = read(&mut session);
        tokio::pin!(account_read);

        assert!(futures::poll!(account_read.as_mut()).is_pending());
        let (second_id, command) = ready_request(&mut commands);
        assert_eq!(
            tokio::time::Instant::now().duration_since(recovery_started_at),
            Duration::ZERO,
            "same-session account/read delayed its initial request after a fatal error"
        );
        command
            .response
            .send(Ok(Ok(serde_json::to_value(&expected).unwrap())))
            .unwrap();
        let actual = match futures::poll!(account_read.as_mut()) {
            std::task::Poll::Ready(Ok(actual)) => actual,
            std::task::Poll::Ready(Err(error)) => {
                panic!("same-session account/read failed after a fatal error: {error}")
            }
            std::task::Poll::Pending => {
                panic!("same-session account/read did not succeed immediately after a fatal error")
            }
        };
        assert_eq!(actual, expected);
        assert_ne!(first_id, second_id);

        for _ in 0..4 {
            tokio::task::yield_now().await;
        }
        assert_eq!(
            commands.try_recv().err(),
            Some(tokio::sync::mpsc::error::TryRecvError::Empty),
            "successful account/read after a fatal error must leave an open, idle channel"
        );
        assert_eq!(
            tokio::time::Instant::now().duration_since(recovery_started_at),
            Duration::ZERO
        );
        assert_eq!(
            tokio::time::Instant::now().duration_since(started_at),
            Duration::ZERO
        );

        keep_runtime_runnable.abort();
        let _ = keep_runtime_runnable.await;
    }

    #[test]
    fn structured_reason_and_data_presence_are_authoritative() {
        let error = |message: &str, data| TypedRequestError::Server {
            method: "account/read".to_string(),
            source: JSONRPCErrorError {
                code: INTERNAL_ERROR_CODE,
                data,
                message: message.to_string(),
            },
        };

        assert!(is_workspace_routing_timeout(&error(
            ROUTING_TIMEOUT_MESSAGE,
            None,
        )));
        assert!(is_workspace_routing_timeout(&error(
            "wording changed without changing the wire contract",
            Some(serde_json::json!({"reason": ROUTING_TIMEOUT_REASON})),
        )));
        for data in [
            serde_json::json!({}),
            serde_json::json!({"requestId": "not-a-reason"}),
            serde_json::json!({"reason": "differentFailure"}),
        ] {
            assert!(!is_workspace_routing_timeout(&error(
                ROUTING_TIMEOUT_MESSAGE,
                Some(data),
            )));
        }
    }

    #[test]
    fn structured_reason_still_requires_the_account_read_internal_error() {
        let error = |method: &str, code| TypedRequestError::Server {
            method: method.to_string(),
            source: JSONRPCErrorError {
                code,
                data: Some(serde_json::json!({"reason": ROUTING_TIMEOUT_REASON})),
                message: ROUTING_TIMEOUT_MESSAGE.to_string(),
            },
        };

        assert!(!is_workspace_routing_timeout(&error(
            "account/login/status",
            INTERNAL_ERROR_CODE,
        )));
        assert!(!is_workspace_routing_timeout(&error(
            "account/read",
            -32600
        )));
    }

    #[test]
    fn terminal_rendering_hides_only_the_timeout_retry_reason() {
        let timeout = TypedRequestError::Server {
            method: "account/read".to_string(),
            source: JSONRPCErrorError {
                code: INTERNAL_ERROR_CODE,
                data: Some(serde_json::json!({
                    "reason": ROUTING_TIMEOUT_REASON,
                    "requestId": "keep-this-diagnostic",
                })),
                message: ROUTING_TIMEOUT_MESSAGE.to_string(),
            },
        };

        let rendered = without_workspace_routing_timeout_reason(timeout).to_string();
        assert!(!rendered.contains(ROUTING_TIMEOUT_REASON), "{rendered}");
        assert!(rendered.contains("keep-this-diagnostic"), "{rendered}");

        let reason_only = TypedRequestError::Server {
            method: "account/read".to_string(),
            source: timeout_error(),
        };
        let rendered = without_workspace_routing_timeout_reason(reason_only).to_string();
        assert!(!rendered.contains(ROUTING_TIMEOUT_REASON), "{rendered}");
        assert!(!rendered.contains("data:"), "{rendered}");

        let other = TypedRequestError::Server {
            method: "account/read".to_string(),
            source: JSONRPCErrorError {
                code: INTERNAL_ERROR_CODE,
                data: Some(serde_json::json!({"reason": "differentFailure"})),
                message: "another failure".to_string(),
            },
        };
        let rendered = without_workspace_routing_timeout_reason(other).to_string();
        assert!(rendered.contains("differentFailure"), "{rendered}");
    }
}
