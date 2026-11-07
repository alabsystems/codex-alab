// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Explicit session selection stays in local account UI, never conversation history.
#![cfg(unix)]

use super::ChatWidget;
use crate::app_event::AppEvent;
use codex_alab_core::assignment;

fn progress(observation: &assignment::Observation) -> &'static str {
    use assignment::Progress;
    match observation.progress {
        Progress::Preparing => "Checking the requested session account…",
        Progress::Draining => {
            "Waiting for active work to finish before changing this session's account…"
        }
        Progress::Committing => "Changing this session's account…",
        Progress::Rebinding => "Loading the verified session account…",
        Progress::Initializing => "Preparing account services…",
        Progress::Acknowledging => "Confirming the new session account…",
        Progress::Applied => {
            "This session's account selection is applied. Existing work is preserved."
        }
        Progress::Refused => {
            "Account selection was refused before installation. The session account is unchanged."
        }
        Progress::Cancelled => "Account selection was cancelled before installation.",
        Progress::Reconciling => {
            "Account selection needs reconciliation. New account work remains paused."
        }
    }
}

impl ChatWidget {
    pub(crate) fn alab_accounts_assignment(&mut self, verb: &str, label: &str) {
        let view = self.alab_accounts_view();
        let thread_id = self.thread_id();
        let Some(thread) = thread_id else {
            self.alab_accounts_ready(
                view,
                thread_id,
                "Start a conversation before selecting its session account.".into(),
            );
            return;
        };
        let Some(consumer) = assignment::consumer(&thread.to_string()) else {
            self.alab_accounts_ready(
                view,
                thread_id,
                "Session account selection is unavailable on this client.".into(),
            );
            return;
        };
        let label = label.trim().to_owned();
        let auto = verb == "auto";
        if (!auto && label.is_empty())
            || (auto && !label.is_empty())
            || label.chars().count() > 128
            || label.chars().any(char::is_control)
        {
            self.alab_accounts_ready(
                view,
                thread_id,
                "Use /accounts use <configured label> or /accounts auto.".into(),
            );
            return;
        }
        let operation = codex_alab_tui::aterm::enrollment::new_operation_id();
        let tx = self.app_event_tx.clone();
        // The accepted explicit command owns this request. Pane disposal cancels observation only.
        tokio::spawn(async move {
            let metadata = async {
                let client = codex_alab_tui::aterm::Client::connect_accounts_self()
                    .await
                    .ok()
                    .flatten()?;
                let identity = codex_alab_tui::identity::current().ok()?;
                let birth = codex_alab_tui::identity::proc_start_utc(&identity)?;
                let snapshot = client.accounts_snapshot(&birth).await.ok()?;
                let selection = if auto {
                    assignment::Selection::Auto {
                        revision: snapshot.revision,
                    }
                } else {
                    let mut matching = snapshot.accounts.iter().filter(|row| row.label == label);
                    let target = matching.next()?;
                    if matching.next().is_some() {
                        return None;
                    }
                    assignment::Selection::Manual {
                        account: target.id.clone(),
                        revision: snapshot.revision,
                    }
                };
                Some(assignment::Intent {
                    operation: operation.clone(),
                    thread: thread.to_string(),
                    selection,
                })
            }
            .await;
            let Some(intent) = metadata else {
                tx.send(AppEvent::AlabAccountsReady { view, thread_id, text: "The configured account could not be verified. Refresh /accounts before selecting it.".into() });
                return;
            };
            if consumer.request(intent).await.is_err() {
                tx.send(AppEvent::AlabAccountsReady { view, thread_id, text: "Session account selection is unavailable or an existing request is pending.".into() });
                return;
            }
            loop {
                if view.upgrade().is_none() {
                    break;
                }
                if let Some(observation) = consumer
                    .observation()
                    .filter(|state| state.operation == operation)
                {
                    tx.send(AppEvent::AlabAccountsReady {
                        view: view.clone(),
                        thread_id,
                        text: progress(&observation).into(),
                    });
                    if matches!(
                        observation.progress,
                        assignment::Progress::Applied
                            | assignment::Progress::Cancelled
                            | assignment::Progress::Refused
                            | assignment::Progress::Reconciling
                    ) {
                        break;
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chatwidget::tests::make_chatwidget_manual_with_sender;
    use crate::chatwidget::tests::render_bottom_popup;

    #[tokio::test]
    async fn assignment_command_without_a_thread_changes_only_local_pane() {
        let (mut chat, _tx, mut rx, mut ops) = make_chatwidget_manual_with_sender().await;
        chat.thread_id = None;
        chat.apply_external_edit("original unfinished work".into());
        while rx.try_recv().is_ok() {}
        while ops.try_recv().is_ok() {}
        chat.alab_accounts_assignment("use", "Spare");
        assert!(render_bottom_popup(&chat, 80).contains("Start a conversation"));
        assert_eq!(
            chat.composer_text_with_pending(),
            "original unfinished work"
        );
        assert!(rx.try_recv().is_err(), "no conversation event is emitted");
        assert!(ops.try_recv().is_err(), "no model operation is emitted");
    }

    #[test]
    fn assignment_unknown_commit_wording_never_claims_rollback() {
        let observation = assignment::Observation {
            operation: "a".repeat(32),
            progress: assignment::Progress::Reconciling,
            target: None,
            mode: None,
            reason: None,
        };
        assert!(progress(&observation).contains("paused"));
        assert!(!progress(&observation).contains("unchanged"));
    }
}
