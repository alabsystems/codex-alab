// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Drive real asynchronous credential preparation without changing the lookup clock.

pub(super) async fn until_requested<F: std::future::Future>(
    requested: &tokio::sync::Notify,
    mut lookup: std::pin::Pin<&mut F>,
) {
    tokio::select! {
        _ = requested.notified() => {},
        _ = lookup.as_mut() => panic!("lookup ended before the gated HTTP request"),
    }
}
