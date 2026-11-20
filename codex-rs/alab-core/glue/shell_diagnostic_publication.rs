// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::ShellLog;
use crate::alab_shell_log_store;
use codex_alab_core::shell_log_store::PublicationLock;
use codex_protocol::ThreadId;
use codex_state::ShellDiagnosticPublication;
use codex_state::StateRuntime;
use std::collections::HashSet;
use std::io;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

/// Hold the directory's OS lock from preparation through the SQLite commit. A
/// dropped future releases that lock. The durable intent then owns reconciliation.
pub(super) async fn persist(
    db: &StateRuntime,
    thread: ThreadId,
    number: i32,
    code: i32,
    log: &ShellLog,
) -> anyhow::Result<bool> {
    let prepared = loop {
        let capture = log.clone();
        match tokio::task::spawn_blocking(move || capture.prepare_diagnostic()).await? {
            Ok(Some(prepared)) => break prepared,
            Ok(None) => return Ok(false),
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            Err(err) => return Err(err.into()),
        }
    };
    let intent = ShellDiagnosticPublication {
        thread_id: thread,
        invocation: log.invocation_id(),
        session_id: number,
        exit_code: code,
        capture_name: prepared.name.clone(),
        expected_bytes: prepared.expected_bytes,
        prefix_sha256: prepared.prefix_sha256.clone(),
        partial: prepared.partial,
        tail: prepared.tail.clone(),
    };
    if !db.reserve_shell_diagnostic_publication(&intent).await? {
        prepared.cleanup(false)?;
        return Ok(false);
    }
    let (prepared, valid) = tokio::task::spawn_blocking(move || {
        let verified = prepared.publish();
        (prepared, verified)
    })
    .await?;
    let attached = settle(db, &intent, valid?).await?;
    prepared.cleanup(attached.preserve_file)?;
    Ok(attached.collected)
}

struct Attachment {
    collected: bool,
    preserve_file: bool,
}

/// A missing reservation or evicted invocation cannot establish that a final
/// file is ours to remove. Preserve the final pathname in an ambiguous case.
async fn settle(
    db: &StateRuntime,
    intent: &ShellDiagnosticPublication,
    valid: bool,
) -> anyhow::Result<Attachment> {
    if !valid {
        let preserve_file = db
            .reject_shell_diagnostic_publication(intent)
            .await?
            .unwrap_or(true);
        return Ok(Attachment {
            collected: false,
            preserve_file,
        });
    }
    let collected = db.complete_shell_diagnostic_publication(intent).await?;
    if collected {
        return Ok(Attachment {
            collected,
            preserve_file: true,
        });
    }
    let original = db
        .shell_invocation(intent.thread_id, intent.invocation)
        .await?;
    let preserve_file = original
        .is_none_or(|row| row.capture_name.as_deref() == Some(intent.capture_name.as_str()));
    Ok(Attachment {
        collected,
        preserve_file,
    })
}

async fn recover(
    db: &StateRuntime,
    lease: &Arc<PublicationLock>,
    intent: &ShellDiagnosticPublication,
) -> anyhow::Result<()> {
    let held = Arc::clone(lease);
    let name = intent.capture_name.clone();
    let expected = intent.expected_bytes;
    let digest = intent.prefix_sha256.clone();
    let valid =
        tokio::task::spawn_blocking(move || held.publish(&name, expected, &digest)).await??;
    let attachment = settle(db, intent, valid).await?;
    lease.cleanup(&intent.capture_name, attachment.preserve_file)?;
    Ok(())
}

pub(crate) async fn reconcile_diagnostics(
    db: &StateRuntime,
    home: &Path,
    thread: ThreadId,
    eligible: Option<&HashSet<Uuid>>,
) -> anyhow::Result<HashSet<Uuid>> {
    let before = db.pending_shell_diagnostic_publications(thread).await?;
    let mut unfinished: HashSet<_> = before
        .iter()
        .map(|intent| intent.invocation)
        .filter(|id| eligible.is_none_or(|ids| ids.contains(id)))
        .collect();
    let need_directory = !before.is_empty();
    let home = home.to_owned();
    let directory = tokio::task::spawn_blocking(move || {
        let directory = if need_directory {
            Some(alab_shell_log_store::directory(&home, thread)?)
        } else {
            alab_shell_log_store::existing_directory(&home, thread)?
        };
        match directory {
            Some(directory) => directory.try_publication_lock(),
            None => Ok(None),
        }
    })
    .await?;
    let lease = match directory {
        Ok(Some(lease)) => Arc::new(lease),
        Ok(None) => return Ok(unfinished),
        Err(err) => {
            tracing::warn!("could not open the shell diagnostic publication directory: {err}");
            return Ok(unfinished);
        }
    };
    // Refresh under the publisher lock before reclaiming any private stage. A
    // producer might have committed a new intent since our first read.
    let current = db.pending_shell_diagnostic_publications(thread).await?;
    let names: Vec<_> = current
        .iter()
        .map(|intent| intent.capture_name.clone())
        .collect();
    let sweep = Arc::clone(&lease);
    if let Err(err) = tokio::task::spawn_blocking(move || sweep.clean_orphan_stages(&names)).await?
    {
        tracing::warn!("could not retire unreferenced diagnostic staging files: {err}");
    }
    unfinished.clear();
    for intent in current {
        if eligible.is_none_or(|ids| ids.contains(&intent.invocation))
            && let Err(err) = recover(db, &lease, &intent).await
        {
            tracing::warn!(
                "could not recover shell diagnostic for {}: {err:#}",
                intent.invocation
            );
            unfinished.insert(intent.invocation);
        }
    }
    Ok(unfinished)
}
