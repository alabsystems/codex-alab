// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::SqliteConfig;
use crate::runtime::test_support::test_thread_metadata;
use tempfile::TempDir;

impl StateRuntime {
    async fn acknowledge_all_named_shell_exits(
        &self,
        thread: ThreadId,
        exits: &[PendingShellExit],
    ) -> anyhow::Result<()> {
        let disclosed_paths = exits
            .iter()
            .filter(|exit| exit.capture_name.is_some())
            .map(|exit| exit.invocation)
            .collect();
        self.acknowledge_shell_exits(thread, exits, &disclosed_paths)
            .await
    }
}

fn diagnostic<'a>(
    capture_name: &'a str,
    partial: bool,
    tail: Option<&'a ShellOutputTail>,
) -> WatchedShellDiagnostic<'a> {
    WatchedShellDiagnostic {
        capture_name,
        partial,
        tail,
    }
}

#[tokio::test]
async fn interrupted_publication_reopens_and_attaches_only_the_exact_native_receipt()
-> anyhow::Result<()> {
    let home = TempDir::new()?;
    let config = SqliteConfig::new_for_testing(home.path().to_owned().try_into()?);
    let state = StateRuntime::init(config.clone(), "test".to_owned()).await?;
    let thread = ThreadId::new();
    let fork = ThreadId::new();
    for id in [thread, fork] {
        state
            .upsert_thread(&test_thread_metadata(
                home.path(),
                id,
                home.path().to_owned(),
            ))
            .await?;
    }
    let invocation = Uuid::new_v4();
    let prefix = codex_alab_core::SHELL_OUTPUT_PREFIX_BYTES as u64;
    let intent = ShellDiagnosticPublication {
        thread_id: thread,
        invocation,
        session_id: 17,
        exit_code: 62,
        capture_name: "stderr.log".into(),
        expected_bytes: prefix,
        prefix_sha256: "a".repeat(64),
        partial: true,
        tail: ShellOutputTail::from_bytes(prefix, vec![0xff]),
    };
    state
        .record_shell_invocation(
            thread,
            invocation,
            17,
            true,
            Some(62),
            None,
            false,
            "",
            true,
            Some(("targo trust build", None)),
            true,
            None,
        )
        .await?;
    assert!(
        state
            .pending_shell_diagnostic_publications(thread)
            .await?
            .is_empty()
    );
    let mut connection = state.pool.acquire().await?;
    assert!(!table_exists(&mut connection, PUBLICATIONS).await?);
    drop(connection);
    let stale_digest = ShellDiagnosticPublication {
        prefix_sha256: "b".repeat(64),
        ..intent.clone()
    };
    for invalid in [
        ShellDiagnosticPublication {
            prefix_sha256: "A".repeat(64),
            ..intent.clone()
        },
        ShellDiagnosticPublication {
            expected_bytes: prefix - 1,
            ..intent.clone()
        },
    ] {
        assert!(
            state
                .reserve_shell_diagnostic_publication(&invalid)
                .await
                .is_err()
        );
    }
    for foreign in [
        ShellDiagnosticPublication {
            thread_id: fork,
            ..intent.clone()
        },
        ShellDiagnosticPublication {
            session_id: 16,
            ..intent.clone()
        },
        ShellDiagnosticPublication {
            exit_code: 1,
            ..intent.clone()
        },
    ] {
        assert!(!state.reserve_shell_diagnostic_publication(&foreign).await?);
    }
    assert!(state.reserve_shell_diagnostic_publication(&intent).await?);
    assert!(state.reserve_shell_diagnostic_publication(&intent).await?);
    assert!(
        !state
            .reserve_shell_diagnostic_publication(&stale_digest)
            .await?
    );
    assert_eq!(
        state
            .reject_shell_diagnostic_publication(&stale_digest)
            .await?,
        None
    );
    assert!(
        !state
            .complete_shell_diagnostic_publication(&stale_digest)
            .await?
    );
    assert!(state.pending_shell_exits(thread).await?.is_empty());
    assert!(
        state
            .pending_shell_diagnostic_publications(fork)
            .await?
            .is_empty()
    );
    sqlx::query(
        "CREATE TRIGGER interrupt_diagnostic BEFORE UPDATE OF capture_name ON alab_shell_invocations
         BEGIN SELECT RAISE(ABORT, 'fixture interrupted attachment'); END"
    ).execute(state.pool.as_ref()).await?;
    assert!(
        state
            .complete_shell_diagnostic_publication(&intent)
            .await
            .is_err()
    );
    sqlx::query("DROP TRIGGER interrupt_diagnostic")
        .execute(state.pool.as_ref())
        .await?;
    state.close().await;
    let state = StateRuntime::init(config, "test".to_owned()).await?;
    assert_eq!(
        state.pending_shell_diagnostic_publications(thread).await?,
        vec![intent.clone()]
    );
    assert!(state.pending_shell_exits(thread).await?.is_empty());
    assert!(state.complete_shell_diagnostic_publication(&intent).await?);
    let pending = state.pending_shell_exits(thread).await?;
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].capture_name.as_deref(), Some("stderr.log"));
    assert_eq!(pending[0].capture_partial, Some(true));
    assert_eq!(pending[0].tail, intent.tail);
    assert!(
        state
            .pending_shell_diagnostic_publications(thread)
            .await?
            .is_empty()
    );
    state
        .acknowledge_all_named_shell_exits(thread, &pending)
        .await?;
    assert!(!state.complete_shell_diagnostic_publication(&intent).await?);
    assert!(state.pending_shell_exits(thread).await?.is_empty());
    state.close().await;
    Ok(())
}

