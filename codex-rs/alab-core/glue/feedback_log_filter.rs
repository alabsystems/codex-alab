// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Keep the installed feedback layer responsive to the current experience mode.

use tracing::Metadata;
use tracing::Subscriber;
use tracing::level_filters::LevelFilter;
use tracing::subscriber::Interest;
use tracing_subscriber::filter::Targets;
use tracing_subscriber::filter::dynamic_filter_fn;
use tracing_subscriber::layer::Context;
use tracing_subscriber::layer::Filter;

pub(super) fn filter<S: Subscriber>(initial: Targets) -> impl Filter<S> + Send + Sync {
    let native = initial.clone().with_default(LevelFilter::TRACE);
    let enhanced = initial.with_default(LevelFilter::OFF);
    let native_sites = native.clone();
    let enhanced_sites = enhanced.clone();
    // A static FilterFn would cache a disabled callsite permanently. This layer
    // must keep checking sites first seen while the enhanced ring was disabled.
    dynamic_filter_fn(move |metadata: &Metadata<'_>, _: &Context<'_, S>| {
        let selected = if codex_alab_core::active() {
            &enhanced
        } else {
            &native
        };
        selected.would_enable(metadata.target(), metadata.level())
    })
    .with_callsite_filter(move |metadata| {
        match (
            native_sites.would_enable(metadata.target(), metadata.level()),
            enhanced_sites.would_enable(metadata.target(), metadata.level()),
        ) {
            (true, true) => Interest::always(),
            (false, false) => Interest::never(),
            _ => Interest::sometimes(),
        }
    })
    .with_max_level_hint(LevelFilter::TRACE)
}

#[cfg(test)]
mod tests {
    use tracing_subscriber::prelude::*;

    fn emit(message: &str) {
        tracing::trace!(target: "alab_live_feedback", "{message}");
        tracing::trace!(target: "codex_http_client::transport", "excluded payload {message}");
    }

    #[test]
    fn permanently_excluded_sites_keep_their_never_interest() {
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
        let feedback = crate::CodexFeedback::new();
        let subscriber = tracing_subscriber::registry()
            .with(feedback.logger_layer())
            .with(
                tracing_subscriber::fmt::layer().with_filter(codex_state::log_db::default_filter()),
            );
        assert!(tracing::Subscriber::register_callsite(&subscriber, &METADATA).is_never());
    }

    #[test]
    fn installed_feedback_filter_follows_live_mode_at_the_same_callsite() {
        const CHILD: &str = "ALAB_LIVE_FEEDBACK_TEST";
        if std::env::var_os(CHILD).is_none() {
            let name = format!(
                "{}::installed_feedback_filter_follows_live_mode_at_the_same_callsite",
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
            let feedback = crate::CodexFeedback::new();
            let subscriber = tracing_subscriber::registry().with(feedback.logger_layer());
            tracing::subscriber::with_default(subscriber, || {
                codex_alab_core::set_enhanced(true);
                emit("enhanced-before");
                codex_alab_core::set_enhanced(false);
                emit("vanilla-visible");
                codex_alab_core::set_enhanced(true);
                emit("enhanced-after");
            });
            let captured = String::from_utf8(feedback.snapshot(None).bytes).unwrap();
            assert!(captured.contains("vanilla-visible"), "{captured}");
            assert!(!captured.contains("enhanced-before"), "{captured}");
            assert!(!captured.contains("enhanced-after"), "{captured}");
            assert!(!captured.contains("excluded payload"), "{captured}");
        }
    }
}

#[cfg(test)]
#[path = "state_log_filter_tests.rs"]
mod state_tests;
