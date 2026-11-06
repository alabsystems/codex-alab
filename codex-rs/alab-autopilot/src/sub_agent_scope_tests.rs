// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A SUB-AGENT'S WORK IS ITS ROOT SESSION'S (the forever-persist lane, item d; owner 2026-10-19,
//! restated 2026-10-29: "if landing LAND EVERYTHING INTO REMOTE MAIN NO EXCEPTIONS"): a thread a
//! sub-agent runs has no runtime of its own, so its commands and edits never entered the probe
//! scope Land reads before it lands, and a folder only a sub-agent touched was landed past.

use super::upgrade_handoff_tests::Harness;
use super::upgrade_handoff_tests::native_test;
use super::*;
use codex_extension_api::CommandStartInput;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::SubAgentSource;
use std::path::PathBuf;

/// A thread of `source` starts with `store`.
async fn start(h: &Harness, store: &ExtensionData, source: &SessionSource) {
    h.autopilot
        .on_thread_start(ThreadStartInput {
            config: &h.test.config,
            session_source: source,
            persistent_thread_state_available: true,
            environments: &[],
            mcp_resource_client: None,
            extension_metrics: None,
            session_store: &ExtensionData::new("session"),
            thread_store: store,
        })
        .await;
}

/// A sub-agent the thread `parent` spawned.
fn spawned_by(parent: ThreadId, depth: i32) -> SessionSource {
    SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
        parent_thread_id: parent,
        depth,
        agent_path: None,
        agent_nickname: None,
        agent_role: None,
    })
}

/// Command `argv` started by the thread of `store` in folder `dir`.
async fn command(h: &Harness, store: &ExtensionData, dir: &str, argv: &[&str]) {
    let cwd = codex_utils_path_uri::PathUri::parse(&format!("file://{dir}"))
        .unwrap_or_else(|err| panic!("{dir}: {err}"));
    let command: Vec<String> = argv.iter().map(ToString::to_string).collect();
    h.autopilot
        .on_command_start(CommandStartInput {
            session_store: &ExtensionData::new("session"),
            thread_store: store,
            turn_store: &ExtensionData::new("turn-1"),
            turn_id: "turn-1",
            call_id: "call-1",
            command: &command,
            cwd: &cwd,
            file_system: codex_exec_server::LOCAL_FS.as_ref(),
        })
        .await;
}

/// An edit by the thread of `store` that added the file `path`.
async fn edit(h: &Harness, store: &ExtensionData, path: &str) {
    let item = TurnItem::FileChange(codex_protocol::items::FileChangeItem {
        id: "patch-1".to_string(),
        changes: [(
            PathBuf::from(path),
            codex_protocol::protocol::FileChange::Add {
                content: String::new(),
            },
        )]
        .into_iter()
        .collect(),
        status: None,
        auto_approved: None,
        stdout: None,
        stderr: None,
    });
    h.autopilot
        .on_item_completed(store, &ExtensionData::new("turn-1"), &item)
        .await;
}

