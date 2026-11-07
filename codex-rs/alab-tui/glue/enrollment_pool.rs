// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Enrollment is a local native picker. Dismissing observation never cancels custody.
#![cfg(unix)]

use super::ChatWidget;
use crate::app_event::AppEvent;
use crate::bottom_pane::ListSelectionView;
use crate::bottom_pane::SelectionItem;
use crate::bottom_pane::SelectionViewParams;
use codex_alab_tui::aterm::Client;
use codex_alab_tui::aterm::Error;
#[cfg(test)]
use codex_alab_tui::aterm::enrollment;
use codex_alab_tui::aterm::enrollment::Reply;
use codex_alab_tui::enrollment_ui::Action;
#[cfg(test)]
use codex_alab_tui::enrollment_ui::Active;
use codex_alab_tui::enrollment_ui::Event;
use codex_alab_tui::enrollment_ui::View;
use codex_protocol::ThreadId;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::Weak;

const VIEW_ID: &str = "alab-enrollment";
type Row = (String, Option<String>, Option<Action>);

fn refresh() -> Row {
    (
        "Refresh enrollment status".into(),
        None,
        Some(Action::Discover { label: None }),
    )
}

fn failure(error: &Error) -> &'static str {
    match error {
        Error::Refused(line)
            if line == "ERR codex-accounts-change unsupported"
                || line == "ERR codex-accounts unsupported"
                || line.starts_with("ERR unknown") =>
        {
            "This aterm version does not support account enrollment."
        }
        Error::Refused(line)
            if line == "ERR codex-accounts-change denied"
                || line == "ERR codex-accounts denied" =>
        {
            "aterm could not authorize account enrollment for this session."
        }
        Error::Refused(_) => "aterm refused enrollment. Refresh to check the existing request.",
        Error::Malformed(_) | Error::BadRequest(_) => {
            "aterm returned incompatible enrollment metadata. Refresh to check the existing request."
        }
        _ => {
            "Enrollment status is unavailable. Progress is unknown; refresh to check the existing request."
        }
    }
}

impl ChatWidget {
    pub(crate) fn alab_accounts_command(&mut self, args: &str) {
        if !codex_alab_core::is_alab_binary() {
            return;
        }
        let args = args.trim();
        if args.is_empty() {
            self.alab_open_accounts();
            return;
        }
        let (verb, label) = args.split_once(char::is_whitespace).unwrap_or((args, ""));
        if matches!(verb, "use" | "auto") {
            self.alab_accounts_assignment(verb, label);
            return;
        }
        let view = Arc::new(Mutex::new(View { busy: true }));
        self.alab_enrollment_show(view.clone(), "Loading account enrollment", vec![]);
        let event = if verb == "add"
            && label.chars().count() <= 128
            && !label.chars().any(char::is_control)
        {
            Event::Action(Action::Discover {
                label: (!label.trim().is_empty()).then(|| label.trim().to_owned()),
            })
        } else {
            Event::Failed("Use /accounts add, /accounts use <configured label>, or /accounts auto.")
        };
        self.alab_enrollment_event(Arc::downgrade(&view), self.thread_id(), event);
    }

    fn alab_enrollment_show(&mut self, view: Arc<Mutex<View>>, subtitle: &str, mut rows: Vec<Row>) {
        rows.push(refresh());
        let thread_id = self.thread_id();
        let items = rows
            .into_iter()
            .map(|(name, description, action)| {
                let holder = view.clone();
                let is_disabled = action.is_none();
                let actions: Vec<crate::bottom_pane::SelectionAction> = action
                    .map(|action| {
                        Box::new(move |tx: &crate::app_event_sender::AppEventSender| {
                            if let Ok(mut state) = holder.lock()
                                && !state.busy
                            {
                                state.busy = true;
                                drop(state);
                                let event = match action.clone() {
                                    Action::Enroll {
                                        account_id,
                                        revision,
                                        session,
                                    } => match codex_alab_tui::enrollment_ui::admit_intent(
                                        &session, account_id, revision,
                                    ) {
                                        Ok(action) => Event::Action(action),
                                        Err(message) => Event::Failed(message),
                                    },
                                    action => Event::Action(action),
                                };
                                tx.send(AppEvent::AlabEnrollment {
                                    view: Arc::downgrade(&holder),
                                    thread_id,
                                    event,
                                });
                            }
                        }) as crate::bottom_pane::SelectionAction
                    })
                    .into_iter()
                    .collect();
                SelectionItem {
                    name,
                    description,
                    is_disabled,
                    actions,
                    dismiss_on_select: false,
                    ..Default::default()
                }
            })
            .collect();
        self.bottom_pane.dismiss_view_by_id(VIEW_ID);
        let params = SelectionViewParams {
            title: Some("Account enrollment".into()),
            subtitle: Some(subtitle.into()),
            view_id: Some(VIEW_ID),
            items,
            allow_input_when_disconnected: true,
            ..SelectionViewParams::picker()
        };
        self.show_bottom_pane_view(Box::new(ListSelectionView::new(
            params,
            self.app_event_tx.clone(),
            self.bottom_pane.list_keymap(),
        )));
    }

