// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use crate::StateDbHandle;
use crate::client_common::Prompt;
use crate::session::session::Session;
use crate::unified_exec::shell_observation::reconcile_diagnostics;
use crate::unified_exec::shell_observation::resume_exit_text;
use codex_context_fragments::AnnotatedContent;
use codex_context_fragments::message_from_parts;
use codex_extension_api::ExtensionData;
use codex_protocol::ThreadId;
use codex_protocol::models::ContentItemKind;
use codex_protocol::models::ContentItemMetadata;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::SessionSource;
use codex_state::PendingShellExit;
use std::collections::HashSet;
use std::path::Path;
use tokio::sync::Mutex;
use tokio::sync::OwnedMutexGuard;
use uuid::Uuid;

const MAX_PER_REQUEST: usize = 8;

/// Membership freezes on the first real sampling attempt. Retrying a failed
/// request can refresh the exit code or discard an invocation collected meanwhile.
#[derive(Default)]
struct ResumeBacklog(Option<HashSet<Uuid>>);
type ResumeWindow = Mutex<ResumeBacklog>;

impl ResumeBacklog {
    fn batch(
        &mut self,
        mut pending: Vec<PendingShellExit>,
        unfinished: &HashSet<Uuid>,
    ) -> Vec<PendingShellExit> {
        let eligible = self.0.get_or_insert_with(|| {
            pending
                .iter()
                .map(|exit| exit.invocation)
                .chain(unfinished.iter().copied())
                .collect()
        });
        // Its earlier path may no longer contain the bytes being attached. Only
        // this invocation waits for verification. Other original exits can pass.
        pending.retain(|exit| {
            eligible.contains(&exit.invocation) && !unfinished.contains(&exit.invocation)
        });
        if pending.is_empty() {
            eligible.retain(|id| unfinished.contains(id));
        }
        pending.truncate(MAX_PER_REQUEST);
        pending
    }
}

pub(crate) fn arm(store: &ExtensionData, db: Option<&StateDbHandle>, source: &SessionSource) {
    // Session construction supplies a state DB only for a local, durable thread.
    if codex_alab_core::active()
        && db.is_some()
        && !crate::guardian::is_basic_session_source(source)
    {
        store.insert(ResumeWindow::default());
    }
}

pub(crate) struct SubmittedExits {
    db: StateDbHandle,
    thread: ThreadId,
    exits: Vec<PendingShellExit>,
    disclosed_paths: HashSet<Uuid>,
    // Simultaneous attempts on this thread must not submit the same backlog.
    _window: OwnedMutexGuard<ResumeBacklog>,
}

pub(crate) async fn prepare(
    session: &Session,
    home: &Path,
    prompt: &mut Prompt,
) -> Option<SubmittedExits> {
    let epoch = codex_alab_core::mode_epoch()?;
    let store = &session.services.thread_extension_data;
    let window = store.get::<ResumeWindow>()?;
    let db = session.state_db()?;
    let mut backlog = window.lock_owned().await;
    if backlog.0.as_ref().is_some_and(HashSet::is_empty) {
        return None;
    }
    let unfinished =
        match reconcile_diagnostics(&db, home, session.thread_id, backlog.0.as_ref()).await {
            Ok(unfinished) => unfinished,
            Err(err) => {
                tracing::warn!("could not reconcile retained shell output on resume: {err}");
                return None;
            }
        };
    let pending = match db.pending_shell_exits(session.thread_id).await {
        Ok(exits) => backlog.batch(exits, &unfinished),
        Err(err) => {
            tracing::warn!("could not read uncollected shell exits on resume: {err}");
            return None;
        }
    };
    if pending.is_empty() {
        if backlog.0.as_ref().is_some_and(HashSet::is_empty) {
            let _ = store.remove::<ResumeWindow>();
        }
        return None;
    }
    if codex_alab_core::mode_epoch() != Some(epoch) {
        return None;
    }
    let (text, disclosed_paths) = resume_exit_text(home, session.thread_id, &pending);
    let item = message_from_parts(
        "user",
        vec![AnnotatedContent::text(
            text,
            ContentItemKind("observed_shell_exits".into()),
            ContentItemMetadata::harness(),
        )],
    );
    // When the final item is the new user request, keep that instruction last.
    let index = prompt.input.len()
        - usize::from(matches!(
            prompt.input.last(), Some(ResponseItem::Message { role, .. }) if role == "user"
        ));
    prompt.input.insert(index, item);
    Some(SubmittedExits {
        db,
        thread: session.thread_id,
        exits: pending,
        disclosed_paths,
        _window: backlog,
    })
}

pub(crate) async fn accept(batch: &mut Option<SubmittedExits>, resampling: bool) {
    if !resampling
        && let Some(batch) = batch.take()
        && let Err(err) = batch
            .db
            .acknowledge_shell_exits(batch.thread, &batch.exits, &batch.disclosed_paths)
            .await
    {
        tracing::warn!("could not acknowledge shell exits in a completed model request: {err}");
    }
}

#[cfg(test)]
#[path = "shell_resume_exits_tests.rs"]
mod tests;
