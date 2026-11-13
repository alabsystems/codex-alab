// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Explicit session selection stays in local account UI, never conversation history.
#![cfg(unix)]

use super::ChatWidget;
use super::accounts_pool::CHANGES_UNAVAILABLE;
use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use codex_alab_core::assignment;
use codex_alab_tui::accounts_ui::Update;
use codex_alab_tui::accounts_ui::View;
use codex_alab_tui::aterm::accounts::Snapshot;
use codex_protocol::ThreadId;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::Weak;

#[cfg(test)]
#[path = "assignment_pool_tests.rs"]
mod observation_tests;

const UNVERIFIED: &str =
    "The configured account could not be verified. Refresh /accounts before selecting it.";
const PENDING: &str = "Another account change is already pending for this session.";

/// The selection an accepted command names in this snapshot, or the sentence that refuses it.
/// The native owner's own capability is read first: core cannot see it, and a host that says
/// it cannot select must not be asked to.
fn selection(
    snapshot: &Snapshot,
    auto: bool,
    label: &str,
) -> Result<assignment::Selection, &'static str> {
    if auto {
        if !snapshot.capabilities.auto {
            return Err(CHANGES_UNAVAILABLE);
        }
        return Ok(assignment::Selection::Auto {
            revision: snapshot.revision.clone(),
        });
    }
    if !snapshot.capabilities.select {
        return Err(CHANGES_UNAVAILABLE);
    }
    let mut matching = snapshot.accounts.iter().filter(|row| row.label == label);
    match (matching.next(), matching.next()) {
        (Some(target), None) => Ok(assignment::Selection::Manual {
            account: target.id.clone(),
            revision: snapshot.revision.clone(),
        }),
        _ => Err(UNVERIFIED),
    }
}

/// Why core refused, in the pane's words. Nothing was started in any of these cases.
fn refusal(refused: assignment::Refused) -> &'static str {
    use assignment::Refused;
    match refused {
        Refused::Unavailable => CHANGES_UNAVAILABLE,
        Refused::Busy | Refused::Pending => PENDING,
        Refused::Invalid | Refused::WrongDomain => UNVERIFIED,
    }
}

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

