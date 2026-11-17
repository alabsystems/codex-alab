use super::alab_request_diagnostics; // ALAB(recovery): request metadata stays local and content-free.
use crate::auth::SharedAuthProvider;
use crate::common::ResponseStream;
use crate::common::ResponsesApiRequest;
use crate::endpoint::session::EndpointSession;
use crate::error::ApiError;
use crate::provider::Provider;
use crate::requests::Compression;
use crate::requests::headers::build_session_headers;
use crate::requests::headers::insert_header;
use crate::requests::headers::subagent_header;
use crate::sse::spawn_response_stream;
use crate::telemetry::SseTelemetry;
use codex_client::EncodedJsonBody;
use codex_client::HttpTransport;
use codex_client::RequestCompression;
use codex_client::RequestTelemetry;
use codex_protocol::protocol::SessionSource;
use http::HeaderMap;
use http::HeaderValue;
use http::Method;
use serde_json::Value;
use std::sync::Arc;
use std::sync::OnceLock;
use tracing::instrument;

pub struct ResponsesClient<T: HttpTransport> {
    session: EndpointSession<T>,
    sse_telemetry: Option<Arc<dyn SseTelemetry>>,
}

#[derive(Default)]
pub struct ResponsesOptions {
    pub session_id: Option<String>,
    pub thread_id: Option<String>,
    pub session_source: Option<SessionSource>,
    pub extra_headers: HeaderMap,
    pub compression: Compression,
    pub turn_state: Option<Arc<OnceLock<String>>>,
}

impl<T: HttpTransport> ResponsesClient<T> {
    pub fn new(transport: T, provider: Provider, auth: SharedAuthProvider) -> Self {
        Self {
            session: EndpointSession::new(transport, provider, auth),
            sse_telemetry: None,
        }
    }

    pub fn with_telemetry(
        self,
        request: Option<Arc<dyn RequestTelemetry>>,
        sse: Option<Arc<dyn SseTelemetry>>,
    ) -> Self {
        Self {
            session: self.session.with_request_telemetry(request),
            sse_telemetry: sse,
        }
    }

    #[instrument(
        name = "responses.stream_request",
        level = "info",
        skip_all,
        fields(
            transport = "responses_http",
            http.method = "POST",
            api.path = "/responses"
        )
    )]
    pub async fn stream_request(
        &self,
        request: ResponsesApiRequest,
        options: ResponsesOptions,
    ) -> Result<ResponseStream, ApiError> {
        let ResponsesOptions {
            session_id,
            thread_id,
            session_source,
            extra_headers,
            compression,
            turn_state,
        } = options;
        let body = EncodedJsonBody::encode(&request)
            .map_err(|e| ApiError::Stream(format!("failed to encode responses request: {e}")))?;

        let alab_diagnostic = alab_request_diagnostics::http(&request, body.as_bytes().len(), compression); // ALAB(recovery): record the actual wire request shape without retaining its content.
        let mut headers = extra_headers;
        if let Some(ref thread_id) = thread_id {
            insert_header(&mut headers, "x-client-request-id", thread_id);
        }
        headers.extend(build_session_headers(session_id, thread_id));
        if let Some(subagent) = subagent_header(&session_source) {
            insert_header(&mut headers, "x-openai-subagent", &subagent);
        }

        self.stream_encoded(body, headers, compression, turn_state, alab_diagnostic.as_ref()) // ALAB(recovery): retain this attempt through the existing SSE task.
            .await.inspect_err(|error| alab_request_diagnostics::failed(alab_diagnostic.as_ref(), error)) // ALAB(recovery): preserve local request correlation before mapping the error.
    }

    #[instrument(
        name = "responses.stream",
        level = "info",
        skip_all,
        fields(
            transport = "responses_http",
            http.method = "POST",
            api.path = "/responses",
            turn.has_state = turn_state.is_some()
        )
    )]
    pub async fn stream(
        &self,
        body: Value,
        extra_headers: HeaderMap,
        compression: Compression,
        turn_state: Option<Arc<OnceLock<String>>>,
    ) -> Result<ResponseStream, ApiError> {
        let alab_body = &body; // ALAB(recovery): count the original Value before its encoded bytes shadow it.
        let body = EncodedJsonBody::encode(&body)
            .map_err(|e| ApiError::Stream(format!("failed to encode responses request: {e}")))?;
        let alab_diagnostic = alab_request_diagnostics::http_value(alab_body, body.as_bytes().len(), compression); // ALAB(recovery): generic compaction requests share the local shape diagnostic.
        self.stream_encoded(body, extra_headers, compression, turn_state, alab_diagnostic.as_ref()) // ALAB(recovery): retain this attempt through the existing SSE task.
            .await.inspect_err(|error| alab_request_diagnostics::failed(alab_diagnostic.as_ref(), error)) // ALAB(recovery): HTTP setup failures retain their request correlation.
    }

    async fn stream_encoded(
        &self,
        body: EncodedJsonBody,
        extra_headers: HeaderMap,
        compression: Compression,
        turn_state: Option<Arc<OnceLock<String>>>,
        alab_diagnostic: Option<&alab_request_diagnostics::RequestDiagnostic>, // ALAB(recovery): the stream retains the request attempt that opened it.
    ) -> Result<ResponseStream, ApiError> {
        let request_compression = match compression {
            Compression::None => RequestCompression::None,
            Compression::Zstd => RequestCompression::Zstd,
        };

        let stream_response = self
            .session
            .stream_encoded_json_with(
                Method::POST,
                "/responses",
                extra_headers,
                Some(body),
                alab_diagnostic, // ALAB(recovery): each HTTP operation retains its opening request context.
                |req| {
                    req.headers.insert(
                        http::header::ACCEPT,
                        HeaderValue::from_static("text/event-stream"),
                    );
                    req.compression = request_compression;
                },
            )
            .await?;

        let alab_sse_telemetry = alab_request_diagnostics::sse_telemetry(alab_diagnostic, self.sse_telemetry.clone(), &stream_response.headers); // ALAB(recovery): capture context and successful response IDs before the SSE task starts.
        Ok(spawn_response_stream(
            stream_response,
            self.session.provider().stream_idle_timeout,
            alab_sse_telemetry, // ALAB(recovery): observe actual stream failures without another channel or task.
            turn_state,
        ))
    }
}
