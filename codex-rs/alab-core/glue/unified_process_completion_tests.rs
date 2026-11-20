// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

#[tokio::test]
async fn watcher_preserves_reported_and_unreported_exits_on_termination() -> anyhow::Result<()> {
    for (timed_out, code) in [
        (false, None),
        (true, None),
        (false, Some(-1)),
        (true, Some(0)),
    ] {
        let StreamingOutputHarness {
            process,
            stdout_tx,
            exit_tx,
            output_buffer,
            context,
            rx_event,
        } = streaming_output_harness().await?;
        start_streaming_output(&process, &context);
        #[allow(deprecated)]
        let cwd = context.step_context.turn.cwd.clone().into();
        if timed_out {
            process.mark_timed_out();
            assert_eq!(process.exit_code(), Some(124));
        }
        spawn_exit_watcher(
            Arc::clone(&process),
            &context,
            vec!["example".to_owned()],
            cwd,
            123,
            None,
            output_buffer,
            Instant::now(),
            None,
            None,
        );
        drop(stdout_tx);
        if let Some(code) = code {
            exit_tx.send(code).expect("report the driver's exit");
        } else {
            // The driver remains alive but has supplied no exit. A timed out tool still
            // has its response convention, independently of what the backend reported.
            process.terminate_confirmed().await?;
        }
        let item = tokio::time::timeout(Duration::from_secs(60), async {
            loop {
                if let EventMsg::ItemCompleted(event) = rx_event.recv().await?.msg
                    && let TurnItem::CommandExecution(item) = event.item
                {
                    return Ok::<_, async_channel::RecvError>(item);
                }
            }
        })
        .await??;
        assert_eq!(item.status, CommandExecutionStatus::Failed);
        assert_eq!(item.exit_code, code);
        assert!(item.duration.is_some());
        if timed_out {
            assert_eq!(process.exit_code(), Some(124));
        }
    }
    Ok(())
}