async fn request_and_observe_assignment(
    consumer: Arc<dyn assignment::Consumer>,
    intent: assignment::Intent,
    view: Weak<Mutex<View>>,
    thread_id: Option<ThreadId>,
    tx: AppEventSender,
) {
    let operation = intent.operation.clone();
    // Core refuses before it closes admission; a refusal leaves the session as it was.
    if let Err(refused) = consumer.request(intent).await {
        tx.send(AppEvent::AlabAccountsReady {
            view,
            thread_id,
            text: Update::Observation(refusal(refused).into()),
        });
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
                text: Update::Observation(progress(&observation).into()),
            });
            if matches!(
                observation.progress,
                assignment::Progress::Applied
                    | assignment::Progress::Cancelled
                    | assignment::Progress::Refused
            ) {
                break;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
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
                Update::Observation(
                    "Start a conversation before selecting its session account.".into(),
                ),
            );
            return;
        };
        let Some(consumer) = assignment::consumer(&thread.to_string()) else {
            self.alab_accounts_ready(
                view,
                thread_id,
                Update::Observation(
                    "Session account selection is unavailable on this client.".into(),
                ),
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
                Update::Observation(
                    "Use /accounts use <configured label> or /accounts auto.".into(),
                ),
            );
            return;
        }
        let operation = codex_alab_tui::aterm::enrollment::new_operation_id();
        let tx = self.app_event_tx.clone();
        // The accepted explicit command owns this request. Pane disposal cancels observation only.
        tokio::spawn(async move {
            let snapshot = async {
                let client = codex_alab_tui::aterm::Client::connect_accounts_self()
                    .await
                    .ok()
                    .flatten()?;
                let identity = codex_alab_tui::identity::current().ok()?;
                let birth = codex_alab_tui::identity::proc_start_utc(&identity)?;
                client.accounts_snapshot(&birth).await.ok()
            }
            .await;
            let selected = snapshot
                .as_ref()
                .ok_or(UNVERIFIED)
                .and_then(|snapshot| selection(snapshot, auto, &label));
            match selected {
                Ok(selection) => {
                    let intent = assignment::Intent {
                        operation,
                        thread: thread.to_string(),
                        selection,
                    };
                    request_and_observe_assignment(consumer, intent, view, thread_id, tx).await;
                }
                // What the snapshot refuses never reaches core.
                Err(text) => tx.send(AppEvent::AlabAccountsReady {
                    view,
                    thread_id,
                    text: Update::Observation(text.into()),
                }),
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

    fn snapshot(capable: bool) -> Snapshot {
        use codex_alab_tui::aterm::accounts::*;
        let account = |id: &str, label: &str| Account {
            id: id.repeat(64),
            label: label.into(),
            masked_email: None,
            condition: Condition::Usable,
            enrolled: true,
            held: false,
            eligible: false,
            quota: Quota {
                stale: true,
                observed_at: None,
                five_hour_used_percent: None,
                five_hour_reset_at: None,
                weekly_used_percent: None,
                weekly_reset_at: None,
            },
        };
        Snapshot {
            schema: 1,
            observed_at: 1_800_000_000,
            request: "a".repeat(32),
            session: Session {
                sid: "s-a".into(),
                process_pid: 1,
                process_birth: "birth".into(),
                epoch: "epoch".into(),
            },
            revision: "b".repeat(64),
            assignment: Assignment {
                account_id: None,
                mode: Mode::Unknown,
            },
            accounts: vec![
                account("1", "Spare"),
                account("2", "Twin"),
                account("3", "Twin"),
            ],
            capabilities: Capabilities {
                add: false,
                select: capable,
                auto: capable,
            },
        }
    }

    #[test]
    fn selection_refuses_when_the_host_advertises_no_capability() {
        // Every snapshot aterm's parser accepts today looks like this one.
        let snapshot = snapshot(false);
        assert_eq!(selection(&snapshot, true, ""), Err(CHANGES_UNAVAILABLE));
        assert_eq!(
            selection(&snapshot, false, "Spare"),
            Err(CHANGES_UNAVAILABLE)
        );
        assert_eq!(
            selection(&snapshot, false, "Unknown"),
            Err(CHANGES_UNAVAILABLE),
            "the host's refusal is not a reason to refresh and retry"
        );
        // Each capability is its own: one does not lend the other.
        let mut only_auto = snapshot;
        only_auto.capabilities.auto = true;
        assert!(selection(&only_auto, true, "").is_ok());
        assert_eq!(
            selection(&only_auto, false, "Spare"),
            Err(CHANGES_UNAVAILABLE)
        );
    }

    #[test]
    fn selection_names_exactly_one_configured_account_or_auto() {
        let snapshot = snapshot(true);
        assert_eq!(
            selection(&snapshot, true, ""),
            Ok(assignment::Selection::Auto {
                revision: "b".repeat(64)
            })
        );
        assert_eq!(
            selection(&snapshot, false, "Spare"),
            Ok(assignment::Selection::Manual {
                account: "1".repeat(64),
                revision: "b".repeat(64)
            })
        );
        assert_eq!(selection(&snapshot, false, "Twin"), Err(UNVERIFIED));
        assert_eq!(selection(&snapshot, false, "Unknown"), Err(UNVERIFIED));
    }

    #[test]
    fn a_refused_request_says_why_in_the_pane_sentence() {
        use assignment::Refused;
        assert_eq!(refusal(Refused::Unavailable), CHANGES_UNAVAILABLE);
        assert_eq!(
            CHANGES_UNAVAILABLE,
            "Account changes are unavailable on this host."
        );
        assert_eq!(refusal(Refused::Busy), PENDING);
        assert_eq!(refusal(Refused::Pending), PENDING);
        assert_eq!(refusal(Refused::Invalid), UNVERIFIED);
        assert_eq!(refusal(Refused::WrongDomain), UNVERIFIED);
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