#[tokio::test]
async fn a_definitively_rejected_tool_path_is_hidden_without_replaying_its_exit()
-> anyhow::Result<()> {
    let home = TempDir::new()?;
    let config = SqliteConfig::new_for_testing(home.path().to_owned().try_into()?);
    let state = StateRuntime::init(config, "test".to_owned()).await?;
    let thread = ThreadId::new();
    state
        .upsert_thread(&test_thread_metadata(
            home.path(),
            thread,
            home.path().to_owned(),
        ))
        .await?;
    for accepted in [false, true] {
        let id = Uuid::new_v4();
        let name = format!("{id}.log");
        let prefix = codex_alab_core::SHELL_OUTPUT_PREFIX_BYTES as u64;
        let intent = ShellDiagnosticPublication {
            thread_id: thread,
            invocation: id,
            session_id: 17,
            exit_code: 62,
            capture_name: name.clone(),
            expected_bytes: prefix,
            prefix_sha256: "a".repeat(64),
            partial: true,
            tail: ShellOutputTail::from_bytes(prefix, vec![0xff]),
        };
        state
            .record_shell_invocation(
                thread,
                id,
                17,
                false,
                None,
                Some(&name),
                true,
                "running",
                true,
                None,
                false,
                None,
            )
            .await?;
        state
            .record_watched_shell_exit(thread, id, 17, Some(62))
            .await?;
        if accepted {
            assert!(
                state
                    .record_watched_shell_diagnostic(
                        thread,
                        id,
                        17,
                        62,
                        diagnostic(&name, true, intent.tail.as_ref()),
                    )
                    .await?
            );
        }
        let before = state.pending_shell_exits(thread).await?;
        assert_eq!(before.len(), 1);
        assert_eq!(before[0].capture_name.as_deref(), Some(name.as_str()));
        assert!(!before[0].capture_rejected);
        if accepted {
            state
                .acknowledge_all_named_shell_exits(thread, &before)
                .await?;
        }
        assert!(state.reserve_shell_diagnostic_publication(&intent).await?);
        let wrong = ShellDiagnosticPublication {
            prefix_sha256: "b".repeat(64),
            ..intent.clone()
        };
        assert_eq!(
            state.reject_shell_diagnostic_publication(&wrong).await?,
            None
        );
        assert_eq!(
            state
                .shell_invocation(thread, id)
                .await?
                .expect("original")
                .capture_name
                .as_deref(),
            Some(name.as_str())
        );
        assert_eq!(
            state.reject_shell_diagnostic_publication(&intent).await?,
            Some(true)
        );
        let row = state.shell_invocation(thread, id).await?.expect("original");
        assert_eq!(row.capture_name, None);
        assert!(row.capture_rejected);
        assert_eq!(row.capture_partial, None);
        assert_eq!(row.exit_code, Some(62));
        assert_eq!(row.tail, before[0].tail);
        assert!(!state.reserve_shell_diagnostic_publication(&intent).await?);
        assert!(
            !state
                .record_watched_shell_diagnostic(
                    thread,
                    id,
                    17,
                    62,
                    diagnostic(&name, true, intent.tail.as_ref())
                )
                .await?
        );
        state
            .acknowledge_all_named_shell_exits(thread, &before)
            .await?;
        let after = state.pending_shell_exits(thread).await?;
        if accepted {
            assert!(after.is_empty());
        } else {
            assert_eq!(after.len(), 1);
            assert_eq!(after[0].exit_code, Some(62));
            assert_eq!(after[0].capture_name, None);
            assert!(after[0].capture_rejected);
            assert_eq!(after[0].tail, None);
            // A report with no capture information cannot collect evidence that
            // the original pathname was definitively rejected.
            let pathless = PendingShellExit {
                capture_rejected: false,
                ..after[0].clone()
            };
            state
                .acknowledge_shell_exits(thread, &[pathless], &HashSet::new())
                .await?;
            assert_eq!(state.pending_shell_exits(thread).await?, after);
            state
                .acknowledge_all_named_shell_exits(thread, &after)
                .await?;
        }
        assert!(state.pending_shell_exits(thread).await?.is_empty());
    }
    state.close().await;
    Ok(())
}

#[tokio::test]
async fn a_resume_acknowledges_only_the_watcher_facts_actually_reported() -> anyhow::Result<()> {
    let home = TempDir::new()?;
    let config = SqliteConfig::new_for_testing(home.path().to_owned().try_into()?);
    let state = StateRuntime::init(config.clone(), "test".to_owned()).await?;
    let thread = ThreadId::new();
    state
        .upsert_thread(&test_thread_metadata(
            home.path(),
            thread,
            home.path().to_owned(),
        ))
        .await?;
    let invocation = Uuid::new_v4();
    let exit = PendingShellExit {
        invocation,
        session_id: 11,
        exit_code: None,
        capture_name: None,
        capture_rejected: false,
        capture_partial: None,
        tail: None,
        launch: None,
    };

    // A legacy terminal tool result, without a watcher receipt, remains historical data.
    let legacy = Uuid::new_v4();
    state
        .record_shell_invocation(
            thread,
            legacy,
            10,
            true,
            Some(0),
            None,
            false,
            "done",
            false,
            None,
            false,
            None,
        )
        .await?;
    assert!(state.pending_shell_exits(thread).await?.is_empty());
    state
        .record_watched_shell_exit(thread, legacy, 10, Some(0))
        .await?;
    assert!(state.pending_shell_exits(thread).await?.is_empty());

    state
        .record_shell_invocation(
            thread, invocation, 11, false, None, None, false, "running", false, None, false, None,
        )
        .await?;
    state
        .record_watched_shell_exit(thread, invocation, 11, None)
        .await?;
    assert_eq!(
        state.pending_shell_exits(thread).await?.as_slice(),
        std::slice::from_ref(&exit)
    );
    // An older database retains the old observation and pending tables alone.
    let mut connection = state.pool.acquire().await?;
    assert!(!table_exists(&mut connection, LAUNCHES).await?);
    assert!(!table_exists(&mut connection, DIAGNOSTICS).await?);
    assert!(!table_exists(&mut connection, TAILS).await?);
    drop(connection);
    // A richer observation survives delivery of the initial unknown status.
    state
        .record_watched_shell_exit(thread, invocation, 11, Some(23))
        .await?;
    state
        .acknowledge_all_named_shell_exits(thread, std::slice::from_ref(&exit))
        .await?;
    let with_code = PendingShellExit {
        exit_code: Some(23),
        ..exit
    };
    assert_eq!(
        state.pending_shell_exits(thread).await?.as_slice(),
        std::slice::from_ref(&with_code)
    );
    state
        .record_shell_invocation(
            thread,
            invocation,
            11,
            false,
            None,
            Some("capture.log"),
            true,
            "old",
            false,
            Some(("poll must not supply launch data", None)),
            false,
            None,
        )
        .await?;
    state
        .acknowledge_all_named_shell_exits(thread, std::slice::from_ref(&with_code))
        .await?;
    let with_capture = PendingShellExit {
        capture_name: Some("capture.log".into()),
        ..with_code
    };
    assert_eq!(
        state.pending_shell_exits(thread).await?.as_slice(),
        std::slice::from_ref(&with_capture)
    );
    // The original exec response can arrive after a poll and the watcher.
    // Accepting the payload already in flight cannot collect the new label.
    let cwd = if cfg!(windows) {
        r"C:\work tree"
    } else {
        "/work tree"
    };
    state
        .record_shell_invocation(
            thread,
            invocation,
            11,
            false,
            None,
            None,
            false,
            "initial",
            true,
            Some(("targo trust check", Some(cwd))),
            false,
            None,
        )
        .await?;
    state
        .acknowledge_all_named_shell_exits(thread, std::slice::from_ref(&with_capture))
        .await?;
    state
        .record_shell_invocation(
            thread,
            invocation,
            11,
            false,
            None,
            None,
            false,
            "replay",
            true,
            Some(("different command", None)),
            false,
            None,
        )
        .await?;
    let complete = PendingShellExit {
        launch: Some(ShellLaunchContext {
            command: "targo trust check".into(),
            command_truncated: false,
            cwd: Some(cwd.into()),
        }),
        ..with_capture
    };
    state.close().await;
    let state = StateRuntime::init(config, "test".to_owned()).await?;
    assert_eq!(
        state
            .shell_invocation(thread, legacy)
            .await?
            .expect("legacy observation")
            .launch,
        None
    );
    assert_eq!(
        state.pending_shell_exits(thread).await?.as_slice(),
        std::slice::from_ref(&complete)
    );
    let wrong_number = PendingShellExit {
        session_id: 22,
        ..complete.clone()
    };
    state
        .acknowledge_all_named_shell_exits(thread, &[wrong_number])
        .await?;
    state
        .acknowledge_all_named_shell_exits(ThreadId::new(), std::slice::from_ref(&complete))
        .await?;
    assert_eq!(
        state.pending_shell_exits(thread).await?.as_slice(),
        std::slice::from_ref(&complete)
    );
    state
        .acknowledge_all_named_shell_exits(thread, std::slice::from_ref(&complete))
        .await?;
    state
        .record_watched_shell_exit(thread, invocation, 11, Some(23))
        .await?;
    assert!(state.pending_shell_exits(thread).await?.is_empty());
    let recorded = state
        .shell_invocation(thread, invocation)
        .await?
        .expect("original observation");
    assert_eq!(recorded.exit_code, Some(23));
    assert_eq!(recorded.launch, complete.launch);
    state.close().await;
    Ok(())
}

