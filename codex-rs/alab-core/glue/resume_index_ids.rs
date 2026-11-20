// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::StateRuntime;
use crate::sqlite::SqliteWriteQueryExt;
use codex_protocol::ThreadId;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

impl StateRuntime {
    /// A present ID always owns its selected path. Inspect only empty user-visible rows.
    pub async fn resume_index_paths(
        &self,
        archived: bool,
    ) -> anyhow::Result<HashMap<String, Option<PathBuf>>> {
        let rows: Vec<(String, Option<String>)> = sqlx::query_as(
            "SELECT id, CASE WHEN preview = '' AND archived = ? AND attached_thread_parent_id IS NULL
             AND source IN ('cli', 'vscode', 'exec', 'mcp') THEN rollout_path END FROM threads",
        )
        .bind(archived)
        .fetch_all(self.pool.as_ref())
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, path)| (id, path.map(PathBuf::from)))
            .collect())
    }

    /// Do not let concurrent resume, revert, archive, or an ordinary writer lose its selection.
    pub async fn repair_empty_resume_preview(
        &self,
        id: ThreadId,
        selected: &Path,
        archived: bool,
        preview: &str,
        first_user: Option<&str>,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE threads SET preview = ?, first_user_message =
             CASE WHEN first_user_message = '' THEN COALESCE(?, '') ELSE first_user_message END
             WHERE id = ? AND rollout_path = ? AND archived = ? AND preview = ''
             AND attached_thread_parent_id IS NULL AND source IN ('cli', 'vscode', 'exec', 'mcp')",
        )
        .bind(preview)
        .bind(first_user)
        .bind(id.to_string())
        .bind(selected.to_string_lossy().as_ref())
        .bind(archived)
        .execute_with_busy_retry(self.pool.as_ref())
        .await?;
        Ok(())
    }
}
