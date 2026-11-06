// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

#[path = "state_log_filter.rs"]
mod state_log_filter;

use std::sync::Arc;
use std::sync::Mutex;
use tracing::Event;
use tracing::Subscriber;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use tracing_subscriber::prelude::*;

#[derive(Clone, Default)]
struct Events(Arc<Mutex<Vec<(&'static str, tracing::Level)>>>);

impl<S: Subscriber> Layer<S> for Events {
    fn on_event(&self, event: &Event<'_>, _: Context<'_, S>) {
        let metadata = event.metadata();
        self.0
            .lock()
            .unwrap()
            .push((metadata.target(), *metadata.level()));
    }
}

fn emit() {
    tracing::trace!(target: "alab_live_state", "trace");
    tracing::info!(target: "alab_live_state", "info");
    tracing::info!(target: "feedback_tags", "tags");
    tracing::trace!(target: "codex_http_client::transport", "excluded payload");
}

#[test]
fn permanently_excluded_state_sites_keep_their_never_interest() {
    static METADATA: tracing::Metadata<'static> = tracing::metadata! {
        name: "excluded estimate",
        target: "codex_core::post_sampling_token_estimate",
        level: tracing::Level::TRACE,
        fields: &[],
        callsite: &CALLSITE,
        kind: tracing::metadata::Kind::EVENT.hint(),
    };
    static CALLSITE: tracing::callsite::DefaultCallsite =
        tracing::callsite::DefaultCallsite::new(&METADATA);
    let subscriber = tracing_subscriber::registry().with(Events::default().with_filter(
        state_log_filter::filter(codex_state::log_db::default_filter()),
    ));
    assert!(tracing::Subscriber::register_callsite(&subscriber, &METADATA).is_never());
}

#[test]
fn installed_state_filter_follows_live_mode_at_the_same_callsites() {
    const CHILD: &str = "ALAB_LIVE_STATE_LOG_TEST";
    if std::env::var_os(CHILD).is_none() {
        let name = format!(
            "{}::installed_state_filter_follows_live_mode_at_the_same_callsites",
            module_path!().split_once("::").unwrap().1
        );
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &name])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
        return;
    }
    codex_alab_core::activate();
    for born_enhanced in [true, false] {
        codex_alab_core::set_enhanced(born_enhanced);
        let events = Events::default();
        let subscriber = tracing_subscriber::registry().with(events.clone().with_filter(
            state_log_filter::filter(codex_state::log_db::default_filter()),
        ));
        tracing::subscriber::with_default(subscriber, || {
            codex_alab_core::set_enhanced(true);
            emit();
            codex_alab_core::set_enhanced(false);
            emit();
            codex_alab_core::set_enhanced(true);
            emit();
        });
        assert_eq!(
            *events.0.lock().unwrap(),
            vec![
                ("alab_live_state", tracing::Level::INFO),
                ("alab_live_state", tracing::Level::TRACE),
                ("alab_live_state", tracing::Level::INFO),
                ("feedback_tags", tracing::Level::INFO),
                ("alab_live_state", tracing::Level::INFO),
            ]
        );
    }
}