#[tokio::test]
async fn newly_watched_eof_bytes_survive_acceptance_of_the_prefix_alone() -> anyhow::Result<()> {
    let home = TempDir::new()?;
    let config = SqliteConfig::new_for_testing(home.path().to_owned().try_into()?);
    let state = StateRuntime::init(config.clone(), "test".to_owned()).await?;
    let thread = ThreadId::new();
    state
        .upsert_thread(&test_thread_metadata(
            home.path(),
            thread,
            home.path().to_owned(),
        ))
        .await?;
    let id = Uuid::new_v4();
    let name = "compiler.log";
    let tail = ShellOutputTail::from_bytes(u64::MAX - 4, vec![0xfe, 0, b'!', b'\n'])
        .expect("native EOF range");
    state
        .record_shell_invocation(
            thread,
            id,
            19,
            false,
            None,
            Some(name),
            true,
            "prefix",
            true,
            None,
            false,
            None,
        )
        .await?;
    state
        .record_watched_shell_exit(thread, id, 19, Some(4))
        .await?;
    assert!(
        state
            .record_watched_shell_diagnostic(thread, id, 19, 4, diagnostic(name, true, None))
            .await?
    );
    let earlier = state.pending_shell_exits(thread).await?;
    assert_eq!(earlier.len(), 1);
    assert_eq!(earlier[0].tail, None);
    state
        .acknowledge_all_named_shell_exits(thread, &earlier)
        .await?;
    assert!(
        !state
            .record_watched_shell_diagnostic(
                thread,
                id,
                19,
                4,
                diagnostic("other.log", true, Some(&tail))
            )
            .await?
    );
    assert!(
        state
            .record_watched_shell_diagnostic(thread, id, 19, 4, diagnostic(name, true, Some(&tail)))
            .await?
    );
    // An in-flight payload or a native terminal poll that disclosed only the
    // prefix cannot silently consume the new tail.
    state
        .acknowledge_all_named_shell_exits(thread, &earlier)
        .await?;
    state
        .record_shell_invocation(
            thread,
            id,
            19,
            true,
            Some(4),
            Some(name),
            true,
            "prefix",
            false,
            None,
            false,
            None,
        )
        .await?;
    state.close().await;
    let state = StateRuntime::init(config, "test".to_owned()).await?;
    let fresh = state.pending_shell_exits(thread).await?;
    assert_eq!(fresh.len(), 1);
    assert_eq!(fresh[0].tail.as_ref(), Some(&tail));
    let changed =
        ShellOutputTail::from_bytes(tail.start_byte(), vec![1, 2, 3, 4]).expect("same range");
    let wrong_snapshot = PendingShellExit {
        tail: Some(changed.clone()),
        ..fresh[0].clone()
    };
    state
        .acknowledge_all_named_shell_exits(thread, &[wrong_snapshot])
        .await?;
    assert!(
        state
            .record_watched_shell_diagnostic(
                thread,
                id,
                19,
                4,
                diagnostic(name, true, Some(&changed))
            )
            .await
            .is_err()
    );
    assert_eq!(state.pending_shell_exits(thread).await?, fresh);
    state
        .acknowledge_all_named_shell_exits(thread, &fresh)
        .await?;
    assert!(
        state
            .record_watched_shell_diagnostic(thread, id, 19, 4, diagnostic(name, true, Some(&tail)))
            .await?
    );
    assert!(state.pending_shell_exits(thread).await?.is_empty());
    assert_eq!(
        state
            .shell_invocation(thread, id)
            .await?
            .expect("settled shell")
            .tail,
        Some(tail)
    );
    state.close().await;
    Ok(())
}

#[tokio::test]
async fn an_initial_terminal_result_offers_only_its_later_drained_diagnostic() -> anyhow::Result<()>
{
    let home = TempDir::new()?;
    let config = SqliteConfig::new_for_testing(home.path().to_owned().try_into()?);
    let state = StateRuntime::init(config, "test".to_owned()).await?;
    let thread = ThreadId::new();
    state
        .upsert_thread(&test_thread_metadata(
            home.path(),
            thread,
            home.path().to_owned(),
        ))
        .await?;
    let invocation = Uuid::new_v4();
    let cwd = home.path().to_str().expect("test path");
    let record = |command| {
        state.record_shell_invocation(
            thread,
            invocation,
            17,
            true,
            Some(2),
            None,
            false,
            "",
            true,
            Some((command, Some(cwd))),
            true,
            None,
        )
    };
    // The backend can observe the exit before the first tool result publishes its row.
    state
        .record_watched_shell_exit(thread, invocation, 17, Some(2))
        .await?;
    record("targo trust check").await?;
    assert!(state.pending_shell_exits(thread).await?.is_empty());
    let attach = |number| {
        state.record_watched_shell_diagnostic(
            thread,
            invocation,
            number,
            2,
            diagnostic("failure.log", false, None),
        )
    };
    assert!(!attach(18).await?);
    assert!(attach(17).await?);
    let pending = state.pending_shell_exits(thread).await?;
    assert_eq!(
        pending,
        vec![PendingShellExit {
            invocation,
            session_id: 17,
            exit_code: Some(2),
            capture_name: Some("failure.log".to_owned()),
            capture_rejected: false,
            capture_partial: Some(false),
            tail: None,
            launch: Some(ShellLaunchContext {
                command: "targo trust check".to_owned(),
                command_truncated: false,
                cwd: Some(cwd.to_owned()),
            }),
        }]
    );
    // A late or replayed tool response carries neither the new bytes nor a new launch.
    record("different command").await?;
    assert_eq!(state.pending_shell_exits(thread).await?, pending);
    state
        .acknowledge_all_named_shell_exits(thread, &pending)
        .await?;
    record("different command").await?;
    state
        .record_watched_shell_exit(thread, invocation, 17, Some(2))
        .await?;
    assert!(attach(17).await?);
    assert!(state.pending_shell_exits(thread).await?.is_empty());
    state.close().await;
    Ok(())
}

