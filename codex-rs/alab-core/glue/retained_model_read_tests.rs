// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual command/response channel +same original typed decoder and cache tap.
//! No Native/current/Source grant or original-value publication is constructed.
use super::*;
use crate::AppServerRequestHandle;
use crate::CloudClient;
use crate::CloudOperation;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ModelListParams;
use codex_app_server_protocol::ModelListResponse;
use codex_app_server_protocol::RequestId;
const WAIT: Duration = Duration::from_secs(60);
fn invalid(message: &str) -> IoError {
    IoError::other(message)
}
#[tokio::test]
async fn actual_original_model_errors_replay_exactly_from_cache_without_another_command()
-> IoResult<()> {
    for case in 0..3 {
        let (client, mut commands, events) = CloudClient::channel(2);
        let handle = AppServerRequestHandle::Cloud(client.request_handle());
        let capture = Capture::prepare();
        let worker_capture = capture.clone();
        let mut joined = false;
        let mut worker = tokio::spawn(async move {
            worker_capture
                .scope(
                    handle.request_typed::<ModelListResponse>(ClientRequest::ModelList {
                        request_id: RequestId::Integer(71),
                        params: ModelListParams {
                            cursor: None,
                            limit: None,
                            include_hidden: Some(true),
                            model_provider: None,
                        },
                    }),
                )
                .await
        });
        let body=async {
            let command=tokio::time::timeout(WAIT,commands.recv()).await.map_err(|_|invalid("original command did not arrive"))?.ok_or_else(||invalid("original command channel closed"))?;
            if !matches!(command.operation,CloudOperation::Request(ref request) if matches!(request.as_ref(),ClientRequest::ModelList {..})) {return Err(invalid("a different original method was sent"));}
            let response=match case {
                0=>Ok(Ok(serde_json::json!({"data":false}))),
                1=>Ok(Err(JSONRPCErrorError {code:-32000,message:"actual fixture model server error".into(),data:None})),
                _=>Err(IoError::from(ErrorKind::BrokenPipe)),
            };
            command.response.send(response).map_err(|_|invalid("original response observer lost"))?;
            let result=tokio::time::timeout(WAIT,&mut worker).await.map_err(|_|invalid("same original model reader did not join"))?;
            joined=true;
            let result=result.map_err(IoError::other)?;
            let original=match result {Ok(_)=>return Err(invalid("failing original model response was accepted")),Err(error)=>error};
            let recorded=capture.recorded()?;let empty=Capture::prepare();
            let cached=empty.scope(async {recorded.decode::<ModelListResponse>()}).await;
            let cached=match cached {Ok(_)=>return Err(invalid("cached original failure became success")),Err(error)=>error};
            if original.to_string()!=cached.to_string() {return Err(invalid("cached original error changed"));}
            if !matches!((&original,&cached),(TypedRequestError::Transport {..},TypedRequestError::Transport {..})|(TypedRequestError::Server {..},TypedRequestError::Server {..})|(TypedRequestError::Deserialize {..},TypedRequestError::Deserialize {..})) {return Err(invalid("cached original typed error variant changed"));}
            if empty.recorded().is_ok() {return Err(invalid("historical cache decode was recorded as a fresh RPC"));}
            if !matches!(commands.try_recv(),Err(tokio::sync::mpsc::error::TryRecvError::Empty)) {return Err(invalid("cache decode issued another original request"));}
            Ok(())
        }.await;
        if !joined {
            worker.abort();
            let _joined = worker.await;
        }
        // A completed handle above was consumed once; no task is detached.
        drop(client);
        drop(events);
        body?;
    }
    Ok(())
}
#[test]
fn unsupported_transport_kind_remains_explicit_instead_of_becoming_other_or_success() {
    let recorded = Recorded {
        schema: 1,
        reply: Reply::Transport {
            error: IoData {
                kind: "not-a-native-kind".into(),
                raw: None,
                message: "old actual failure DATA".into(),
            },
        },
    };
    assert!(recorded.validate_data().is_err());
}

#[derive(Debug)]
struct CustomTransport {
    cause: IoError,
}
impl fmt::Display for CustomTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "original nested transport")
    }
}
impl std::error::Error for CustomTransport {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.cause)
    }
}
#[tokio::test]
async fn nested_custom_transport_payload_is_retained_natively_and_refused_as_cache() -> IoResult<()>
{
    let capture = Capture::prepare();
    let native = capture
        .scope(async {
            crate::decode_typed_response::<ModelListResponse>(
                "model/list",
                Err(IoError::new(
                    ErrorKind::BrokenPipe,
                    CustomTransport {
                        cause: IoError::from(ErrorKind::TimedOut),
                    },
                )),
            )
        })
        .await;
    let Err(TypedRequestError::Transport { source, .. }) = native else {
        return Err(invalid("actual custom transport error changed variant"));
    };
    let Some(original) = source
        .get_ref()
        .and_then(|payload| payload.downcast_ref::<CustomTransport>())
    else {
        return Err(invalid("ordinary custom error payload was flattened"));
    };
    if original.cause.kind() != ErrorKind::TimedOut || capture.recorded().is_ok() {
        return Err(invalid(
            "unsupported original custom payload was accepted as exact cache",
        ));
    }
    Ok(())
}
#[tokio::test]
async fn nested_different_model_response_invalidates_both_original_capture_identities()
-> IoResult<()> {
    let outer = Capture::prepare();
    let inner = Capture::prepare();
    let native = outer
        .scope(async {
            let _first = crate::decode_typed_response::<ModelListResponse>(
                "model/list",
                Ok(Ok(serde_json::json!({"data":[],"nextCursor":"R1"}))),
            )?;
            inner
                .scope(async {
                    crate::decode_typed_response::<ModelListResponse>(
                        "model/list",
                        Ok(Ok(serde_json::json!({"data":[],"nextCursor":"R2"}))),
                    )
                })
                .await
        })
        .await
        .map_err(IoError::other)?;
    if native.next_cursor.as_deref() != Some("R2")
        || outer.recorded().is_ok()
        || inner.recorded().is_ok()
    {
        return Err(invalid(
            "nested actual R2 output retained a falsely exact R1 or inner record",
        ));
    }
    Ok(())
}
