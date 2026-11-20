// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::LocalThreadStore;
use super::LocalThreadStoreConfig;
use super::rollout_lineage::RolloutLineage;
use crate::ThreadStoreError;
use crate::ThreadStoreResult;
use codex_protocol::ThreadId;
use codex_protocol::items::TurnItem;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::ThreadHistoryMode;
use codex_protocol::protocol::user_message_preview;
use codex_rollout::RolloutConfigView;
use codex_rollout::RolloutItem;
use codex_rollout::state_db;
use std::collections::HashMap;
use std::io;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Read;
use std::path::Path;

impl LocalThreadStore {
    /// Restore discoverable local user conversations before the first SQLite page is issued.
    pub async fn repair_local_resume_index(
        config: &impl RolloutConfigView,
        archived: bool,
    ) -> io::Result<()> {
        let db = state_db::get_state_db(config)
            .await
            .ok_or_else(|| io::Error::other("local history index unavailable"))?;
        let indexed = db
            .resume_index_paths(archived)
            .await
            .map_err(io::Error::other)?;
        let home = config.codex_home();
        let candidates = codex_rollout::local_resume_rollouts(home, archived, &indexed).await?;
        if candidates.is_empty() {
            return Ok(());
        }
        let ids = candidates.iter().map(|(id, _)| *id).collect();
        let mut names = codex_rollout::find_thread_names_by_ids(home, &ids)
            .await
            .unwrap_or_else(|error| {
                tracing::debug!(%error, "could not read saved names for restored local sessions");
                HashMap::new()
            });
        let store = Self::new(
            LocalThreadStoreConfig::from_config(config),
            Some(db.clone()),
        );
        for (id, path) in candidates {
            let Ok(header) = codex_rollout::read_session_meta_line(&path).await else {
                continue;
            };
            if header.meta.id != id
                || !matches!(
                    header.meta.source,
                    SessionSource::Cli
                        | SessionSource::VSCode
                        | SessionSource::Exec
                        | SessionSource::Mcp
                )
            {
                continue;
            }
            let Ok(outcome) =
                codex_rollout::extract_metadata_from_rollout(&path, config.model_provider_id())
                    .await
            else {
                continue;
            };
            let mut metadata = outcome.metadata;
            if metadata.id != id {
                continue;
            }
            if header.meta.history_mode == ThreadHistoryMode::Paginated
                && header.meta.history_base.is_some()
            {
                match store.inherited_resume_preview(id, &path).await {
                    Ok(Some(preview)) => {
                        metadata.first_user_message = Some(preview.clone());
                        metadata.preview = Some(preview);
                    }
                    Ok(None) => {}
                    Err(error) => {
                        tracing::debug!(%id, %error, "could not read inherited resume preview")
                    }
                }
            }
            let preview = metadata
                .preview
                .as_deref()
                .or(metadata.first_user_message.as_deref())
                .unwrap_or_default();
            if let Some(Some(selected)) = indexed.get(&id.to_string()) {
                if !preview.is_empty() {
                    db.repair_empty_resume_preview(
                        id,
                        selected,
                        archived,
                        preview,
                        metadata.first_user_message.as_deref(),
                    )
                    .await
                    .map_err(io::Error::other)?;
                }
                continue;
            }
            // A restored file may still be receiving its first turn. Leave it eligible next time.
            if !archived && preview.is_empty() {
                continue;
            }
            metadata.cwd = state_db::normalize_cwd_for_state_db(&metadata.cwd);
            metadata.archived_at = archived.then_some(metadata.updated_at);
            metadata.name = names
                .remove(&id)
                .filter(|name| !name.is_empty())
                .or(metadata.name);
            db.insert_thread_if_absent_with_memory_mode(
                &metadata,
                outcome.memory_mode.as_deref().unwrap_or("enabled"),
            )
            .await
            .map_err(io::Error::other)?;
        }
        Ok(())
    }

    async fn inherited_resume_preview(
        &self,
        id: ThreadId,
        path: &Path,
    ) -> ThreadStoreResult<Option<String>> {
        let lineage = self
            .resolve_rollout_lineage(id, Some(path.to_path_buf()))
            .await?;
        lineage
            .read_with_fallback(self, |lineage| async move {
                let visible = lineage.for_history_page().await?.into_owned();
                let (stop, keep_running) = tokio::sync::oneshot::channel::<()>();
                let result = tokio::task::spawn_blocking(move || {
                    read_inherited_preview(&visible, || stop.is_closed())
                })
                .await;
                drop(keep_running);
                result
                    .map_err(|error| ThreadStoreError::Internal {
                        message: error.to_string(),
                    })?
                    .map_err(|error| ThreadStoreError::Internal {
                        message: error.to_string(),
                    })
            })
            .await
    }
}

/// Use only complete user events in visible, inherited ranges. A different selected revision,
/// a discarded suffix, and hidden attached-parent history cannot supply this thread's preview.
fn read_inherited_preview(
    lineage: &RolloutLineage,
    cancelled: impl Fn() -> bool,
) -> io::Result<Option<String>> {
    for segment in lineage.segments() {
        let Some(end) = segment.end else { continue };
        let file = lineage.reads.open(&segment.rollout_path, &cancelled)?;
        let preview = read_preview_range(
            BufReader::new(file.take(end.end_byte_offset)),
            segment.start_ordinal(),
            end.end_ordinal_exclusive,
            &cancelled,
        )
        .inspect_err(|_| {
            if !cancelled() {
                lineage.reads.record_failure(&segment.rollout_path);
            }
        })?;
        if preview.is_some() {
            return Ok(preview);
        }
    }
    Ok(None)
}

fn read_preview_range(
    mut reader: BufReader<std::io::Take<impl Read>>,
    start: u64,
    end: u64,
    cancelled: impl Fn() -> bool,
) -> io::Result<Option<String>> {
    let mut bytes = Vec::new();
    loop {
        if cancelled() {
            return Err(io::Error::other("resume preview scan cancelled"));
        }
        bytes.clear();
        if reader.read_until(b'\n', &mut bytes)? == 0 {
            return if reader.get_ref().limit() == 0 {
                Ok(None)
            } else {
                Err(io::Error::other(
                    "inherited preview ended before its cutoff",
                ))
            };
        }
        if !bytes.ends_with(b"\n") {
            return Err(io::Error::other("incomplete inherited preview record"));
        }
        let line = codex_rollout::parse_rollout_line_bytes(&bytes).map_err(io::Error::other)?;
        let ordinal = line
            .ordinal
            .ok_or_else(|| io::Error::other("inherited preview record has no ordinal"))?;
        if ordinal < start || ordinal >= end {
            continue;
        }
        let preview = match line.item {
            RolloutItem::EventMsg(EventMsg::UserMessage(user)) => user_message_preview(&user),
            RolloutItem::EventMsg(EventMsg::ItemCompleted(event)) => match event.item {
                TurnItem::UserMessage(user) => {
                    user_message_preview(&user.as_legacy_user_message_event())
                }
                _ => None,
            },
            _ => None,
        };
        if preview.is_some() {
            return Ok(preview);
        }
    }
}
