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
                // the terminal error shown to the person.
                return Err(color_eyre::Report::new(error));
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

    async fn request(
        commands: &mut tokio::sync::mpsc::Receiver<codex_app_server_client::CloudCommand>,
    ) -> (RequestId, codex_app_server_client::CloudCommand) {
        let command = commands.recv().await.expect("account/read request");
        let CloudOperation::Request(request) = &command.operation else {
            panic!("expected account/read request")
        };
        let ClientRequest::GetAccount { request_id, params } = request.as_ref() else {
            panic!("expected account/read request")
        };
        assert!(!params.refresh_token);
        (request_id.clone(), command)
    }

    #[tokio::test(start_paused = true)]
    async fn routing_timeout_gets_a_fresh_startup_read() {
        let (mut session, mut commands) = session();
        let expected = account();
        let response = expected.clone();

        let (actual, request_ids) = tokio::join!(read(&mut session), async {
            let (first_id, first) = request(&mut commands).await;
            first.response.send(Ok(Err(timeout_error()))).unwrap();
            let (second_id, second) = request(&mut commands).await;
            second
                .response
                .send(Ok(Ok(serde_json::to_value(response).unwrap())))
                .unwrap();
            [first_id, second_id]
        });

        assert_eq!(actual.unwrap(), expected);
        assert_ne!(request_ids[0], request_ids[1]);
    }

    #[tokio::test(start_paused = true)]
    async fn routing_timeout_retries_only_within_the_bound() {
        let (mut session, mut commands) = session();

        let (actual, request_ids) = tokio::join!(read(&mut session), async {
            let (first_id, first) = request(&mut commands).await;
            first.response.send(Ok(Err(timeout_error()))).unwrap();
            let (second_id, second) = request(&mut commands).await;
            second.response.send(Ok(Err(timeout_error()))).unwrap();
            let (third_id, third) = request(&mut commands).await;
            third.response.send(Ok(Err(timeout_error()))).unwrap();
            [first_id, second_id, third_id]
        });

        let error = actual.unwrap_err().to_string();
        assert!(error.contains(ROUTING_TIMEOUT_MESSAGE), "{error}");
        assert_eq!(error.matches("account/read failed").count(), 1, "{error}");
        assert!(error.contains("(code -32603)"), "{error}");
        assert_ne!(request_ids[0], request_ids[1]);
        assert_ne!(request_ids[1], request_ids[2]);
        assert!(commands.try_recv().is_err(), "unexpected fourth request");
    }

    #[tokio::test]
    async fn other_account_errors_are_not_retried() {
        let (mut session, mut commands) = session();

        let (actual, ()) = tokio::join!(read(&mut session), async {
            let (_, command) = request(&mut commands).await;
            command
                .response
                .send(Ok(Err(JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    data: None,
                    message: "selected workspace missing from routing discovery".to_string(),
                })))
                .unwrap();
        });

        let error = actual.unwrap_err().to_string();
        assert!(error.contains("selected workspace missing"), "{error}");
        assert_eq!(error.matches("account/read failed").count(), 1, "{error}");
        assert!(commands.try_recv().is_err(), "unexpected retry");
    }

    #[test]
    fn structured_reason_is_authoritative() {
        let error = |data| TypedRequestError::Server {
            method: "account/read".to_string(),
            source: JSONRPCErrorError {
                code: INTERNAL_ERROR_CODE,
                data,
                message: ROUTING_TIMEOUT_MESSAGE.to_string(),
            },
        };

        assert!(is_workspace_routing_timeout(&error(None)));
        assert!(is_workspace_routing_timeout(&error(Some(
            serde_json::json!({"reason": ROUTING_TIMEOUT_REASON}),
        ))));
        assert!(!is_workspace_routing_timeout(&error(Some(
            serde_json::json!({"reason": "differentFailure"}),
        ))));
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
}
