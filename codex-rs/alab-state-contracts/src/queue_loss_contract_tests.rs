// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Active queue-loss accounting is sticky; vanilla mode keeps upstream behavior.

use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use tracing_subscriber::prelude::*;

use crate::state_log_filter;

const WAIT: Duration = Duration::from_secs(5);
const TARGET: &str = "alab_state_contracts::queue_loss";
const HEALTHY: &str = "accepted by the healthy queue";
const ACCEPTED: &str = "accepted before queue overflow";
const DROPPED: &str = "rejected by the full queue";
const RECOVERED: &str = "accepted after queue overflow";

#[derive(Default)]
struct BatchFailures(Mutex<Vec<String>>);

impl codex_state::log_db::LogWriteFailureReporter for BatchFailures {
    fn report_failure(&self, diagnostic: &str) {
        self.0.lock().unwrap().push(diagnostic.to_owned());
    }
}

async fn state(home: &tempfile::TempDir) -> Arc<codex_state::StateRuntime> {
    tokio::time::timeout(
        WAIT,
        codex_state::StateRuntime::init(
            codex_state::SqliteConfig::new_for_testing(
                home.path().to_path_buf().try_into().unwrap(),
            ),
            "queue-loss-fixture".into(),
        ),
    )
    .await
    .expect("real SQLite initialization deadline")
    .expect("real SQLite runtime")
}

async fn flush(layer: &codex_state::log_db::LogDbLayer) {
    tokio::time::timeout(WAIT, layer.flush())
        .await
        .expect("accepted log commands finish before querying");
}

async fn rows(state: &codex_state::StateRuntime) -> Vec<codex_state::LogRow> {
    tokio::time::timeout(WAIT, state.query_logs(&codex_state::LogQuery::default()))
        .await
        .expect("real SQLite query deadline")
        .expect("real SQLite query")
        .into_iter()
        .filter(|row| row.target == TARGET)
        .collect()
}

fn config() -> codex_state::log_db::LogSinkQueueConfig {
    codex_state::log_db::LogSinkQueueConfig {
        queue_capacity: 1,
        ..Default::default()
    }
}

async fn full_case(expected_loss: bool) -> codex_state::log_db::LogDbLayer {
    let home = tempfile::tempdir().unwrap();
    let state = state(&home).await;
    let failures = Arc::new(BatchFailures::default());
    let layer = codex_state::log_db::LogDbLayer::start_with_config(
        state.clone(),
        failures.clone(),
        config(),
    );
    let observer = layer.clone();
    assert!(!observer.has_write_failure());
    {
        let subscriber = tracing_subscriber::registry().with(layer.clone().with_filter(
            state_log_filter::filter(codex_state::log_db::default_filter()),
        ));
        let _subscriber = tracing::subscriber::set_default(subscriber);
        tracing::info!(target: TARGET, "{HEALTHY}");
    }
    flush(&layer).await;
    let healthy = rows(&state).await;
    assert_eq!(healthy.len(), 1, "one real healthy row: {healthy:?}");
    assert_eq!(healthy[0].level, "INFO");
    assert_eq!(healthy[0].message.as_deref(), Some(HEALTHY));
    assert!(healthy[0].process_uuid.is_some());
    assert!(!layer.has_write_failure(), "an accepted entry is healthy");
    assert!(!observer.has_write_failure(), "its clone is healthy too");
    {
        let subscriber = tracing_subscriber::registry().with(layer.clone().with_filter(
            state_log_filter::filter(codex_state::log_db::default_filter()),
        ));
        let _subscriber = tracing::subscriber::set_default(subscriber);
        // Flush consumed the prior commands. There is no await between these
        // paired entries, so the current-thread inserter cannot drain the first
        // entry before the second encounters the one-entry queue at capacity.
        tracing::info!(target: TARGET, "{ACCEPTED}");
        tracing::info!(target: TARGET, "{DROPPED}");
    }
    let lost_before_flush = observer.has_write_failure();
    if expected_loss {
        codex_alab_core::set_enhanced(false);
        assert!(!codex_alab_core::active());
    }
    flush(&layer).await;
    let accepted = rows(&state).await;
    assert_eq!(accepted.len(), 2, "one later accepted row: {accepted:?}");
    assert_eq!(accepted[0].message.as_deref(), Some(HEALTHY));
    assert_eq!(accepted[1].level, "INFO");
    assert_eq!(accepted[1].message.as_deref(), Some(ACCEPTED));
    assert!(accepted[1].process_uuid.is_some());
    {
        let subscriber = tracing_subscriber::registry().with(layer.clone().with_filter(
            state_log_filter::filter(codex_state::log_db::default_filter()),
        ));
        let _subscriber = tracing::subscriber::set_default(subscriber);
        tracing::info!(target: TARGET, "{RECOVERED}");
    }
    flush(&layer).await;
    let recovered = rows(&state).await;
    assert_eq!(recovered.len(), 3, "later writes succeed: {recovered:?}");
    assert_eq!(recovered[2].message.as_deref(), Some(RECOVERED));
    assert!(
        recovered
            .iter()
            .all(|row| row.message.as_deref() != Some(DROPPED))
    );
    assert!(
        failures.0.lock().unwrap().is_empty(),
        "queue accounting must not invoke the batch-error reporter"
    );
    let original_after_flush = layer.has_write_failure();
    drop(layer);
    let sticky = observer.has_write_failure();
    tokio::time::timeout(WAIT, state.close())
        .await
        .expect("real SQLite close deadline");
    // Persistence and recovery controls precede the expected RED: the old sink
    // loses the second event while reporting a clean writer, even after flush.
    assert_eq!(
        lost_before_flush, expected_loss,
        "queue-loss accounting follows active enhanced mode synchronously"
    );
    assert_eq!(
        original_after_flush, expected_loss,
        "successful writes retain the original mode-selected health flag"
    );
    assert_eq!(
        sticky, expected_loss,
        "clones retain the mode-selected health after successful flush and writes"
    );
    observer
}

