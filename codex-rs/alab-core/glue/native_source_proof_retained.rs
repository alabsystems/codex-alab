// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Real SDK/app-server debug integration, never an ordinary TUI startup route.
//! Native admits the foreground/controller/backend/SDK and both physical A/F1
//! transfers. Markers choose test dispatch only and grant no native permission.
use codex_alab_aterm::frontend_control::native_source_proof as retained;
use codex_alab_aterm::source_retained_service::StartupSettings;
use codex_app_server_client::AppServerEvent;
use codex_app_server_client::RemoteAppServerClient;
use codex_app_server_client::RemoteAppServerConnectArgs;
use codex_app_server_client::RemoteAppServerEndpoint;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ConfigReadParams;
use codex_app_server_protocol::ConfigReadResponse;
use codex_app_server_protocol::RequestId;
use std::io;
use std::time::Duration;
use tokio::io::AsyncBufReadExt as _;
use tokio::io::AsyncReadExt as _;
use tokio::io::AsyncWriteExt as _;
use tokio::io::BufReader;

const FRONTEND: &str = "alab-native-retained-proof";
const BACKEND: &str = "--alab-retained-service";
const BOUND: Duration = Duration::from_secs(120);

pub(crate) fn requested() -> bool {
    std::env::args_os()
        .nth(1)
        .is_some_and(|value| value == FRONTEND || value == BACKEND)
}
fn unavailable() -> io::Error {
    io::Error::other("retained Source SDK qualification unavailable")
}
fn operation() -> String {
    format!(
        "{:016x}{:016x}",
        codex_alab_core::native_source::connection_request_prefix(),
        codex_alab_core::native_source::connection_request_prefix()
    )
}
async fn phase(value: &'static str) -> io::Result<()> {
    let mut output = tokio::io::stdout();
    output
        .write_all(format!("Retained qualification: phase={value}\n").as_bytes())
        .await?;
    output.flush().await
}

async fn failure(stage: &'static str, kind: io::ErrorKind) -> io::Result<()> {
    // Match the existing native driver's closed diagnostic grammar. ErrorKind
    // contains no request, configuration, credential, or server error payload.
    let mut output = tokio::io::stdout();
    output
        .write_all(
            format!("Retained qualification: failure-stage={stage} kind={kind:?}\n").as_bytes(),
        )
        .await?;
    output.flush().await
}

async fn transport_failure(
    client: &mut RemoteAppServerClient,
    kind: io::ErrorKind,
) -> io::Result<()> {
    if kind != io::ErrorKind::InvalidData {
        return failure("typed-config-read", kind).await;
    }
    // These two terminal worker paths enqueue Disconnected before failing the
    // pending request. Inspect only their fixed context prefixes in the event's
    // existing String; never format the IO error or expose any message tail.
    let stage = tokio::time::timeout(Duration::from_secs(1), async {
        while let Some(event) = client.next_event().await {
            if let AppServerEvent::Disconnected { message } = event {
                if message.starts_with(
                    "remote app server at `native://retained-stream` sent invalid JSON-RPC: ",
                ) {
                    return Some("typed-config-read");
                }
                if message.starts_with(
                    "remote app server at `native://retained-stream` transport failed: ",
                ) {
                    // The bridge failed during the first application startup
                    // request, after WebSocket initialize but before any result.
                    return Some("bridge-startup");
                }
                return None;
            }
        }
        None
    })
    .await
    .ok()
    .flatten();
    // Missing/unknown context stays unclassified instead of being presented as
    // either known worker failure. The caller still returns its fixed error.
    if let Some(stage) = stage {
        failure(stage, kind).await?;
    }
    Ok(())
}

pub(crate) async fn run(arg0_paths: codex_arg0::Arg0DispatchPaths) -> io::Result<()> {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    let result = match arguments.as_slice() {
        [name] if name == BACKEND => run_backend(arg0_paths).await,
        [name] if name == FRONTEND => run_frontend(None).await,
        [name, option, image] if name == FRONTEND && option == "--replacement-image" => {
            run_frontend(Some(std::path::Path::new(image))).await
        }
        [name, option, state] if name == FRONTEND && option == "--observe-retained-work" => {
            pair::resume(state).await
        }
        _ => return Err(unavailable()),
    };
    // Config/auth errors can contain private values; expose only this fixed class.
    result.map_err(|_| unavailable())
}

async fn run_backend(arg0_paths: codex_arg0::Arg0DispatchPaths) -> io::Result<()> {
    let controller = codex_alab_tui::frontend_control::open().await?;
    let incoming = retained::bind_backend(controller).await?;
    // Independent physical controller binding precedes the credential seed.
    // run_main then constructs its real AuthManager through installed adapters.
    codex_alab_core::native_source_launch::enter().await?;
    let exit = codex_app_server::run_main_with_transport_options(
        arg0_paths,
        codex_utils_cli::CliConfigOverrides::default(),
        Default::default(),
        false,
        false,
        codex_app_server::AppServerTransport::Off,
        codex_protocol::protocol::SessionSource::VSCode,
        Default::default(),
        codex_app_server::AppServerRuntimeOptions {
            alab_incoming: Some(incoming),
            ..Default::default()
        },
    )
    .await?;
    if exit == codex_app_server::AppServerExit::Forced {
        return Err(unavailable());
    }
    Ok(())
}

