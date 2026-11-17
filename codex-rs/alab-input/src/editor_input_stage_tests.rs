// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual local PTY/Broker/File lifetime controls only. No Native/C0 lineage,
//! accepted-custody, reader-retirement, or Go receipt is fabricated.
use super::*;
use std::os::fd::AsRawFd;
use std::os::fd::FromRawFd;
use std::sync::atomic::AtomicUsize;
fn error(message: &'static str) -> io::Error {
    io::Error::other(message)
}
fn fixture() -> io::Result<(File, Arc<Broker>, Arc<AtomicUsize>)> {
    let (mut master, mut slave) = (-1, -1);
    // SAFETY: successful openpty returns two uniquely owned descriptors.
    if unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    let master = unsafe { File::from_raw_fd(master) };
    let slave = Arc::new(unsafe { File::from_raw_fd(slave) });
    let opens = Arc::new(AtomicUsize::new(0));
    let broker = Broker::new({
        let opens = opens.clone();
        move || {
            opens.fetch_add(1, Ordering::Relaxed);
            slave.try_clone()
        }
    });
    broker.buffer_input(b"k\x1b[200~unfinished", true)?;
    Ok((master, broker, opens))
}
fn actual_fd(stage: &EditorInputStage) -> io::Result<i32> {
    let input = stage
        .retained
        .actual
        .input
        .lock()
        .map_err(|_| error("test actual staged File poisoned"))?;
    Ok(input
        .as_ref()
        .ok_or_else(|| error("test actual staged File absent"))?
        .as_raw_fd())
}
#[test]
fn real_original_file_moves_and_rolls_back_without_reader_or_parser_replacement() -> io::Result<()>
{
    let (_master, broker, opens) = fixture()?;
    let original = broker.prepare_editor()?;
    let snapshot = original.snapshot().to_vec();
    let descriptor = broker
        .state
        .lock()
        .map_err(|_| error("test broker poisoned"))?
        .input
        .as_ref()
        .ok_or_else(|| error("actual original input absent"))?
        .as_raw_fd();
    let stage = original
        .stage_original_input()
        .map_err(|refused| refused.error)?;
    assert_eq!(actual_fd(&stage)?, descriptor);
    assert_eq!(stage.snapshot(), snapshot);
    assert!(
        broker
            .state
            .lock()
            .map_err(|_| error("test broker poisoned"))?
            .input
            .is_none()
    );
    assert!(broker.subscribe(true).is_err());
    assert_eq!(opens.load(Ordering::Relaxed), 1);
    let mut restored = stage
        .rollback_before_offer()
        .map_err(|refused| refused.error)?;
    assert_eq!(restored.snapshot(), snapshot);
    assert_eq!(
        broker
            .state
            .lock()
            .map_err(|_| error("test broker poisoned"))?
            .input
            .as_ref()
            .ok_or_else(|| error("returned actual File absent"))?
            .as_raw_fd(),
        descriptor
    );
    assert!(broker.subscribe(true).is_err()); // Ownership rollback is NOT reader resume.
    let mut parser = crate::Parser::restore(restored.snapshot())?;
    parser.feed(b" suffix\x1b[201~", true)?;
    assert!(
        matches!(parser.pop(),Some(crate::InternalEvent::Event(crossterm::event::Event::Key(key)))
        if key.code==crossterm::event::KeyCode::Char('k'))
    );
    assert!(
        matches!(parser.pop(),Some(crate::InternalEvent::Event(crossterm::event::Event::Paste(text)))
        if text=="unfinished suffix")
    );
    restored.finish_with(|_| Ok(()))?; // Local resource control, no SDK/Native handback claim.
    assert_eq!(opens.load(Ordering::Relaxed), 1);
    Ok(())
}
#[test]
fn original_registry_keeps_whole_generic_broker_after_every_external_observer_drops()
-> io::Result<()> {
    let (_master, broker, _opens) = fixture()?;
    let weak_broker = Arc::downgrade(&broker);
    let stage = broker
        .prepare_editor()?
        .stage_original_input()
        .map_err(|refused| refused.error)?;
    let weak_actual = Arc::downgrade(&stage.retained);
    let snapshot = stage.snapshot().to_vec();
    drop(stage);
    drop(broker);
    let actual = weak_actual
        .upgrade()
        .ok_or_else(|| error("original registry lost actual stage"))?;
    let retained_broker = weak_broker
        .upgrade()
        .ok_or_else(|| error("original registry lost whole parser/Broker"))?;
    assert!(Arc::ptr_eq(&actual.broker, &retained_broker));
    assert_eq!(actual.actual.snapshot, snapshot);
    let state = retained_broker
        .state
        .lock()
        .map_err(|_| error("test retained broker poisoned"))?;
    assert!(state.frozen && state.input.is_none() && state.driver.is_none());
    drop(state);
    // Recover only the SAME actual weak resource, never a key/DTO constructor.
    let mut original = EditorInputStage { retained: actual }
        .rollback_before_offer()
        .map_err(|refused| refused.error)?;
    original.finish_with(|_| Ok(()))?;
    drop(original);
    drop(retained_broker);
    assert!(weak_actual.upgrade().is_none());
    assert!(weak_broker.upgrade().is_none());
    Ok(())
}
#[test]
fn blocked_original_offer_keeps_its_file_and_denies_optimistic_rollback() -> io::Result<()> {
    let (_master, broker, opens) = fixture()?;
    let stage = broker
        .prepare_editor()?
        .stage_original_input()
        .map_err(|refused| refused.error)?;
    let descriptor = actual_fd(&stage)?;
    let result: io::Result<()> = stage.with_offered_description(|actual| {
        assert_eq!(actual.as_raw_fd(), descriptor);
        Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "test exporter did no IO",
        ))
    });
    assert!(matches!(result,Err(error) if error.kind()==io::ErrorKind::WouldBlock));
    let refused = match stage.rollback_before_offer() {
        Ok(_) => return Err(error("blocked actual offer was optimistically rolled back")),
        Err(refused) => refused,
    };
    let weak = Arc::downgrade(&refused.staged.retained);
    let key = refused.staged.retained.key;
    drop(refused);
    let retained = weak
        .upgrade()
        .ok_or_else(|| error("blocked offer dropped actual File/parser"))?;
    assert_eq!(
        actual_fd(&EditorInputStage {
            retained: retained.clone()
        })?,
        descriptor
    );
    assert!(broker.subscribe(true).is_err());
    assert_eq!(opens.load(Ordering::Relaxed), 1);
    assert_eq!(
        retained
            .actual
            .token
            .upgrade()
            .ok_or_else(|| error("original token absent"))?
            .phase
            .load(Ordering::Acquire),
        OFFERED
    );
    // Test-only disposal of this exact local fixture after proving retention.
    // The closure above performed no send/Native call. This private teardown
    // does NOT create a public acceptance/NoIO/final-retirement constructor.
    let removed = stages()
        .lock()
        .map_err(|_| error("test stage registry poisoned"))?
        .remove(&key);
    drop(removed);
    drop(retained);
    drop(broker);
    assert!(weak.upgrade().is_none());
    Ok(())
}
