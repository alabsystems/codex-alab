// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::time::Duration;

#[tokio::test]
async fn a_forced_root_cannot_claim_a_failure_tail_after_its_descriptors_reached_eof()
-> anyhow::Result<()> {
    for forced in [false, true] {
        let mut child = crate::spawn_pipe_process(
            "/bin/sh",
            &[
                "-c".into(),
                "printf seen; exec >&- 2>&-; IFS= read -r release; exit 9".into(),
            ],
            Path::new("."),
            &Default::default(),
            &None,
            &[],
        )
        .await?;
        let receipt = child.session.native_output_eof();
        tokio::time::timeout(Duration::from_secs(60), async {
            assert_eq!(child.stdout_rx.recv().await, Some(b"seen".to_vec()));
            assert_eq!(child.stdout_rx.recv().await, None);
            assert_eq!(child.stderr_rx.recv().await, None);
        })
        .await?;
        assert_eq!(receipt.observed(), Some(true));
        assert!(!child.session.has_exited());
        if forced {
            child.session.request_terminate();
        } else {
            child.session.writer_sender().send(b"go\n".to_vec()).await?;
        }
        let exit = tokio::time::timeout(Duration::from_secs(60), child.exit_rx).await??;
        if !forced {
            assert_eq!(exit, 9);
        }
        drop(child.session);
        assert_eq!(receipt.observed(), Some(true));
        assert_eq!(receipt.forced_before_exit(), forced);
    }
    Ok(())
}
