// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The installed SQLite log layer follows mode changes without replacing the subscriber.

use tracing::Metadata;
use tracing::Subscriber;
use tracing::level_filters::LevelFilter;
use tracing::subscriber::Interest;
use tracing_subscriber::filter::Targets;
use tracing_subscriber::filter::dynamic_filter_fn;
use tracing_subscriber::layer::Context;
use tracing_subscriber::layer::Filter;

pub(super) fn filter<S: Subscriber>(initial: Targets) -> impl Filter<S> + Send + Sync {
    let native = initial
        .clone()
        .with_default(LevelFilter::TRACE)
        .with_target("feedback_tags", LevelFilter::TRACE)
        .with_target("codex_http_client::custom_ca", LevelFilter::TRACE);
    let quiet = initial
        .with_default(LevelFilter::INFO)
        .with_target("feedback_tags", LevelFilter::OFF)
        .with_target("codex_http_client::custom_ca", LevelFilter::WARN);
    let with_ca = quiet
        .clone()
        .with_target("codex_http_client::custom_ca", LevelFilter::INFO);
    let native_sites = native.clone();
    let quiet_sites = quiet.clone();
    let ca_sites = with_ca.clone();
    dynamic_filter_fn(move |metadata: &Metadata<'_>, _: &Context<'_, S>| {
        let selected = if !codex_alab_core::active() {
            &native
        } else if metadata
            .target()
            .starts_with("codex_http_client::custom_ca")
            && codex_alab_core::ca_choice_logged()
        {
            &with_ca
        } else {
            &quiet
        };
        selected.would_enable(metadata.target(), metadata.level())
    })
    .with_callsite_filter(move |metadata| {
        match (
            native_sites.would_enable(metadata.target(), metadata.level()),
            quiet_sites.would_enable(metadata.target(), metadata.level()),
            ca_sites.would_enable(metadata.target(), metadata.level()),
        ) {
            (true, true, true) => Interest::always(),
            (false, false, false) => Interest::never(),
            _ => Interest::sometimes(),
        }
    })
    .with_max_level_hint(LevelFilter::TRACE)
}
