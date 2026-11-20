// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Bounded shell observations in the local thread owner's state database.

use super::StateRuntime;
use crate::SqlitePoolWriteExt;
use codex_alab_core::ShellOutputTail;
use codex_protocol::ThreadId;
use sqlx::Row;
use sqlx::sqlite::SqliteRow;
use std::collections::HashSet;
use std::path::Path;
use uuid::Uuid;

const MAX_INVOCATIONS: i64 = 128;
const MAX_PREVIEW_BYTES: usize = 1024;
const MAX_COMMAND_BYTES: usize = 512;
const MAX_CWD_BYTES: usize = 1024;
const OBSERVATIONS: &str = "alab_shell_invocations";
const PENDING_EXITS: &str = "alab_pending_shell_exits";
const LAUNCHES: &str = "alab_shell_launches";
const DIAGNOSTICS: &str = "alab_watched_shell_diagnostics";
const TAILS: &str = "alab_shell_output_tails";
const PUBLICATIONS: &str = "alab_shell_diagnostic_publications";
const REJECTED_CAPTURES: &str = "alab_rejected_shell_captures";

// Reads never create the launch table on an older database. Both schemas use
// a static SQL statement, with the legacy join supplying no launch rows.
macro_rules! with_launch_join {
    ($has_launches:expr, $before:literal, $after:expr) => {
        if $has_launches {
            concat!(
                $before,
                " LEFT JOIN alab_shell_launches l USING (thread_id, invocation) ",
                $after
            )
        } else {
            concat!(
                $before,
                " LEFT JOIN (SELECT NULL AS thread_id, NULL AS invocation,
                NULL AS command, NULL AS command_truncated, NULL AS cwd WHERE 0)
                l USING (thread_id, invocation) ",
                $after
            )
        }
    };
}

macro_rules! with_diagnostic_join {
    ($launches:expr, $diagnostics:expr, $before:literal, $after:expr) => {{
        if $diagnostics {
            with_launch_join!($launches, $before, concat!(
                " LEFT JOIN alab_watched_shell_diagnostics d USING (thread_id, invocation) ", $after
            ))
        } else {
            with_launch_join!($launches, $before, concat!(
                " LEFT JOIN (SELECT NULL AS thread_id, NULL AS invocation, NULL AS partial WHERE 0)
                  d USING (thread_id, invocation) ", $after
            ))
        }
    }};
}

macro_rules! with_tail_join {
    ($launches:expr, $diagnostics:expr, $tails:expr, $before:literal, $after:expr) => {{
        if $tails {
            with_diagnostic_join!(
                $launches,
                $diagnostics,
                $before,
                concat!(
                    " LEFT JOIN alab_shell_output_tails t ON t.thread_id = s.thread_id
                  AND t.invocation = s.invocation AND t.capture_name = s.capture_name ",
                    $after
                )
            )
        } else {
            with_diagnostic_join!(
                $launches,
                $diagnostics,
                $before,
                concat!(
                    " LEFT JOIN (SELECT NULL AS start_byte, NULL AS bytes WHERE 0) t ON 0 ",
                    $after
                )
            )
        }
    }};
}

macro_rules! with_evidence_joins {
    ($tables:expr, $before:literal, $after:literal) => {{
        let (launches, diagnostics, tails, rejections) = $tables;
        if rejections {
            with_tail_join!(
                launches,
                diagnostics,
                tails,
                $before,
                concat!(
                    " LEFT JOIN alab_rejected_shell_captures r ON r.thread_id = s.thread_id
                  AND r.invocation = s.invocation AND r.capture_name = s.capture_name ",
                    $after
                )
            )
        } else {
            with_tail_join!(
                launches,
                diagnostics,
                tails,
                $before,
                concat!(
                    " LEFT JOIN (SELECT NULL AS capture_name WHERE 0) r ON 0 ",
                    $after
                )
            )
        }
    }};
}

/// Original native exec command and, when representable in full, its resolved directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellLaunchContext {
    pub command: String,
    pub command_truncated: bool,
    pub cwd: Option<String>,
}