#[tokio::test]
async fn a_one_shot_tail_that_could_not_fit_history_survives_retry_and_is_collected_once()
-> anyhow::Result<()> {
    let home = TempDir::new()?;
    let config = SqliteConfig::new_for_testing(home.path().to_owned().try_into()?);
    let state = StateRuntime::init(config, "test".to_owned()).await?;
    let thread = ThreadId::new();
    state
        .upsert_thread(&test_thread_metadata(
            home.path(),
            thread,
            home.path().to_owned(),
        ))
        .await?;
    let tail = ShellOutputTail::from_bytes(
        codex_alab_core::SHELL_OUTPUT_PREFIX_BYTES as u64,
        vec![0; codex_alab_core::MAX_SHELL_OUTPUT_TAIL_BYTES],
    )
    .expect("bounded tail");
    for collect_on_resume in [true, false] {
        let id = Uuid::new_v4();
        let record = |undisclosed| {
            state.record_shell_invocation(
                thread,
                id,
                17,
                true,
                Some(2),
                Some("compiler.log"),
                true,
                "prefix",
                true,
                Some(("targo trust check", None)),
                undisclosed,
                Some(&tail),
            )
        };
        record(true).await?;
        let pending = state.pending_shell_exits(thread).await?;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].invocation, id);
        assert_eq!(pending[0].tail.as_ref(), Some(&tail));
        assert_eq!(
            pending[0].launch.as_ref().expect("original launch").command,
            "targo trust check"
        );
        record(true).await?;
        assert_eq!(state.pending_shell_exits(thread).await?, pending);
        if collect_on_resume {
            state
                .acknowledge_all_named_shell_exits(thread, &pending)
                .await?;
        } else {
            record(false).await?;
        }
        assert!(state.pending_shell_exits(thread).await?.is_empty());
        record(true).await?;
        assert!(state.pending_shell_exits(thread).await?.is_empty());
        assert_eq!(
            state
                .shell_invocation(thread, id)
                .await?
                .expect("one-shot evidence")
                .tail
                .as_ref(),
            Some(&tail)
        );
    }
    state.close().await;
    Ok(())
}

#[tokio::test]
async fn native_disclosure_collects_the_exact_tail_in_either_watcher_order() -> anyhow::Result<()> {
    let home = TempDir::new()?;
    let config = SqliteConfig::new_for_testing(home.path().to_owned().try_into()?);
    let state = StateRuntime::init(config, "test".to_owned()).await?;
    let thread = ThreadId::new();
    state
        .upsert_thread(&test_thread_metadata(
            home.path(),
            thread,
            home.path().to_owned(),
        ))
        .await?;
    let name = "compiler.log";
    let tail =
        ShellOutputTail::from_bytes(codex_alab_core::SHELL_OUTPUT_PREFIX_BYTES as u64, vec![7])
            .expect("one overflow byte");
    for watcher_first in [false, true] {
        let id = Uuid::new_v4();
        state
            .record_shell_invocation(
                thread,
                id,
                19,
                false,
                None,
                Some(name),
                true,
                "prefix",
                true,
                None,
                false,
                None,
            )
            .await?;
        if watcher_first {
            state
                .record_watched_shell_exit(thread, id, 19, Some(4))
                .await?;
            assert!(
                state
                    .record_watched_shell_diagnostic(
                        thread,
                        id,
                        19,
                        4,
                        diagnostic(name, true, Some(&tail))
                    )
                    .await?
            );
        }
        assert!(
            state
                .record_shell_invocation(
                    thread,
                    id,
                    19,
                    true,
                    Some(4),
                    Some("wrong.log"),
                    true,
                    "",
                    false,
                    None,
                    false,
                    Some(&tail)
                )
                .await
                .is_err()
        );
        state
            .record_shell_invocation(
                thread,
                id,
                19,
                true,
                Some(4),
                Some(name),
                true,
                "prefix",
                false,
                None,
                false,
                Some(&tail),
            )
            .await?;
        state
            .record_watched_shell_exit(thread, id, 19, Some(4))
            .await?;
        assert!(
            state
                .record_watched_shell_diagnostic(
                    thread,
                    id,
                    19,
                    4,
                    diagnostic(name, true, Some(&tail))
                )
                .await?
        );
        assert!(state.pending_shell_exits(thread).await?.is_empty());
        let observation = state
            .shell_invocation(thread, id)
            .await?
            .expect("retained native observation");
        assert_eq!(observation.capture_partial, Some(true));
        assert_eq!(observation.tail.as_ref(), Some(&tail));
    }
    state.delete_thread(thread).await?;
    let tails: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM alab_shell_output_tails")
        .fetch_one(state.pool.as_ref())
        .await?;
    assert_eq!(tails, 0);
    state.close().await;
    Ok(())
}

#[tokio::test]
async fn an_accepted_bare_failure_can_acquire_its_original_diagnostic() -> anyhow::Result<()> {
    let home = TempDir::new()?;
    let config = SqliteConfig::new_for_testing(home.path().to_owned().try_into()?);
    let state = StateRuntime::init(config.clone(), "test".to_owned()).await?;
    let thread = ThreadId::new();
    state
        .upsert_thread(&test_thread_metadata(
            home.path(),
            thread,
            home.path().to_owned(),
        ))
        .await?;
    let id = Uuid::new_v4();
    state
        .record_shell_invocation(
            thread, id, 17, false, None, None, false, "", true, None, false, None,
        )
        .await?;
    state
        .record_watched_shell_exit(thread, id, 17, Some(9))
        .await?;
    let bare = state.pending_shell_exits(thread).await?;
    assert_eq!(bare.len(), 1);
    state
        .acknowledge_all_named_shell_exits(thread, &bare)
        .await?;
    state.close().await;
    let state = StateRuntime::init(config, "test".to_owned()).await?;
    assert!(state.pending_shell_exits(thread).await?.is_empty());
    for (wrong_thread, wrong_id, wrong_number, wrong_code) in [
        (ThreadId::new(), id, 17, 9),
        (thread, Uuid::new_v4(), 17, 9),
        (thread, id, 3, 9),
        (thread, id, 17, 2),
        (thread, id, 17, 0),
        (thread, id, 17, -1),
    ] {
        assert!(
            !state
                .record_watched_shell_diagnostic(
                    wrong_thread,
                    wrong_id,
                    wrong_number,
                    wrong_code,
                    diagnostic("stderr.log", true, None)
                )
                .await?
        );
    }
    assert!(
        state
            .record_watched_shell_diagnostic(
                thread,
                id,
                17,
                9,
                diagnostic("../outside", true, None)
            )
            .await
            .is_err()
    );
    assert!(state.pending_shell_exits(thread).await?.is_empty());
    assert!(
        state
            .record_watched_shell_diagnostic(
                thread,
                id,
                17,
                9,
                diagnostic("stderr.log", true, None)
            )
            .await?
    );
    state
        .acknowledge_all_named_shell_exits(thread, &bare)
        .await?;
    state
        .record_shell_invocation(
            thread,
            id,
            17,
            true,
            Some(9),
            None,
            false,
            "code only",
            false,
            None,
            false,
            None,
        )
        .await?;
    let partial = state.pending_shell_exits(thread).await?;
    assert_eq!(partial.len(), 1);
    assert_eq!(partial[0].capture_name.as_deref(), Some("stderr.log"));
    assert_eq!(partial[0].capture_partial, Some(true));
    assert!(
        !state
            .record_watched_shell_diagnostic(
                thread,
                id,
                17,
                9,
                diagnostic("other.log", false, None)
            )
            .await?
    );
    assert!(
        state
            .record_watched_shell_diagnostic(
                thread,
                id,
                17,
                9,
                diagnostic("stderr.log", false, None)
            )
            .await?
    );
    state
        .acknowledge_all_named_shell_exits(thread, &partial)
        .await?;
    let full = state.pending_shell_exits(thread).await?;
    assert_eq!(full.len(), 1);
    assert_eq!(full[0].capture_partial, Some(false));
    state
        .acknowledge_all_named_shell_exits(thread, &full)
        .await?;
    for repeated in [true, false] {
        assert!(
            state
                .record_watched_shell_diagnostic(
                    thread,
                    id,
                    17,
                    9,
                    diagnostic("stderr.log", repeated, None)
                )
                .await?
        );
    }
    assert!(state.pending_shell_exits(thread).await?.is_empty());
    let retained = state
        .shell_invocation(thread, id)
        .await?
        .expect("retained observation");
    assert_eq!(retained.capture_name.as_deref(), Some("stderr.log"));
    assert_eq!(retained.capture_partial, Some(false));
    state.close().await;
    Ok(())
}

