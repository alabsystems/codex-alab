// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Exercise the recovery diagnostic through the installed SQLite filter and writer.

use std::sync::Arc;
use std::sync::Mutex;
use tracing_subscriber::prelude::*;

use crate::state_log_filter;

#[derive(Default)]
struct Failures(Mutex<Vec<String>>);

impl codex_state::log_db::LogWriteFailureReporter for Failures {
    fn report_failure(&self, diagnostic: &str) {
        self.0.lock().unwrap().push(diagnostic.to_owned());
    }
}

#[test]
fn opaque_request_correlation_reaches_sqlite_without_unrelated_headers() {
    const CHILD: &str = "ALAB_RECOVERY_SQLITE_TEST";
    if crate::test_process::parent_handled(
        module_path!(),
        "opaque_request_correlation_reaches_sqlite_without_unrelated_headers",
        CHILD,
    ) {
        return;
    }
    codex_alab_core::activate();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let home = tempfile::tempdir().unwrap();
            let runtime = codex_state::StateRuntime::init(
                codex_state::SqliteConfig::new_for_testing(
                    home.path().to_path_buf().try_into().unwrap(),
                ),
                "test-provider".to_owned(),
            )
            .await
            .unwrap();
            let failures = Arc::new(Failures::default());
            let layer = codex_state::log_db::start(runtime.clone(), failures.clone());
            let subscriber = tracing_subscriber::registry().with(layer.clone().with_filter(
                state_log_filter::filter(codex_state::log_db::default_filter()),
            ));
            let error = tracing::subscriber::with_default(subscriber, || {
                tracing::info_span!("request", thread_id = "opaque-recovery-thread").in_scope(
                    || {
                        codex_api::map_api_error(codex_api::ApiError::Transport(
                            codex_api::TransportError::Http {
                                status: 400.try_into().unwrap(),
                                url: None,
                                headers: Some(
                                    [
                                        ("x-request-id", "req_sqlite-1"),
                                        ("cf-ray", "1234abcd-SJC"),
                                        ("authorization", "Bearer never-store-this"),
                                        ("x-unrelated", "never-store-this-either"),
                                    ]
                                    .into_iter()
                                    .map(|(name, value)| {
                                        (name.parse().unwrap(), value.parse().unwrap())
                                    })
                                    .collect(),
                                ),
                                body: Some(r#"{"detail":"Bad Request"}"#.into()),
                                retry_after: None,
                            },
                        ))
                    },
                )
            });
            assert!(error.retry_delay(1).is_some());
            layer.flush().await;
            assert!(failures.0.lock().unwrap().is_empty());
            let rows = runtime
                .query_logs(&codex_state::LogQuery {
                    thread_ids: vec!["opaque-recovery-thread".into()],
                    ..Default::default()
                })
                .await
                .unwrap();
            assert_eq!(rows.len(), 1, "{rows:?}");
            let row = &rows[0];
            assert_eq!(row.level, "INFO");
            assert_eq!(row.thread_id.as_deref(), Some("opaque-recovery-thread"));
            let message = row.message.as_deref().unwrap();
            assert!(message.contains("Opaque backend HTTP 400 classified as recoverable"));
            assert!(message.contains("req_sqlite-1"), "{message}");
            assert!(message.contains("1234abcd-SJC"), "{message}");
            assert!(!message.contains("never-store"), "{message}");
            assert!(!message.contains("authorization"), "{message}");
            assert!(!message.contains("x-unrelated"), "{message}");
        });
}