impl ShellLaunchContext {
    fn from_row(row: &SqliteRow) -> sqlx::Result<Option<Self>> {
        let Some(command) = row.try_get("command")? else {
            return Ok(None);
        };
        Ok(Some(Self {
            command,
            command_truncated: row.try_get("command_truncated")?,
            cwd: row.try_get("cwd")?,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellInvocationObservation {
    pub session_id: i32,
    pub finished: bool,
    pub exit_code: Option<i32>,
    pub capture_name: Option<String>,
    /// Validation rejected the recorded capture belonging to this invocation.
    pub capture_rejected: bool,
    /// Confirmed coverage of the retained prefix. Older tool captures have unknown coverage.
    pub capture_partial: Option<bool>,
    pub tail: Option<ShellOutputTail>,
    pub preview: String,
    pub launch: Option<ShellLaunchContext>,
}

/// The retained file, its coverage, and any EOF bytes beyond its capped prefix.
#[derive(Debug)]
pub struct WatchedShellDiagnostic<'a> {
    pub capture_name: &'a str,
    pub partial: bool,
    pub tail: Option<&'a ShellOutputTail>,
}

/// Native facts frozen before the watcher publishes its final file. Recovery
/// verifies the named prefix against its length and digest before attaching it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellDiagnosticPublication {
    pub thread_id: ThreadId,
    pub invocation: Uuid,
    pub session_id: i32,
    pub exit_code: i32,
    pub capture_name: String,
    pub expected_bytes: u64,
    pub prefix_sha256: String,
    pub partial: bool,
    pub tail: Option<ShellOutputTail>,
}

impl ShellDiagnosticPublication {
    fn validate(&self) -> anyhow::Result<()> {
        validate_capture_name(&self.capture_name)?;
        let cap = codex_alab_core::SHELL_OUTPUT_PREFIX_BYTES as u64;
        anyhow::ensure!(
            (1..=cap).contains(&self.expected_bytes),
            "invalid shell diagnostic prefix length"
        );
        anyhow::ensure!(
            self.prefix_sha256.len() == 64
                && self
                    .prefix_sha256
                    .bytes()
                    .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f')),
            "invalid shell diagnostic SHA-256"
        );
        anyhow::ensure!(
            self.tail.is_none() || (self.partial && self.expected_bytes == cap),
            "shell output tail requires the entire capped prefix"
        );
        Ok(())
    }

    fn from_row(thread_id: ThreadId, row: SqliteRow) -> anyhow::Result<Self> {
        let intent = Self {
            thread_id,
            invocation: Uuid::from_slice(row.try_get("invocation")?)?,
            session_id: row.try_get("session_id")?,
            exit_code: row.try_get("exit_code")?,
            capture_name: row.try_get("capture_name")?,
            expected_bytes: u64::try_from(row.try_get::<i64, _>("expected_bytes")?)?,
            prefix_sha256: row.try_get("prefix_sha256")?,
            partial: row.try_get("partial")?,
            tail: tail_from_row(&row)?,
        };
        intent.validate()?;
        Ok(intent)
    }

    fn diagnostic(&self) -> WatchedShellDiagnostic<'_> {
        WatchedShellDiagnostic {
            capture_name: &self.capture_name,
            partial: self.partial,
            tail: self.tail.as_ref(),
        }
    }
}

/// The native facts available to the model about this particular watched exit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingShellExit {
    pub invocation: Uuid,
    pub session_id: i32,
    pub exit_code: Option<i32>,
    pub capture_name: Option<String>,
    /// Validation rejected the recorded capture belonging to this invocation.
    pub capture_rejected: bool,
    pub capture_partial: Option<bool>,
    pub tail: Option<ShellOutputTail>,
    pub launch: Option<ShellLaunchContext>,
}

impl StateRuntime {
    /// Record the host's observation for a durable thread. A missing or deleted
    /// thread cannot acquire evidence. A terminal observation survives late polls.
    /// The original capture name identifies its bytes even when the current tool
    /// response omits the path. Only a disclosed path can collect that file.
    #[allow(
        clippy::too_many_arguments,
        reason = "the native result supplies these evidence fields together"
    )]
    pub async fn record_shell_invocation(
        &self,
        thread: ThreadId,
        invocation: Uuid,
        session_id: i32,
        finished: bool,
        exit_code: Option<i32>,
        capture_name: Option<&str>,
        path_disclosed: bool,
        preview: &str,
        initial: bool,
        launch: Option<(&str, Option<&str>)>,
        output_uncollected: bool,
        tail: Option<&ShellOutputTail>,
    ) -> anyhow::Result<()> {
        if let Some(name) = capture_name {
            validate_capture_name(name)?;
        }
        let preview = &preview[..preview.floor_char_boundary(MAX_PREVIEW_BYTES)];
        let thread = thread.to_string();
        let disclosed_name = capture_name.filter(|_| path_disclosed);
        let deferred_initial_tail = initial && output_uncollected && tail.is_some();
        let awaiting_diagnostic = output_uncollected
            && tail.is_none()
            && finished
            && exit_code.is_some_and(|code| code != 0 && code != -1);
        let mut tx = self.pool.begin_immediate().await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS alab_shell_invocations (
                thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
                invocation BLOB NOT NULL CHECK (length(invocation) = 16),
                session_id INTEGER NOT NULL,
                finished INTEGER NOT NULL,
                exit_code INTEGER,
                capture_name TEXT,
                preview TEXT NOT NULL,
                UNIQUE (thread_id, invocation)
            )",
        )
        .execute(&mut *tx)
        .await?;
        if !initial && (awaiting_diagnostic || tail.is_some()) {
            // A delayed final poll cannot reacquire an evicted invocation's quota slot.
            let original: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM alab_shell_invocations WHERE thread_id = ? AND invocation = ?)",
            )
            .bind(&thread)
            .bind(invocation.as_bytes().as_slice())
            .fetch_one(&mut *tx)
            .await?;
            if !original {
                return Ok(());
            }
        }
        let stored = sqlx::query(
            "INSERT INTO alab_shell_invocations
                (thread_id, invocation, session_id, finished, exit_code, capture_name, preview)
             VALUES (?, ?, ?, ?, NULLIF(?, -1), ?, ?)
             ON CONFLICT (thread_id, invocation) DO UPDATE SET
                finished = (finished AND exit_code IS NOT -1) OR excluded.finished,
                exit_code = COALESCE(NULLIF(exit_code, -1), excluded.exit_code),
                capture_name = COALESCE(capture_name, excluded.capture_name),
                preview = CASE
                    WHEN excluded.preview = '' OR (finished AND exit_code IS NOT -1 AND NOT excluded.finished)
                        OR (? AND preview <> '') THEN preview
                    ELSE excluded.preview
                END
             WHERE session_id = excluded.session_id",
        )
        .bind(&thread)
        .bind(invocation.as_bytes().as_slice())
        .bind(session_id)
        // The native wait-failure sentinel establishes neither an exit nor its code.
        .bind(finished && exit_code != Some(-1))
        .bind(exit_code)
        .bind(capture_name)
        .bind(preview)
        .bind(initial)
        .execute(&mut *tx)
        .await?;
        anyhow::ensure!(
            stored.rows_affected() == 1,
            "shell invocation's session ID changed"
        );
        if let Some(tail) = tail {
            let code = exit_code
                .filter(|code| finished && *code != 0 && *code != -1)
                .ok_or_else(|| {
                    anyhow::anyhow!("shell output tail requires a confirmed failed invocation")
                })?;
            let name = capture_name.ok_or_else(|| {
                anyhow::anyhow!("shell output tail requires its retained basename")
            })?;
            let matches: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM alab_shell_invocations WHERE thread_id = ? AND invocation = ?
                 AND session_id = ? AND exit_code = ? AND capture_name = ?)",
            )
            .bind(&thread)
            .bind(invocation.as_bytes().as_slice())
            .bind(session_id)
            .bind(exit_code)
            .bind(name)
            .fetch_one(&mut *tx)
            .await?;
            anyhow::ensure!(
                matches,
                "shell output tail disagrees with original observation"
            );
            let new_tail = store_tail(&mut tx, &thread, invocation, name, tail).await?;
            ensure_diagnostics(&mut tx).await?;
            let marked = sqlx::query(
                "INSERT INTO alab_watched_shell_diagnostics (thread_id, invocation, partial, path_acknowledged)
                 VALUES (?, ?, 1, ?) ON CONFLICT (thread_id, invocation) DO UPDATE
                 SET partial = 1, path_acknowledged = path_acknowledged OR excluded.path_acknowledged
                 WHERE partial IS NOT 0",
            )
            .bind(&thread)
            .bind(invocation.as_bytes().as_slice())
            .bind(disclosed_name.is_some() && !deferred_initial_tail)
            .execute(&mut *tx)
            .await?;
            anyhow::ensure!(
                marked.rows_affected() == 1,
                "shell output tail contradicts the complete retained file"
            );
            if deferred_initial_tail && new_tail {
                // The first synchronous result could not fit these bytes in history.
                // Their immutable row prevents a retry from requeuing an accepted tail.
                ensure_pending_exits(&mut tx).await?;
                enqueue_diagnostic(&mut tx, &thread, invocation, code).await?;
            }
        }
        // A poll can publish the invocation before the original exec response.
        // Only that response supplies launch facts, once for this invocation.
        if initial && let Some((command, cwd)) = launch {
            sqlx::query(
                "CREATE TABLE IF NOT EXISTS alab_shell_launches (
                    thread_id TEXT NOT NULL,
                    invocation BLOB NOT NULL,
                    command TEXT NOT NULL,
                    command_truncated INTEGER NOT NULL,
                    cwd TEXT,
                    PRIMARY KEY (thread_id, invocation),
                    FOREIGN KEY (thread_id, invocation)
                        REFERENCES alab_shell_invocations(thread_id, invocation) ON DELETE CASCADE
                )",
            )
            .execute(&mut *tx)
            .await?;
            let prefix = &command[..command.floor_char_boundary(MAX_COMMAND_BYTES)];
            sqlx::query(
                "INSERT INTO alab_shell_launches (thread_id, invocation, command, command_truncated, cwd)
                 VALUES (?, ?, ?, ?, ?) ON CONFLICT (thread_id, invocation) DO NOTHING",
            )
            .bind(&thread)
            .bind(invocation.as_bytes().as_slice())
            .bind(prefix)
            .bind(prefix.len() < command.len())
            .bind(cwd.filter(|path| path.len() <= MAX_CWD_BYTES && Path::new(path).is_absolute()))
            .execute(&mut *tx)
            .await?;
        }
        if awaiting_diagnostic {
            // The tool collected the native status before the original raw output
            // was fully returned. Even the first terminal result may precede the
            // watcher. Give its drain callback a receipt slot, without enqueuing the exit.
            ensure_pending_exits(&mut tx).await?;
            ensure_diagnostics(&mut tx).await?;
            sqlx::query(
                "INSERT INTO alab_watched_shell_diagnostics (thread_id, invocation, path_acknowledged)
                 SELECT thread_id, invocation, 0 FROM alab_shell_invocations
                 WHERE thread_id = ? AND invocation = ? AND session_id = ? AND exit_code = ?
                 ON CONFLICT (thread_id, invocation) DO UPDATE SET path_acknowledged = 0
                 WHERE partial IS NULL",
            )
            .bind(&thread)
            .bind(invocation.as_bytes().as_slice())
            .bind(session_id)
            .bind(exit_code)
            .execute(&mut *tx)
            .await?;
        }
        // A tool that reports the exit has collected this fact. An earlier running
        // response or a less informative terminal result cannot clear a known code.
        if (finished || exit_code.is_some())
            && !deferred_initial_tail
            && exit_code != Some(-1)
            && let (true, has_diagnostics, has_tails, _) =
                evidence_tables(&mut tx, PENDING_EXITS).await?
            && (tail.is_some() || !has_tails || !tail_exists(&mut tx, &thread, invocation).await?)
        {
            let delete = if has_diagnostics {
                "DELETE FROM alab_pending_shell_exits WHERE thread_id = ?1 AND invocation = ?2
                 AND (exit_code IS NULL OR exit_code = ?3)
                 AND NOT EXISTS (
                     SELECT 1 FROM alab_watched_shell_diagnostics d
                     JOIN alab_shell_invocations s USING (thread_id, invocation)
                     WHERE d.thread_id = ?1 AND d.invocation = ?2
                       AND d.partial IS NOT NULL AND (?5 OR s.capture_name IS NOT ?4)
                 )"
            } else {
                "DELETE FROM alab_pending_shell_exits WHERE thread_id = ?1 AND invocation = ?2
                 AND (exit_code IS NULL OR exit_code = ?3)"
            };
            let delete = sqlx::query(delete)
                .bind(&thread)
                .bind(invocation.as_bytes().as_slice())
                .bind(exit_code);
            if has_diagnostics {
                delete
                    .bind(disclosed_name)
                    .bind(awaiting_diagnostic)
                    .execute(&mut *tx)
                    .await?;
                // A terminal poll can retire a bare exit or its own file. If it
                // omitted an earlier file, the reader can still confirm its coverage.
                sqlx::query(
                    "DELETE FROM alab_watched_shell_diagnostics WHERE thread_id = ?1 AND invocation = ?2 AND NOT ?4
                     AND partial IS NULL AND NOT path_acknowledged AND EXISTS (SELECT 1 FROM alab_shell_invocations
                         WHERE thread_id = ?1 AND invocation = ?2 AND exit_code = ?3
                           AND (capture_name IS NULL OR capture_name IS ?5))",
                )
                .bind(&thread)
                .bind(invocation.as_bytes().as_slice())
                .bind(exit_code)
                .bind(awaiting_diagnostic)
                .bind(disclosed_name)
                .execute(&mut *tx)
                .await?;
            } else {
                delete.execute(&mut *tx).await?;
            }
        }
        // A poll updates its row in place; quota eviction is ordered by first publication.
        sqlx::query(
            "DELETE FROM alab_shell_invocations WHERE rowid IN (
                SELECT rowid FROM alab_shell_invocations WHERE thread_id = ?
                ORDER BY rowid DESC LIMIT -1 OFFSET ?
             )",
        )
        .bind(&thread)
        .bind(MAX_INVOCATIONS)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Observe an existing row without creating storage for a thread or an older database.
    pub async fn shell_invocation(
        &self,
        thread: ThreadId,
        invocation: Uuid,
    ) -> anyhow::Result<Option<ShellInvocationObservation>> {
        let mut tx = self.pool.begin().await?;
        let row = if table_exists(&mut tx, OBSERVATIONS).await? {
            sqlx::query(with_evidence_joins!(
                evidence_tables(&mut tx, LAUNCHES).await?,
                "SELECT s.session_id, (s.finished AND s.exit_code IS NOT -1) AS finished,
                        NULLIF(s.exit_code, -1) AS exit_code, NULLIF(s.capture_name, r.capture_name) AS capture_name, r.capture_name IS NOT NULL AS capture_rejected, s.preview,
                        l.command, l.command_truncated, l.cwd, CASE WHEN r.capture_name IS NULL THEN d.partial END AS partial, t.start_byte, t.bytes
                 FROM alab_shell_invocations s",
                "WHERE s.thread_id = ? AND s.invocation = ?"
            ))
            .bind(thread.to_string())
            .bind(invocation.as_bytes().as_slice())
            .fetch_optional(&mut *tx)
            .await?
        } else {
            None
        };
        tx.commit().await?;
        row.map(|row| {
            Ok(ShellInvocationObservation {
                session_id: row.try_get("session_id")?,
                finished: row.try_get("finished")?,
                exit_code: row.try_get("exit_code")?,
                capture_name: row.try_get("capture_name")?,
                capture_rejected: row.try_get("capture_rejected")?,
                capture_partial: row.try_get("partial")?,
                tail: tail_from_row(&row)?,
                preview: row.try_get("preview")?,
                launch: ShellLaunchContext::from_row(&row)?,
            })
        })
        .transpose()
    }

    /// Settle the native terminal status of an invocation which a tool already published.
    /// A watcher can outlive its thread or its quota slot. Only the original retained
    /// row can receive this fact. Exit status alone cannot supply output bytes or a log.
    pub async fn record_watched_shell_exit(
        &self,
        thread: ThreadId,
        invocation: Uuid,
        session_id: i32,
        exit_code: Option<i32>,
    ) -> anyhow::Result<()> {
        if exit_code == Some(-1) {
            return Ok(());
        }
        let mut tx = self.pool.begin_immediate().await?;
        if table_exists(&mut tx, OBSERVATIONS).await? {
            // Only a new watcher receipt can put a result on the resume list. An
            // older version's finished row has no evidence of who collected it.
            // Once a receipt is acknowledged, a duplicate watcher callback sees
            // the already settled observation and cannot enqueue it again.
            ensure_pending_exits(&mut tx).await?;
            sqlx::query(
                "INSERT INTO alab_pending_shell_exits (thread_id, invocation, exit_code)
                 SELECT thread_id, invocation, ? FROM alab_shell_invocations
                 WHERE thread_id = ? AND invocation = ? AND session_id = ?
                   AND NULLIF(exit_code, -1) IS NULL
                   AND (NOT finished OR exit_code = -1 OR ? IS NOT NULL)
                 ON CONFLICT (thread_id, invocation) DO UPDATE SET
                     exit_code = COALESCE(alab_pending_shell_exits.exit_code, excluded.exit_code)",
            )
            .bind(exit_code)
            .bind(thread.to_string())
            .bind(invocation.as_bytes().as_slice())
            .bind(session_id)
            .bind(exit_code)
            .execute(&mut *tx)
            .await?;
            sqlx::query(
                "UPDATE alab_shell_invocations
                 SET finished = 1, exit_code = COALESCE(NULLIF(exit_code, -1), ?)
                 WHERE thread_id = ? AND invocation = ? AND session_id = ?",
            )
            .bind(exit_code)
            .bind(thread.to_string())
            .bind(invocation.as_bytes().as_slice())
            .bind(session_id)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Attach a real retained file to this watcher's confirmed failure. A bare exit
    /// already accepted by a resumed model can acquire its diagnostic once output drains.
    /// Returns true if this exact file belongs to the retained watcher observation.
    pub async fn record_watched_shell_diagnostic(
        &self,
        thread: ThreadId,
        invocation: Uuid,
        session_id: i32,
        exit_code: i32,
        diagnostic: WatchedShellDiagnostic<'_>,
    ) -> anyhow::Result<bool> {
        let mut tx = self.pool.begin_immediate().await?;
        let attached = attach_diagnostic(
            &mut tx,
            &thread.to_string(),
            invocation,
            session_id,
            exit_code,
            diagnostic,
        )
        .await?;
        tx.commit().await?;
        Ok(attached)
    }

    /// Reserve only the original eligible failure. A retry must describe exactly
    /// the same bytes. The reservation itself discloses nothing to a resumed model.
    pub async fn reserve_shell_diagnostic_publication(
        &self,
        intent: &ShellDiagnosticPublication,
    ) -> anyhow::Result<bool> {
        intent.validate()?;
        let thread = intent.thread_id.to_string();
        let id = intent.invocation.as_bytes().as_slice();
        let mut tx = self.pool.begin_immediate().await?;
        let Some(existing) = diagnostic_state(
            &mut tx,
            &thread,
            intent.invocation,
            intent.session_id,
            intent.exit_code,
            &intent.capture_name,
        )
        .await?
        else {
            return Ok(false);
        };
        if intent.partial && matches!(existing.prior, Some((Some(false), _))) {
            return Ok(false);
        }
        if existing.has_tails {
            let row = sqlx::query(
                "SELECT start_byte, bytes FROM alab_shell_output_tails
                 WHERE thread_id = ? AND invocation = ? AND capture_name = ?",
            )
            .bind(&thread)
            .bind(id)
            .bind(&intent.capture_name)
            .fetch_optional(&mut *tx)
            .await?;
            if let Some(row) = row
                && (!intent.partial
                    || (intent.tail.is_some() && tail_from_row(&row)? != intent.tail))
            {
                return Ok(false);
            }
        }
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS alab_shell_diagnostic_publications (
                thread_id TEXT NOT NULL, invocation BLOB NOT NULL,
                session_id INTEGER NOT NULL, exit_code INTEGER NOT NULL CHECK (exit_code NOT IN (0, -1)),
                capture_name TEXT NOT NULL, expected_bytes INTEGER NOT NULL CHECK (expected_bytes BETWEEN 1 AND 8388608),
                prefix_sha256 TEXT NOT NULL CHECK (length(prefix_sha256) = 64 AND prefix_sha256 NOT GLOB '*[^0-9a-f]*'),
                partial INTEGER NOT NULL CHECK (partial IN (0, 1)), start_byte BLOB, bytes BLOB,
                CHECK ((start_byte IS NULL AND bytes IS NULL) OR
                       (partial = 1 AND expected_bytes = 8388608 AND typeof(start_byte) = 'blob'
                        AND length(start_byte) = 8 AND typeof(bytes) = 'blob' AND length(bytes) BETWEEN 1 AND 4096)),
                PRIMARY KEY (thread_id, invocation), UNIQUE (thread_id, capture_name),
                FOREIGN KEY (thread_id, invocation)
                    REFERENCES alab_shell_invocations(thread_id, invocation) ON DELETE CASCADE
            )",
        )
        .execute(&mut *tx)
        .await?;
        let inserted = sqlx::query(
            "INSERT INTO alab_shell_diagnostic_publications
             (thread_id, invocation, session_id, exit_code, capture_name, expected_bytes, prefix_sha256, partial, start_byte, bytes)
             SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10 WHERE NOT EXISTS (
                 SELECT 1 FROM alab_shell_invocations WHERE thread_id = ?1 AND invocation <> ?2 AND capture_name = ?5
             ) ON CONFLICT DO NOTHING",
        )
        .bind(&thread)
        .bind(id)
        .bind(intent.session_id)
        .bind(intent.exit_code)
        .bind(&intent.capture_name)
        .bind(i64::try_from(intent.expected_bytes)?)
        .bind(&intent.prefix_sha256)
        .bind(intent.partial)
        .bind(intent.tail.as_ref().map(|tail| tail.start_byte().to_be_bytes().to_vec()))
        .bind(intent.tail.as_ref().map(ShellOutputTail::bytes))
        .execute(&mut *tx)
        .await?;
        let reserved = inserted.rows_affected() == 1
            || publication_for(&mut tx, intent).await?.as_ref() == Some(intent);
        tx.commit().await?;
        Ok(reserved)
    }

    pub async fn pending_shell_diagnostic_publications(
        &self,
        thread: ThreadId,
    ) -> anyhow::Result<Vec<ShellDiagnosticPublication>> {
        let mut tx = self.pool.begin().await?;
        if !table_exists(&mut tx, PUBLICATIONS).await? {
            return Ok(Vec::new());
        }
        let rows = sqlx::query(
            "SELECT invocation, session_id, exit_code, capture_name, expected_bytes, prefix_sha256, partial, start_byte, bytes
             FROM alab_shell_diagnostic_publications WHERE thread_id = ? ORDER BY rowid LIMIT ?",
        )
        .bind(thread.to_string())
        .bind(MAX_INVOCATIONS)
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        rows.into_iter()
            .map(|row| ShellDiagnosticPublication::from_row(thread, row))
            .collect()
    }

    /// The file publisher has verified the named bytes. Attach and consume the
    /// matching reservation under the same write transaction, rechecking the exit.
    pub async fn complete_shell_diagnostic_publication(
        &self,
        intent: &ShellDiagnosticPublication,
    ) -> anyhow::Result<bool> {
        let mut tx = self.pool.begin_immediate().await?;
        if !take_publication(&mut tx, intent).await? {
            return Ok(false);
        }
        let attached = attach_diagnostic(
            &mut tx,
            &intent.thread_id.to_string(),
            intent.invocation,
            intent.session_id,
            intent.exit_code,
            intent.diagnostic(),
        )
        .await?;
        tx.commit().await?;
        Ok(attached)
    }

    /// Consume a definitively mismatched file receipt under the publisher lock.
    /// A previous tool's pathname remains its immutable identity but is withheld
    /// from future observations. Independently recorded EOF bytes stay readable.
    /// Some(true) preserves that already-disclosed file. None preserves an
    /// ambiguous final name. Some(false) leaves only a private stage to retire.
    pub async fn reject_shell_diagnostic_publication(
        &self,
        intent: &ShellDiagnosticPublication,
    ) -> anyhow::Result<Option<bool>> {
        let mut tx = self.pool.begin_immediate().await?;
        if !take_publication(&mut tx, intent).await? {
            return Ok(None);
        }
        let thread = intent.thread_id.to_string();
        let id = intent.invocation.as_bytes().as_slice();
        let disclosed: Option<bool> = sqlx::query_scalar(
            "SELECT capture_name IS ? FROM alab_shell_invocations WHERE thread_id = ?
             AND invocation = ? AND session_id = ? AND finished AND exit_code = ?",
        )
        .bind(&intent.capture_name)
        .bind(&thread)
        .bind(id)
        .bind(intent.session_id)
        .bind(intent.exit_code)
        .fetch_optional(&mut *tx)
        .await?;
        if disclosed == Some(true) {
            sqlx::query(
                "CREATE TABLE IF NOT EXISTS alab_rejected_shell_captures (
                    thread_id TEXT NOT NULL, invocation BLOB NOT NULL, capture_name TEXT NOT NULL,
                    PRIMARY KEY (thread_id, invocation),
                    FOREIGN KEY (thread_id, invocation)
                        REFERENCES alab_shell_invocations(thread_id, invocation) ON DELETE CASCADE
                )",
            )
            .execute(&mut *tx)
            .await?;
            sqlx::query(
                "INSERT INTO alab_rejected_shell_captures (thread_id, invocation, capture_name)
                 VALUES (?, ?, ?) ON CONFLICT (thread_id, invocation) DO NOTHING",
            )
            .bind(&thread)
            .bind(id)
            .bind(&intent.capture_name)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(disclosed)
    }

    /// A read alone never collects a result. An older database has no pending receipts.
    pub async fn pending_shell_exits(
        &self,
        thread: ThreadId,
    ) -> anyhow::Result<Vec<PendingShellExit>> {
        let mut tx = self.pool.begin().await?;
        let mut exits = Vec::new();
        if table_exists(&mut tx, PENDING_EXITS).await? {
            let rows = sqlx::query(with_evidence_joins!(
                evidence_tables(&mut tx, LAUNCHES).await?,
                "SELECT p.invocation, s.session_id, p.exit_code, NULLIF(s.capture_name, r.capture_name) AS capture_name, r.capture_name IS NOT NULL AS capture_rejected,
                        l.command, l.command_truncated, l.cwd, CASE WHEN r.capture_name IS NULL THEN d.partial END AS partial, t.start_byte, t.bytes
                 FROM alab_pending_shell_exits p JOIN alab_shell_invocations s USING (thread_id, invocation)",
                "WHERE p.thread_id = ? ORDER BY p.rowid LIMIT ?"
            ))
            .bind(thread.to_string())
            .bind(MAX_INVOCATIONS)
            .fetch_all(&mut *tx)
            .await?;
            for row in rows {
                exits.push(PendingShellExit {
                    invocation: Uuid::from_slice(row.try_get("invocation")?)?,
                    session_id: row.try_get("session_id")?,
                    exit_code: row.try_get("exit_code")?,
                    capture_name: row.try_get("capture_name")?,
                    capture_rejected: row.try_get("capture_rejected")?,
                    capture_partial: row.try_get("partial")?,
                    tail: tail_from_row(&row)?,
                    launch: ShellLaunchContext::from_row(&row)?,
                });
            }
        }
        tx.commit().await?;
        Ok(exits)
    }

    /// Collect only the observations in the accepted payload. If a further
    /// receipt supplied a code or a late tool result supplied launch facts or a log,
    /// it will still be offered on a subsequent resume. The snapshots name the
    /// database facts we matched. Only paths rendered in that payload were disclosed.
    pub async fn acknowledge_shell_exits(
        &self,
        thread: ThreadId,
        exits: &[PendingShellExit],
        disclosed_paths: &HashSet<Uuid>,
    ) -> anyhow::Result<()> {
        if exits.is_empty() {
            return Ok(());
        }
        let thread = thread.to_string();
        let mut tx = self.pool.begin_immediate().await?;
        if table_exists(&mut tx, PENDING_EXITS).await? {
            let delete = with_evidence_joins!(
                evidence_tables(&mut tx, LAUNCHES).await?,
                "DELETE FROM alab_pending_shell_exits WHERE thread_id = ? AND invocation = ?
                 AND exit_code IS ? AND EXISTS (
                     SELECT 1 FROM alab_shell_invocations s",
                "WHERE s.thread_id = alab_pending_shell_exits.thread_id
                       AND s.invocation = alab_pending_shell_exits.invocation
                       AND s.session_id = ? AND NULLIF(s.capture_name, r.capture_name) IS ?
                       AND (r.capture_name IS NOT NULL) = ?
                       AND l.command IS ? AND l.command_truncated IS ? AND l.cwd IS ?
                       AND (CASE WHEN r.capture_name IS NULL THEN d.partial END) IS ?
                       AND t.start_byte IS ? AND t.bytes IS ?
                 )"
            );
            for exit in exits {
                let launch = exit.launch.as_ref();
                let collected = sqlx::query(delete)
                    .bind(&thread)
                    .bind(exit.invocation.as_bytes().as_slice())
                    .bind(exit.exit_code)
                    .bind(exit.session_id)
                    .bind(exit.capture_name.as_deref())
                    .bind(exit.capture_rejected)
                    .bind(launch.map(|l| l.command.as_str()))
                    .bind(launch.map(|l| l.command_truncated))
                    .bind(launch.and_then(|l| l.cwd.as_deref()))
                    .bind(exit.capture_partial)
                    .bind(
                        exit.tail
                            .as_ref()
                            .map(|tail| tail.start_byte().to_be_bytes().to_vec()),
                    )
                    .bind(exit.tail.as_ref().map(ShellOutputTail::bytes))
                    .execute(&mut *tx)
                    .await?;
                if collected.rows_affected() == 1
                    && exit.capture_partial.is_none()
                    && exit.exit_code.is_some_and(|code| code != 0 && code != -1)
                {
                    // The output reader may finish after the model accepts the exit.
                    // A stored name can outlive the file the renderer checked.
                    ensure_diagnostics(&mut tx).await?;
                    sqlx::query(
                        "INSERT INTO alab_watched_shell_diagnostics (thread_id, invocation, path_acknowledged)
                         VALUES (?, ?, ?) ON CONFLICT (thread_id, invocation) DO UPDATE
                         SET path_acknowledged = path_acknowledged OR excluded.path_acknowledged
                         WHERE partial IS NULL",
                    )
                    .bind(&thread)
                    .bind(exit.invocation.as_bytes().as_slice())
                    .bind(exit.capture_name.is_some() && disclosed_paths.contains(&exit.invocation))
                    .execute(&mut *tx)
                    .await?;
                }
            }
        }
        tx.commit().await?;
        Ok(())
    }
}

