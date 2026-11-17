// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual original App event future/owners, no Native/E1/clipboard result is faked.
use super::*;
fn require(value: bool, message: &str) -> Result<()> {
    if value {
        Ok(())
    } else {
        Err(color_eyre::eyre::eyre!("{message}"))
    }
}
#[tokio::test]
async fn actual_original_app_draw_finishes_before_joined_checkpoint_capture() -> Result<()> {
    let (mut app, _events, _ops) = Box::pin(crate::app::tests::make_test_app_with_channels()).await;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    let mut server = Box::pin(crate::start_embedded_app_server_for_picker(
        app.chat_widget.config_ref(),
    ))
    .await?;
    let result = Box::pin(async {
        let mut operation = C1Clipboard::begin(&mut app, &mut tui)?;
        let work = operation
            .drive_original_draw(&mut server, TuiEvent::Draw)
            .map_err(|failed| color_eyre::eyre::Report::new(failed.error))?;
        let control = work.await?;
        require(
            matches!(control, AppRunControl::Continue),
            "original Draw unexpectedly exited",
        )?;
        require(
            operation.close_if_original_results_delivered()?,
            "original no-resident UI state did not close",
        )?;
        require(
            matches!(operation.observe()?, JoinObservation::Joined(_)),
            "actual no-resident join missing",
        )?;
        let data = operation.capture(
            &mut Cells::default(),
            &mut text::Capture::default(),
            &ScopeCapture::default(),
        )?;
        data.validate_data()?;
        Ok::<(), color_eyre::eyre::Report>(())
    })
    .await;
    let cleanup = server.shutdown().await;
    result?;
    cleanup?;
    Ok(())
}
#[tokio::test]
async fn actual_original_input_remains_owned_and_unpolled_draw_cancellation_blocks_capture()
-> Result<()> {
    let (mut app, _events, _ops) = Box::pin(crate::app::tests::make_test_app_with_channels()).await;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    let mut server = Box::pin(crate::start_embedded_app_server_for_picker(
        app.chat_widget.config_ref(),
    ))
    .await?;
    let result = Box::pin(async {
        let mut operation = C1Clipboard::begin(&mut app, &mut tui)?;
        let refused = match operation.drive_original_draw(
            &mut server,
            TuiEvent::Paste("original outer FIFO bytes".into()),
        ) {
            Ok(work) => {
                drop(work);
                return Err(color_eyre::eyre::eyre!("non-Draw input was accepted"));
            }
            Err(refused) => refused,
        };
        require(
            matches!(&refused.original,TuiEvent::Paste(value)if value=="original outer FIFO bytes"),
            "actual refused input was lost or rewritten",
        )?;
        let original_work = operation
            .drive_original_draw(&mut server, TuiEvent::Draw)
            .map_err(|failed| color_eyre::eyre::Report::new(failed.error))?;
        // The synchronous guard must already own this actual event, even before poll.
        drop(original_work);
        require(
            operation.close_if_original_results_delivered()?,
            "original no-worker close failed",
        )?;
        require(
            matches!(operation.observe()?, JoinObservation::Joined(_)),
            "actual original join missing",
        )?;
        require(
            operation
                .capture(
                    &mut Cells::default(),
                    &mut text::Capture::default(),
                    &ScopeCapture::default(),
                )
                .is_err(),
            "canceled original App operation became a complete checkpoint",
        )
    })
    .await;
    let cleanup = server.shutdown().await;
    result?;
    cleanup?;
    Ok(())
}

#[tokio::test]
async fn actual_original_app_draw_delivers_accepted_clipboard_result_before_join_and_capture()
-> Result<()> {
    let (mut app, mut events, _ops) =
        Box::pin(crate::app::tests::make_test_app_with_channels()).await;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    let mut server = Box::pin(crate::start_embedded_app_server_for_picker(
        app.chat_widget.config_ref(),
    ))
    .await?;
    let result = Box::pin(async {
        let frames = tui.frame_requester();
        let id = ClipboardData::test_submit_original(&mut tui.clipboard, frames)?;
        app.chat_widget.show_copy_result(
            "original fixture",
            Ok(crate::clipboard_copy::CopyStatus::Pending(id)),
        );
        while events.try_recv().is_ok() {}
        let mut operation = C1Clipboard::begin(&mut app, &mut tui)?;
        let limit = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            match operation.observe()? {
                JoinObservation::AwaitingOriginalUi => break,
                JoinObservation::WaitingProbe => {}
                _ => {
                    return Err(color_eyre::eyre::eyre!(
                        "original fixture probe did not reach its no-lease pause"
                    ));
                }
            }
            if std::time::Instant::now() >= limit {
                return Err(color_eyre::eyre::eyre!("actual original probe timed out"));
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
        require(
            !operation.close_if_original_results_delivered()?,
            "native pending copy was skipped before original App callback",
        )?;
        operation
            .drive_original_draw(&mut server, TuiEvent::Draw)
            .map_err(|failed| color_eyre::eyre::Report::new(failed.error))?
            .await?;
        let mut delivered = false;
        while let Ok(event) = events.try_recv() {
            if let AppEvent::InsertHistoryCell(cell) = event {
                let text = cell
                    .transcript_lines(80)
                    .into_iter()
                    .map(|line| {
                        line.spans
                            .into_iter()
                            .map(|span| span.content.into_owned())
                            .collect::<String>()
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                delivered |= text.contains("Copy unconfirmed; /export saves chat");
            }
        }
        require(
            delivered,
            "FULL original App handler did not deliver its accepted clipboard result",
        )?;
        require(
            operation.close_if_original_results_delivered()?,
            "actual original callback did not release native pending copy",
        )?;
        loop {
            if matches!(operation.observe()?, JoinObservation::Joined(_)) {
                break;
            }
            if std::time::Instant::now() >= limit {
                return Err(color_eyre::eyre::eyre!(
                    "actual original worker did not join"
                ));
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
        let data = operation.capture(
            &mut Cells::default(),
            &mut text::Capture::default(),
            &ScopeCapture::default(),
        )?;
        let data = serde_json::to_value(data)?;
        require(
            data["nextId"] == serde_json::json!(id)
                && data["completed"][0] == serde_json::json!(id)
                && data["completed"][1]["status"]["kind"] == "unconfirmed",
            "capture changed original delivered result or ID",
        )
    })
    .await;
    let joined = ClipboardData::test_join_cleanup(&mut tui.clipboard);
    let cleanup = server.shutdown().await;
    result?;
    joined?;
    cleanup?;
    Ok(())
}