    pub(crate) fn alab_enrollment_event(
        &mut self,
        weak: Weak<Mutex<View>>,
        thread_id: Option<ThreadId>,
        event: Event,
    ) {
        if let Event::Action(action) = &event
            && matches!(
                action,
                Action::Start(_) | Action::Cancel(_) | Action::Poll(_)
            )
        {
            // The accepted selection owns this effect. Presentation can disappear meanwhile.
            if self.thread_id() == thread_id
                && self.bottom_pane.active_view_id() == Some(VIEW_ID)
                && let Some(view) = weak.upgrade()
            {
                self.alab_enrollment_show(
                    view,
                    "Request pending · closing this view does not cancel enrollment",
                    vec![],
                );
            }
            self.alab_enrollment_fetch(weak, thread_id, action.clone());
            return;
        }
        let Some(view) = weak.upgrade() else {
            return;
        };
        if self.thread_id() != thread_id || self.bottom_pane.active_view_id() != Some(VIEW_ID) {
            // A covered view can reconcile when revisited; it must not stay latched busy.
            if let Ok(mut state) = view.lock() {
                state.busy = false;
            }
            return;
        }
        match event {
            Event::Action(Action::Enroll {
                account_id,
                revision,
                session,
            }) => {
                let event = match codex_alab_tui::enrollment_ui::admit_intent(
                    &session, account_id, revision,
                ) {
                    Ok(action) => Event::Action(action),
                    Err(message) => Event::Failed(message),
                };
                self.alab_enrollment_event(weak, thread_id, event);
            }
            Event::Action(Action::Observe(operation)) => {
                if let Ok(mut state) = view.lock() {
                    state.busy = false;
                }
                let reply = Reply::Enrollment {
                    phase: operation.phase,
                    generation: operation.generation,
                };
                self.alab_enrollment_event(
                    weak,
                    thread_id,
                    Event::Progress {
                        active: operation.into(),
                        reply,
                    },
                );
            }
            Event::Action(action) => {
                self.alab_enrollment_show(
                    view,
                    "Request pending · closing this view does not cancel enrollment",
                    vec![],
                );
                self.alab_enrollment_fetch(weak, thread_id, action);
            }
            Event::Status { status, label } => {
                codex_alab_tui::enrollment_ui::reconcile(&status.session, &status.operations);
                if let Ok(mut state) = view.lock() {
                    state.busy = false;
                }
                if let Some(label) = label {
                    let matches: Vec<_> = status
                        .targets
                        .iter()
                        .filter(|target| target.label == label)
                        .collect();
                    if matches.len() == 1 {
                        let target = matches[0];
                        if let Some(active) =
                            codex_alab_tui::enrollment_ui::retained(&status.session)
                                .into_iter()
                                .find(|active| active.account_id == target.account_id)
                        {
                            self.alab_enrollment_event(
                                weak,
                                thread_id,
                                Event::Action(Action::Poll(active)),
                            );
                            return;
                        }
                        if let Some(existing) = status.operations.iter().rev().find(|op| {
                            op.account_id == target.account_id && op.phase.blocks_new_enrollment()
                        }) {
                            self.alab_enrollment_event(
                                weak,
                                thread_id,
                                Event::Action(Action::Observe(existing.clone())),
                            );
                            return;
                        }
                        if status.capabilities.enroll {
                            if let Ok(mut state) = view.lock() {
                                state.busy = true;
                            }
                            self.alab_enrollment_event(
                                weak,
                                thread_id,
                                Event::Action(Action::Enroll {
                                    account_id: target.account_id.clone(),
                                    revision: status.revision,
                                    session: status.session,
                                }),
                            );
                            return;
                        }
                        self.alab_enrollment_show(
                            view,
                            "Enrollment is unavailable on this aterm version",
                            vec![],
                        );
                        return;
                    }
                    self.alab_enrollment_show(view, "Choose a configured account; unknown or duplicate labels cannot authorize enrollment", vec![]);
                    return;
                }
                let retained = codex_alab_tui::enrollment_ui::retained(&status.session);
                let missing: Vec<_> = retained
                    .into_iter()
                    .filter(|active| {
                        !status
                            .operations
                            .iter()
                            .any(|op| op.operation == active.operation)
                    })
                    .map(|active| codex_alab_tui::enrollment_ui::retry_action(active, &status))
                    .collect();
                let blocking: Vec<_> = status
                    .operations
                    .iter()
                    .filter(|op| op.phase.blocks_new_enrollment())
                    .cloned()
                    .collect();
                let mut rows: Vec<Row> = status
                    .operations
                    .into_iter()
                    .map(|operation| {
                        let label = status
                            .targets
                            .iter()
                            .find(|target| target.account_id == operation.account_id)
                            .map(|target| target.label.as_str())
                            .unwrap_or("Retained account");
                        (
                            label.to_owned(),
                            Some(operation.phase.label().into()),
                            Some(Action::Observe(operation)),
                        )
                    })
                    .collect();
                for action in missing {
                    rows.push((
                        "Check pending enrollment request".into(),
                        Some("Check the existing request without creating another".into()),
                        Some(action),
                    ));
                }
                for target in status.targets {
                    let action = if let Some(operation) = blocking
                        .iter()
                        .find(|operation| operation.account_id == target.account_id)
                    {
                        Some(Action::Observe(operation.clone()))
                    } else if let Some(active) =
                        codex_alab_tui::enrollment_ui::retained(&status.session)
                            .into_iter()
                            .find(|active| active.account_id == target.account_id)
                    {
                        Some(Action::Poll(active))
                    } else {
                        status.capabilities.enroll.then(|| Action::Enroll {
                            account_id: target.account_id,
                            revision: status.revision.clone(),
                            session: status.session.clone(),
                        })
                    };
                    let description = match &action {
                        Some(Action::Observe(_) | Action::Poll(_)) => {
                            "View existing enrollment operation"
                        }
                        Some(_) => "Enroll this configured account",
                        None => "Enrollment is unavailable on this aterm version",
                    };
                    rows.push((target.label, Some(description.into()), action));
                }
                self.alab_enrollment_show(
                    view,
                    "Configured accounts · enrollment does not switch this session",
                    rows,
                );
            }
            Event::Progress { active, reply } => {
                if let Ok(mut state) = view.lock() {
                    state.busy = false;
                }
                let (description, can_cancel) = match reply {
                    Reply::Waiting => ("Request pending; enrollment has not been confirmed", true),
                    Reply::Enrollment { phase, .. } => (phase.label(), phase.can_cancel()),
                };
                let mut rows = vec![(
                    "Refresh this operation".into(),
                    None,
                    Some(Action::Poll(active.clone())),
                )];
                if can_cancel {
                    rows.push((
                        "Cancel enrollment".into(),
                        Some("Waits for confirmed cancellation".into()),
                        Some(Action::Cancel(active)),
                    ));
                }
                if rows.is_empty() {
                    rows.push((
                        "No accounts are configured".into(),
                        Some("Creating a new account is not available in this client.".into()),
                        None,
                    ));
                }
                self.alab_enrollment_show(view, description, rows);
            }
            Event::Failed(message) => {
                if let Ok(mut state) = view.lock() {
                    state.busy = false;
                }
                self.alab_enrollment_show(view, message, vec![]);
            }
        }
    }