async fn publication_for(
    connection: &mut sqlx::SqliteConnection,
    intent: &ShellDiagnosticPublication,
) -> anyhow::Result<Option<ShellDiagnosticPublication>> {
    let row = sqlx::query(
        "SELECT invocation, session_id, exit_code, capture_name, expected_bytes, prefix_sha256, partial, start_byte, bytes
         FROM alab_shell_diagnostic_publications WHERE thread_id = ? AND invocation = ?",
    )
    .bind(intent.thread_id.to_string()).bind(intent.invocation.as_bytes().as_slice())
    .fetch_optional(connection).await?;
    row.map(|row| ShellDiagnosticPublication::from_row(intent.thread_id, row))
        .transpose()
}

async fn take_publication(
    connection: &mut sqlx::SqliteConnection,
    intent: &ShellDiagnosticPublication,
) -> anyhow::Result<bool> {
    if !table_exists(connection, PUBLICATIONS).await?
        || publication_for(connection, intent).await?.as_ref() != Some(intent)
    {
        return Ok(false);
    }
    // All callers hold BEGIN IMMEDIATE from before the exact snapshot comparison.
    let deleted = sqlx::query(
        "DELETE FROM alab_shell_diagnostic_publications WHERE thread_id = ? AND invocation = ?",
    )
    .bind(intent.thread_id.to_string())
    .bind(intent.invocation.as_bytes().as_slice())
    .execute(connection)
    .await?;
    Ok(deleted.rows_affected() == 1)
}