/// A SUB-AGENT'S COMMANDS AND EDITS MOVE ITS ROOT SESSION'S SCOPE: the folder a spawned
/// sub-agent ran a command in, one its command reached, a branch it named and a folder its edit
/// landed in are the root's scope, which the record carries, and so are those of a sub-agent
/// that sub-agent spawned (it names only its parent). The root's own folder, where its probe
/// reads first, stays its worker's.
/// NEGATIVE CONTROLS: a review's sub-agent (it names no parent) and a sub-agent of a thread this
/// process keeps no runtime for fold nothing, nor does one a stopped sub-agent spawned (the stop
/// took its parent out of the sub-agents read); no sub-agent gets a runtime of its own.
#[test]
fn a_sub_agents_commands_and_edits_move_its_root_sessions_scope() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            let h = Harness::new().await;
            let workdir = lock(&h.rt.workdir).clone();
            let child = ThreadId::new();
            let child_store = ExtensionData::new(child.to_string());
            start(&h, &child_store, &spawned_by(h.rt.id, 1)).await;
            assert!(
                child_store.get::<ThreadRt>().is_none(),
                "no runtime of its own"
            );
            command(
                &h,
                &child_store,
                "/work/child",
                &[
                    "bash",
                    "-lc",
                    "make -C ../reached all && git switch -c sub-fix",
                ],
            )
            .await;
            edit(&h, &child_store, "/work/edited/x.rs").await;
            // A sub-agent the sub-agent spawned: its root is the root's too.
            let grandchild = ThreadId::new();
            let grandchild_store = ExtensionData::new(grandchild.to_string());
            start(&h, &grandchild_store, &spawned_by(child, 2)).await;
            command(
                &h,
                &grandchild_store,
                "/work/grandchild",
                &["git", "status"],
            )
            .await;
            // NEGATIVE CONTROLS: a review's, and one whose parent has no runtime here.
            let review = ExtensionData::new(ThreadId::new().to_string());
            start(
                &h,
                &review,
                &SessionSource::SubAgent(SubAgentSource::Review),
            )
            .await;
            command(&h, &review, "/elsewhere/review", &["git", "status"]).await;
            edit(&h, &review, "/elsewhere/review-edit/x.rs").await;
            let stray = ExtensionData::new(ThreadId::new().to_string());
            start(&h, &stray, &spawned_by(ThreadId::new(), 1)).await;
            command(&h, &stray, "/elsewhere/stray", &["git", "status"]).await;
            // And one a stopped sub-agent spawned.
            h.autopilot
                .on_thread_stop(ThreadStopInput {
                    session_store: &ExtensionData::new("session"),
                    thread_store: &grandchild_store,
                })
                .await;
            let late = ExtensionData::new(ThreadId::new().to_string());
            start(&h, &late, &spawned_by(grandchild, 3)).await;
            command(&h, &late, "/elsewhere/late", &["git", "status"]).await;
            let scope = h.rt.probe_scope();
            for dir in [
                "/work/child",
                "/work/reached",
                "/work/edited",
                "/work/grandchild",
            ] {
                assert!(scope.dirs.contains(&PathBuf::from(dir)), "{dir}: {scope:?}");
            }
            assert!(
                scope.dirs.iter().all(|dir| !dir.starts_with("/elsewhere")),
                "{scope:?}"
            );
            assert!(
                scope.branches.iter().any(|branch| branch.name == "sub-fix"),
                "{scope:?}"
            );
            assert_eq!(h.shared.probe_scope(h.rt.key()), Some(scope));
            assert_eq!(
                lock(&h.rt.workdir).clone(),
                workdir,
                "the root's folder is its own worker's"
            );
            h.test
                .codex
                .shutdown_and_wait()
                .await
                .expect("native shutdown");
        })
        .await
        .expect("sub-agent scope hang detector");
    });
}

/// A SUB-AGENT'S WORK REACHES A ROOT STOPPED FOR aterm's RELAUNCH (review of the forever-persist
/// lane, F4): a relaunch signal's teardown stops the root and keeps it (`on_thread_stop`), so the
/// record goes on carrying its scope to the relaunch; the root's own commands still move that
/// scope, and a sub-agent's command or edit in the teardown moves it too, where a stopped root
/// had folded nothing and the image after landed past that work. NEGATIVE CONTROL: a root
/// stopped for good is kept no more, and a sub-agent's later work folds into nothing.
#[test]
fn a_sub_agents_work_reaches_a_root_stopped_for_a_relaunch() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            for relaunch in [true, false] {
                let h = Harness::new().await;
                let child = ThreadId::new();
                let child_store = ExtensionData::new(child.to_string());
                start(&h, &child_store, &spawned_by(h.rt.id, 1)).await;
                if relaunch {
                    assert!(
                        h.shared
                            .set_signal(codex_alab_core::session::RELAUNCH_SIGNALS[1])
                    );
                }
                let root_store = ExtensionData::new(h.rt.id.to_string());
                root_store.insert_arc(Arc::clone(&h.rt));
                h.autopilot
                    .on_thread_stop(ThreadStopInput {
                        session_store: &ExtensionData::new("session"),
                        thread_store: &root_store,
                    })
                    .await;
                assert!(h.rt.stopped.load(Ordering::SeqCst), "relaunch={relaunch}");
                command(&h, &child_store, "/work/teardown", &["git", "status"]).await;
                edit(&h, &child_store, "/work/teardown-edit/x.rs").await;
                let scope = h.rt.probe_scope();
                for dir in ["/work/teardown", "/work/teardown-edit"] {
                    assert_eq!(
                        scope.dirs.contains(&PathBuf::from(dir)),
                        relaunch,
                        "relaunch={relaunch} {dir}: {scope:?}"
                    );
                }
                if relaunch {
                    assert_eq!(h.shared.probe_scope(h.rt.key()), Some(scope));
                }
                h.test
                    .codex
                    .shutdown_and_wait()
                    .await
                    .expect("native shutdown");
            }
        })
        .await
        .expect("stopped-root scope hang detector");
    });
}
