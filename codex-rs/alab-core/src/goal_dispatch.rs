// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Final synchronous authorization for an automatic goal write.
//!
//! Validation that records an instruction can await. A task-local predicate lets
//! the native goal service recheck process-local provenance immediately before it
//! creates and dispatches the durable mutation, without widening upstream APIs.

use std::future::Future;
use std::sync::Arc;

pub type Authorization = Arc<dyn Fn() -> bool + Send + Sync>;

tokio::task_local! {
    static AUTHORIZATION: Authorization;
}

pub async fn scope<T>(authorization: Authorization, future: impl Future<Output = T>) -> T {
    AUTHORIZATION.scope(authorization, future).await
}

/// Calls made outside a scoped automatic recovery retain native behavior.
pub fn authorized() -> bool {
    AUTHORIZATION
        .try_with(|authorization| authorization())
        .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering;

    #[tokio::test]
    async fn authorization_is_rechecked_after_the_scoped_future_awaits() {
        assert!(authorized());
        let current = Arc::new(AtomicBool::new(true));
        let observed = Arc::clone(&current);
        let current_for_work = Arc::clone(&current);
        let result = scope(
            Arc::new(move || observed.load(Ordering::SeqCst)),
            async move {
                current_for_work.store(false, Ordering::SeqCst);
                tokio::task::yield_now().await;
                authorized()
            },
        )
        .await;
        assert!(!result);
        assert!(authorized());
    }
}
