// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

#[tokio::test]
async fn final_poll_retirement_only_closes_the_original_exited_local_readers() -> anyhow::Result<()>
{
    use std::io::Write;

    let dir = tempfile::tempdir()?;
    let gate = dir.path().join("hold-stderr");
    anyhow::ensure!(
        std::process::Command::new("mkfifo")
            .arg(&gate)
            .status()?
            .success()
    );
    // The helper owns a read end. Closing this test's writer also releases the
    // helper if the test exits before its explicit release.
    let mut release = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&gate)?;
    let backend = LocalProcess::default();
    let mut params = test_exec_params(HashMap::new());
    params.pipe_stdin = true;
    params.argv = vec![
        "sh".into(), "-c".into(),
        r#"exec 3< "$1"; (IFS= read -r go <&3; printf trailing >&2) & printf ready; IFS= read -r code; exit "$code""#.into(),
        "sh".into(), gate.to_string_lossy().into_owned(),
    ];
    let (observed, mut raw) = mpsc::unbounded_channel();
    let observer: LocalOutputObserver = Arc::new(move |chunk| {
        let _ = observed.send(chunk.map(<[u8]>::to_vec));
    });
    let started = backend
        .start_with_local_output_observer(params, observer)
        .await?;
    let root = &started.process;
    assert!(root.started_with_local_output_observer());
    let mut events = root.subscribe_events();
    let wait = Duration::from_secs(60);
    root.stop_local_output_after_exit().await?;
    assert_eq!(
        timeout(wait, raw.recv()).await?,
        Some(Some(b"ready".to_vec()))
    );
    let impostor = LocalExecProcess {
        process_id: root.process_id().clone(),
        backend: backend.clone(),
        started_with_output_observer: true,
        wake_tx: watch::channel(0).0,
        events: ExecProcessEventLog::new(1, 128),
    };
    root.write(b"19\n".to_vec()).await?;
    timeout(wait, async {
        loop {
            if let ExecProcessEvent::Exited { exit_code, .. } = events.recv().await? {
                anyhow::ensure!(exit_code == 19);
                return Ok::<_, anyhow::Error>(());
            }
        }
    })
    .await??;

    assert!(!impostor.dispose_owned_local_capture().await?);
    impostor.stop_local_output_after_exit().await?;
    {
        let processes = backend.inner.processes.lock().await;
        let Some(ProcessEntry::Running(process)) = processes.get(root.process_id()) else {
            anyhow::bail!("original process record missing")
        };
        assert!(process.output_observer.is_some());
    }
    root.stop_local_output_after_exit().await?;
    timeout(wait, async {
        loop {
            if matches!(events.recv().await?, ExecProcessEvent::Closed { .. }) {
                return Ok::<_, anyhow::Error>(());
            }
        }
    })
    .await??;
    // The owner retired the file as partial. No synthetic raw EOF may mark it complete.
    assert!(timeout(wait, raw.recv()).await?.is_none());
    release.write_all(b"finish\n")?;
    backend.shutdown().await;
    Ok(())
}