struct DiagnosticState {
    has_diagnostics: bool,
    has_tails: bool,
    prior: Option<(Option<bool>, bool)>,
}

async fn diagnostic_state(
    connection: &mut sqlx::SqliteConnection,
    thread: &str,
    invocation: Uuid,
    session_id: i32,
    exit_code: i32,
    capture_name: &str,
) -> anyhow::Result<Option<DiagnosticState>> {
    if exit_code == 0 || exit_code == -1 {
        return Ok(None);
    }
    let id = invocation.as_bytes().as_slice();
    let (has_pending, has_diagnostics, has_tails, has_rejections) =
        evidence_tables(connection, PENDING_EXITS).await?;
    if !has_pending {
        return Ok(None);
    }
    if has_rejections {
        let rejected: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM alab_rejected_shell_captures
             WHERE thread_id = ? AND invocation = ? AND capture_name = ?)",
        )
        .bind(thread)
        .bind(id)
        .bind(capture_name)
        .fetch_one(&mut *connection)
        .await?;
        if rejected {
            return Ok(None);
        }
    }
    let original: Option<(Option<String>, bool)> = sqlx::query_as(
        "SELECT s.capture_name, EXISTS (
             SELECT 1 FROM alab_pending_shell_exits p
             WHERE p.thread_id = s.thread_id AND p.invocation = s.invocation AND p.exit_code = s.exit_code
         ) FROM alab_shell_invocations s
         WHERE s.thread_id = ? AND s.invocation = ? AND s.session_id = ? AND s.finished AND s.exit_code = ?",
    )
    .bind(thread)
    .bind(id)
    .bind(session_id)
    .bind(exit_code)
    .fetch_optional(&mut *connection)
    .await?;
    let Some((original_name, pending)) = original else {
        return Ok(None);
    };
    if original_name
        .as_deref()
        .is_some_and(|name| name != capture_name)
    {
        return Ok(None);
    }
    let prior: Option<(Option<bool>, bool)> = if has_diagnostics {
        sqlx::query_as(
            "SELECT partial, path_acknowledged FROM alab_watched_shell_diagnostics WHERE thread_id = ? AND invocation = ?",
        )
        .bind(thread)
        .bind(id)
        .fetch_optional(&mut *connection)
        .await?
    } else {
        None
    };
    if !pending && prior.is_none() {
        return Ok(None);
    }
    Ok(Some(DiagnosticState {
        has_diagnostics,
        has_tails,
        prior,
    }))
}

