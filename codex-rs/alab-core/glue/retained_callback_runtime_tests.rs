// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual observer dispatch/TaskTracker join policy only. No Native grant or
//! callback retirement is simulated by a successful oneshot.
use super::*;
struct Job(Option<tokio::task::JoinHandle<Result<(), JSONRPCErrorError>>>);
impl Job {
    async fn finish(&mut self, expect_error: bool) -> Result<(), &'static str> {
        let Some(task) = self.0.as_mut() else {
            return Ok(());
        };
        match tokio::time::timeout(std::time::Duration::from_secs(60), &mut *task).await {
            Ok(joined) => {
                self.0.take();
                match joined {
                    Ok(result) if result.is_err() == expect_error => Ok(()),
                    Ok(_) => Err("observer returned unexpected result"),
                    Err(_) => Err("observer task failed"),
                }
            }
            Err(_) => {
                task.abort();
                let _joined = task.await;
                self.0.take();
                Err("observer failed to stop")
            }
        }
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        if let Some(task) = &self.0 {
            task.abort();
        }
    }
}
async fn check_stop(cancel_request: bool) -> Result<(), &'static str> {
    let tracker = TaskTracker::new();
    let token = tracker.token(); // before spawn/close
    let stop = tokio_util::sync::CancellationToken::new();
    let cancel = tokio_util::sync::CancellationToken::new();
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (held_tx, held_rx) = tokio::sync::oneshot::channel::<()>();
    let worker_stop = stop.clone();
    let worker_cancel = cancel.clone();
    let mut job = Job(Some(tokio::spawn(async move {
        let _token = token;
        observe_until_stop(
            worker_stop.cancelled(),
            worker_cancel.cancelled(),
            async move {
                let _sent = started_tx.send(());
                held_rx
                    .await
                    .map_err(|_| internal_error("original held observation closed"))?;
                Ok(())
            },
        )
        .await
    })));
    tracker.close();
    let outcome = async {
        tokio::time::timeout(std::time::Duration::from_secs(60), started_rx)
            .await
            .map_err(|_| "worker did not start")?
            .map_err(|_| "worker start lost")?;
        let waiting = tracker.wait();
        tokio::pin!(waiting);
        if !futures::poll!(waiting.as_mut()).is_pending() {
            return Err("original registered task was not joined");
        }
        Ok(())
    }
    .await;
    if cancel_request {
        cancel.cancel()
    } else {
        stop.cancel()
    }
    let finished = job.finish(true).await;
    drop(held_tx);
    let drained = tokio::time::timeout(std::time::Duration::from_secs(60), tracker.wait())
        .await
        .map_err(|_| "same tracker did not drain");
    outcome?;
    finished?;
    drained?;
    Ok(())
}
#[tokio::test]
async fn source_stop_wakes_blocked_observer_before_original_endpoint_cleanup()
-> Result<(), &'static str> {
    check_stop(false).await
}
#[tokio::test]
async fn request_cancel_ends_only_observation_and_joins_same_original_task()
-> Result<(), &'static str> {
    check_stop(true).await
}

#[tokio::test]
async fn actual_book_retirement_wakes_owned_observer_without_endpoint_event()
-> Result<(), &'static str> {
    let tracker = TaskTracker::new();
    let token = tracker.token();
    let owner = Arc::new(());
    let weak = Arc::downgrade(&owner);
    let mut book = book::Book::default();
    book.insert(1, "actual local owner".into(), owner);
    let held = book
        .active
        .get(&1)
        .cloned()
        .ok_or("owner missing before spawn")?;
    let mut changed = book.subscribe(); // production arm-before-check ordering
    let book = Arc::new(StdMutex::new(book));
    let observed = book.clone();
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let mut job = Job(Some(tokio::spawn(async move {
        let _token = token;
        let _held = held;
        let _sent = started_tx.send(());
        changed
            .changed()
            .await
            .map_err(|_| internal_error("book retirement notice lost"))?;
        if !observed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retired
            .contains(1)
        {
            return Err(invalid_request("notification was not retirement proof"));
        }
        Ok(())
    })));
    tracker.close();
    let ready = async {
        tokio::time::timeout(std::time::Duration::from_secs(60), started_rx)
            .await
            .map_err(|_| "retirement observer did not start")?
            .map_err(|_| "observer start lost")?;
        let waiting = tracker.wait();
        tokio::pin!(waiting);
        if !futures::poll!(waiting.as_mut()).is_pending() {
            return Err("observer was not held by actual tracker");
        }
        Ok(())
    }
    .await;
    // Always run actual production retirement and join, including setup errors.
    let removed = book
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .retire(1, "actual local owner");
    drop(removed);
    let finished = job.finish(false).await;
    let drained = tokio::time::timeout(std::time::Duration::from_secs(60), tracker.wait())
        .await
        .map_err(|_| "retired observer token was not drained");
    ready?;
    finished?;
    drained?;
    if weak.upgrade().is_some() {
        return Err("completed observer retained the retired local owner");
    }
    Ok(())
}

#[test]
fn full_observer_admission_keeps_actual_retirement_control_capacity() -> Result<(), &'static str> {
    let admission = Admission::new(2, 1);
    let first = admission
        .acquire(true)
        .map_err(|_| "first observer denied")?;
    let second = admission
        .acquire(true)
        .map_err(|_| "second observer denied")?;
    if admission.acquire(true).is_ok() {
        return Err("observer bound ignored");
    }
    let control = admission
        .acquire(false)
        .map_err(|_| "full observers starved actual cleanup control")?;
    drop(control);
    drop(first);
    drop(second);
    Ok(())
}
#[tokio::test]
async fn actual_frontier_publication_wakes_source_book_observer() -> Result<(), &'static str> {
    let state = CallbackState::default();
    let mut changed = state.book.subscribe();
    state.frontier_changed();
    tokio::time::timeout(std::time::Duration::from_secs(60), changed.changed())
        .await
        .map_err(|_| "actual frontier wake lost")?
        .map_err(|_| "book notification closed")?;
    // Wake alone proves no retirement or Native currentness.
    if state.book.retired.contains(1) {
        return Err("frontier notification fabricated ticket retirement");
    }
    Ok(())
}