#[tokio::test]
async fn a_previously_reported_path_requeues_only_when_its_coverage_changes_the_report()
-> anyhow::Result<()> {
    let home = TempDir::new()?;
    let config = SqliteConfig::new_for_testing(home.path().to_owned().try_into()?);
    let state = StateRuntime::init(config, "test".to_owned()).await?;
    let thread = ThreadId::new();
    state
        .upsert_thread(&test_thread_metadata(
            home.path(),
            thread,
            home.path().to_owned(),
        ))
        .await?;
    for (name_at_accept, path_disclosed, poll_disclosed, partial) in [
        (true, true, false, true),
        (true, true, false, false),
        (true, false, false, false),
        (true, false, true, false),
        (false, false, false, false),
    ] {
        let id = Uuid::new_v4();
        let name = "original.log";
        state
            .record_shell_invocation(
                thread,
                id,
                1,
                false,
                None,
                name_at_accept.then_some(name),
                name_at_accept,
                "",
                true,
                None,
                false,
                None,
            )
            .await?;
        state
            .record_watched_shell_exit(thread, id, 1, Some(7))
            .await?;
        let first = state.pending_shell_exits(thread).await?;
        assert_eq!(first.len(), 1);
        assert_eq!(
            first[0].capture_name.as_deref(),
            name_at_accept.then_some(name)
        );
        assert_eq!(first[0].capture_partial, None);
        let disclosed_paths = if path_disclosed {
            HashSet::from([id])
        } else {
            HashSet::new()
        };
        state
            .acknowledge_shell_exits(thread, &first, &disclosed_paths)
            .await?;
        if name_at_accept {
            // A terminal poll still knows the original basename, whether or not
            // the provider received the path in this particular result.
            state
                .record_shell_invocation(
                    thread,
                    id,
                    1,
                    true,
                    Some(7),
                    Some(name),
                    poll_disclosed,
                    "",
                    false,
                    None,
                    false,
                    None,
                )
                .await?;
        } else {
            // The basename is now stored, but wasn't part of the provider's accepted snapshot.
            state
                .record_shell_invocation(
                    thread,
                    id,
                    1,
                    false,
                    None,
                    Some(name),
                    true,
                    "",
                    false,
                    None,
                    false,
                    None,
                )
                .await?;
        }
        if poll_disclosed {
            assert!(
                !state
                    .record_watched_shell_diagnostic(
                        thread,
                        id,
                        1,
                        7,
                        diagnostic(name, partial, None)
                    )
                    .await?
            );
            assert!(state.pending_shell_exits(thread).await?.is_empty());
            continue;
        }
        assert!(
            state
                .record_watched_shell_diagnostic(thread, id, 1, 7, diagnostic(name, partial, None))
                .await?
        );
        state
            .acknowledge_shell_exits(thread, &first, &disclosed_paths)
            .await?;
        let later = state.pending_shell_exits(thread).await?;
        assert_eq!(later.len(), usize::from(partial || !path_disclosed));
        if let Some(fresh) = later.first() {
            assert_eq!(fresh.capture_name.as_deref(), Some(name));
            assert_eq!(fresh.capture_partial, Some(partial));
        }
        let observation = state
            .shell_invocation(thread, id)
            .await?
            .expect("watcher coverage");
        assert_eq!(observation.capture_partial, Some(partial));
        state
            .acknowledge_all_named_shell_exits(thread, &later)
            .await?;
        assert!(
            state
                .record_watched_shell_diagnostic(thread, id, 1, 7, diagnostic(name, partial, None))
                .await?
        );
        assert!(state.pending_shell_exits(thread).await?.is_empty());
    }
    state.close().await;
    Ok(())
}

#[tokio::test]
async fn a_terminal_tool_collects_its_file_and_retires_pathless_watcher_eligibility()
-> anyhow::Result<()> {
    let home = TempDir::new()?;
    let config = SqliteConfig::new_for_testing(home.path().to_owned().try_into()?);
    let state = StateRuntime::init(config, "test".to_owned()).await?;
    let thread = ThreadId::new();
    state
        .upsert_thread(&test_thread_metadata(
            home.path(),
            thread,
            home.path().to_owned(),
        ))
        .await?;
    for provider_accepted in [false, true] {
        let id = Uuid::new_v4();
        state
            .record_shell_invocation(
                thread, id, 1, false, None, None, false, "", true, None, false, None,
            )
            .await?;
        state
            .record_watched_shell_exit(thread, id, 1, Some(7))
            .await?;
        if provider_accepted {
            state
                .acknowledge_all_named_shell_exits(
                    thread,
                    &state.pending_shell_exits(thread).await?,
                )
                .await?;
        }
        state
            .record_shell_invocation(
                thread,
                id,
                1,
                true,
                Some(7),
                None,
                false,
                "terminal",
                false,
                None,
                false,
                None,
            )
            .await?;
        assert!(
            !state
                .record_watched_shell_diagnostic(
                    thread,
                    id,
                    1,
                    7,
                    diagnostic("late.log", false, None)
                )
                .await?
        );
    }
    let id = Uuid::new_v4();
    state
        .record_shell_invocation(
            thread, id, 1, false, None, None, false, "", true, None, false, None,
        )
        .await?;
    state
        .record_watched_shell_exit(thread, id, 1, Some(7))
        .await?;
    assert!(
        state
            .record_watched_shell_diagnostic(
                thread,
                id,
                1,
                7,
                diagnostic("original.log", true, None)
            )
            .await?
    );
    state
        .record_shell_invocation(
            thread,
            id,
            1,
            true,
            Some(7),
            Some("different.log"),
            true,
            "terminal",
            false,
            None,
            false,
            None,
        )
        .await?;
    assert_eq!(state.pending_shell_exits(thread).await?.len(), 1);
    for path_disclosed in [false, true] {
        state
            .record_shell_invocation(
                thread,
                id,
                1,
                true,
                Some(7),
                Some("original.log"),
                path_disclosed,
                "terminal",
                false,
                None,
                false,
                None,
            )
            .await?;
        assert_eq!(
            state.pending_shell_exits(thread).await?.is_empty(),
            path_disclosed
        );
    }
    state.close().await;
    Ok(())
}