async fn attach_diagnostic(
    connection: &mut sqlx::SqliteConnection,
    thread: &str,
    invocation: Uuid,
    session_id: i32,
    exit_code: i32,
    diagnostic: WatchedShellDiagnostic<'_>,
) -> anyhow::Result<bool> {
    let WatchedShellDiagnostic {
        capture_name,
        partial,
        tail,
    } = diagnostic;
    if exit_code == 0 || exit_code == -1 {
        return Ok(false);
    }
    validate_capture_name(capture_name)?;
    anyhow::ensure!(
        partial || tail.is_none(),
        "shell output tail requires a partial prefix"
    );
    let id = invocation.as_bytes().as_slice();
    let Some(DiagnosticState {
        has_diagnostics,
        has_tails,
        prior,
    }) = diagnostic_state(
        connection,
        thread,
        invocation,
        session_id,
        exit_code,
        capture_name,
    )
    .await?
    else {
        return Ok(false);
    };
    anyhow::ensure!(
        tail.is_none() || !matches!(prior, Some((Some(false), _))),
        "shell output tail contradicts the complete retained file"
    );
    if !partial && has_tails && tail_exists(connection, thread, invocation).await? {
        // A settled overflow tail proves that this physical prefix was capped.
        return Ok(true);
    }
    let new_tail = if let Some(tail) = tail {
        store_tail(connection, thread, invocation, capture_name, tail).await?
    } else {
        false
    };
    if let Some((Some(was_partial), _)) = prior
        && (!was_partial || partial)
        && !new_tail
    {
        return Ok(true);
    }
    if !has_diagnostics {
        ensure_diagnostics(connection).await?;
    }
    // The IMMEDIATE transaction freezes identity and eligibility for all three writes.
    sqlx::query(
        "INSERT INTO alab_watched_shell_diagnostics (thread_id, invocation, partial) VALUES (?, ?, ?)
         ON CONFLICT (thread_id, invocation) DO UPDATE SET partial = excluded.partial",
    )
    .bind(thread)
    .bind(id)
    .bind(partial)
    .execute(&mut *connection)
    .await?;
    sqlx::query(
        "UPDATE alab_shell_invocations SET capture_name = ? WHERE thread_id = ? AND invocation = ?",
    )
    .bind(capture_name)
    .bind(thread)
    .bind(id)
    .execute(&mut *connection)
    .await?;
    // A complete file renders just like the identical path already sent. A
    // partial correction, a new file, or a formerly partial file changes the report.
    if partial || prior != Some((None, true)) {
        enqueue_diagnostic(connection, thread, invocation, exit_code).await?;
    }
    Ok(true)
}

