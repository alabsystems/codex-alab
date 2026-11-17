// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Same original std worker and destructor/channel/join ordering. Private injected
//! backends never touch a native clipboard and do not fabricate SDK permissions.
use super::*;
use crate::clipboard_copy::CopyOutcome;
use std::io;
use std::sync::atomic::AtomicUsize;
fn invalid(message: impl std::fmt::Display) -> io::Error {
    io::Error::other(message.to_string())
}
fn require(value: bool, message: &str) -> io::Result<()> {
    if value { Ok(()) } else { Err(invalid(message)) }
}
struct Fixture {
    worker: ClipboardWorker,
    frames: FrameRequester,
    _draws: tokio::sync::mpsc::UnboundedReceiver<Instant>,
    release: Vec<mpsc::Sender<()>>,
}
impl Fixture {
    fn new() -> Self {
        let (frames, draws) = FrameRequester::test_channel();
        Self {
            worker: ClipboardWorker::default(),
            frames,
            _draws: draws,
            release: Vec::new(),
        }
    }
    fn cleanup(&mut self) -> io::Result<()> {
        self.worker.alab_retained.owner_dropped();
        for release in self.release.drain(..) {
            let _ = release.send(());
        }
        self.worker.requests.take();
        if let Some(worker) = self.worker.alab_retained.worker.take() {
            worker
                .join()
                .map_err(|_| invalid("original fixture worker panicked during cleanup"))?;
        }
        Ok(())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}
fn observe(
    fixture: &mut Fixture,
    prepared: &JoinPreparation,
    predicate: impl Fn(&JoinObservation) -> bool,
) -> io::Result<JoinObservation> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let state = fixture
            .worker
            .alab_observe_join(prepared)
            .map_err(invalid)?;
        if predicate(&state) {
            return Ok(state);
        }
        if Instant::now() >= deadline {
            return Err(invalid("actual clipboard worker observation timed out"));
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn plain_start(
    fixture: &mut Fixture,
    copy: impl FnMut(&str, CopyFormat, &CopySetup) -> (Result<CopyOutcome, String>, Option<String>)
    + Send
    + 'static,
) -> io::Result<()> {
    fixture
        .worker
        .start(fixture.frames.clone(), copy, || {
            |_| Ok("original reader result".into())
        })
        .map_err(invalid)
}
#[test]
fn actual_never_started_worker_preparation_cannot_mint_a_thread_or_bind_another_worker()
-> io::Result<()> {
    let mut fixture = Fixture::new();
    let result = (|| {
        let prepared = fixture
            .worker
            .alab_prepare_join(fixture.frames.clone())
            .map_err(invalid)?;
        require(
            fixture.worker.alab_retained.worker.is_none(),
            "no-worker prepare started a thread",
        )?;
        require(
            fixture.worker.alab_close_join(&prepared).map_err(invalid)?,
            "actual no-worker close failed",
        )?;
        let JoinObservation::Joined(joined) = fixture
            .worker
            .alab_observe_join(&prepared)
            .map_err(invalid)?
        else {
            return Err(invalid("actual never-started proof missing"));
        };
        require(
            joined.same_preparation(&prepared) && joined.is_for(&fixture.worker),
            "exact no-worker record mismatch",
        )?;
        require(
            !joined.is_for(&ClipboardWorker::default()),
            "no-worker proof bound unrelated worker",
        )?;
        require(
            fixture.worker.next_id == 0 && fixture.worker.completed.is_none(),
            "no-worker preparation changed original ids",
        )
    })();
    let cleanup = fixture.cleanup();
    result?;
    cleanup
}
struct ReaderDrop {
    entered: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
    finished: Arc<AtomicBool>,
}
impl Drop for ReaderDrop {
    fn drop(&mut self) {
        let _ = self.entered.send(());
        let _ = self.release.recv_timeout(Duration::from_secs(5));
        self.finished.store(true, Ordering::Release);
    }
}
#[test]
fn actual_closed_response_channel_precedes_reader_drop_and_only_real_join_completes()
-> io::Result<()> {
    let mut fixture = Fixture::new();
    let (ready, ready_rx) = mpsc::channel();
    let (entered, entered_rx) = mpsc::channel();
    let (release, released) = mpsc::channel();
    fixture.release.push(release.clone());
    let finished = Arc::new(AtomicBool::new(false));
    let done = finished.clone();
    fixture
        .worker
        .start(
            fixture.frames.clone(),
            |_, _, _| (Ok(CopyOutcome::Requested), None),
            move || {
                let guard = ReaderDrop {
                    entered,
                    release: released,
                    finished: done,
                };
                let _ = ready.send(());
                move |_| {
                    let _ = &guard;
                    Ok("original reader".into())
                }
            },
        )
        .map_err(invalid)?;
    let result = (|| {
        fixture
            .worker
            .warm_text_reader(fixture.frames.clone())
            .map_err(invalid)?;
        ready_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(invalid)?;
        let prepared = fixture
            .worker
            .alab_prepare_join(fixture.frames.clone())
            .map_err(invalid)?;
        observe(&mut fixture, &prepared, |state| {
            matches!(state, JoinObservation::AwaitingOriginalUi)
        })?;
        require(
            fixture.worker.alab_close_join(&prepared).map_err(invalid)?,
            "no-lease worker did not commit exact inlet close",
        )?;
        entered_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(invalid)?;
        require(
            matches!(
                fixture
                    .worker
                    .responses
                    .as_ref()
                    .ok_or_else(|| invalid("original receiver missing"))?
                    .try_recv(),
                Err(mpsc::TryRecvError::Disconnected)
            ),
            "original response channel was not closed before reader destructor",
        )?;
        require(
            !finished.load(Ordering::Acquire),
            "reader destructor unexpectedly already finished",
        )?;
        require(
            !fixture
                .worker
                .alab_retained
                .worker
                .as_ref()
                .ok_or_else(|| invalid("actual JoinHandle lost"))?
                .is_finished(),
            "actual worker finished while destructor remained blocked",
        )?;
        require(
            matches!(
                fixture
                    .worker
                    .alab_observe_join(&prepared)
                    .map_err(invalid)?,
                JoinObservation::Joining
            ),
            "channel disconnect was accepted as joined",
        )?;
        release.send(()).map_err(invalid)?;
        let JoinObservation::Joined(joined) = observe(&mut fixture, &prepared, |state| {
            matches!(state, JoinObservation::Joined(_))
        })?
        else {
            return Err(invalid("actual joined outcome missing"));
        };
        require(
            finished.load(Ordering::Acquire) && joined.is_for(&fixture.worker),
            "join did not follow actual destructor",
        )?;
        require(
            fixture.worker.alab_retained.worker.is_none(),
            "joined handle retained as unobserved",
        )
    })();
    let cleanup = fixture.cleanup();
    result?;
    cleanup
}
#[test]
fn original_copy_result_and_id_remain_owned_until_original_poll_before_join() -> io::Result<()> {
    let mut fixture = Fixture::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    plain_start(&mut fixture, move |_, _, setup| {
        let result = setup.begin_delivery().map(|()| {
            counted.fetch_add(1, Ordering::Relaxed);
            CopyOutcome::Requested
        });
        (result, None)
    })?;
    let result = (|| {
        let CopyStatus::Pending(id) = fixture
            .worker
            .copy(
                "original request".into(),
                CopyFormat::PlainText,
                fixture.frames.clone(),
            )
            .map_err(invalid)?
        else {
            return Err(invalid("original copy was not accepted"));
        };
        let prepared = fixture
            .worker
            .alab_prepare_join(fixture.frames.clone())
            .map_err(invalid)?;
        let same = fixture
            .worker
            .alab_prepare_join(fixture.frames.clone())
            .map_err(invalid)?;
        require(
            Arc::ptr_eq(&same.actual, &prepared.actual),
            "same preparation reissued native control",
        )?;
        let retained = Arc::downgrade(&prepared.actual);
        drop(same);
        drop(prepared);
        let prepared = fixture
            .worker
            .alab_prepare_join(fixture.frames.clone())
            .map_err(invalid)?;
        require(
            Weak::ptr_eq(&retained, &Arc::downgrade(&prepared.actual)),
            "last observer drop lost accepted original preparation",
        )?;
        observe(&mut fixture, &prepared, |state| {
            matches!(state, JoinObservation::AwaitingOriginalUi)
        })?;
        require(
            !fixture.worker.alab_close_join(&prepared).map_err(invalid)?,
            "unobserved original copy was closed",
        )?;
        require(
            fixture.worker.poll() == Some(&(id, Ok(CopyStatus::Unconfirmed))),
            "original result/id changed before delivery",
        )?;
        require(
            calls.load(Ordering::Relaxed) == 1 && fixture.worker.next_id == id,
            "preparation recopied or changed request domain",
        )?;
        require(
            fixture.worker.alab_close_join(&prepared).map_err(invalid)?,
            "settled original result could not close",
        )?;
        observe(&mut fixture, &prepared, |state| {
            matches!(state, JoinObservation::Joined(_))
        })?;
        require(
            fixture.worker.completed == Some((id, Ok(CopyStatus::Unconfirmed))),
            "join cleared original cached result",
        )
    })();
    let cleanup = fixture.cleanup();
    result?;
    cleanup
}
#[test]
fn original_resident_lease_slot_is_denied_and_cancel_preserves_same_worker_and_next_ids()
-> io::Result<()> {
    let mut fixture = Fixture::new();
    plain_start(&mut fixture, |_, _, setup| {
        (
            setup
                .begin_delivery()
                .map(|()| CopyOutcome::Copied(Some(ClipboardLease::test()))),
            None,
        )
    })?;
    let result = (|| {
        let CopyStatus::Pending(id) = fixture
            .worker
            .copy(
                "lease slot fixture".into(),
                CopyFormat::PlainText,
                fixture.frames.clone(),
            )
            .map_err(invalid)?
        else {
            return Err(invalid("original private lease copy not accepted"));
        };
        let thread = fixture
            .worker
            .alab_retained
            .worker
            .as_ref()
            .ok_or_else(|| invalid("actual worker missing"))?
            .thread()
            .id();
        let prepared = fixture
            .worker
            .alab_prepare_join(fixture.frames.clone())
            .map_err(invalid)?;
        observe(&mut fixture, &prepared, |state| {
            matches!(state, JoinObservation::ResidentLease)
        })?;
        require(
            fixture.worker.poll() == Some(&(id, Ok(CopyStatus::Confirmed))),
            "resident denial changed original result",
        )?;
        require(
            !fixture.worker.alab_close_join(&prepared).map_err(invalid)?,
            "settled resident native slot authorized join",
        )?;
        require(
            fixture.worker.requests.is_some() && fixture.worker.alab_retained.worker.is_some(),
            "resident denial dropped actual inlet or thread",
        )?;
        fixture
            .worker
            .alab_cancel_join(&prepared)
            .map_err(invalid)?;
        require(
            matches!(
                fixture
                    .worker
                    .alab_observe_join(&prepared)
                    .map_err(invalid)?,
                JoinObservation::Cancelled
            ),
            "resident cancellation not observed",
        )?;
        require(
            fixture.worker.poll() == Some(&(id, Ok(CopyStatus::Confirmed))),
            "resident denial changed original result",
        )?;
        let CopyStatus::Pending(next) = fixture
            .worker
            .copy(
                "actual next request".into(),
                CopyFormat::PlainText,
                fixture.frames.clone(),
            )
            .map_err(invalid)?
        else {
            return Err(invalid("same original worker did not resume"));
        };
        require(
            next == id + 1
                && fixture
                    .worker
                    .alab_retained
                    .worker
                    .as_ref()
                    .is_some_and(|worker| worker.thread().id() == thread),
            "cancel restarted worker or changed id domain",
        )?;
        Ok(())
    })();
    let cleanup = fixture.cleanup();
    result?;
    cleanup
}
#[test]
fn actual_no_lease_pause_cancels_without_closing_original_inlet_and_foreign_frame_is_denied()
-> io::Result<()> {
    let mut fixture = Fixture::new();
    plain_start(&mut fixture, |_, _, setup| {
        (
            setup.begin_delivery().map(|()| CopyOutcome::Requested),
            None,
        )
    })?;
    let result = (|| {
        let (foreign, _foreign_rx) = FrameRequester::test_channel();
        require(
            fixture.worker.alab_prepare_join(foreign).is_err(),
            "foreign scheduler captured original worker",
        )?;
        require(
            fixture.worker.alab_retained.current.is_none(),
            "foreign scheduler altered actual admission",
        )?;
        let prepared = fixture
            .worker
            .alab_prepare_join(fixture.frames.clone())
            .map_err(invalid)?;
        observe(&mut fixture, &prepared, |state| {
            matches!(state, JoinObservation::AwaitingOriginalUi)
        })?;
        fixture
            .worker
            .alab_cancel_join(&prepared)
            .map_err(invalid)?;
        observe(&mut fixture, &prepared, |state| {
            matches!(state, JoinObservation::Cancelled)
        })?;
        require(
            fixture.worker.requests.is_some() && !fixture.worker.alab_retained.fenced(),
            "cancel did not retain/reopen exact inlet",
        )?;
        let CopyStatus::Pending(id) = fixture
            .worker
            .copy(
                "first original copy".into(),
                CopyFormat::PlainText,
                fixture.frames.clone(),
            )
            .map_err(invalid)?
        else {
            return Err(invalid("cancelled worker did not accept original copy"));
        };
        require(id == 1, "cancel invented a request id")
    })();
    let cleanup = fixture.cleanup();
    result?;
    cleanup
}

#[test]
fn actual_joined_rollback_restores_original_service_and_deferred_scope_without_replaying()
-> io::Result<()> {
    let mut fixture = Fixture::new();
    plain_start(&mut fixture, |_, _, setup| {
        (
            setup.begin_delivery().map(|()| CopyOutcome::Requested),
            None,
        )
    })?;
    let result = (|| {
        let CopyStatus::Pending(id) = fixture
            .worker
            .copy(
                "original before join".into(),
                CopyFormat::PlainText,
                fixture.frames.clone(),
            )
            .map_err(invalid)?
        else {
            return Err(invalid("original copy not accepted"));
        };
        let read = fixture
            .worker
            .read_text(PasteSource::Primary, fixture.frames.clone())
            .map_err(invalid)?
            .ok_or_else(|| invalid("actual deferred primary read was not accepted"))?;
        let deadline = fixture
            .worker
            .text_read_deadline(&read)
            .ok_or_else(|| invalid("actual original read budget missing"))?;
        require(
            fixture
                .worker
                .pending_read
                .as_ref()
                .is_some_and(|pending| pending.response.is_none()),
            "actual read unexpectedly crossed original busy copy",
        )?;
        let prepared = fixture
            .worker
            .alab_prepare_join(fixture.frames.clone())
            .map_err(invalid)?;
        observe(&mut fixture, &prepared, |state| {
            matches!(state, JoinObservation::AwaitingOriginalUi)
        })?;
        fixture.worker.poll();
        require(
            fixture.worker.alab_close_join(&prepared).map_err(invalid)?,
            "original worker could not commit close",
        )?;
        require(
            fixture.worker.alab_cancel_join(&prepared).is_err(),
            "committed close optimistically reopened old inlet",
        )?;
        let JoinObservation::Joined(joined) = observe(&mut fixture, &prepared, |state| {
            matches!(state, JoinObservation::Joined(_))
        })?
        else {
            return Err(invalid("actual join proof missing"));
        };
        require(
            fixture
                .worker
                .alab_resume_after_join_with(&joined, |_, _| {
                    Err("actual pre-birth restart refusal fixture".into())
                })
                .is_err(),
            "restart refusal unexpectedly succeeded",
        )?;
        require(
            joined.is_for(&fixture.worker) && fixture.worker.alab_retained.fenced(),
            "failed restart lost actual joined custody",
        )?;
        let replayed = Arc::new(AtomicUsize::new(0));
        let count = replayed.clone();
        fixture
            .worker
            .alab_resume_after_join_with(&joined, move |worker, frames| {
                worker.start(
                    frames,
                    move |_, _, setup| {
                        count.fetch_add(1, Ordering::Relaxed);
                        (
                            setup.begin_delivery().map(|()| CopyOutcome::Requested),
                            None,
                        )
                    },
                    || |_| Ok("original future read".into()),
                )
            })
            .map_err(invalid)?;
        require(
            !joined.is_for(&fixture.worker) && !fixture.worker.alab_retained.fenced(),
            "retired join proof stayed active or actual UI remained fenced",
        )?;
        require(
            fixture.worker.next_id == id
                && fixture.worker.completed == Some((id, Ok(CopyStatus::Unconfirmed))),
            "rollback reset or completed an original operation",
        )?;
        require(
            fixture.worker.requests.is_some() && fixture.worker.alab_retained.worker.is_some(),
            "deferred original start_read has no restored service",
        )?;
        require(
            fixture.worker.text_read_deadline(&read) == Some(deadline)
                && fixture.worker.pending_read.as_ref().is_some_and(|pending| {
                    pending.response.is_none() && pending.owner.ptr_eq(&Arc::downgrade(&read))
                }),
            "rollback replaced original read scope/deadline or submitted it",
        )?;
        require(
            replayed.load(Ordering::Relaxed) == 0,
            "rollback replayed a copy",
        )?;
        let CopyStatus::Pending(next) = fixture
            .worker
            .copy(
                "genuine future original copy".into(),
                CopyFormat::PlainText,
                fixture.frames.clone(),
            )
            .map_err(invalid)?
        else {
            return Err(invalid(
                "original future operation did not restart native worker",
            ));
        };
        require(
            next == id + 1,
            "future operation reused old request identity",
        )?;
        Ok(())
    })();
    let cleanup = fixture.cleanup();
    result?;
    cleanup
}