#[path = "native_source_proof_retained_pair.rs"]
mod pair;

async fn run_frontend(replacement: Option<&std::path::Path>) -> io::Result<()> {
    // Match the existing native driver contract. This frontend constructs only
    // independent controllers; it never creates a credential reader/manager.
    let mut input = BufReader::new(tokio::io::stdin());
    let mut command = Vec::new();
    (&mut input)
        .take(64)
        .read_until(b'\n', &mut command)
        .await?;
    if command != b"RUN retained\n" {
        return Err(unavailable());
    }
    let first = codex_alab_tui::frontend_control::open().await?;
    phase("independent-control").await?;
    let (start, service) = start_service(&first).await?;
    if let Some(image) = replacement {
        return pair::replace(&first, service, image).await;
    }
    let first_config = request_config(&first, &service).await?;
    // Release the completed startup object as well as the first controller's
    // physical connection. Its accepted native backend operation is independent.
    drop(start);
    first.close().await?;
    drop(first);
    phase("withdraw-frontend").await?;
    let second = codex_alab_tui::frontend_control::open().await?;
    let second_config = request_config(&second, &service).await?;
    if first_config.config.model != second_config.config.model {
        return Err(unavailable());
    }
    second.close().await?;
    drop(second);
    pass().await
}

async fn start_service(
    controller: &codex_alab_aterm::frontend_control::Controller,
) -> io::Result<(retained::RetainedStart, String)> {
    let settings = StartupSettings::new(&[], &std::env::vars_os().collect::<Vec<_>>())?;
    let mut start = retained::prepare_start(controller, &operation(), settings).await?;
    phase("startup-settings").await?;
    let service = match start.observe().await {
        retained::ServiceObservation::Observed {
            service,
            phase: retained::RetainedPhase::Starting | retained::RetainedPhase::Spawned,
        } => service,
        _ => return Err(unavailable()),
    };
    Ok((start, service))
}

async fn pass() -> io::Result<()> {
    let mut output = tokio::io::stdout();
    output
        .write_all(
            b"PASS retained SDK startup, physical attachment and actual configuration response\n",
        )
        .await?;
    output.flush().await
}

async fn connect(
    controller: &codex_alab_aterm::frontend_control::Controller,
    service: &str,
) -> io::Result<(retained::RetainedAttachment, RemoteAppServerClient)> {
    let mut attachment = retained::prepare_attachment(controller, &operation(), service)?;
    // Native SDK construction Busy becomes member-waiting on the SAME request;
    // Transport.call reconciles that owned request within its existing deadline.
    if tokio::time::timeout(BOUND, attachment.observe())
        .await
        .map_err(|_| unavailable())?
        != retained::AttachmentObservation::Confirmed
    {
        return Err(unavailable());
    }
    let stream = attachment.take_stream()?;
    phase("physical-endpoint-received").await?;
    let client = RemoteAppServerClient::connect_native_stream(
        stream,
        RemoteAppServerConnectArgs {
            required_human_approval: false,
            // Display/context DATA only: connect_native_stream never connects to it.
            endpoint: RemoteAppServerEndpoint::UnixSocket {
                socket_path: "/native-retained-proof-unused".try_into()?,
            },
            client_name: "codex-alab-native-retained-proof".into(),
            client_version: "0.0.0".into(),
            experimental_api: true,
            mcp_server_openai_form_elicitation: false,
            opt_out_notification_methods: Vec::new(),
            channel_capacity: 64,
        },
    )
    .await?;
    Ok((attachment, client))
}

async fn request_config(
    controller: &codex_alab_aterm::frontend_control::Controller,
    service: &str,
) -> io::Result<ConfigReadResponse> {
    let (attachment, mut client) = connect(controller, service).await?;
    let result = read_config(&mut client).await?;
    if let Err(error) = client.shutdown().await {
        failure("bridge-startup", error.kind()).await?;
        return Err(unavailable());
    }
    drop(attachment);
    Ok(result)
}

async fn read_config(client: &mut RemoteAppServerClient) -> io::Result<ConfigReadResponse> {
    phase("actual-config-request").await?;
    let reply = client
        .request(ClientRequest::ConfigRead {
            request_id: RequestId::Integer(1),
            params: ConfigReadParams {
                include_layers: false,
                cwd: None,
            },
        })
        .await;
    let reply = match reply {
        Ok(Ok(reply)) => reply,
        Ok(Err(_)) => {
            failure("typed-config-read", io::ErrorKind::Other).await?;
            return Err(unavailable());
        }
        Err(error) => {
            transport_failure(client, error.kind()).await?;
            return Err(unavailable());
        }
    };
    // The actual successful JSON-RPC result crossed the shared client boundary.
    // Separate transport completion from the same typed-response deserializer
    // used by request_typed, without exposing or changing any response data.
    phase("shared-client-application-boundary").await?;
    let reply = match serde_json::from_value::<ConfigReadResponse>(reply) {
        Ok(reply) => reply,
        Err(_) => {
            failure("typed-config-read", io::ErrorKind::InvalidData).await?;
            return Err(unavailable());
        }
    };
    Ok(reply)
}
