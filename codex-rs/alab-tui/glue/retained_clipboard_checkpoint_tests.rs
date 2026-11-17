// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Original native lifecycle and closed DATA, never a fabricated E1/Native binding.
use super::*;
use crate::clipboard_copy::CopyOutcome;
use std::sync::mpsc;
use std::time::Duration;
use std::time::Instant;
fn require(value: bool, message: &str) -> io::Result<()> {
    if value { Ok(()) } else { Err(invalid(message)) }
}
struct Fixture {
    worker: ClipboardWorker,
    frames: FrameRequester,
    _draws: tokio::sync::mpsc::UnboundedReceiver<Instant>,
    release: Option<mpsc::Sender<()>>,
}
impl Fixture {
    fn new() -> Self {
        let (frames, draws) = FrameRequester::test_channel();
        Self {
            worker: ClipboardWorker::default(),
            frames,
            _draws: draws,
            release: None,
        }
    }
    fn start(
        &mut self,
        mut copy: impl FnMut(
            &str,
            CopyFormat,
            &CopySetup,
        ) -> (Result<CopyOutcome, String>, Option<String>)
        + Send
        + 'static,
    ) -> io::Result<()> {
        self.worker
            .start(
                self.frames.clone(),
                move |text, format, setup| match setup.begin_delivery() {
                    Ok(()) => copy(text, format, setup),
                    Err(error) => (Err(error), None),
                },
                || |_| Ok("fixture text".into()),
            )
            .map_err(invalid)
    }
    fn wait(&mut self, prepared: &JoinPreparation) -> io::Result<JoinedWorker> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let state = self.worker.alab_observe_join(prepared).map_err(invalid)?;
            if let JoinObservation::Joined(joined) = state {
                return Ok(joined);
            }
            if Instant::now() >= deadline {
                return Err(invalid("original fixture join timed out"));
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn cleanup(&mut self) -> io::Result<()> {
        self.worker.alab_retained.owner_dropped();
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
        self.worker.requests.take();
        if let Some(handle) = self.worker.alab_retained.worker.take() {
            handle
                .join()
                .map_err(|_| invalid("original clipboard worker cleanup panicked"))?;
        }
        Ok(())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}
fn poll_completed(fixture: &mut Fixture, id: u64) -> io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if fixture.worker.poll().is_some_and(|actual| actual.0 == id)
            && fixture.worker.pending.is_none()
        {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(invalid("original copy result timed out"));
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn copy(fixture: &mut Fixture, text: &str) -> io::Result<u64> {
    match fixture
        .worker
        .copy(
            Arc::from(text),
            CopyFormat::PlainText,
            fixture.frames.clone(),
        )
        .map_err(invalid)?
    {
        CopyStatus::Pending(id) => Ok(id),
        _ => Err(invalid("original copy did not accept native request")),
    }
}
#[test]
fn actual_two_request_domain_preserves_an_older_hidden_selection_without_claiming_completion()
-> io::Result<()> {
    let mut fixture = Fixture::new();
    let result = (|| {
        fixture.start(|_, _, _| (Ok(CopyOutcome::Requested), None))?;
        let old = copy(&mut fixture, "first")?;
        poll_completed(&mut fixture, old)?;
        let newer = copy(&mut fixture, "second")?;
        poll_completed(&mut fixture, newer)?;
        require(
            newer > old
                && fixture
                    .worker
                    .completed
                    .as_ref()
                    .is_some_and(|done| done.0 == newer),
            "original cached result domain mismatch",
        )?;
        let actual = &fixture.worker.alab_retained.domain;
        actual.validate_ids_and_scopes(Some(old), None)?;
        actual.validate_ids_and_scopes(Some(newer), None)?;
        require(
            actual.validate_ids_and_scopes(Some(0), None).is_err()
                && actual
                    .validate_ids_and_scopes(Some(newer + 1), None)
                    .is_err(),
            "foreign selection ID accepted",
        )?;
        // A local domain is not a genuine C2 binder: no public positive receipt.
        let bound = BoundClipboard {
            actual: Arc::downgrade(actual),
        };
        require(
            bound.validate_selection(Some(old), None).is_err(),
            "unentered worker manufactured BoundClipboard permission",
        )
    })();
    let cleanup = fixture.cleanup();
    result?;
    cleanup
}
#[test]
fn actual_deferred_primary_and_read_checkpoint_keep_scopes_deadline_text_and_original_ids()
-> io::Result<()> {
    let mut fixture = Fixture::new();
    let (release, wait) = mpsc::channel();
    fixture.release = Some(release);
    let result = (|| {
        fixture.start(move |_, _, _| {
            let _ = wait.recv_timeout(Duration::from_secs(5));
            (Ok(CopyOutcome::Requested), None)
        })?;
        let id = copy(&mut fixture, "in flight")?;
        let text: Arc<str> = Arc::from("actual deferred PRIMARY");
        let (status, primary) = fixture.worker.select(
            text.clone(),
            CopyFormat::PlainText,
            CopyDestination::Primary,
            fixture.frames.clone(),
        );
        require(
            status == Ok(CopyStatus::Busy),
            "original PRIMARY did not defer behind accepted request",
        )?;
        let read = fixture
            .worker
            .read_text(PasteSource::Primary, fixture.frames.clone())
            .map_err(invalid)?
            .ok_or_else(|| invalid("original deferred read owner missing"))?;
        let deadline = fixture
            .worker
            .text_read_deadline(&read)
            .ok_or_else(|| invalid("original read deadline missing"))?;
        let prepared = fixture
            .worker
            .alab_prepare_join(fixture.frames.clone())
            .map_err(invalid)?;
        if let Some(release) = fixture.release.take() {
            release.send(()).map_err(invalid)?;
        }
        poll_completed(&mut fixture, id)?;
        let limit = Instant::now() + Duration::from_secs(5);
        loop {
            if fixture.worker.alab_close_join(&prepared).map_err(invalid)? {
                break;
            }
            if Instant::now() >= limit {
                return Err(invalid("original no-lease pause timed out"));
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        let joined = fixture.wait(&prepared)?;
        let mut scopes = ScopeCapture::default();
        let primary_ref = scopes.strong(&primary)?;
        let read_ref = scopes.strong(&read)?;
        let mut cells = Cells::default();
        let mut strings = text::Capture::default();
        let text_ref = strings.string(&text)?;
        let data = ClipboardData::capture_worker(
            &fixture.worker,
            fixture.frames.clone(),
            &joined,
            &mut cells,
            &mut strings,
            &scopes,
        )?;
        require(
            data.next_id == id && data.completed.as_ref().is_some_and(|done| done.0 == id),
            "checkpoint changed actual request/result domain",
        )?;
        require(
            data.primary == Some((text_ref, Some(primary_ref))),
            "checkpoint split native text/scope aliases",
        )?;
        let pending = data
            .pending_read
            .as_ref()
            .ok_or_else(|| invalid("checkpoint discarded original deferred read"))?;
        require(
            pending.owner == Some(read_ref) && pending.response.is_none() && !pending.expired,
            "checkpoint changed deferred read fields",
        )?;
        let restored_deadline = pending.deadline.restore().map_err(invalid)?;
        let delta = restored_deadline
            .checked_duration_since(deadline)
            .unwrap_or_else(|| deadline.duration_since(restored_deadline));
        require(
            delta <= Duration::from_micros(100),
            "checkpoint reset native read deadline",
        )?;
        require(
            Weak::ptr_eq(
                &fixture
                    .worker
                    .pending_read
                    .as_ref()
                    .ok_or_else(|| invalid("original deferred read disappeared"))?
                    .owner,
                &Arc::downgrade(&read),
            ),
            "capture changed original scope",
        )?;
        let wire = serde_json::to_value(&data).map_err(invalid)?;
        let decoded: ClipboardData = serde_json::from_value(wire.clone()).map_err(invalid)?;
        decoded.validate_data()?;
        require(
            serde_json::to_value(decoded).map_err(invalid)? == wire,
            "closed checkpoint changed actual fields",
        )
    })();
    let cleanup = fixture.cleanup();
    result?;
    cleanup
}
#[test]
fn required_clipboard_fields_and_completed_statuses_refuse_invented_native_work() -> io::Result<()>
{
    let actual = ClipboardData::test_joined_original()?;
    actual.validate_data()?;
    let wire = serde_json::to_value(actual).map_err(invalid)?;
    for key in [
        "pending",
        "completed",
        "primary",
        "pendingRead",
        "readResult",
    ] {
        let mut bad = wire.clone();
        bad.as_object_mut()
            .ok_or_else(|| invalid("clipboard wire not object"))?
            .remove(key);
        require(
            serde_json::from_value::<ClipboardData>(bad).is_err(),
            "required explicit clipboard field defaulted",
        )?;
    }
    for value in [
        serde_json::json!({"kind":"confirmed","extra":true}),
        serde_json::json!({"kind":"unconfirmed","extra":true}),
        serde_json::json!({"kind":"pending","id":1}),
        serde_json::json!({"kind":"busy"}),
    ] {
        let mut bad = wire.clone();
        bad["nextId"] = serde_json::json!(1);
        bad["completed"] = serde_json::json!([1,{"kind":"ok","status":value}]);
        require(
            serde_json::from_value::<ClipboardData>(bad).is_err(),
            "submission status accepted as completed result",
        )?;
    }
    for (field, value) in [
        ("schema", serde_json::json!(2)),
        ("original", serde_json::json!("pid:1")),
        ("pending", serde_json::json!({})),
        ("readResult", serde_json::json!({})),
    ] {
        let mut bad = wire.clone();
        bad[field] = value;
        let accepted = serde_json::from_value::<ClipboardData>(bad)
            .ok()
            .is_some_and(|data| data.validate_data().is_ok());
        require(!accepted, "malformed clipboard data accepted")?;
    }
    require(
        Status::capture(CopyStatus::Pending(1)).is_err()
            && Status::capture(CopyStatus::Busy).is_err(),
        "submission-only status captured as native result",
    )
}
#[test]
fn actual_original_event_guard_retains_cancellation_and_refuses_later_capture() -> io::Result<()> {
    let mut fixture = Fixture::new();
    let prepared = fixture
        .worker
        .alab_prepare_join(fixture.frames.clone())
        .map_err(invalid)?;
    require(
        fixture
            .worker
            .alab_begin_original_draw(&prepared, &crate::tui::TuiEvent::Paste("owned".into()))
            .is_err(),
        "non-Draw input was consumed",
    )?;
    let guard = fixture
        .worker
        .alab_begin_original_draw(&prepared, &crate::tui::TuiEvent::Draw)?;
    drop(guard);
    require(
        fixture.worker.alab_close_join(&prepared).map_err(invalid)?,
        "original no-worker close denied",
    )?;
    let joined = fixture.wait(&prepared)?;
    require(
        ClipboardData::capture_worker(
            &fixture.worker,
            fixture.frames.clone(),
            &joined,
            &mut Cells::default(),
            &mut text::Capture::default(),
            &ScopeCapture::default(),
        )
        .is_err(),
        "canceled original callback operation encoded as complete",
    )?;
    let state = fixture
        .worker
        .alab_retained
        .domain
        .ui
        .state
        .lock()
        .map_err(|_| invalid("event custody poisoned"))?;
    require(
        state.unknown
            && !state.active
            && matches!(state.last_event, Some(crate::tui::TuiEvent::Draw)),
        "original event was lost after cancellation",
    )
}
#[test]
fn same_restored_marker_index_never_keeps_a_ui_owner_or_worker_alive() -> io::Result<()> {
    let first = Arc::new(());
    let second = Arc::new(());
    let mut captured = ScopeCapture::default();
    let first_ref = captured.strong(&first)?;
    let second_ref = captured.strong(&second)?;
    let restored = ScopeRestored::new(captured.export()?)?;
    let first = restored.strong(first_ref)?;
    let second = restored.strong(second_ref)?;
    restored.validate_roots()?;
    let actual = WorkerDomain {
        scopes: restored.weak_index(),
        ..WorkerDomain::default()
    };
    actual.validate_ids_and_scopes(None, Some(&first))?;
    actual.validate_ids_and_scopes(None, Some(&second))?;
    require(
        actual
            .validate_ids_and_scopes(None, Some(&Arc::new(())))
            .is_err(),
        "foreign UI marker was accepted",
    )?;
    let known = Arc::downgrade(&first);
    drop(restored);
    drop(first);
    require(
        known.upgrade().is_none(),
        "worker domain strongly pinned a historical marker",
    )?;
    let actual = Arc::new(actual);
    let bound = BoundClipboard {
        actual: Arc::downgrade(&actual),
    };
    drop(actual);
    require(
        bound.actual.upgrade().is_none() && bound.validate_selection(None, None).is_err(),
        "historical clipboard handle pinned original worker",
    )
}

#[test]
fn actual_never_started_join_still_binds_its_original_scheduler() -> io::Result<()> {
    let mut fixture = Fixture::new();
    let prepared = fixture
        .worker
        .alab_prepare_join(fixture.frames.clone())
        .map_err(invalid)?;
    require(
        fixture.worker.alab_close_join(&prepared).map_err(invalid)?,
        "original no-worker close failed",
    )?;
    let joined = fixture.wait(&prepared)?;
    let (other, _events) = FrameRequester::test_channel();
    require(
        ClipboardData::capture_worker(
            &fixture.worker,
            other,
            &joined,
            &mut Cells::default(),
            &mut text::Capture::default(),
            &ScopeCapture::default(),
        )
        .is_err(),
        "no-resident join accepted another original scheduler",
    )?;
    ClipboardData::capture_worker(
        &fixture.worker,
        fixture.frames.clone(),
        &joined,
        &mut Cells::default(),
        &mut text::Capture::default(),
        &ScopeCapture::default(),
    )?
    .validate_data()
}
#[test]
fn real_staged_worker_deferred_jobs_stay_dormant_on_refusal_and_only_open_after_local_install()
-> io::Result<()> {
    for succeeds in [false, true] {
        let mut fixture = Fixture::new();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed = calls.clone();
        let result: io::Result<()> = (|| {
            fixture.start(move |_, _, _| {
                observed.fetch_add(1, Ordering::SeqCst);
                (Ok(CopyOutcome::Requested), None)
            })?;
            let first = copy(&mut fixture, "accepted before stage")?;
            let text: Arc<str> = Arc::from("deferred PRIMARY remains owned");
            let (status, primary) = fixture.worker.select(
                text.clone(),
                CopyFormat::PlainText,
                CopyDestination::Primary,
                fixture.frames.clone(),
            );
            require(
                status == Ok(CopyStatus::Busy),
                "actual PRIMARY did not defer",
            )?;
            let read = fixture
                .worker
                .read_text(PasteSource::Primary, fixture.frames.clone())
                .map_err(invalid)?
                .ok_or_else(|| invalid("actual deferred read missing"))?;
            let deadline = fixture
                .worker
                .text_read_deadline(&read)
                .ok_or_else(|| invalid("actual deferred deadline missing"))?;
            poll_completed(&mut fixture, first)?;
            let actual = fixture.worker.alab_retained.domain.clone();
            actual.install.store(1, Ordering::Release);
            fixture.worker.advance(fixture.frames.clone());
            require(
                fixture.worker.poll().is_none() && fixture.worker.is_busy(),
                "staged native fields were observable as installed",
            )?;
            require(
                fixture.worker.copy(
                    Arc::from("must not submit"),
                    CopyFormat::PlainText,
                    fixture.frames.clone(),
                ) == Ok(CopyStatus::Busy),
                "staged clipboard admitted a new copy",
            )?;
            require(
                calls.load(Ordering::SeqCst) == 1
                    && fixture.worker.next_id == first
                    && fixture
                        .worker
                        .pending_read
                        .as_ref()
                        .is_some_and(|pending| pending.response.is_none()),
                "staged deferred native work escaped",
            )?;
            let mut trace = Vec::new();
            let mut installed_owner = None;
            // This lower private helper records local admission ordering only;
            // the closure does not claim a whole-App/Native graph installation.
            let install = actual.install_graph(|| {
                trace.push("local actual install attempted");
                if succeeds {
                    installed_owner = Some(primary.clone());
                    Ok(())
                } else {
                    Err(invalid("actual local install refused"))
                }
            });
            require(install.is_ok() == succeeds, "local install result changed")?;
            require(
                actual.install_held() != succeeds,
                "clipboard admission opened on a refused/unfinished local install",
            )?;
            require(
                actual
                    .install_graph(|| {
                        trace.push("unexpected replay");
                        Ok(())
                    })
                    .is_err()
                    && trace == ["local actual install attempted"],
                "local install callback replayed",
            )?;
            require(
                fixture
                    .worker
                    .primary
                    .as_ref()
                    .is_some_and(|(value, owner)| {
                        Arc::ptr_eq(value, &text) && owner.ptr_eq(&Arc::downgrade(&primary))
                    })
                    && fixture.worker.text_read_deadline(&read) == Some(deadline),
                "local install hold changed original deferred payload",
            )?;
            if succeeds {
                require(
                    installed_owner
                        .as_ref()
                        .is_some_and(|owner| Arc::ptr_eq(owner, &primary)),
                    "local installed owner changed actual alias",
                )?;
                let next = copy(&mut fixture, "new explicit request after install")?;
                poll_completed(&mut fixture, next)?;
                require(
                    next == first + 1 && calls.load(Ordering::SeqCst) == 2,
                    "original native request counter did not continue after install",
                )?;
            } else {
                fixture.worker.advance(fixture.frames.clone());
                require(
                    calls.load(Ordering::SeqCst) == 1 && fixture.worker.is_busy(),
                    "failed local install released deferred native work",
                )?;
            }
            Ok(())
        })();
        let cleanup = fixture.cleanup();
        result?;
        cleanup?;
    }
    Ok(())
}
