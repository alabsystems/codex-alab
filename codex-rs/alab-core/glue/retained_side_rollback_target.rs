// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Exact original root under the original per-thread removal serialization.
use super::*;
use futures::FutureExt as _;
tokio::task_local! {static ROOT:(ThreadId,Arc<CodexThread>);}
pub(crate) async fn run<T>(
    id: ThreadId,
    actual: Arc<CodexThread>,
    original: impl std::future::Future<Output = T>,
) -> T {
    ROOT.scope((id, actual), original).await
}
pub(super) async fn check(
    processor: &ThreadRequestProcessor,
    id: ThreadId,
) -> Result<(), JSONRPCErrorError> {
    let Ok((root, expected)) = ROOT.try_with(Clone::clone) else {
        return Ok(());
    };
    if root != id {
        return Ok(());
    } // Original subtree policy still owns descendants.
    match processor.thread_manager.get_thread(id).await {
        Ok(actual) if Arc::ptr_eq(&actual, &expected) => Ok(()),
        Err(_) if expected.wait_until_terminated().now_or_never().is_some() => Ok(()),
        _ => Err(invalid_request(
            "Side rollback resolved another actual root",
        )),
    }
}