fn validate_capture_name(name: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !name.is_empty()
            && name.len() <= 96
            && name != "."
            && name != ".."
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.')),
        "invalid shell capture basename"
    );
    Ok(())
}

fn tail_from_row(row: &SqliteRow) -> anyhow::Result<Option<ShellOutputTail>> {
    let Some(start) = row.try_get::<Option<&[u8]>, _>("start_byte")? else {
        return Ok(None);
    };
    let start = u64::from_be_bytes(start.try_into()?);
    let bytes = row.try_get::<Vec<u8>, _>("bytes")?;
    let tail = ShellOutputTail::from_bytes(start, bytes)
        .ok_or_else(|| anyhow::anyhow!("invalid stored shell output tail"))?;
    Ok(Some(tail))
}

async fn tail_exists(
    connection: &mut sqlx::SqliteConnection,
    thread: &str,
    invocation: Uuid,
) -> sqlx::Result<bool> {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM alab_shell_output_tails WHERE thread_id = ? AND invocation = ?)")
        .bind(thread)
        .bind(invocation.as_bytes().as_slice())
        .fetch_one(connection)
        .await
}

/// A tail is captured at real EOF. A retry must present the same range, bytes
/// and prefix file. Big-endian bytes preserve the full unsigned native offset.
async fn store_tail(
    connection: &mut sqlx::SqliteConnection,
    thread: &str,
    invocation: Uuid,
    capture_name: &str,
    tail: &ShellOutputTail,
) -> anyhow::Result<bool> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS alab_shell_output_tails (
            thread_id TEXT NOT NULL, invocation BLOB NOT NULL, capture_name TEXT NOT NULL,
            start_byte BLOB NOT NULL CHECK(typeof(start_byte) = 'blob' AND length(start_byte) = 8),
            bytes BLOB NOT NULL CHECK(typeof(bytes) = 'blob' AND length(bytes) BETWEEN 1 AND 4096),
            PRIMARY KEY (thread_id, invocation),
            FOREIGN KEY (thread_id, invocation)
                REFERENCES alab_shell_invocations(thread_id, invocation) ON DELETE CASCADE
        )",
    )
    .execute(&mut *connection)
    .await?;
    let start = tail.start_byte().to_be_bytes();
    let inserted = sqlx::query(
        "INSERT INTO alab_shell_output_tails (thread_id, invocation, capture_name, start_byte, bytes)
         VALUES (?, ?, ?, ?, ?) ON CONFLICT (thread_id, invocation) DO NOTHING",
    )
    .bind(thread)
    .bind(invocation.as_bytes().as_slice())
    .bind(capture_name)
    .bind(start.as_slice())
    .bind(tail.bytes())
    .execute(&mut *connection)
    .await?;
    if inserted.rows_affected() == 1 {
        return Ok(true);
    }
    let identical: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM alab_shell_output_tails WHERE thread_id = ? AND invocation = ?
         AND capture_name = ? AND start_byte = ? AND bytes = ?)",
    )
    .bind(thread)
    .bind(invocation.as_bytes().as_slice())
    .bind(capture_name)
    .bind(start.as_slice())
    .bind(tail.bytes())
    .fetch_one(connection)
    .await?;
    anyhow::ensure!(identical, "shell output tail disagrees with original EOF");
    Ok(false)
}

