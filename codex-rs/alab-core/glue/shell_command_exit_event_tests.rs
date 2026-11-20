// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

#[tokio::test]
async fn exec_completion_reports_only_observed_process_exit_codes() {
    use CommandExecutionStatus::Completed;
    use CommandExecutionStatus::Declined;
    use CommandExecutionStatus::Failed;

    let (session, turn, rx_event) =
        make_session_and_context_with_dynamic_tools_and_rx(Vec::new()).await;
    let directory = tempdir().unwrap();
    let emitter = ToolEmitter::unified_exec(
        &["example".to_owned()],
        PathUri::from_host_native_path(directory.path()).unwrap(),
        ExecCommandSource::UnifiedExecStartup,
        None,
        None,
    );
    let elapsed = Duration::from_millis(17);
    for (id, stage, status, code, duration) in [
        (
            "setup",
            ToolEventStage::Failure(ToolEventFailure::Message("setup failed".into())),
            Failed,
            None,
            None,
        ),
        (
            "decline",
            ToolEventStage::Failure(ToolEventFailure::Rejected {
                message: "declined".into(),
                applied_patch_delta: None,
            }),
            Declined,
            None,
            None,
        ),
        (
            "nonzero",
            ToolEventStage::Failure(ToolEventFailure::Output(ExecToolCallOutput {
                exit_code: -1,
                duration: elapsed,
                ..Default::default()
            })),
            Failed,
            Some(-1),
            Some(elapsed),
        ),
        (
            "zero",
            ToolEventStage::Success {
                output: ExecToolCallOutput {
                    exit_code: 0,
                    duration: Duration::ZERO,
                    ..Default::default()
                },
                applied_patch_delta: None,
            },
            Completed,
            Some(0),
            Some(Duration::ZERO),
        ),
    ] {
        emitter
            .emit(
                ToolEventCtx::new(session.as_ref(), turn.as_ref(), turn.model_info(), id, None),
                stage,
            )
            .await;
        let item = tokio::time::timeout(Duration::from_secs(60), async {
            loop {
                let event = rx_event.recv().await.expect("session event stream");
                if let EventMsg::ItemCompleted(event) = event.msg
                    && let TurnItem::CommandExecution(item) = event.item
                    && item.id == id
                {
                    break item;
                }
            }
        })
        .await
        .expect("command completion");
        assert_eq!(
            (item.status, item.exit_code, item.duration),
            (status, code, duration)
        );
    }
}
