// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual App entry and embedded backend; SAME local Git effect and queued result.
//! No Git hook/filter, spawn replacement, Native grant or cross-image future transfer.
use crate::app::AppEvent;
use crate::app_event::ManagedWorktreeCreated;
use crate::app_event::ManagedWorktreeMode;
use codex_utils_absolute_path::AbsolutePathBuf;
use std::collections::VecDeque;
use std::future::Future;
use std::future::poll_fn;
use std::io;
use std::path::Path;
use std::process::Command;
use std::task::Poll;
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(60);
fn failure(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}
fn git(source: &Path, args: &[&str]) -> io::Result<String> {
    let mut command = Command::new("git");
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
    // Restore isolation after removing inherited repository/config selectors.
    // These apply only to this child; the test never changes process-global env.
    command
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_ATTR_NOSYSTEM", "1")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_LFS_SKIP_SMUDGE", "1");
    let result = command
        .current_dir(source)
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=",
            "-c",
            "attr.tree=",
            "-c",
            "core.attributesFile=",
            "-c",
            "user.name=Owned Worktree Test",
            "-c",
            "user.email=test@example.invalid",
        ])
        .args(args)
        .output()?;
    if !result.status.success() {
        return Err(failure(String::from_utf8_lossy(&result.stderr)));
    }
    String::from_utf8(result.stdout)
        .map(|s| s.trim().to_owned())
        .map_err(failure)
}

