// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Separate construction of large startup futures from their parent's poll frame.
//!
//! `Box::pin(make_future())` at an await site still constructs the concrete future
//! in that async function's poll frame. In an unoptimized build the temporary can
//! stay reserved across the nested poll: measured Session startup had 601 KiB in
//! spawn_internal and another 341 KiB in spawn before reaching Session::new/auth.
//! A synchronous, non-inlined factory returns the box before it is polled. This
//! keeps the same task, task-local source admissions, tracing, cancellation and
//! panic propagation; no detached initialization or larger stack is introduced.
//!
//! The R5 trace confirmed preparation separation, but initialization still had a
//! 535,104-byte poll frame while awaiting Session::new. Its later submission-loop
//! spawn constructed an inline concrete future in that same poll function.
//! Construct that spawned future through this factory too: its construction
//! stack ends before spawn, rather than remaining reserved during Session::new.
//! The original capture-tree regression is the runtime acceptance check; this
//! source change alone does not establish a bounded startup stack.
use futures::future::BoxFuture;
use std::future::Future;

#[inline(never)]
pub(crate) fn boxed_startup<'a, T, F>(make: impl FnOnce() -> F) -> BoxFuture<'a, T>
where
    F: Future<Output = T> + Send + 'a,
{
    Box::pin(make())
}

/// Finish preparation before entering initialization's poll frame. R4 still
/// measured a 570 KiB preparation poll under Session::new's 174 + 137 KiB polls:
/// boxing the future's storage alone did not release the caller's poll stack.
/// Returning the erased continuation ends that preparation poll before the
/// initialization poll starts. Both phases stay on the caller's task; dropping
/// this future synchronously drops the active phase. Owned admission guards
/// that cover initialization must move into that continuation explicitly.
#[inline(never)]
pub(crate) fn phased_startup<'a, T, E, F>(
    make: impl FnOnce() -> F + Send + 'a,
) -> BoxFuture<'a, Result<T, E>>
where
    T: Send + 'a,
    E: Send + 'a,
    F: Future<Output = Result<BoxFuture<'a, Result<T, E>>, E>> + Send + 'a,
{
    boxed_startup(move || async move {
        let continuation = boxed_startup(make).await?;
        continuation.await
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;

    tokio::task_local! { static STARTUP_CONTEXT: usize; }

    #[tokio::test]
    async fn construction_and_poll_keep_actual_task_context_and_result() {
        STARTUP_CONTEXT
            .scope(7, async {
                let constructions = AtomicUsize::new(0);
                let future = boxed_startup(|| {
                    constructions.fetch_add(1, Ordering::SeqCst);
                    assert_eq!(STARTUP_CONTEXT.get(), 7);
                    async {
                        tokio::task::yield_now().await;
                        assert_eq!(STARTUP_CONTEXT.get(), 7);
                        Err::<(), _>("original startup error")
                    }
                });
                assert_eq!(constructions.load(Ordering::SeqCst), 1);
                assert_eq!(future.await, Err("original startup error"));
            })
            .await;
    }

    #[tokio::test]
    async fn dropping_pending_startup_synchronously_releases_owned_future() {
        struct Owned(Arc<AtomicUsize>);
        impl Drop for Owned {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let drops = Arc::new(AtomicUsize::new(0));
        let owned = Owned(Arc::clone(&drops));
        let mut future = boxed_startup(move || async move {
            let _owned = owned;
            std::future::pending::<()>().await;
        });
        assert!(futures::poll!(&mut future).is_pending());
        drop(future);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }
}

#[cfg(test)]
mod phased_tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    tokio::task_local! { static CONTEXT: usize; }
    struct Lease(Arc<AtomicUsize>);
    impl Drop for Lease {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    #[tokio::test]
    async fn preparation_returns_before_initialization_with_context_and_owned_source_alive() {
        CONTEXT
            .scope(19, async {
                let preparation = Arc::new(AtomicUsize::new(0));
                let source = Arc::new(AtomicUsize::new(0));
                let prepare_lease = Lease(preparation.clone());
                let source_lease = Lease(source.clone());
                let preparation_check = preparation.clone();
                let source_check = source.clone();
                let inherited_environment = String::from("caller-owned environment");
                let environment = inherited_environment.as_str();
                let result = phased_startup(move || async move {
                    let _preparation = prepare_lease;
                    assert_eq!(CONTEXT.get(), 19);
                    Ok::<_, &'static str>(boxed_startup(move || async move {
                        let _source = source_lease;
                        assert_eq!(preparation_check.load(Ordering::SeqCst), 1);
                        assert_eq!(source_check.load(Ordering::SeqCst), 0);
                        assert_eq!(environment, "caller-owned environment");
                        tokio::task::yield_now().await;
                        assert_eq!(CONTEXT.get(), 19);
                        Err::<(), _>("original initialization error")
                    }))
                })
                .await;
                assert_eq!(result, Err("original initialization error"));
                assert_eq!(source.load(Ordering::SeqCst), 1);
            })
            .await;
    }
    #[tokio::test]
    async fn cancellation_drops_the_active_phase_without_detaching_work() {
        for after_preparation in [false, true] {
            let releases = Arc::new(AtomicUsize::new(0));
            let tail_entered = Arc::new(AtomicUsize::new(0));
            let lease = Lease(releases.clone());
            let entered = tail_entered.clone();
            let mut startup = phased_startup(move || async move {
                if !after_preparation {
                    let _lease = lease;
                    std::future::pending::<()>().await;
                    unreachable!();
                }
                Ok::<_, ()>(boxed_startup(move || async move {
                    let _lease = lease;
                    entered.fetch_add(1, Ordering::SeqCst);
                    std::future::pending::<Result<(), ()>>().await
                }))
            });
            assert!(futures::poll!(&mut startup).is_pending());
            assert_eq!(releases.load(Ordering::SeqCst), 0);
            assert_eq!(
                tail_entered.load(Ordering::SeqCst),
                usize::from(after_preparation)
            );
            drop(startup);
            assert_eq!(releases.load(Ordering::SeqCst), 1);
        }
    }
}
