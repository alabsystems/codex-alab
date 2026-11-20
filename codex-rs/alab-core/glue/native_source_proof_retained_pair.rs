// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Two-image component for the native lab's authenticated retained fixture. The
//! host independently leases both images and verifies both real SDK backends.
//! Only public observation data crosses exec. Every attachment and service
//! constructor must be admitted again by the actual host.
use super::*;
use codex_alab_aterm::frontend_control::Controller;
use codex_app_server_protocol::CommandExecutionSource;
use codex_app_server_protocol::CommandExecutionStatus;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadItemPayload;
use codex_app_server_protocol::ThreadLoadedListParams;
use codex_app_server_protocol::ThreadLoadedListResponse;
use codex_app_server_protocol::ThreadResumeParams;
use codex_app_server_protocol::ThreadResumeResponse;
use codex_app_server_protocol::ThreadShellCommandParams;
use codex_app_server_protocol::ThreadShellCommandResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use serde::Deserialize;
use serde::Serialize;
use std::ffi::OsStr;
use std::fs;
use std::os::unix::fs::DirBuilderExt as _;
use std::os::unix::fs::MetadataExt as _;
use std::os::unix::process::CommandExt as _;
use std::path::Path;
use std::path::PathBuf;

// Static shell operations in the fixture's private workspace. Only the
// replacement releases the original work, after its own Core finishes first.
const ORIGINAL: &str = "printf 'started\\n' >> started; i=0; while test ! -f release && test \"$i\" -lt 1000; do /bin/sleep 0.1; i=$((i+1)); done; test -f release || exit 74; printf 'original completed\\n'; printf 'done\\n' >> done";
const NEXT: &str = "printf 'replacement completed\\n'";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Observation {
    pid: u32,
    image_device: u64,
    image_inode: u64,
    workspace_nonce: String,
    service: String,
    thread: String,
    turn: String,
    item: String,
}

fn workspace(nonce: &str) -> io::Result<PathBuf> {
    if !codex_alab_core::managed_auth::valid_hex(nonce, 32) {
        return Err(unavailable());
    }
    Ok(std::env::temp_dir()
        .canonicalize()?
        .join(format!("codex-retained-{nonce}")))
}

async fn start_thread(client: &RemoteAppServerClient, dir: &Path) -> io::Result<String> {
    let reply: ThreadStartResponse = client
        .request_typed(ClientRequest::ThreadStart {
            request_id: RequestId::Integer(2),
            params: ThreadStartParams {
                cwd: Some(dir.to_str().ok_or_else(unavailable)?.to_owned()),
                ..Default::default()
            },
        })
        .await
        .map_err(|_| unavailable())?;
    Ok(reply.thread.id)
}

async fn shell(client: &RemoteAppServerClient, thread: &str, command: &str) -> io::Result<()> {
    let _: ThreadShellCommandResponse = client
        .request_typed(ClientRequest::ThreadShellCommand {
            request_id: RequestId::Integer(3),
            params: ThreadShellCommandParams {
                thread_id: thread.to_owned(),
                command: command.to_owned(),
                timeout_ms: Some(110_000),
            },
        })
        .await
        .map_err(|_| unavailable())?;
    Ok(())
}

async fn command_event(
    client: &mut RemoteAppServerClient,
    thread: &str,
    output: Option<&str>,
) -> io::Result<(String, String)> {
    tokio::time::timeout(BOUND, async {
        while let Some(event) = client.next_event().await {
            let AppServerEvent::ServerNotification(notice) = event else {
                continue;
            };
            let (owner, turn, item) = match (*notice, output) {
                (ServerNotification::ItemStarted(event), None) => {
                    (event.thread_id, event.turn_id, event.item)
                }
                (ServerNotification::ItemCompleted(event), Some(_)) => {
                    (event.thread_id, event.turn_id, event.item)
                }
                _ => continue,
            };
            if owner == thread
                && let ThreadItemPayload::CommandExecution {
                    id,
                    source: CommandExecutionSource::UserShell,
                    status,
                    aggregated_output,
                    exit_code,
                    ..
                } = item.payload
            {
                match output {
                    None if status == CommandExecutionStatus::InProgress => return Ok((turn, id)),
                    Some(text)
                        if status == CommandExecutionStatus::Completed
                            && exit_code == Some(0)
                            && aggregated_output
                                .as_deref()
                                .is_some_and(|value| value.trim_end() == text) =>
                    {
                        return Ok((turn, id));
                    }
                    _ => return Err(unavailable()),
                }
            }
        }
        Err(unavailable())
    })
    .await
    .map_err(|_| unavailable())?
}

async fn loaded(client: &RemoteAppServerClient, tid: &str) -> io::Result<bool> {
    let response: ThreadLoadedListResponse = client
        .request_typed(ClientRequest::ThreadLoadedList {
            request_id: RequestId::Integer(4),
            params: ThreadLoadedListParams::default(),
        })
        .await
        .map_err(|_| unavailable())?;
    if response.next_cursor.is_some() {
        return Err(unavailable());
    }
    Ok(response.data.iter().any(|id| id == tid))
}