#[tokio::test]
async fn a_final_poll_collects_the_status_while_the_original_output_can_still_advance()
-> anyhow::Result<()> {
    let home = TempDir::new()?;
    let config = SqliteConfig::new_for_testing(home.path().to_owned().try_into()?);
    let state = StateRuntime::init(config, "test".to_owned()).await?;
    let thread = ThreadId::new();
    state
        .upsert_thread(&test_thread_metadata(
            home.path(),
            thread,
            home.path().to_owned(),
        ))
        .await?;
    let name = "failure.log";
    for (watcher_first, provider_accepted) in [(false, false), (true, false), (true, true)] {
        for disclosed_path in [None, Some(name)] {
            let id = Uuid::new_v4();
            state
                .record_shell_invocation(
                    thread,
                    id,
                    19,
                    false,
                    None,
                    disclosed_path,
                    disclosed_path.is_some(),
                    "",
                    true,
                    None,
                    false,
                    None,
                )
                .await?;
            if watcher_first {
                state
                    .record_watched_shell_exit(thread, id, 19, Some(4))
                    .await?;
            }
            if provider_accepted {
                let earlier = state.pending_shell_exits(thread).await?;
                assert_eq!(earlier.len(), 1);
                state
                    .acknowledge_all_named_shell_exits(thread, &earlier)
                    .await?;
            }
            state
                .record_shell_invocation(
                    thread,
                    id,
                    19,
                    true,
                    Some(4),
                    disclosed_path,
                    disclosed_path.is_some(),
                    "prefix",
                    false,
                    None,
                    true,
                    None,
                )
                .await?;
            state
                .record_watched_shell_exit(thread, id, 19, Some(4))
                .await?;
            assert!(state.pending_shell_exits(thread).await?.is_empty());
            // Only a real callback proving an output advance may offer the eventual file,
            // even when earlier previews already named it.
            assert!(
                state
                    .record_watched_shell_diagnostic(
                        thread,
                        id,
                        19,
                        4,
                        diagnostic(name, false, None)
                    )
                    .await?
            );
            let advanced = state.pending_shell_exits(thread).await?;
            assert_eq!(advanced.len(), 1);
            assert_eq!(advanced[0].invocation, id);
            assert_eq!(advanced[0].capture_name.as_deref(), Some(name));
            assert_eq!(advanced[0].capture_partial, Some(false));
            // A late write of the same poll cannot acknowledge a file ahead of its watermark.
            state
                .record_shell_invocation(
                    thread,
                    id,
                    19,
                    true,
                    Some(4),
                    Some(name),
                    true,
                    "prefix",
                    false,
                    None,
                    true,
                    None,
                )
                .await?;
            assert_eq!(state.pending_shell_exits(thread).await?, advanced);
            state
                .acknowledge_all_named_shell_exits(thread, &advanced)
                .await?;
            assert!(
                state
                    .record_watched_shell_diagnostic(
                        thread,
                        id,
                        19,
                        4,
                        diagnostic(name, false, None)
                    )
                    .await?
            );
            assert!(state.pending_shell_exits(thread).await?.is_empty());
        }
    }
    state.close().await;
    Ok(())
}

#[tokio::test]
async fn a_terminal_poll_collects_only_the_outcome_it_contains() -> anyhow::Result<()> {
    let home = TempDir::new()?;
    let config = SqliteConfig::new_for_testing(home.path().to_owned().try_into()?);
    let state = StateRuntime::init(config, "test".to_owned()).await?;
    let thread = ThreadId::new();
    state
        .upsert_thread(&test_thread_metadata(
            home.path(),
            thread,
            home.path().to_owned(),
        ))
        .await?;
    let invocation = Uuid::new_v4();
    state
        .record_shell_invocation(
            thread, invocation, 11, false, None, None, false, "", true, None, false, None,
        )
        .await?;
    state
        .record_watched_shell_exit(thread, invocation, 11, None)
        .await?;
    let unknown = state.pending_shell_exits(thread).await?;
    assert_eq!(unknown.len(), 1);
    state
        .acknowledge_all_named_shell_exits(thread, &unknown)
        .await?;
    state
        .record_watched_shell_exit(thread, invocation, 11, None)
        .await?;
    assert!(state.pending_shell_exits(thread).await?.is_empty());
    state
        .record_watched_shell_exit(thread, invocation, 11, Some(0))
        .await?;
    // An earlier running result, an unknown code and a contradictory terminal
    // observation cannot consume the newly reported success.
    state
        .record_shell_invocation(
            thread, invocation, 11, false, None, None, false, "", true, None, false, None,
        )
        .await?;
    state
        .record_shell_invocation(
            thread, invocation, 11, true, None, None, false, "", false, None, false, None,
        )
        .await?;
    state
        .record_shell_invocation(
            thread,
            invocation,
            11,
            true,
            Some(7),
            None,
            false,
            "",
            false,
            None,
            false,
            None,
        )
        .await?;
    let known = state.pending_shell_exits(thread).await?;
    assert_eq!(known.len(), 1);
    assert_eq!(known[0].exit_code, Some(0));
    // A remote command can still expose its numeric session while draining
    // output. Its reported exit code already gave the model the watched fact.
    state
        .record_shell_invocation(
            thread,
            invocation,
            11,
            false,
            Some(0),
            None,
            false,
            "",
            false,
            None,
            false,
            None,
        )
        .await?;
    state
        .record_watched_shell_exit(thread, invocation, 11, Some(0))
        .await?;
    assert!(state.pending_shell_exits(thread).await?.is_empty());
    state.close().await;
    Ok(())
}

#[tokio::test]
async fn legacy_wait_sentinel_yields_to_confirmed_watcher_or_poll_status() -> anyhow::Result<()> {
    let home = TempDir::new()?;
    let config = SqliteConfig::new_for_testing(home.path().to_owned().try_into()?);
    let state = StateRuntime::init(config, "test".to_owned()).await?;
    let thread = ThreadId::new();
    state
        .upsert_thread(&test_thread_metadata(
            home.path(),
            thread,
            home.path().to_owned(),
        ))
        .await?;
    for (watcher, finished, actual) in [
        (true, true, Some(0)),
        (true, true, None),
        (false, true, Some(7)),
        (false, false, None),
    ] {
        let invocation = Uuid::new_v4();
        state
            .record_shell_invocation(
                thread,
                invocation,
                11,
                true,
                Some(-1),
                None,
                false,
                "original",
                false,
                None,
                false,
                None,
            )
            .await?;
        let stored_status = || {
            sqlx::query_as::<_, (bool, Option<i32>)>(
                "SELECT finished, exit_code FROM alab_shell_invocations
                 WHERE thread_id = ? AND invocation = ?",
            )
            .bind(thread.to_string())
            .bind(invocation.as_bytes().as_slice())
        };
        assert_eq!(
            stored_status().fetch_one(state.pool.as_ref()).await?,
            (false, None)
        );
        // Seed the bytes written by a previous runtime, bypassing today's normalization.
        sqlx::query(
            "UPDATE alab_shell_invocations SET finished = 1, exit_code = -1
             WHERE thread_id = ? AND invocation = ?",
        )
        .bind(thread.to_string())
        .bind(invocation.as_bytes().as_slice())
        .execute(state.pool.as_ref())
        .await?;
        let old = state
            .shell_invocation(thread, invocation)
            .await?
            .expect("legacy row");
        assert!(!old.finished);
        assert_eq!(old.exit_code, None);
        state
            .record_watched_shell_exit(thread, invocation, 11, Some(-1))
            .await?;
        assert_eq!(
            stored_status().fetch_one(state.pool.as_ref()).await?,
            (true, Some(-1))
        );
        let preview = if finished { "" } else { "still running" };
        if watcher {
            state
                .record_watched_shell_exit(thread, invocation, 11, actual)
                .await?;
        } else {
            state
                .record_shell_invocation(
                    thread, invocation, 11, finished, actual, None, false, preview, false, None,
                    false, None,
                )
                .await?;
        }
        assert_eq!(
            stored_status().fetch_one(state.pool.as_ref()).await?,
            (finished, actual)
        );
        let updated = state
            .shell_invocation(thread, invocation)
            .await?
            .expect("updated row");
        assert_eq!(
            updated,
            ShellInvocationObservation {
                finished,
                exit_code: actual,
                preview: if finished {
                    old.preview.clone()
                } else {
                    preview.to_owned()
                },
                ..old
            }
        );
    }
    state.close().await;
    Ok(())
}