async fn evidence_tables(
    connection: &mut sqlx::SqliteConnection,
    first: &str,
) -> sqlx::Result<(bool, bool, bool, bool)> {
    sqlx::query_as(
        "SELECT EXISTS (SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?),
                EXISTS (SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?),
                EXISTS (SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?),
                EXISTS (SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?)",
    )
    .bind(first)
    .bind(DIAGNOSTICS)
    .bind(TAILS)
    .bind(REJECTED_CAPTURES)
    .fetch_one(connection)
    .await
}

async fn ensure_diagnostics(connection: &mut sqlx::SqliteConnection) -> sqlx::Result<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS alab_watched_shell_diagnostics (
            thread_id TEXT NOT NULL, invocation BLOB NOT NULL, partial INTEGER,
            path_acknowledged INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (thread_id, invocation),
            FOREIGN KEY (thread_id, invocation)
                REFERENCES alab_shell_invocations(thread_id, invocation) ON DELETE CASCADE
        )",
    )
    .execute(connection)
    .await?;
    Ok(())
}

async fn ensure_pending_exits(connection: &mut sqlx::SqliteConnection) -> sqlx::Result<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS alab_pending_shell_exits (
            thread_id TEXT NOT NULL, invocation BLOB NOT NULL, exit_code INTEGER,
            PRIMARY KEY (thread_id, invocation),
            FOREIGN KEY (thread_id, invocation)
                REFERENCES alab_shell_invocations(thread_id, invocation) ON DELETE CASCADE
        )",
    )
    .execute(connection)
    .await?;
    Ok(())
}

async fn enqueue_diagnostic(
    connection: &mut sqlx::SqliteConnection,
    thread: &str,
    invocation: Uuid,
    code: i32,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO alab_pending_shell_exits (thread_id, invocation, exit_code) VALUES (?, ?, ?)
         ON CONFLICT (thread_id, invocation) DO UPDATE SET exit_code = excluded.exit_code
         WHERE alab_pending_shell_exits.exit_code IS NULL",
    )
    .bind(thread)
    .bind(invocation.as_bytes().as_slice())
    .bind(code)
    .execute(connection)
    .await?;
    Ok(())
}

async fn table_exists(connection: &mut sqlx::SqliteConnection, name: &str) -> sqlx::Result<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?)",
    )
    .bind(name)
    .fetch_one(connection)
    .await
}

#[cfg(test)]
#[path = "shell_invocation_state_tests.rs"]
mod tests;