    fn alab_enrollment_fetch(
        &self,
        view: Weak<Mutex<View>>,
        thread_id: Option<ThreadId>,
        action: Action,
    ) {
        let tx = self.app_event_tx.clone();
        tokio::spawn(async move {
            if matches!(action, Action::Discover { .. }) && view.upgrade().is_none() {
                return;
            }
            let result: Result<Event, Error> = async {
                let client = Client::connect_accounts_self()
                    .await?
                    .ok_or(Error::Closed)?;
                let birth = codex_alab_tui::identity::current()
                    .ok()
                    .and_then(|id| codex_alab_tui::identity::proc_start_utc(&id))
                    .ok_or_else(|| Error::BadRequest("process identity unavailable".into()))?;
                match action {
                    Action::Discover { label } => Ok(Event::Status {
                        status: client.enrollment_status(&birth).await?,
                        label,
                    }),
                    Action::Start(active) => {
                        let reply = client
                            .enrollment_start(
                                &birth,
                                &active.operation,
                                &active.revision,
                                &active.account_id,
                            )
                            .await?;
                        Ok(Event::Progress { active, reply })
                    }
                    Action::Poll(active) => {
                        let reply = client
                            .enrollment_poll(&birth, &active.operation, &active.account_id)
                            .await?;
                        Ok(Event::Progress { active, reply })
                    }
                    Action::Cancel(active) => {
                        let reply = client
                            .enrollment_cancel(&birth, &active.operation, &active.account_id)
                            .await?;
                        Ok(Event::Progress { active, reply })
                    }
                    Action::Observe(_) | Action::Enroll { .. } => {
                        unreachable!("intent is admitted before spawning")
                    }
                }
            }
            .await;
            tx.send(AppEvent::AlabEnrollment {
                view,
                thread_id,
                event: result.unwrap_or_else(|error| Event::Failed(failure(&error))),
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chatwidget::tests::make_chatwidget_manual_with_sender;
    use crate::chatwidget::tests::render_bottom_popup;
    use codex_alab_tui::aterm::accounts::Session;
    use enrollment::Capabilities;
    use enrollment::Operation;
    use enrollment::Phase;
    use enrollment::Status;
    use enrollment::Target;

    fn status() -> Status {
        Status {
            schema: 1,
            request: "a".repeat(32),
            revision: "b".repeat(64),
            session: Session {
                sid: "s-one".into(),
                process_pid: std::process::id(),
                process_birth: "birth".into(),
                epoch: "epoch".into(),
            },
            capabilities: Capabilities {
                enroll: true,
                create: false,
            },
            targets: vec![Target {
                account_id: "c".repeat(64),
                label: "Work".into(),
            }],
            operations: vec![Operation {
                operation: "d".repeat(32),
                account_id: "c".repeat(64),
                revision: "e".repeat(64),
                phase: Phase::AuthenticationRequired,
                generation: None,
            }],
        }
    }

    #[tokio::test]
    async fn accepted_start_and_cancel_survive_close_before_event_dispatch() {
        let (mut chat, _tx, mut rx, mut ops) = make_chatwidget_manual_with_sender().await;
        while rx.try_recv().is_ok() {}
        while ops.try_recv().is_ok() {}
        assert!(
            !codex_alab_core::is_alab_binary(),
            "fixture must not contact a live host"
        );
        let mut status = status();
        status.session.sid = "s-click-close-fixture".into();
        status.operations.clear();
        let view = Arc::new(Mutex::new(View::default()));
        let weak = Arc::downgrade(&view);
        let thread = chat.thread_id();
        chat.alab_enrollment_show(view, "Loading", vec![]);
        chat.alab_enrollment_event(
            weak,
            thread,
            Event::Status {
                status: status.clone(),
                label: None,
            },
        );
        chat.handle_key_event(crossterm::event::KeyCode::Enter.into());
        let AppEvent::AlabEnrollment {
            view,
            thread_id,
            event: Event::Action(Action::Start(active)),
        } = rx.try_recv().unwrap()
        else {
            panic!("accepted selection must record start before posting");
        };
        assert_eq!(
            codex_alab_tui::enrollment_ui::retained(&status.session)[0].operation,
            active.operation
        );
        chat.bottom_pane.dismiss_view_by_id(VIEW_ID);
        assert!(view.upgrade().is_none());
        chat.alab_enrollment_event(
            view,
            thread_id,
            Event::Action(Action::Start(active.clone())),
        );
        tokio::task::yield_now().await;
        assert!(
            matches!(
                rx.try_recv(),
                Ok(AppEvent::AlabEnrollment {
                    event: Event::Failed(_),
                    ..
                })
            ),
            "accepted effect must run even though its display closed"
        );
        assert_ne!(chat.bottom_pane.active_view_id(), Some(VIEW_ID));

        let pane = Arc::new(Mutex::new(View::default()));
        let weak = Arc::downgrade(&pane);
        chat.alab_enrollment_show(pane, "Loading", vec![]);
        chat.alab_enrollment_event(
            weak,
            thread,
            Event::Progress {
                active,
                reply: Reply::Enrollment {
                    phase: Phase::Running,
                    generation: None,
                },
            },
        );
        chat.handle_key_event(crossterm::event::KeyCode::Down.into());
        chat.handle_key_event(crossterm::event::KeyCode::Enter.into());
        let AppEvent::AlabEnrollment {
            view,
            thread_id,
            event: Event::Action(Action::Cancel(active)),
        } = rx.try_recv().unwrap()
        else {
            panic!("cancel selection");
        };
        chat.bottom_pane.dismiss_view_by_id(VIEW_ID);
        chat.alab_enrollment_event(view, thread_id, Event::Action(Action::Cancel(active)));
        tokio::task::yield_now().await;
        assert!(matches!(
            rx.try_recv(),
            Ok(AppEvent::AlabEnrollment {
                event: Event::Failed(_),
                ..
            })
        ));
        assert_ne!(chat.bottom_pane.active_view_id(), Some(VIEW_ID));
        assert!(ops.try_recv().is_err());
    }

    #[tokio::test]
    async fn lost_reply_and_reopen_reconcile_the_original_admission_id() {
        let (mut chat, _tx, mut rx, mut ops) = make_chatwidget_manual_with_sender().await;
        while rx.try_recv().is_ok() {}
        while ops.try_recv().is_ok() {}
        let mut status = status();
        status.session.sid = "s-view-intent-fixture".into();
        status.operations.clear();
        let Action::Start(original) = codex_alab_tui::enrollment_ui::admit_intent(
            &status.session,
            "c".repeat(64),
            status.revision.clone(),
        )
        .unwrap() else {
            panic!("new intent");
        };
        let view = Arc::new(Mutex::new(View::default()));
        let weak = Arc::downgrade(&view);
        let thread = chat.thread_id();
        chat.alab_enrollment_show(view, "Request pending", vec![]);
        chat.alab_enrollment_event(
            weak.clone(),
            thread,
            Event::Failed(failure(&Error::Unanswered)),
        );
        chat.bottom_pane.dismiss_view_by_id(VIEW_ID);
        assert!(weak.upgrade().is_none());

        let reopened = Arc::new(Mutex::new(View::default()));
        let weak = Arc::downgrade(&reopened);
        chat.alab_enrollment_show(reopened, "Loading", vec![]);
        chat.alab_enrollment_event(
            weak,
            thread,
            Event::Status {
                status,
                label: None,
            },
        );
        assert!(render_bottom_popup(&chat, 100).contains("Check pending enrollment request"));
        assert!(
            rx.try_recv().is_err(),
            "reopening cannot start or retry an operation"
        );
        chat.handle_key_event(crossterm::event::KeyCode::Enter.into());
        chat.handle_key_event(crossterm::event::KeyCode::Enter.into());
        let AppEvent::AlabEnrollment {
            event: Event::Action(Action::Start(retry)),
            ..
        } = rx.try_recv().unwrap()
        else {
            panic!("explicit reconciliation must reuse admission");
        };
        assert_eq!(retry.operation, original.operation);
        assert_eq!(retry.revision, original.revision);
        assert!(
            rx.try_recv().is_err(),
            "pending action must serialize repeated input"
        );
        assert!(ops.try_recv().is_err());
    }

    #[tokio::test]
    async fn reopening_discovers_existing_operation_without_starting_another() {
        let (mut chat, _tx, mut rx, mut ops) = make_chatwidget_manual_with_sender().await;
        while rx.try_recv().is_ok() {}
        while ops.try_recv().is_ok() {}
        chat.apply_external_edit("original work".into());
        let view = Arc::new(Mutex::new(View::default()));
        let weak = Arc::downgrade(&view);
        chat.alab_enrollment_show(view, "Loading", vec![]);
        let thread = chat.thread_id();
        chat.alab_enrollment_event(
            weak.clone(),
            thread,
            Event::Status {
                status: status(),
                label: None,
            },
        );
        assert!(render_bottom_popup(&chat, 80).contains("Work"));
        assert!(
            rx.try_recv().is_err(),
            "opening discovery must not start enrollment"
        );
        chat.handle_key_event(crossterm::event::KeyCode::Down.into());
        chat.handle_key_event(crossterm::event::KeyCode::Enter.into());
        let AppEvent::AlabEnrollment {
            event: Event::Action(Action::Observe(operation)),
            ..
        } = rx.try_recv().unwrap()
        else {
            panic!("existing operation must be observed rather than started");
        };
        assert_eq!(operation.operation, "d".repeat(32));
        assert_eq!(chat.bottom_pane.active_view_id(), Some(VIEW_ID));
        chat.alab_enrollment_event(
            weak.clone(),
            thread,
            Event::Action(Action::Observe(operation)),
        );
        assert!(render_bottom_popup(&chat, 100).contains("Account authentication required"));
        assert!(!render_bottom_popup(&chat, 100).contains("Cancel enrollment"));
        chat.bottom_pane.dismiss_view_by_id(VIEW_ID);
        assert!(weak.upgrade().is_none());
        assert_eq!(chat.composer_text_with_pending(), "original work");
        assert!(ops.try_recv().is_err());
    }

    #[tokio::test]
    async fn stale_enrollment_actions_cannot_replace_new_views_or_cross_threads() {
        let (mut chat, _tx, mut rx, mut ops) = make_chatwidget_manual_with_sender().await;
        while rx.try_recv().is_ok() {}
        while ops.try_recv().is_ok() {}
        let view = Arc::new(Mutex::new(View::default()));
        let weak = Arc::downgrade(&view);
        chat.alab_enrollment_show(view, "original", vec![]);
        let thread = chat.thread_id();
        chat.thread_id = Some(ThreadId::new());
        chat.alab_enrollment_event(weak.clone(), thread, Event::Failed("wrong thread"));
        assert!(!render_bottom_popup(&chat, 80).contains("wrong thread"));
        chat.thread_id = thread;
        chat.bottom_pane.dismiss_view_by_id(VIEW_ID);
        chat.show_bottom_pane_view(Box::new(super::super::DetailsView::new(
            "replacement",
            "Other view",
            || ratatui::widgets::Paragraph::new("replacement"),
            "close",
            None,
            chat.bottom_pane.list_keymap(),
        )));
        chat.alab_enrollment_event(
            weak,
            thread,
            Event::Action(Action::Discover { label: None }),
        );
        tokio::task::yield_now().await;
        assert_eq!(chat.bottom_pane.active_view_id(), Some("replacement"));
        assert!(rx.try_recv().is_err());
        assert!(ops.try_recv().is_err());
    }

    #[tokio::test]
    async fn cancellation_and_transport_wait_do_not_claim_terminal_completion() {
        let (mut chat, _tx, _rx, _ops) = make_chatwidget_manual_with_sender().await;
        let view = Arc::new(Mutex::new(View::default()));
        let weak = Arc::downgrade(&view);
        chat.alab_enrollment_show(view, "Loading", vec![]);
        let thread = chat.thread_id();
        let active: Active = status().operations.remove(0).into();
        chat.alab_enrollment_event(
            weak.clone(),
            thread,
            Event::Progress {
                active: active.clone(),
                reply: Reply::Waiting,
            },
        );
        assert!(render_bottom_popup(&chat, 100).contains("has not been confirmed"));
        chat.alab_enrollment_event(
            weak,
            thread,
            Event::Progress {
                active,
                reply: Reply::Enrollment {
                    phase: Phase::CancelRequested,
                    generation: None,
                },
            },
        );
        let text = render_bottom_popup(&chat, 100);
        assert!(text.contains("Cancellation requested"));
        assert!(!text.contains("Enrollment cancelled"));
        let error = Error::Refused("ERR private token path mailbox".into());
        assert!(!failure(&error).contains("private"));
    }
}
