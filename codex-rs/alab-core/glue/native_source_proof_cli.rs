// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Isolated debug qualification child. Native admission and all credential
//! operations use the normal installed adapters. This module supplies no host,
//! source, readiness stamp, credential, or provider endpoint.
use codex_alab_core::assignment::Activity;
use codex_alab_core::native_reader_retirement::Observer;
use codex_core::config::Config;
use codex_core::config::ConfigBuilder;
use codex_login::AuthManager;
use codex_login::CodexAuth;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;
use std::io;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncBufReadExt as _;
use tokio::io::AsyncWriteExt as _;

// Explicit feature AND debug_assertions are required by the mounting CLI.
// The normal production image does not compile or advertise this marker.
const SOURCE_V1_MARK: &str = "aterm session credential source v1";
const CANDIDATE_MARK: &str = "aterm isolated source proof candidate v1";
const MAX_COMMAND: usize = 4096;
const MAX_REQUESTS: usize = 64;
const STEP_BOUND: Duration = Duration::from_secs(120);

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Action {
    Startup,
    Contextual,
    Resample,
    Derived,
    Workrebind,
    Stop,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Command {
    schema: u32,
    request: String,
    action: Action,
}
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum Reason {
    Unavailable,
    Timeout,
    RequestChanged,
}
#[derive(Clone, Serialize)]
struct ActualState {
    auth: u64,
    owner: u64,
    source: u64,
    login: u64,
}
#[derive(Clone, Serialize)]
struct Reply {
    schema: u32,
    request: String,
    action: Action,
    outcome: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<Reason>,
    #[serde(rename = "authState", skip_serializing_if = "Option::is_none")]
    auth_state: Option<ActualState>,
}
#[derive(Default)]
struct Proof {
    config: Option<Config>,
    manager: Option<Arc<AuthManager>>,
    activity: Option<Activity>,
    snapshots: Vec<CodexAuth>,
    derived: Option<Arc<AuthManager>>,
    observers: Vec<Observer>,
}
fn unavailable() -> io::Error {
    io::Error::other("native source proof unavailable")
}
fn actual_state(manager: &AuthManager) -> ActualState {
    let state = *manager.auth_change_state_receiver().borrow();
    ActualState {
        auth: state.generation,
        owner: state.owner_generation,
        source: state.source_generation,
        login: state.login_generation,
    }
}
fn prove_snapshot(auth: &CodexAuth) -> io::Result<()> {
    let origin = auth.alab_credential_epoch();
    if !origin.has_native_member() || !origin.native_ready() {
        return Err(unavailable());
    }
    Ok(())
}
impl Proof {
    async fn step(&mut self, action: Action) -> Result<Option<ActualState>, Reason> {
        self.step_inner(action)
            .await
            .map_err(|_| Reason::Unavailable)
    }
    async fn step_inner(&mut self, action: Action) -> io::Result<Option<ActualState>> {
        match action {
            Action::Startup => {
                if self.manager.is_some() {
                    return Err(unavailable());
                }
                let config = ConfigBuilder::default()
                    .build()
                    .await
                    .map_err(|_| unavailable())?;
                let manager = AuthManager::shared_from_config(&config, false)
                    .await
                    .map_err(|_| unavailable())?;
                self.observers.push(manager.alab_debug_native_retirement()?);
                let auth = manager.auth_cached().ok_or_else(unavailable)?;
                prove_snapshot(&auth)?;
                let state = actual_state(&manager);
                self.snapshots.push(auth);
                self.manager = Some(manager);
                self.config = Some(config);
                Ok(Some(state))
            }
            Action::Contextual => {
                if self.activity.is_some() {
                    return Err(unavailable());
                }
                let manager = self.manager.as_ref().ok_or_else(unavailable)?.clone();
                let config = self.config.as_ref().ok_or_else(unavailable)?;
                let activity = manager
                    .alab_assignment_domain()
                    .admit_work(None)
                    .map_err(|_| unavailable())?;
                // Retain the real root beyond this observer's command. Its
                // native use covers subsequent refresh and derived construction.
                self.activity = Some(activity.clone());
                let (reader, snapshot) = activity
                    .scope(async {
                        let snapshot = manager.auth_cached().ok_or_else(unavailable)?;
                        prove_snapshot(&snapshot)?;
                        let factory = snapshot
                            .alab_bind_http_client_factory(manager.http_client_factory())?;
                        let url = "https://api.openai.com/"
                            .parse()
                            .map_err(|_| unavailable())?;
                        let permit = factory
                            .network_policy()
                            .acquire(&url)
                            .map_err(|_| unavailable())?;
                        permit
                            .run(async { snapshot.get_token().map(|_| ()) })
                            .await
                            .map_err(|_| unavailable())?
                            .map_err(|_| unavailable())?;
                        // Both the direct Reader and actual parent Manager must
                        // retain their independent permissions on this transport.
                        let reader = config
                            .auth_config()
                            .load_auth(false)
                            .await?
                            .ok_or_else(unavailable)?;
                        prove_snapshot(&reader)?;
                        let factory =
                            reader.alab_bind_http_client_factory(manager.http_client_factory())?;
                        let permit = factory
                            .network_policy()
                            .acquire(&url)
                            .map_err(|_| unavailable())?;
                        permit
                            .run(async { reader.get_token().map(|_| ()) })
                            .await
                            .map_err(|_| unavailable())?
                            .map_err(|_| unavailable())?;
                        Ok::<_, io::Error>((reader, snapshot))
                    })
                    .await?;
                self.observers.push(reader.alab_debug_native_retirement()?);
                self.snapshots.extend([reader, snapshot]);
                Ok(Some(actual_state(&manager)))
            }
            Action::Resample | Action::Workrebind => {
                let manager = self.manager.as_ref().ok_or_else(unavailable)?.clone();
                if action == Action::Workrebind {
                    manager.alab_debug_close_native_work()?;
                }
                if action == Action::Workrebind {
                    // Ordinary auth recovery must not force a provider refresh
                    // when the exact retained credential is still current.
                    match self.activity.as_ref() {
                        Some(activity) => activity.clone().scope(manager.auth()).await,
                        None => manager.auth().await,
                    }
                    .ok_or_else(unavailable)?;
                } else {
                    // The native issuer controls actual same-grant refresh.
                    let activity = self.activity.as_ref().ok_or_else(unavailable)?.clone();
                    activity
                        .scope(manager.refresh_token_from_authority())
                        .await
                        .map_err(|_| unavailable())?;
                }
                prove_snapshot(&manager.auth_cached().ok_or_else(unavailable)?)?;
                Ok(Some(actual_state(&manager)))
            }
            Action::Derived => {
                if self.derived.is_some() {
                    return Err(unavailable());
                }
                let config = self.config.as_ref().ok_or_else(unavailable)?;
                let activity = self.activity.as_ref().ok_or_else(unavailable)?.clone();
                let derived = activity
                    .scope(AuthManager::shared_from_auth_config(
                        config.auth_config(),
                        false,
                    ))
                    .await
                    .map_err(|_| unavailable())?;
                self.observers.push(derived.alab_debug_native_retirement()?);
                prove_snapshot(&derived.auth_cached().ok_or_else(unavailable)?)?;
                let state = actual_state(&derived);
                self.derived = Some(derived);
                Ok(Some(state))
            }
            Action::Stop => {
                if self.observers.is_empty() {
                    return Err(unavailable());
                }
                self.snapshots.clear();
                self.derived.take();
                self.activity.take();
                self.manager.take();
                self.config.take();
                // Observer metadata retains no reader/manager/use ownership.
                // EOF and a successful local Drop are never Closed receipts.
                for observer in &self.observers {
                    observer.closed().await?;
                }
                self.observers.clear();
                Ok(None)
            }
        }
    }
}

pub(crate) fn requested() -> bool {
    std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == "alab-native-source-proof")
}
fn parse(line: &[u8]) -> io::Result<Command> {
    if line.len() > MAX_COMMAND {
        return Err(unavailable());
    }
    let command: Command = serde_json::from_slice(line).map_err(|_| unavailable())?;
    if command.schema != 1 || !codex_alab_core::managed_auth::valid_hex(&command.request, 32) {
        return Err(unavailable());
    }
    Ok(command)
}
async fn read_command(
    reader: &mut tokio::io::BufReader<tokio::io::Stdin>,
) -> io::Result<Option<Command>> {
    let mut line = Vec::new();
    loop {
        let buffer = reader.fill_buf().await?;
        if buffer.is_empty() {
            return if line.is_empty() {
                Ok(None)
            } else {
                Err(unavailable())
            };
        }
        let end = buffer
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|end| end + 1);
        let count = end.unwrap_or(buffer.len());
        if line.len() + count > MAX_COMMAND {
            return Err(unavailable());
        }
        line.extend_from_slice(&buffer[..count]);
        reader.consume(count);
        if end.is_some() {
            return parse(&line).map(Some);
        }
    }
}
pub(crate) async fn run() -> io::Result<()> {
    // Keep the opt-in candidate's real capability bytes in its executable. No
    // environment variable, config setting or native image override enables it.
    std::hint::black_box(SOURCE_V1_MARK);
    std::hint::black_box(CANDIDATE_MARK);
    if std::env::args_os().count() != 2 || !codex_alab_core::native_source_launch::requested() {
        return Err(unavailable());
    }
    let mut proof = Proof::default();
    let mut seen = BTreeMap::<String, (Action, Reply)>::new();
    let mut input = tokio::io::BufReader::new(tokio::io::stdin());
    let mut output = tokio::io::stdout();
    while let Some(command) = read_command(&mut input).await? {
        let reply = if let Some((action, reply)) = seen.get(&command.request) {
            if *action == command.action {
                reply.clone()
            } else {
                Reply {
                    schema: 1,
                    request: command.request.clone(),
                    action: command.action,
                    outcome: "refused",
                    reason: Some(Reason::RequestChanged),
                    auth_state: None,
                }
            }
        } else {
            if seen.len() >= MAX_REQUESTS {
                return Err(unavailable());
            }
            let result = tokio::time::timeout(STEP_BOUND, proof.step(command.action)).await;
            let (outcome, reason, auth_state) = match result {
                Ok(Ok(state)) => ("ok", None, state),
                Ok(Err(reason)) => ("refused", Some(reason), None),
                Err(_) => ("refused", Some(Reason::Timeout), None),
            };
            let reply = Reply {
                schema: 1,
                request: command.request.clone(),
                action: command.action,
                outcome,
                reason,
                auth_state,
            };
            seen.insert(command.request.clone(), (command.action, reply.clone()));
            reply
        };
        let mut bytes = serde_json::to_vec(&reply).map_err(|_| unavailable())?;
        bytes.push(b'\n');
        output.write_all(&bytes).await?;
        output.flush().await?;
        if command.action == Action::Stop {
            return if reply.outcome == "ok" {
                Ok(())
            } else {
                Err(unavailable())
            };
        }
    }
    Err(unavailable())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn proof_protocol_is_closed_bounded_and_never_echoes_unvalidated_data() {
        let good =
            br#"{"schema":1,"request":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","action":"startup"}"#;
        assert!(parse(good).is_ok());
        for bad in [
            br#"{"schema":1,"request":"secret","action":"startup"}"#.as_slice(),
            br#"{"schema":1,"request":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","action":"ready","auth":7}"#.as_slice(),
            br#"{"schema":1,"request":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","action":"startup","token":"secret"}"#.as_slice(),
        ] {
            assert_eq!(parse(bad).err().unwrap().to_string(), "native source proof unavailable");
        }
        assert!(parse(&vec![b' '; MAX_COMMAND + 1]).is_err());
    }
}
