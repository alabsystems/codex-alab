// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Private backend startup through the original native frontend controller.
use super::*;
use codex_alab_core::native_root::TransportSecret;
use std::net::IpAddr;
use std::net::Ipv4Addr;
use std::net::SocketAddr;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Frame<T> {
    schema: u32,
    request: String,
    operation: String,
    session: crate::accounts::Session,
    member: String,
    backend: T,
}
impl<T> Frame<T> {
    fn validate(&self, controller: &Controller, operation: &str) -> io::Result<()> {
        if self.schema != 1
            || !valid_hex(&self.request, 32)
            || self.operation != operation
            || self.member != controller.member
            || self.session != controller.session
        {
            return Err(unavailable());
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
enum State {
    Starting,
    Ready {
        endpoint: String,
        credential: String,
    },
}

/// RPC transport data returned on this controller's original connection. This
/// is neither a replacement-frontend handoff nor native tool birth permission.
pub struct Backend {
    address: SocketAddr,
    credential: TransportSecret,
}
impl Backend {
    pub fn address(&self) -> SocketAddr {
        self.address
    }
    pub fn credential(&self) -> &str {
        &self.credential
    }
}
impl std::fmt::Debug for Backend {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("NativeBackend(<private>)")
    }
}

fn ready(state: State) -> io::Result<Option<Backend>> {
    match state {
        State::Starting => Ok(None),
        State::Ready {
            endpoint,
            credential,
        } => {
            let credential = TransportSecret::new(credential);
            let address: SocketAddr = endpoint
                .strip_prefix("ws://")
                .ok_or_else(unavailable)?
                .parse()
                .map_err(|_| unavailable())?;
            if address.ip() != IpAddr::V4(Ipv4Addr::LOCALHOST)
                || address.port() == 0
                || !valid_hex(&credential, 64)
            {
                return Err(unavailable());
            }
            Ok(Some(Backend {
                address,
                credential,
            }))
        }
    }
}

pub(super) async fn start(
    controller: &Controller,
    operation: &str,
    overrides: &[String],
) -> io::Result<Backend> {
    if !valid_hex(operation, 32) || overrides.len() > 128 {
        return Err(unavailable());
    }
    let payload = serde_json::to_vec(overrides).map_err(|_| unavailable())?;
    let configuration: String = payload.iter().map(|byte| format!("{byte:02x}")).collect();
    let fields = format!(
        " member={} controller_operation={} configuration={configuration} environment_count={}",
        controller.member, controller.operation, controller.environment_count
    );
    // Leave room for the authenticated request envelope in the native frame.
    if fields.len() > 15 * 1024 {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    loop {
        let body = TransportSecret::new(
            controller
                .transport
                .call(
                    "controller-root-start",
                    operation,
                    &fields,
                    Some(&controller.member),
                )
                .await?,
        );
        let frame: Frame<State> = serde_json::from_str(&body).map_err(|_| unavailable())?;
        frame.validate(controller, operation)?;
        if let Some(backend) = ready(frame.backend)? {
            return Ok(backend);
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}

#[derive(Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
enum StopState {
    Stopping,
    Stopped { receipt_scope: String },
}

fn stopped(state: StopState) -> io::Result<bool> {
    match state {
        StopState::Stopping => Ok(false),
        StopState::Stopped { receipt_scope } if receipt_scope == "captured-native-root" => Ok(true),
        _ => Err(unavailable()),
    }
}

pub(super) async fn stop(controller: &Controller) -> io::Result<()> {
    let operation = format!(
        "{:016x}{:016x}",
        codex_alab_core::native_source::connection_request_prefix(),
        codex_alab_core::native_source::connection_request_prefix()
    );
    let fields = format!(
        " member={} controller_operation={}",
        controller.member, controller.operation
    );
    loop {
        let body = controller
            .transport
            .call(
                "controller-root-stop",
                &operation,
                &fields,
                Some(&controller.member),
            )
            .await?;
        let frame: Frame<StopState> = serde_json::from_str(&body).map_err(|_| unavailable())?;
        frame.validate(controller, &operation)?;
        if stopped(frame.backend)? {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}

#[cfg(test)]
#[path = "frontend_backend_tests.rs"]
mod tests;
