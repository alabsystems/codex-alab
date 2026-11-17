// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual lower native pump/stream ownership. No entered Constructor, Core,
//! callback, Native/UI receipt or successful Go is constructed.
use super::*;
use futures::Stream;
use futures::StreamExt;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::task::Context;
use std::task::Poll;
struct OriginalStream(Arc<AtomicBool>);
impl Stream for OriginalStream {
    type Item = crate::tui::TuiEvent;
    fn poll_next(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Pending
    }
}
impl Drop for OriginalStream {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
#[tokio::test]
async fn retained_native_recovery_retires_same_input_subscription_but_keeps_draft_and_newline()
-> io::Result<()> {
    let mut pump = crate::startup_draft::tests::quiet_startup_test_pump();
    pump.bottom_pane
        .set_composer_text("late actual C2 typing".into(), Vec::new(), Vec::new());
    pump.pending_paste_newline = Some((Instant::now(), "same pending newline".into()));
    let released = Arc::new(AtomicBool::new(false));
    pump.events = Box::pin(OriginalStream(released.clone()));
    let cause = io::Error::new(
        io::ErrorKind::WouldBlock,
        "actual destination already owns recovery",
    );
    let mut recovery = NativeRecovery::retain(pump, cause);
    assert!(released.load(Ordering::Acquire));
    assert!(recovery.pump.events.next().await.is_none());
    assert_eq!(
        recovery.pump.bottom_pane.composer_text(),
        "late actual C2 typing"
    );
    assert_eq!(
        recovery.pump.pending_paste_newline.as_ref().unwrap().1,
        "same pending newline"
    );
    assert_eq!(recovery.cause().kind(), io::ErrorKind::WouldBlock);
    let data = capture_ui(&recovery.pump, None, 1024 * 1024)?;
    assert!(data.pending_paste_newline.is_some());
    Ok(())
}
