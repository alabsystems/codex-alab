// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Real native provisional editor +watch scheduling only. These channels are
//! private lower fixtures, never genuine Delivery/Binding/Native close proof.
use super::*;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;
const DEADLINE: std::time::Duration = std::time::Duration::from_secs(60);
type HeldChannels = (
    watch::Sender<()>,
    watch::Sender<Option<std::result::Result<(), io::ErrorKind>>>,
    Held,
);
fn channels() -> HeldChannels {
    let (image, images) = watch::channel(());
    let (terminal, close) = watch::channel(None);
    (image, terminal, Held::new(images, close))
}
#[tokio::test]
async fn prearmed_change_keeps_actual_new_native_typing_and_wakes_without_lost_commit()
-> io::Result<()> {
    let mut pump = crate::startup_draft::tests::quiet_startup_test_pump();
    pump.bottom_pane
        .set_composer_text("original draft".into(), Vec::new(), Vec::new());
    let (image, _close, held) = channels();
    // This genuine native edit represents input handled by the SAME pump while
    // run_until drives the held observer. No snapshot is reapplied afterward.
    pump.bottom_pane
        .handle_key_event(KeyEvent::new(KeyCode::Char('λ'), KeyModifiers::NONE));
    pump.bottom_pane.flush_composer_paste_burst();
    let actual = pump.bottom_pane.composer_text();
    assert!(actual.contains('λ'));
    image.send_replace(()); // Real plan sender can publish between draw and wait.
    tokio::time::timeout(DEADLINE, held.wait())
        .await
        .map_err(io::Error::other)??;
    assert_eq!(pump.bottom_pane.composer_text(), actual);
    Ok(())
}
#[tokio::test]
async fn lost_image_observer_is_not_a_close_or_success_and_real_terminal_fact_wakes_wait()
-> io::Result<()> {
    let (image, terminal, held) = channels();
    drop(image);
    let mut task = tokio::spawn(held.wait());
    tokio::task::yield_now().await;
    assert!(!task.is_finished());
    let _ = terminal.send_replace(Some(Err(io::ErrorKind::ConnectionReset)));
    let joined = match tokio::time::timeout(DEADLINE, &mut task).await {
        Ok(joined) => joined,
        Err(error) => {
            task.abort();
            let _ = (&mut task).await; // Join the SAME actual observer before reporting timeout.
            return Err(io::Error::other(error));
        }
    };
    let error = joined
        .map_err(io::Error::other)?
        .expect_err("fixture terminal fact must stop observer");
    assert_eq!(error.kind(), io::ErrorKind::ConnectionReset);
    Ok(())
}
#[tokio::test]
async fn actual_terminal_value_precedes_ready_image_notification_without_dropping_native_draft()
-> io::Result<()> {
    let mut pump = crate::startup_draft::tests::quiet_startup_test_pump();
    pump.bottom_pane.set_composer_text(
        "still owned on closed delivery".into(),
        Vec::new(),
        Vec::new(),
    );
    let (image, terminal, held) = channels();
    image.send_replace(());
    let _ = terminal.send_replace(Some(Ok(())));
    assert_eq!(
        tokio::time::timeout(DEADLINE, held.wait())
            .await
            .map_err(io::Error::other)?
            .expect_err("closed fixture delivery is not UI completion")
            .kind(),
        io::ErrorKind::ConnectionAborted
    );
    assert_eq!(
        pump.bottom_pane.composer_text(),
        "still owned on closed delivery"
    );
    Ok(())
}
#[tokio::test]
async fn lost_both_observers_without_terminal_value_stays_pending_instead_of_spinning_or_completing()
-> io::Result<()> {
    use std::future::Future;
    let (image, terminal, held) = channels();
    drop(image);
    drop(terminal);
    let future = held.wait();
    tokio::pin!(future);
    let pending = tokio::time::timeout(
        DEADLINE,
        std::future::poll_fn(|context| {
            std::task::Poll::Ready(matches!(
                future.as_mut().poll(context),
                std::task::Poll::Pending
            ))
        }),
    )
    .await
    .map_err(io::Error::other)?;
    assert!(pending);
    Ok(())
}
