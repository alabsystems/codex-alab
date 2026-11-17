// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Real app-server library in the explicit debug-only SDK test artifact.
//! No marker or argument grants admission: private startup must bind its actual
//! native constructor and socket before this fixture can configure a server.
use std::io;

pub(crate) fn requested() -> bool {
    std::env::args_os()
        .nth(1)
        .is_some_and(|argument| argument == "--alab-owned-backend")
}

pub(crate) async fn run(arg0_paths: codex_arg0::Arg0DispatchPaths) -> io::Result<()> {
    let _owner = codex_alab_core::native_backend_startup::enter().await?;
    let arguments = codex_alab_core::native_backend_startup::arguments()?;
    let mut tail = &arguments[1..];
    let mut overrides = Vec::new();
    while tail.first().is_some_and(|argument| argument == "--config") {
        let value = tail
            .get(1)
            .and_then(|value| value.to_str())
            .ok_or_else(codex_alab_core::managed_auth::unavailable)?;
        overrides.push(value.to_owned());
        tail = &tail[2..];
    }
    // enter() already validated the entire fixed private dispatch shape.
    let digest = tail
        .last()
        .and_then(|value| value.to_str())
        .ok_or_else(codex_alab_core::managed_auth::unavailable)?;
    let mut bytes = [0_u8; 32];
    for (pair, byte) in digest.as_bytes().as_chunks::<2>().0.iter().zip(&mut bytes) {
        let text = std::str::from_utf8(pair).map_err(io::Error::other)?;
        *byte = u8::from_str_radix(text, 16).map_err(io::Error::other)?;
    }
    let auth = codex_websocket_auth::WebsocketAuthSettings {
        config: Some(codex_websocket_auth::WebsocketAuthConfig::CapabilityToken {
            source: codex_websocket_auth::WebsocketCapabilityTokenSource::TokenSha256 {
                token_sha256: bytes,
            },
        }),
    };
    let exit = codex_app_server::run_main_with_transport_options(
        arg0_paths,
        codex_utils_cli::CliConfigOverrides {
            raw_overrides: overrides,
        },
        Default::default(),
        false,
        false,
        codex_app_server::AppServerTransport::WebSocket {
            bind_address: "127.0.0.1:0".parse().map_err(io::Error::other)?,
        },
        codex_protocol::protocol::SessionSource::VSCode,
        auth,
        Default::default(),
    )
    .await
    .inspect_err(|_| eprintln!("Native backend startup: app server failed"))?;
    if exit == codex_app_server::AppServerExit::Forced {
        return Err(io::Error::other(
            "qualification app-server cleanup was forced",
        ));
    }
    Ok(())
}