#[tokio::test]
async fn watcher_settles_only_the_original_published_invocation() -> anyhow::Result<()> {
    let home = TempDir::new()?;
    let config = SqliteConfig::new_for_testing(home.path().to_owned().try_into()?);
    let state = StateRuntime::init(config, "test".to_owned()).await?;
    let thread = ThreadId::new();
    state
        .upsert_thread(&test_thread_metadata(
            home.path(),
            thread,
            home.path().to_owned(),
        ))
        .await?;
    let invocation = Uuid::new_v4();
    state
        .record_watched_shell_exit(thread, invocation, 11, Some(0))
        .await?;
    assert_eq!(state.shell_invocation(thread, invocation).await?, None);
    let mut connection = state.pool.acquire().await?;
    assert!(!table_exists(&mut connection, OBSERVATIONS).await?);
    drop(connection);

    state
        .record_shell_invocation(
            thread,
            invocation,
            11,
            false,
            None,
            Some("original.log"),
            true,
            "building",
            true,
            None,
            false,
            None,
        )
        .await?;
    for (wrong_thread, wrong_command, wrong_number) in [
        (ThreadId::new(), invocation, 11),
        (thread, Uuid::new_v4(), 11),
        (thread, invocation, 12),
    ] {
        state
            .record_watched_shell_exit(wrong_thread, wrong_command, wrong_number, Some(4))
            .await?;
    }
    let mut expected = ShellInvocationObservation {
        session_id: 11,
        finished: false,
        exit_code: None,
        capture_name: Some("original.log".to_owned()),
        capture_rejected: false,
        capture_partial: None,
        tail: None,
        preview: "building".to_owned(),
        launch: None,
    };
    assert_eq!(
        state.shell_invocation(thread, invocation).await?,
        Some(expected.clone())
    );
    state
        .record_watched_shell_exit(thread, invocation, 11, None)
        .await?;
    expected.finished = true;
    assert_eq!(
        state.shell_invocation(thread, invocation).await?,
        Some(expected.clone())
    );
    state
        .record_watched_shell_exit(thread, invocation, 11, Some(0))
        .await?;
    state
        .record_watched_shell_exit(thread, invocation, 11, Some(4))
        .await?;
    state
        .record_shell_invocation(
            thread,
            invocation,
            11,
            false,
            None,
            None,
            false,
            "still building",
            false,
            None,
            false,
            None,
        )
        .await?;
    expected.exit_code = Some(0);
    assert_eq!(
        state.shell_invocation(thread, invocation).await?,
        Some(expected)
    );
    assert_eq!(state.delete_thread(thread).await?, 1);
    state
        .record_watched_shell_exit(thread, invocation, 11, Some(4))
        .await?;
    assert_eq!(state.shell_invocation(thread, invocation).await?, None);
    state.close().await;
    Ok(())
}

#[tokio::test]
async fn observation_survives_reopen_and_remains_bound_to_thread_and_native_number()
-> anyhow::Result<()> {
    let home = TempDir::new()?;
    let config = SqliteConfig::new_for_testing(home.path().to_owned().try_into()?);
    let state = StateRuntime::init(config.clone(), "test".to_owned()).await?;
    let thread = ThreadId::new();
    let other = ThreadId::new();
    for id in [thread, other] {
        state
            .upsert_thread(&test_thread_metadata(
                home.path(),
                id,
                home.path().to_owned(),
            ))
            .await?;
    }
    let invocation = Uuid::new_v4();
    assert_eq!(state.shell_invocation(thread, invocation).await?, None);
    let schema: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_schema WHERE name = 'alab_shell_invocations'",
    )
    .fetch_one(state.pool.as_ref())
    .await?;
    assert_eq!(schema, 0);
    let prefix = "a".repeat(MAX_PREVIEW_BYTES - 1);
    let full = format!("{prefix}🦀");
    state
        .record_shell_invocation(
            thread, invocation, 17, false, None, None, false, &full, false, None, false, None,
        )
        .await?;
    // A live snapshot can expose the command before exec_command has returned.
    // Its delayed initial publication fills in the capture but keeps the newer poll's bytes.
    state
        .record_shell_invocation(
            thread,
            invocation,
            17,
            false,
            None,
            Some("original.log"),
            true,
            "first result",
            true,
            None,
            false,
            None,
        )
        .await?;
    assert_eq!(
        state
            .shell_invocation(thread, invocation)
            .await?
            .expect("newer poll")
            .preview,
        prefix
    );
    assert!(
        state
            .record_shell_invocation(
                thread,
                invocation,
                22,
                false,
                None,
                None,
                false,
                "other command",
                false,
                None,
                false,
                None,
            )
            .await
            .is_err()
    );
    assert!(
        state
            .record_shell_invocation(
                thread,
                invocation,
                17,
                false,
                None,
                Some("../outside"),
                true,
                "other path",
                false,
                None,
                false,
                None,
            )
            .await
            .is_err()
    );
    state
        .record_shell_invocation(
            thread, invocation, 17, true, None, None, false, "", false, None, false, None,
        )
        .await?;
    let observation = state
        .shell_invocation(thread, invocation)
        .await?
        .expect("recorded finished shell");
    assert!(observation.finished);
    assert_eq!(observation.exit_code, None);
    assert_eq!(observation.preview, prefix);
    // A later native terminal result may supply the code; an earlier delayed
    // running poll or a contradictory terminal poll cannot replace it.
    state
        .record_shell_invocation(
            thread,
            invocation,
            17,
            true,
            Some(7),
            None,
            false,
            "final diagnostic",
            false,
            None,
            false,
            None,
        )
        .await?;
    state
        .record_shell_invocation(
            thread,
            invocation,
            17,
            false,
            None,
            None,
            false,
            "old prefix",
            false,
            None,
            false,
            None,
        )
        .await?;
    state
        .record_shell_invocation(
            thread,
            invocation,
            17,
            true,
            Some(8),
            None,
            false,
            "",
            false,
            None,
            false,
            None,
        )
        .await?;
    state
        .record_shell_invocation(
            other,
            invocation,
            22,
            false,
            None,
            None,
            false,
            "other thread",
            true,
            None,
            false,
            None,
        )
        .await?;
    state.close().await;
    let state = StateRuntime::init(config, "test".to_owned()).await?;
    assert_eq!(
        state.shell_invocation(thread, invocation).await?,
        Some(ShellInvocationObservation {
            session_id: 17,
            finished: true,
            exit_code: Some(7),
            capture_name: Some("original.log".to_owned()),
            capture_rejected: false,
            capture_partial: None,
            tail: None,
            preview: "final diagnostic".to_owned(),
            launch: None,
        })
    );
    assert_eq!(state.delete_thread(thread).await?, 1);
    assert_eq!(state.shell_invocation(thread, invocation).await?, None);
    assert!(
        state
            .record_shell_invocation(
                thread,
                invocation,
                17,
                true,
                Some(7),
                None,
                false,
                "",
                false,
                None,
                false,
                None
            )
            .await
            .is_err()
    );
    assert_eq!(
        state
            .shell_invocation(other, invocation)
            .await?
            .expect("other thread retains its observation")
            .session_id,
        22
    );
    state.close().await;
    Ok(())
}

