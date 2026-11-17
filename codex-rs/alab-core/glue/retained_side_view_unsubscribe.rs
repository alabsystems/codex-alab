// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Exact-Arc check at ORIGINAL unsubscribe serialization, before effects.
use super::*;
use futures::FutureExt as _;
tokio::task_local! {static EXPECTED:Arc<CodexThread>;}
pub(crate) async fn run<T>(
    expected: Arc<CodexThread>,
    original: impl std::future::Future<Output = T>,
) -> T {
    EXPECTED.scope(expected, original).await
}
pub(super) async fn check(
    processor: &ThreadRequestProcessor,
    id: ThreadId,
) -> Result<(), JSONRPCErrorError> {
    let Ok(expected) = EXPECTED.try_with(Clone::clone) else {
        return Ok(());
    };
    match processor.thread_manager.get_thread(id).await {
        Ok(actual) if Arc::ptr_eq(&actual, &expected) => Ok(()),
        Err(_) if expected.wait_until_terminated().now_or_never().is_some() => Ok(()),
        _ => Err(invalid_request("Side close resolved another actual Core")),
    }
}
