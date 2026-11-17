// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Existing native App fixture on a real isolated controlling PTY, original
//! builder/Command/Child/read/callback and actual channel census only. No
//! Native Controller, App scope, Core/Go/currentness or full scene is fabricated.
use super::*;
use std::io;
use std::io::Write;
use std::mem::MaybeUninit;
use std::os::fd::AsRawFd;
use std::os::fd::FromRawFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::process::Stdio;
use std::time::Duration;
use std::time::Instant;
fn failure(message: &'static str) -> io::Error {
    io::Error::other(message)
}
struct ReleaseOnDrop(std::path::PathBuf);
impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        let _ = std::fs::write(&self.0, b"release");
    }
}
fn actual_raw() -> io::Result<bool> {
    let mut modes = MaybeUninit::<libc::termios>::uninit();
    // SAFETY: isolated helper owns its real controlling stdin and output slot.
    if unsafe { libc::tcgetattr(0, modes.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { modes.assume_init() }.c_lflag & (libc::ICANON | libc::ECHO | libc::ISIG) == 0)
}
async fn helper(case: &str) -> io::Result<()> {
    codex_alab_core::activate();
    codex_alab_tui::terminal_modes::enable_raw_mode()?;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    let (mut app, _original_rx, _original_ops) = make_test_app_with_channels().await;
    // Use the native fixture's private Codex home as the first trusted editor
    // directory. Read-only MODEL policy keeps the original directory guard
    // active and avoids falling back to the real user's home in this control.
    app.chat_widget
        .set_permission_profile_from_session_snapshot(PermissionProfileSnapshot::legacy(
            PermissionProfile::read_only(),
        ))
        .map_err(|error| io::Error::other(error.to_string()))?;
    // The existing native fixture is reused. Create the SAME actual pair used
    // by production App::run; neither a label nor a reconstructed census binds.
    let (sender, mut events, census) = crate::copy_input_guard::alab_retained_graph::channel();
    let sender = crate::app_event_sender::AppEventSender::new(sender);
    app.app_event_tx = sender.clone();
    app.chat_widget
        .alab_set_test_app_event_sender(sender.clone());
    let (_foreign_sender, _foreign_rx, foreign) =
        crate::copy_input_guard::alab_retained_graph::channel();
    if app.alab_bind_original_editor_driver(&foreign).is_ok() || !tui.alab_parent_tty_available() {
        return Err(failure(
            "foreign actual App receiver installed an editor driver",
        ));
    }
    app.alab_bind_original_editor_driver(&census)?;
    app.chat_widget
        .apply_external_edit("seed stays on failed spawn".to_string());
    app.request_external_editor_launch(&mut tui);
    if app.chat_widget.external_editor_state() != ExternalEditorState::Requested
        || app.alab_pending_original_editor().is_some()
    {
        return Err(failure(
            "queued original editor request created a Job/Child before dispatch",
        ));
    }
    // This is the original Draw->Launch boundary's actual native state. The
    // production Launch branch calls this SAME original-resource producer.
    app.chat_widget
        .set_external_editor_state(ExternalEditorState::Active);
    if !app.alab_begin_original_editor(&mut tui)? {
        return Err(failure(
            "actual original App driver did not claim the Launch",
        ));
    }
    let job = app
        .alab_pending_original_editor()
        .ok_or_else(|| failure("actual App lost original Job"))?;
    sender.send(AppEvent::Exit(crate::app_event::ExitMode::ShutdownFirst));
    let parked_len = events.len();
    if case == "success" {
        let started = std::path::PathBuf::from(
            std::env::var_os("ALAB_APP_EDITOR_STARTED")
                .ok_or_else(|| failure("actual script marker missing"))?,
        );
        let release = std::path::PathBuf::from(
            std::env::var_os("ALAB_APP_EDITOR_RELEASE")
                .ok_or_else(|| failure("actual script release missing"))?,
        );
        let _release = ReleaseOnDrop(release.clone());
        tokio::time::timeout(Duration::from_secs(60), async {
            while !started.exists() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .map_err(io::Error::other)?;
        if tui.alab_parent_tty_available()
            || !codex_alab_tui::terminal_modes::editor_owns_terminal()
        {
            return Err(failure(
                "real child started without retained actual parent input/modes",
            ));
        }
        // Cancel only the observation, never the internally owned real child.
        if tokio::time::timeout(
            Duration::from_millis(20),
            app.alab_original_editor_or_update(&mut tui, &job, std::future::pending::<()>()),
        )
        .await
        .is_ok()
        {
            return Err(failure(
                "original child wait unexpectedly ended before release",
            ));
        }
        let (update, received) = tokio::sync::oneshot::channel();
        update
            .send(())
            .map_err(|_| failure("real update notification receiver dropped"))?;
        let observed = tokio::time::timeout(
            Duration::from_secs(60),
            app.alab_original_editor_or_update(&mut tui, &job, received),
        )
        .await
        .map_err(io::Error::other)??;
        if !matches!(
            observed,
            super::super::alab_external_editor::Pump::Update(Ok(()))
        ) || !app
            .alab_pending_original_editor()
            .is_some_and(|actual| Arc::ptr_eq(&actual, &job))
            || events.len() != parked_len
        {
            return Err(failure(
                "update observation lost original editor or consumed parked App queue",
            ));
        }
        // The actual registered App Job pins the same capture/spawn and moved
        // parser+mode owners. No Native acceptance, helper or role is faked.
        let c0 = tokio::time::timeout(Duration::from_secs(60), job.stage_original_c0(&tui))
            .await
            .map_err(io::Error::other)??;
        if !c0.is_for(job.operation()) || !c0.spawn()?.is_for(c0.capture()) {
            return Err(failure(
                "actual C0 stage changed original App operation/spawn",
            ));
        }
        let weak = Arc::downgrade(&c0);
        drop(c0); // Dropping the observer does not return or lose real Files.
        let retained = weak
            .upgrade()
            .ok_or_else(|| failure("global App Job lost actual C0 resources"))?;
        let repeated = job.stage_original_c0(&tui).await?;
        if !Arc::ptr_eq(&retained, &repeated) || job.freeze_before_h1().await.is_ok() {
            return Err(failure("C0 stage split owner or admitted same-PID cut"));
        }
        drop(repeated);
        let (update, received) = tokio::sync::oneshot::channel();
        update
            .send(())
            .map_err(|_| failure("held C0 update receiver dropped"))?;
        let observed = tokio::time::timeout(
            Duration::from_secs(60),
            app.alab_original_editor_or_update(&mut tui, &job, received),
        )
        .await
        .map_err(io::Error::other)??;
        if !matches!(
            observed,
            super::super::alab_external_editor::Pump::Update(Ok(()))
        ) || events.len() != parked_len
            || tui.alab_parent_tty_available()
        {
            return Err(failure(
                "C0 stage consumed actual App queue or resumed parent",
            ));
        }
        job.return_local_c0_before_offer(&retained).await?;
        if retained.with_offered_descriptions(|_| Ok(())).is_ok() {
            return Err(failure(
                "returned historical C0 stage exposed original Files",
            ));
        }
        drop(retained);
        // Exercise the actual local C1 resource boundary while this SAME
        // editor still runs. No Native acceptance or permission is invented.
        let cut = tokio::time::timeout(Duration::from_secs(60), job.freeze_before_h1())
            .await
            .map_err(io::Error::other)??;
        if !job.operation().is_for_fence(&cut) || job.finished_before_native(&cut)?.is_some() {
            return Err(failure(
                "actual live App C1 cut changed original operation/result",
            ));
        }
        job.prepare_original_c1_resources(&tui, &cut)?;
        let repeated = job.freeze_before_h1().await?;
        if !Arc::ptr_eq(&cut, &repeated) {
            return Err(failure("actual App C1 observer recreated the original cut"));
        }
        drop(repeated);
        job.release_local_cut_before_native(&cut).await?;
        drop(cut); // The original progression can resume only after SAME last owner drops.
        std::fs::write(release, b"release")?;
    } else if case != "spawn-failure" {
        return Err(failure("unknown isolated App editor case"));
    }
    // SAME ordinary outer-loop consumer; no fake update/Native result is lent.
    tokio::time::timeout(
        Duration::from_secs(60),
        app.alab_wait_local_original_editor(&mut tui),
    )
    .await
    .map_err(io::Error::other)?;
    if app.alab_pending_original_editor().is_some()
        || !tui.alab_parent_tty_available()
        || codex_alab_tui::terminal_modes::editor_owns_terminal()
        || !actual_raw()?
        || app.chat_widget.external_editor_state() != ExternalEditorState::Closed
    {
        return Err(failure(
            "actual App original callback did not retire/restore its native resources",
        ));
    }
    let mut parked_exit_count = 0;
    let mut original_error = false;
    while let Ok(event) = events.try_recv() {
        match event {
            AppEvent::Exit(crate::app_event::ExitMode::ShutdownFirst) => parked_exit_count += 1,
            AppEvent::InsertHistoryCell(cell) => {
                let rendered = cell
                    .display_lines(80)
                    .into_iter()
                    .map(|line| line.to_string())
                    .collect::<String>();
                original_error |= rendered.contains("Failed to open editor:");
            }
            _ => {}
        }
    }
    if parked_exit_count != 1 {
        return Err(failure(
            "original editor consumer dispatched the parked Exit queue",
        ));
    }
    let actual = app.chat_widget.composer_text_with_pending();
    if case == "success" {
        if actual != " actual local App edit" {
            return Err(failure(
                "actual original trim/apply callback changed document content",
            ));
        }
    } else {
        if actual != "seed stays on failed spawn" {
            return Err(failure("failed original spawn changed native composer"));
        }
        if !original_error {
            return Err(failure(
                "actual original App failed-spawn error callback was lost",
            ));
        }
    }
    // Actual worker/root/read were joined by the original callback before the
    // helper restores its private terminal. This is no Native handback receipt.
    codex_alab_tui::terminal_modes::disable_raw_mode()?;
    writeln!(std::io::stdout().lock(), "ALAB_ORIGINAL_APP_EDITOR_OK")?;
    Ok(())
}
#[test]
fn real_app_owns_original_editor_and_parks_queues_until_handback() -> io::Result<()> {
    const CASE: &str = "ALAB_ORIGINAL_APP_EDITOR_CASE";
    const TEST: &str = "app::tests::alab_original_editor_tests::real_app_owns_original_editor_and_parks_queues_until_handback";
    if let Some(case) = std::env::var_os(CASE) {
        let case = case
            .to_str()
            .ok_or_else(|| failure("invalid actual App editor case"))?;
        return tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(helper(case));
    }
    for case in ["success", "spawn-failure"] {
        let root = tempfile::tempdir()?;
        let script = root.path().join("original-editor");
        let started = root.path().join("started");
        let release = root.path().join("release");
        std::fs::write(&script,b"#!/bin/sh\nstty sane </dev/tty || exit 91\nprintf started > \"$ALAB_APP_EDITOR_STARTED\"\nwhile [ ! -e \"$ALAB_APP_EDITOR_RELEASE\" ]; do sleep 0.01; done\nprintf ' actual local App edit \n\n' > \"$1\"\n")?;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700))?;
        let editor = if case == "success" {
            script
        } else {
            root.path().join("missing-original-editor")
        };
        let editor = editor
            .to_str()
            .ok_or_else(|| failure("test editor path is not UTF8"))?;
        let editor = shlex::try_quote(editor).map_err(io::Error::other)?;
        let (mut master, mut slave) = (-1, -1);
        let mut size = libc::winsize {
            ws_row: 24,
            ws_col: 80,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // SAFETY: actual writable out-parameters; both FDs become unique Files.
        if unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut size,
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        let master = unsafe { std::fs::File::from_raw_fd(master) };
        let slave = unsafe { std::fs::File::from_raw_fd(slave) };
        for file in [&master, &slave] {
            if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
                return Err(io::Error::last_os_error());
            }
        }
        let flags = unsafe { libc::fcntl(master.as_raw_fd(), libc::F_GETFL) };
        if flags < 0
            || unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) }
                < 0
        {
            return Err(io::Error::last_os_error());
        }
        let mut command = std::process::Command::new(std::env::current_exe()?);
        command
            .args(["--exact", TEST, "--nocapture"])
            .env(CASE, case)
            .env("VISUAL", editor.as_ref())
            .env("EDITOR", editor.as_ref())
            .env("ALAB_APP_EDITOR_STARTED", &started)
            .env("ALAB_APP_EDITOR_RELEASE", &release)
            .stdin(Stdio::from(slave.try_clone()?))
            .stdout(Stdio::from(slave.try_clone()?))
            .stderr(Stdio::from(slave));
        unsafe {
            command.pre_exec(|| {
                // Only async-signal-safe native calls in the private child.
                if libc::setsid() < 0
                    || libc::ioctl(0, libc::TIOCSCTTY.into(), 0) < 0
                    || libc::tcsetpgrp(0, libc::getpgrp()) < 0
                {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut child = codex_alab_tui::test_pty::PtyChild::spawn(command, master)?;
        let end = Instant::now() + Duration::from_secs(120);
        let mut output = Vec::new();
        loop {
            child.read_output(&mut output)?;
            if let Some(status) = child.try_wait()? {
                while child.read_output(&mut output)? > 0 {
                    if Instant::now() >= end {
                        return Err(failure("actual App helper output never ended"));
                    }
                }
                if !status.success()
                    || !String::from_utf8_lossy(&output).contains("ALAB_ORIGINAL_APP_EDITOR_OK")
                {
                    return Err(io::Error::other(format!(
                        "actual App editor {case} failed: {}",
                        String::from_utf8_lossy(&output)
                    )));
                }
                break;
            }
            if Instant::now() >= end {
                return Err(failure("actual App editor helper timed out"));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    Ok(())
}