#[tokio::test]
async fn launch_command_is_a_utf8_prefix_and_the_directory_is_all_or_nothing() -> anyhow::Result<()>
{
    let home = TempDir::new()?;
    let config = SqliteConfig::new_for_testing(home.path().to_owned().try_into()?);
    let state = StateRuntime::init(config, "test".to_owned()).await?;
    let thread = ThreadId::new();
    state
        .upsert_thread(&test_thread_metadata(
            home.path(),
            thread,
            home.path().to_owned(),
        ))
        .await?;
    let command_prefix = "a".repeat(MAX_COMMAND_BYTES - 1);
    let exact_command = format!("{command_prefix}b");
    let long_command = format!("{command_prefix}🦀");
    let root = if cfg!(windows) { r"C:\" } else { "/" };
    let exact_cwd = format!("{root}{}", "b".repeat(MAX_CWD_BYTES - root.len()));
    let long_cwd = format!("{exact_cwd}🦀");
    for (command, cwd, retained_command, truncated, retained_cwd) in [
        (
            exact_command.as_str(),
            Some(exact_cwd.as_str()),
            exact_command.as_str(),
            false,
            Some(exact_cwd.as_str()),
        ),
        (
            long_command.as_str(),
            Some(long_cwd.as_str()),
            command_prefix.as_str(),
            true,
            None,
        ),
        ("", Some("relative"), "", false, None),
        (
            "quoted 'command'\n",
            None,
            "quoted 'command'\n",
            false,
            None,
        ),
    ] {
        let id = Uuid::new_v4();
        state
            .record_shell_invocation(
                thread,
                id,
                3,
                false,
                None,
                None,
                false,
                "",
                true,
                Some((command, cwd)),
                false,
                None,
            )
            .await?;
        let retained = state
            .shell_invocation(thread, id)
            .await?
            .expect("recorded invocation");
        assert_eq!(
            retained.launch,
            Some(ShellLaunchContext {
                command: retained_command.to_owned(),
                command_truncated: truncated,
                cwd: retained_cwd.map(str::to_owned)
            })
        );
    }
    state.close().await;
    Ok(())
}

#[tokio::test]
async fn oldest_shell_observation_expires_at_its_own_thread_limit() -> anyhow::Result<()> {
    let home = TempDir::new()?;
    let config = SqliteConfig::new_for_testing(home.path().to_owned().try_into()?);
    let state = StateRuntime::init(config, "test".to_owned()).await?;
    let thread = ThreadId::new();
    let other = ThreadId::new();
    for id in [thread, other] {
        state
            .upsert_thread(&test_thread_metadata(
                home.path(),
                id,
                home.path().to_owned(),
            ))
            .await?;
    }
    let earliest = Uuid::new_v4();
    state
        .record_shell_invocation(
            thread,
            earliest,
            1,
            true,
            Some(7),
            None,
            false,
            "first",
            true,
            Some(("first", None)),
            true,
            None,
        )
        .await?;
    state
        .record_shell_invocation(
            other,
            earliest,
            1,
            false,
            None,
            None,
            false,
            "neighbor",
            true,
            Some(("neighbor", None)),
            false,
            None,
        )
        .await?;
    state
        .record_watched_shell_exit(thread, earliest, 1, Some(7))
        .await?;
    state
        .record_watched_shell_exit(other, earliest, 1, Some(8))
        .await?;
    assert!(
        state
            .record_watched_shell_diagnostic(
                thread,
                earliest,
                1,
                7,
                diagnostic(
                    "first.log",
                    true,
                    ShellOutputTail::from_bytes(
                        codex_alab_core::SHELL_OUTPUT_PREFIX_BYTES as u64,
                        vec![1]
                    )
                    .as_ref()
                )
            )
            .await?
    );
    assert!(
        state
            .record_watched_shell_diagnostic(
                other,
                earliest,
                1,
                8,
                diagnostic("neighbor.log", false, None)
            )
            .await?
    );
    for (id, name, code, partial) in [
        (thread, "first.log", 7, true),
        (other, "neighbor.log", 8, false),
    ] {
        let intent = ShellDiagnosticPublication {
            thread_id: id,
            invocation: earliest,
            session_id: 1,
            exit_code: code,
            capture_name: name.to_owned(),
            expected_bytes: codex_alab_core::SHELL_OUTPUT_PREFIX_BYTES as u64,
            prefix_sha256: "a".repeat(64),
            partial,
            tail: None,
        };
        assert!(state.reserve_shell_diagnostic_publication(&intent).await?);
    }
    for _ in 0..MAX_INVOCATIONS {
        state
            .record_shell_invocation(
                thread,
                Uuid::new_v4(),
                1,
                true,
                Some(0),
                None,
                false,
                "next",
                true,
                None,
                false,
                None,
            )
            .await?;
    }
    state
        .record_watched_shell_exit(thread, earliest, 1, Some(7))
        .await?;
    assert_eq!(state.shell_invocation(thread, earliest).await?, None);
    assert!(
        state
            .pending_shell_diagnostic_publications(thread)
            .await?
            .is_empty()
    );
    assert_eq!(
        state
            .pending_shell_diagnostic_publications(other)
            .await?
            .len(),
        1
    );
    state
        .record_shell_invocation(
            thread,
            earliest,
            1,
            true,
            Some(7),
            None,
            false,
            "old poll",
            false,
            None,
            true,
            None,
        )
        .await?;
    assert_eq!(state.shell_invocation(thread, earliest).await?, None);
    assert!(
        !state
            .record_watched_shell_diagnostic(
                thread,
                earliest,
                1,
                7,
                diagnostic("first.log", false, None)
            )
            .await?
    );
    let neighbor = state
        .shell_invocation(other, earliest)
        .await?
        .expect("neighbor");
    assert!(neighbor.finished);
    assert_eq!(neighbor.exit_code, Some(8));
    assert_eq!(neighbor.capture_partial, Some(false));
    assert_eq!(
        neighbor.launch.expect("neighbor launch").command,
        "neighbor"
    );
    assert!(state.pending_shell_exits(thread).await?.is_empty());
    assert_eq!(state.pending_shell_exits(other).await?.len(), 1);
    state.delete_thread(other).await?;
    assert!(
        state
            .pending_shell_diagnostic_publications(other)
            .await?
            .is_empty()
    );
    assert!(state.pending_shell_exits(other).await?.is_empty());
    assert!(
        !state
            .record_watched_shell_diagnostic(
                other,
                earliest,
                1,
                8,
                diagnostic("neighbor.log", false, None)
            )
            .await?
    );
    let diagnostics: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM alab_watched_shell_diagnostics")
            .fetch_one(state.pool.as_ref())
            .await?;
    assert_eq!(diagnostics, 0);
    let tails: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM alab_shell_output_tails")
        .fetch_one(state.pool.as_ref())
        .await?;
    assert_eq!(tails, 0);
    let remaining_launches: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM alab_shell_launches")
        .fetch_one(state.pool.as_ref())
        .await?;
    assert_eq!(remaining_launches, 0);
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM alab_shell_invocations WHERE thread_id = ?")
            .bind(thread.to_string())
            .fetch_one(state.pool.as_ref())
            .await?;
    assert_eq!(count, MAX_INVOCATIONS);
    state.close().await;
    Ok(())
}