fn closed_case(expected_loss: bool) -> codex_state::log_db::LogDbLayer {
    let database_runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let home = tempfile::tempdir().unwrap();
    let state = database_runtime.block_on(state(&home));
    let failures = Arc::new(BatchFailures::default());
    let writer_runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let layer = writer_runtime.block_on(async {
        codex_state::log_db::LogDbLayer::start_with_config(
            state.clone(),
            failures.clone(),
            config(),
        )
    });
    // Only the inserter belongs to this separate runtime. Its initiating future
    // has no await, so no database write starts before shutdown closes the receiver.
    // The independently owned database runtime remains available for real queries.
    drop(writer_runtime);
    let observer = layer.clone();
    assert!(
        !observer.has_write_failure(),
        "shutdown alone lost no entry"
    );
    {
        let subscriber = tracing_subscriber::registry().with(layer.clone().with_filter(
            state_log_filter::filter(codex_state::log_db::default_filter()),
        ));
        let _subscriber = tracing::subscriber::set_default(subscriber);
        tracing::info!(target: TARGET, "entry attempted after writer shutdown");
    }
    let lost_before_flush = layer.has_write_failure();
    if expected_loss {
        codex_alab_core::set_enhanced(false);
        assert!(!codex_alab_core::active());
    }
    let sticky = database_runtime.block_on(async {
        flush(&layer).await;
        assert!(
            rows(&state).await.is_empty(),
            "closed writer accepts no row"
        );
        assert!(
            failures.0.lock().unwrap().is_empty(),
            "queue accounting must not invoke the batch-error reporter"
        );
        drop(layer);
        let sticky = observer.has_write_failure();
        tokio::time::timeout(WAIT, state.close())
            .await
            .expect("real SQLite close deadline");
        sticky
    });
    assert_eq!(
        lost_before_flush, expected_loss,
        "closed-queue accounting follows active enhanced mode"
    );
    assert_eq!(
        sticky, expected_loss,
        "the shared mode-selected flag survives a closed-channel flush"
    );
    observer
}

#[derive(Clone, Copy)]
enum QueueCase {
    Full,
    Closed,
}

fn exercise_modes(case: QueueCase) {
    assert!(!codex_alab_core::is_alab_binary());
    assert!(!codex_alab_core::active());
    let mut prior: Vec<(bool, codex_state::log_db::LogDbLayer)> = Vec::new();
    for (mode, expected_loss) in [
        ("inactive", false),
        ("enhanced-off", false),
        ("enhanced", true),
    ] {
        match mode {
            "inactive" => assert!(!codex_alab_core::is_alab_binary()),
            "enhanced-off" => {
                codex_alab_core::activate();
                codex_alab_core::set_enhanced(false);
                assert!(codex_alab_core::is_alab_binary());
            }
            "enhanced" => {
                codex_alab_core::set_enhanced(true);
            }
            _ => unreachable!(),
        }
        assert_eq!(codex_alab_core::active(), expected_loss, "{mode}");
        for (expected, observer) in &prior {
            assert_eq!(
                observer.has_write_failure(),
                *expected,
                "mode changes do not retroactively mark earlier inactive/off loss"
            );
        }
        // Fresh layers and databases prevent an earlier loss from masquerading
        // as another mode's observation. No mode mutates the parent process.
        let observer = match case {
            QueueCase::Full => tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(full_case(expected_loss)),
            // Runtime destruction stays outside every async runtime context.
            QueueCase::Closed => closed_case(expected_loss),
        };
        // The case has closed its database and returned from its owned runtime.
        // This retained sender/atomic owns no live inserter and is only observed.
        prior.push((expected_loss, observer));
    }
    for (expected, observer) in prior {
        assert_eq!(observer.has_write_failure(), expected);
    }
}

fn isolated(name: &str, case: QueueCase) {
    if crate::test_process::parent_handled(module_path!(), name, "ALAB_SQLITE_QUEUE_LOSS_CHILD") {
        return;
    }
    exercise_modes(case);
}

#[test]
fn full_sqlite_queue_marks_shared_loss_after_successful_writes() {
    isolated(
        "full_sqlite_queue_marks_shared_loss_after_successful_writes",
        QueueCase::Full,
    );
}

#[test]
fn closed_sqlite_queue_marks_loss_without_calling_the_batch_reporter() {
    isolated(
        "closed_sqlite_queue_marks_loss_without_calling_the_batch_reporter",
        QueueCase::Closed,
    );
}