#[tokio::test(flavor = "current_thread")]
async fn actual_worktree_creation_cut_retains_one_git_effect_and_same_created_event()
-> io::Result<()> {
    let fixture = tempfile::tempdir()?;
    let root = dunce::canonicalize(fixture.path())?;
    let home = root.join("home");
    let source = root.join("source");
    let worktrees = root.join("managed-worktrees");
    std::fs::create_dir_all(&home)?;
    std::fs::create_dir_all(&source)?;
    std::fs::create_dir_all(&worktrees)?;
    std::fs::write(
        home.join("config.toml"),
        format!(
            "[features]\nworktrees = true\n[desktop]\ngit-worktree-root = {}\n",
            toml::Value::String(worktrees.display().to_string()),
        ),
    )?;
    std::fs::write(source.join("original"), "same committed worktree bytes\n")?;
    git(&source, &["init", "--quiet"])?;
    git(&source, &["add", "."])?;
    git(
        &source,
        &["commit", "--quiet", "--no-gpg-sign", "-m", "owned source"],
    )?;
    let original_head = git(&source, &["rev-parse", "HEAD"])?;
    crate::legacy_core::config::set_project_trust_level(
        &home,
        &source,
        codex_protocol::config_types::TrustLevel::Trusted,
    )
    .map_err(failure)?;
    let (mut app, mut events, _ops) =
        crate::boxed_future!(crate::app::tests::make_test_app_with_channels()).await;
    app.config.codex_home = AbsolutePathBuf::from_absolute_path(home.clone()).map_err(failure)?;
    app.config.cwd = AbsolutePathBuf::from_absolute_path(source.clone()).map_err(failure)?;
    app.config.sqlite = codex_state::SqliteConfig::new_for_testing(app.config.codex_home.clone());
    app.harness_overrides.permission_profile =
        Some(codex_protocol::models::PermissionProfile::workspace_write());
    app.config = crate::boxed_future!(app.rebuild_config_for_cwd(source.clone()))
        .await
        .map_err(failure)?;
    let mut server = crate::boxed_future!(crate::start_embedded_app_server_for_picker(&app.config))
        .await
        .map_err(failure)?;
    let mut created: Option<Box<ManagedWorktreeCreated>> = None;
    let mut original_fifo = VecDeque::new();
    let mut creation_started = false;
    let body = async {
        // The real embedded server supplies the original thread; no invented
        // ThreadSessionState/Native owner is assigned into App fields.
        let started = crate::boxed_future!(server.start_thread(&app.config))
            .await
            .map_err(failure)?;
        let thread = started.session.thread_id;
        app.enqueue_primary_thread_session(started.session, started.turns)
            .await
            .map_err(failure)?;
        if app.primary_thread_id != Some(thread)
            || !app.chat_widget.can_change_working_directory(thread)
        {
            return Err(failure("genuine original primary is not idle/changeable"));
        }
        // Join the SAME session book before entry. Entry's source-only preflight
        // joins its Config worker and awaits direct background-terminal RPCs;
        // none of those calls register a different Session work job.
        let baseline = server.alab_capture_original_session_jobs()?;
        let baseline_ready = tokio::time::timeout(WAIT, baseline.observe())
            .await
            .map_err(failure)??;
        baseline_ready.current(&server)?;
        app.start_managed_worktree(
            &mut server,
            ManagedWorktreeMode::New,
            Some("owned original".into()),
        )
        .await;
        creation_started = app.pending_managed_worktree_creation;
        if !creation_started {
            return Err(failure(
                "actual worktree entry refused before original spawn",
            ));
        }
        // On current_thread no task accepted by the final spawn can poll until
        // this test yields. No await lies between that entry and this exact cut.
        let cut = server.alab_capture_original_session_jobs()?;
        {
            let mut observing = std::pin::pin!(cut.observe());
            poll_fn(|cx| match observing.as_mut().poll(cx) {
                Poll::Pending => Poll::Ready(Ok(())),
                Poll::Ready(_) => Poll::Ready(Err(failure(
                    "worktree cut settled before the original create task could poll",
                ))),
            })
            .await?;
        }
        // Cancel only the join observer, then join the SAME accepted task. Do
        // not call create/start a second time or abort the blocking Git worker.
        let ready = tokio::time::timeout(WAIT, cut.observe())
            .await
            .map_err(failure)??;
        ready.current(&server)?;
        while let Ok(event) = events.try_recv() {
            match event {
                AppEvent::ManagedWorktreeCreated(actual) if created.is_none() => {
                    created = Some(actual)
                }
                AppEvent::ManagedWorktreeCreated(actual) => {
                    original_fifo.push_back(AppEvent::ManagedWorktreeCreated(actual));
                    return Err(failure("original task published duplicate created events"));
                }
                event => original_fifo.push_back(event),
            }
        }
        let actual = created.as_ref().ok_or_else(|| {
            failure("joined original Git task did not publish its actual created event")
        })?;
        if actual.source_thread_id != thread
            || actual.source_cwd.as_path() != source.as_path()
            || actual.mode != ManagedWorktreeMode::New
            || actual.name.as_deref() != Some("owned original")
        {
            return Err(failure(
                "actual created event changed its original request DATA",
            ));
        }
        let (manager, checkout) = actual.result.as_ref().map_err(failure)?;
        let actual_root = dunce::canonicalize(&checkout.root)?;
        let actual_cwd = dunce::canonicalize(&checkout.cwd)?;
        if actual_root == worktrees
            || !actual_root.starts_with(&worktrees)
            || !actual_cwd.starts_with(&actual_root)
        {
            return Err(failure(
                "actual checkout escaped the explicitly owned allocation root",
            ));
        }
        if checkout.head_sha != original_head
            || std::fs::read(checkout.cwd.join("original"))? != b"same committed worktree bytes\n"
        {
            return Err(failure("actual checkout lost the original committed input"));
        }
        if manager.list(&source).map_err(failure)?.len() != 1
            || git(&source, &["worktree", "list", "--porcelain"])?
                .lines()
                .filter(|line| line.starts_with("worktree "))
                .count()
                != 2
        {
            return Err(failure(
                "creation did not yield exactly one real managed checkout",
            ));
        }
        if git(&source, &["rev-parse", "HEAD"])? != original_head {
            return Err(failure("original source HEAD changed"));
        }
        while let Ok(event) = events.try_recv() {
            if matches!(event, AppEvent::ManagedWorktreeCreated(_)) {
                original_fifo.push_back(event);
                return Err(failure(
                    "duplicate original created event after completion cut",
                ));
            }
            original_fifo.push_back(event);
        }
        Ok::<_, io::Error>(())
    }
    .await;
    // Cleanup owns BOTH the same Session shutdown/join and the original event
    // receiver. On old untracked code, still wait for actual Created so source
    // files cannot disappear while its detached blocking effect is running.
    let shutdown = tokio::time::timeout(WAIT, server.shutdown())
        .await
        .map_err(failure)
        .and_then(|result| result);
    let completion = if creation_started && created.is_none() {
        tokio::time::timeout(WAIT, async {
            loop {
                match events
                    .recv()
                    .await
                    .ok_or_else(|| failure("actual created receiver closed during cleanup"))?
                {
                    AppEvent::ManagedWorktreeCreated(actual) => {
                        created = Some(actual);
                        break Ok::<_, io::Error>(());
                    }
                    event => original_fifo.push_back(event),
                }
            }
        })
        .await
        .map_err(failure)
        .and_then(|result| result)
    } else {
        Ok(())
    };
    if shutdown.is_err() || completion.is_err() {
        let retained = fixture.keep();
        return Err(failure(format!(
            "owned cleanup unresolved; retained source/checkout at {}: body={:?}, shutdown={:?}, completion={:?}",
            retained.display(),
            body.err(),
            shutdown.err(),
            completion.err()
        )));
    }
    body?;
    Ok(())
}