pub(super) async fn replace(
    controller: &Controller,
    service: String,
    image: &Path,
) -> io::Result<()> {
    // The lab alone places the selected images at these two fixed sibling paths.
    let source = std::env::current_exe()?.canonicalize()?;
    let world = source.ancestors().nth(3).ok_or_else(unavailable)?;
    let image = image.canonicalize()?;
    if source != world.join("candidate/bin/codex") || image != world.join("replacement/bin/codex") {
        return Err(unavailable());
    }
    let selected = image.metadata()?;
    let original = source.metadata()?;
    if (original.ino(), original.dev()) == (selected.ino(), selected.dev()) {
        return Err(unavailable());
    }
    let workspace_nonce = operation();
    let dir = workspace(&workspace_nonce)?;
    fs::DirBuilder::new().mode(0o700).create(&dir)?;
    let (_attachment, mut prior) = connect(controller, &service).await?;
    read_config(&mut prior).await?;
    let thread = start_thread(&prior, &dir).await?;
    shell(&prior, &thread, ORIGINAL).await?;
    let (turn, item) = command_event(&mut prior, &thread, None).await?;
    tokio::time::timeout(BOUND, async {
        loop {
            match fs::read(dir.join("started")) {
                Ok(bytes) if bytes == b"started\n" => return Ok(()),
                Ok(bytes) if bytes.is_empty() => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                _ => return Err(unavailable()),
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .map_err(|_| unavailable())??;
    if dir.join("done").try_exists()? {
        return Err(unavailable());
    }
    let observation = Observation {
        pid: std::process::id(),
        image_device: selected.dev(),
        image_inode: selected.ino(),
        workspace_nonce,
        service,
        thread,
        turn,
        item,
    };
    let data = serde_json::to_string(&observation).map_err(|_| unavailable())?;
    // The original Core still owns the shell. Its terminal result arrives
    // on the same thread after reconnect.
    phase("withdraw-frontend").await?;
    Err(std::process::Command::new(image)
        .args([FRONTEND, "--observe-retained-work", &data])
        .exec())
}

pub(super) async fn resume(raw: &OsStr) -> io::Result<()> {
    let encoded = raw
        .to_str()
        .filter(|s| s.len() <= 1024)
        .ok_or_else(unavailable)?;
    let state: Observation = serde_json::from_str(encoded).map_err(|_| unavailable())?;
    let current = std::env::current_exe()?.canonicalize()?;
    if !current.ends_with("replacement/bin/codex") {
        return Err(unavailable());
    }
    let md = current.metadata()?;
    if state.pid != std::process::id()
        || (state.image_device, state.image_inode) != (md.dev(), md.ino())
    {
        return Err(unavailable());
    }
    let controller = codex_alab_tui::frontend_control::open().await?;
    let dir = workspace(&state.workspace_nonce)?;
    let md = dir.symlink_metadata()?;
    // SAFETY: geteuid only reads the process identity.
    let uid = unsafe { libc::geteuid() };
    if !md.is_dir()
        || md.mode() & 0o077 != 0
        || md.uid() != uid
        || fs::read(dir.join("started"))? != b"started\n"
        || dir.join("done").try_exists()?
    {
        return Err(unavailable());
    }
    phase("after-frontend-exec").await?;
    let (_old_attachment, mut old) = connect(&controller, &state.service).await?;
    if !loaded(&old, &state.thread).await? {
        return Err(unavailable());
    }
    let joined: ThreadResumeResponse = old
        .request_typed(ClientRequest::ThreadResume {
            request_id: RequestId::Integer(5),
            params: ThreadResumeParams {
                thread_id: state.thread.clone(),
                ..Default::default()
            },
        })
        .await
        .map_err(|_| unavailable())?;
    if joined.thread.id != state.thread {
        return Err(unavailable());
    }
    phase("new-core-startup").await?;
    let (_start, service) = start_service(&controller).await?;
    if service == state.service {
        return Err(unavailable());
    }
    let (_new_attachment, mut new) = connect(&controller, &service).await?;
    if loaded(&new, &state.thread).await? {
        return Err(unavailable());
    }
    let next = start_thread(&new, &dir).await?;
    if next == state.thread || loaded(&old, &next).await? {
        return Err(unavailable());
    }
    shell(&new, &next, NEXT).await?;
    command_event(&mut new, &next, Some("replacement completed")).await?;
    phase("new-core-work-completed").await?;
    if dir.join("done").try_exists()? {
        return Err(unavailable());
    }
    fs::File::create_new(dir.join("release"))?;
    let event = command_event(&mut old, &state.thread, Some("original completed")).await?;
    if event != (state.turn, state.item)
        || fs::read(dir.join("started"))? != b"started\n"
        || fs::read(dir.join("done"))? != b"done\n"
    {
        return Err(unavailable());
    }
    new.shutdown().await?;
    old.shutdown().await?;
    controller.close().await?;
    for name in ["started", "release", "done"] {
        fs::remove_file(dir.join(name))?;
    }
    fs::remove_dir(dir)?;
    pass().await
}
