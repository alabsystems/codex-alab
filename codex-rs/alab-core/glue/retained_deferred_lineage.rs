// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Full display records from the ORIGINAL frozen lineage resolver. This does
//! not use model-context filtering, project SQLite, create a child header,
//! persist the deferred child, or turn a read failure into empty history.
use super::*;
use codex_rollout::ReverseJsonlScanner;
use codex_rollout::RolloutItem;
use codex_rollout::ScanOutcome;

impl LocalThreadStore {
    pub(in crate::local) async fn retained_inherited_display(
        &self,
        child: ThreadId,
        base: codex_protocol::protocol::HistoryPosition,
    ) -> ThreadStoreResult<Vec<RolloutItem>> {
        // Locate this exact frozen rollout through the ORIGINAL ancestor
        // resolver. The parent's present current rollout may have reverted
        // after the child fork and is not a substitute for this actual base.
        let lineage = self
            .resolve_rollout_lineage_with_mode(
                child,
                LineageResolutionMode::Existing,
                Some(base),
                None,
                &std::collections::HashMap::new(),
            )
            .await?;
        lineage
            .read_with_fallback(self, |lineage| async move {
                // An original fallback can select a new copy/lineage, so apply
                // the SAME actual cutoff on EVERY attempt, never only the first.
                let lineage = lineage.truncate_at(base).await?;
                let display = lineage.for_history_page().await?.into_owned();
                read_display(display).await
            })
            .await
    }
}

async fn read_display(lineage: RolloutLineage) -> ThreadStoreResult<Vec<RolloutItem>> {
    let (stop, keep_running) = tokio::sync::oneshot::channel::<()>();
    let read = tokio::task::spawn_blocking(move || {
        let mut items = Vec::new();
        for segment in lineage.segments() {
            let result = (|| -> std::io::Result<Vec<RolloutItem>> {
                let file = lineage
                    .reads
                    .open(&segment.rollout_path, || stop.is_closed())?;
                let end = segment.end.ok_or_else(|| {
                    std::io::Error::other("inherited display has no frozen cutoff")
                })?;
                let mut scanner = ReverseJsonlScanner::new_at(file, end.end_byte_offset)?;
                let mut local = Vec::new();
                let mut expected = end.end_ordinal_exclusive;
                while let Some(line) = scanner.scan_next_rollout_line()? {
                    if stop.is_closed() {
                        return Err(std::io::Error::other(
                            "inherited display observation canceled",
                        ));
                    }
                    let ScanOutcome::Parsed(line) = line else {
                        return Err(std::io::Error::other(
                            "inherited display has an invalid complete record",
                        ));
                    };
                    if matches!(line.item, RolloutItem::SessionMeta(_)) {
                        break;
                    }
                    let ordinal = line.ordinal.ok_or_else(|| {
                        std::io::Error::other("paginated inherited display record has no ordinal")
                    })?;
                    if ordinal < segment.start_ordinal() {
                        break;
                    }
                    if ordinal.checked_add(1) != Some(expected) {
                        return Err(std::io::Error::other(
                            "inherited display ordinal differs from its actual cutoff",
                        ));
                    }
                    expected = ordinal;
                    local.push(line.item);
                }
                if end.end_ordinal_exclusive > segment.start_ordinal()
                    && expected != segment.start_ordinal()
                {
                    return Err(std::io::Error::other(
                        "inherited display ended before its original segment",
                    ));
                }
                local.reverse();
                Ok(local)
            })();
            match result {
                Ok(local) => items.extend(local),
                Err(error) => {
                    lineage.reads.record_failure(&segment.rollout_path);
                    return Err(ThreadStoreError::Internal {
                        message: error.to_string(),
                    });
                }
            }
        }
        Ok(items)
    })
    .await;
    drop(keep_running);
    read.map_err(|error| ThreadStoreError::Internal {
        message: format!("inherited display reader failed: {error}"),
    })?
}

#[cfg(test)]
impl LocalThreadStore {
    /// Read the actual first selected path before the production read retries;
    /// no index, header, receipt or resolver state is changed by this observer.
    pub(in crate::local) async fn retained_inherited_lineage_for_tests(
        &self,
        child: ThreadId,
        base: codex_protocol::protocol::HistoryPosition,
    ) -> ThreadStoreResult<std::path::PathBuf> {
        let actual = self
            .resolve_rollout_lineage_with_mode(
                child,
                LineageResolutionMode::Existing,
                Some(base),
                None,
                &std::collections::HashMap::new(),
            )
            .await?;
        actual
            .segments()
            .last()
            .map(|segment| segment.rollout_path.clone())
            .ok_or_else(|| ThreadStoreError::Internal {
                message: "actual inherited lineage has no first ancestor".into(),
            })
    }
}
