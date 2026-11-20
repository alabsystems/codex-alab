// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn cleanup_preserves_a_pending_launch_number_until_its_last_release() -> anyhow::Result<()> {
    let (session, turn) = test_session_and_turn().await;
    let manager = &session.services.unified_exec_manager;
    let (old_number, old_launch) = manager.reserve_process_id().await;
    manager.terminate_all_processes().await;
    let (replacement_number, replacement_launch) = manager.reserve_process_id().await;
    assert_ne!(replacement_number, old_number);

    let (terminate_started, _) = watch::channel(false);
    let allow_terminate = Arc::new(Notify::new());
    allow_terminate.notify_one();
    let replacement =
        blocking_terminate_unified_process(replacement_number, terminate_started, allow_terminate)
            .await?;
    #[allow(deprecated)]
    let cwd = turn.cwd.clone().into();
    manager.process_store.lock().await.processes.insert(
        replacement_number,
        ProcessEntry {
            process: Arc::clone(&replacement),
            plugin_metrics_sidecar: None,
            call_id: "replacement".into(),
            process_id: replacement_number,
            cwd,
            initial_exec_command_active: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            hook_command: "replacement".into(),
            tty: false,
            environment_id: codex_exec_server::LOCAL_ENVIRONMENT_ID.into(),
            permissions: TerminalPermissions::for_launch(
                turn.initial_environments
                    .primary()
                    .expect("turn environment"),
                &turn,
                TerminalSandboxSource::Native,
                SandboxPermissions::UseDefault,
                None,
                None,
            ),
            network_approval: None,
            session: Arc::downgrade(&session),
            last_used: Instant::now(),
        },
    );
    drop(replacement_launch);
    manager.release_process_id(old_number).await;
    let live = manager.resolve_terminal(replacement_number).await?;
    assert!(Arc::ptr_eq(&live.process, &replacement));
    drop(old_launch);

    manager.release_process_id(replacement_number).await;
    manager.terminate_all_processes().await;
    let (reusable, _launch) = manager.reserve_process_id().await;
    assert_eq!(reusable, old_number);
    assert!(
        !manager
            .process_store
            .lock()
            .await
            .in_flight_launches
            .contains_key(&replacement_number)
    );
    Ok(())
}
