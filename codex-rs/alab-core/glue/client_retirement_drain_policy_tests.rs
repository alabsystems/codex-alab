// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Cleanup dispatch/join policy only. Local refusal/inspection DATA does NOT
//! construct any SDK resource or establish actual Native retirement success.
use super::*;
use std::future::Future;
use std::future::poll_fn;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::task::Poll;
use tokio::task::JoinHandle;

#[derive(Default)]
struct Jobs(Mutex<Vec<JoinHandle<()>>>);
struct JoinLease<'a> {
    owner: &'a Jobs,
    task: Option<JoinHandle<()>>,
}
impl Drop for JoinLease<'_> {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            self.owner
                .0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(task);
        }
    }
}
impl Jobs {
    fn spawn(&self, tracker: &TaskTracker, work: impl Future<Output = ()> + Send + 'static) {
        // Same actual registration-before-spawn ordering as production guards.
        let token = tracker.token();
        let task = tokio::spawn(async move {
            let _token = token;
            work.await;
        });
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(task);
    }
    async fn finish(&self) -> Result<(), String> {
        let mut error = None;
        loop {
            let task = self.0.lock().unwrap_or_else(PoisonError::into_inner).pop();
            let Some(task) = task else {
                break;
            };
            let mut joined = JoinLease {
                owner: self,
                task: Some(task),
            };
            let task = joined.task.as_mut().ok_or("original tracked handle lost")?;
            match tokio::time::timeout(std::time::Duration::from_secs(60), &mut *task).await {
                Ok(Ok(())) => {}
                Ok(Err(_)) => {
                    error.get_or_insert("tracked cleanup job panicked".to_owned());
                }
                Err(_) => {
                    task.abort();
                    let _joined = task.await;
                    error.get_or_insert("tracked cleanup job did not finish".to_owned());
                }
            }
            // Consume EACH observed handle immediately. Cancellation restores
            // only its original unfinished handle, never an already joined one.
            joined.task.take();
        }
        error.map_or(Ok(()), Err)
    }
}
impl Drop for Jobs {
    fn drop(&mut self) {
        // Emergency Drop is abort-only; tests assert only after finish joins.
        for task in self
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain(..)
        {
            task.abort();
        }
    }
}

#[tokio::test]
async fn first_unknown_refusal_still_dispatches_and_joins_original_and_later_jobs()
-> Result<(), String> {
    let tracker = TaskTracker::new();
    let jobs = Jobs::default();
    let old_done = Arc::new(AtomicBool::new(false));
    let later_done = Arc::new(AtomicBool::new(false));
    let (old_release, old_released) = tokio::sync::oneshot::channel();
    let (old_finish, old_finished) = tokio::sync::oneshot::channel();
    let old_completed = old_done.clone();
    jobs.spawn(&tracker, async move {
        if old_released.await.is_ok() {
            old_completed.store(true, Ordering::Release);
            let _observed = old_finish.send(());
        }
    });
    tracker.close(); // Fresh Source acceptance is already closed in production.
    let (later_release, later_released) = tokio::sync::oneshot::channel();
    let mut later_released = Some(later_released);
    let mut old_release = Some(old_release);
    let mut later_release = Some(later_release);
    let mut started = Vec::new();
    let mut inspected = Vec::new();
    let records = [0u8, 1, 2];
    let mut draining = Box::pin(drain_retirements(
        &tracker,
        &records,
        |record| {
            started.push(*record);
            match *record {
                // Simulated LOCAL dispatch refusal only, never an SDK/Native reply.
                0 => Err("first local Unknown refusal".into()),
                1 => {
                    if let Some(released) = later_released.take() {
                        let completed = later_done.clone();
                        jobs.spawn(&tracker, async move {
                            if released.await.is_ok() {
                                completed.store(true, Ordering::Release);
                            }
                        });
                    }
                    Ok(())
                }
                _ => Ok(()),
            }
        },
        |record| {
            inspected.push(*record);
            if *record == 0 {
                return Err("secondary local unobserved outcome".into());
            }
            if !old_done.load(Ordering::Acquire) || !later_done.load(Ordering::Acquire) {
                return Err("inspection preceded real tracked completion".into());
            }
            Ok(())
        },
    ));
    let observed = async {
        let first = poll_fn(|context| Poll::Ready(draining.as_mut().poll(context))).await;
        let mut result = match first {
            Poll::Ready(result) => Some(result),
            Poll::Pending => None,
        };
        let first_pending = result.is_none();
        old_release
            .take()
            .ok_or("old release missing")?
            .send(())
            .map_err(|_| "old job lost")?;
        tokio::time::timeout(std::time::Duration::from_secs(60), old_finished)
            .await
            .map_err(|_| "old job did not observe release")?
            .map_err(|_| "old job lost completion")?;
        let second_pending = if result.is_none() {
            match poll_fn(|context| Poll::Ready(draining.as_mut().poll(context))).await {
                Poll::Pending => true,
                Poll::Ready(terminal) => {
                    result = Some(terminal);
                    false
                }
            }
        } else {
            false
        };
        later_release
            .take()
            .ok_or("later release missing")?
            .send(())
            .map_err(|_| "later job lost")?;
        let terminal = match result {
            Some(result) => result,
            None => tokio::time::timeout(std::time::Duration::from_secs(60), draining.as_mut())
                .await
                .map_err(|_| "retirement policy did not join")?,
        };
        Ok::<_, String>((first_pending, second_pending, terminal))
    }
    .await;
    drop(draining);
    // Every fallible path releases and joins the SAME tasks before propagating.
    if let Some(release) = old_release.take() {
        let _sent = release.send(());
    }
    if let Some(release) = later_release.take() {
        let _sent = release.send(());
    }
    let joined = jobs.finish().await;
    joined?;
    let (first_pending, second_pending, terminal) = observed?;
    assert!(first_pending && second_pending);
    assert_eq!(started, vec![0, 1, 2]);
    assert_eq!(inspected, vec![0, 1, 2]);
    assert_eq!(terminal, Err("first local Unknown refusal".into()));
    assert!(old_done.load(Ordering::Acquire) && later_done.load(Ordering::Acquire));
    Ok(())
}
