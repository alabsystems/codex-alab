// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Enrollment is a local native picker. Dismissing observation never cancels custody.
#![cfg(unix)]

use super::ChatWidget;
use crate::app_event::AppEvent;
use crate::bottom_pane::ListSelectionView;
use crate::bottom_pane::SelectionDescriptionLayout;
use crate::bottom_pane::SelectionItem;
use crate::bottom_pane::SelectionViewParams;
use codex_alab_tui::aterm::Client;
use codex_alab_tui::aterm::Error;
use codex_alab_tui::aterm::accounts::Account;
use codex_alab_tui::aterm::accounts::Condition;
use codex_alab_tui::aterm::accounts::Snapshot;
use codex_alab_tui::aterm::enrollment;
use codex_alab_tui::aterm::enrollment::Reply;
use codex_alab_tui::enrollment_ui::Action;
use codex_alab_tui::enrollment_ui::Active;
use codex_alab_tui::enrollment_ui::Conflicts;
use codex_alab_tui::enrollment_ui::Event;
use codex_alab_tui::enrollment_ui::View;
use codex_protocol::ThreadId;
use itertools::Itertools;
use std::collections::HashSet;
use std::future::Future;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::Weak;
use std::time::Duration;

const VIEW_ID: &str = "alab-enrollment";
const CONFLICT: &str = "Enrollment records disagree. Check the original pending request.";
type Row = (String, Option<String>, Option<Action>);

fn named_target<'a>(status: &'a enrollment::Status, label: &str) -> Option<&'a enrollment::Target> {
    status
        .targets
        .iter()
        .filter(|target| target.label == label)
        .exactly_one()
        .ok()
}

/// Display reads are optional. Only an enrollment target can authorize a start or retry.
async fn picker_accounts(
    status: &enrollment::Status,
    label: Option<&str>,
    snapshot: impl Future<Output = Result<Snapshot, Error>>,
) -> Vec<Account> {
    if status.targets.is_empty() || label.is_some_and(|label| named_target(status, label).is_some())
    {
        return Vec::new();
    }
    match tokio::time::timeout(Duration::from_secs(2), snapshot).await {
        Ok(Ok(snapshot))
            if snapshot.session == status.session && snapshot.revision == status.revision =>
        {
            snapshot.accounts
        }
        _ => Vec::new(),
    }
}

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
        let view = Arc::new(Mutex::new(View::loading()));
        self.alab_enrollment_show(view.clone(), None, "Loading account enrollment", vec![]);
        let event = if verb == "add"
            && label.chars().count() <= 128
            && !label.chars().any(char::is_control)
        {
            Event::Action(Action::Discover {
                label: (!label.trim().is_empty()).then(|| label.trim().to_owned()),
            })
        } else {
            Event::Failed(
                "Use /accounts add, /accounts use <configured label>, or /accounts auto.",
                None,
            )
        };
        self.alab_enrollment_event(Arc::downgrade(&view), self.thread_id(), event);
    }

    fn alab_enrollment_show(
        &mut self,
        view: Arc<Mutex<View>>,
        label: Option<&str>,
        subtitle: &str,
        mut rows: Vec<Row>,
    ) {
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
                                && state.conflicts.permits(&action)
                            {
                                state.busy = true;
                                drop(state);
                                let event = match action.clone() {
                                    Action::Enroll {
                                        target,
                                        revision,
                                        session,
                                    } => match codex_alab_tui::enrollment_ui::admit_intent(
                                        &session, target, revision,
                                    ) {
                                        Ok(action) => Event::Action(action),
                                        Err(message) => Event::Failed(message, None),
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
            title: Some(label.map_or_else(
                || "Account enrollment".to_owned(),
                |label| format!("Account enrollment · {label}"),
            )),
            subtitle: Some(subtitle.into()),
            view_id: Some(VIEW_ID),
            items,
            description_layout: SelectionDescriptionLayout::SelectedBelowWhenNarrow {
                min_description_width: 24,
            },
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
            && let Some(view) = weak.upgrade()
            && !view
                .lock()
                .is_ok_and(|state| state.conflicts.permits(action))
        {
            return;
        }
        if let Event::Action(action) = &event
            && let Some(active) = action.request_operation()
        {
            // A queued operation from another selection cannot take over the current view.
            if let Some(view) = weak.upgrade()
                && !view.lock().is_ok_and(|mut state| state.select(active))
            {
                return;
            }
            // The accepted selection owns this effect. Presentation can disappear meanwhile.
            if self.thread_id() == thread_id
                && self.bottom_pane.active_view_id() == Some(VIEW_ID)
                && let Some(view) = weak.upgrade()
            {
                let conflicted = view
                    .lock()
                    .is_ok_and(|state| state.conflicts.original(&active.operation).is_some());
                self.alab_enrollment_show(
                    view,
                    active.label.as_deref().filter(|_| !conflicted),
                    if conflicted {
                        CONFLICT
                    } else {
                        "Request pending · closing this view does not cancel enrollment"
                    },
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
            Event::Action(Action::OpenAccounts) => {
                self.bottom_pane.dismiss_view_by_id(VIEW_ID);
                self.alab_open_accounts();
            }
            Event::Action(Action::Enroll {
                target,
                revision,
                session,
            }) => {
                let event =
                    match codex_alab_tui::enrollment_ui::admit_intent(&session, target, revision) {
                        Ok(action) => Event::Action(action),
                        Err(message) => Event::Failed(message, None),
                    };
                self.alab_enrollment_event(weak, thread_id, event);
            }
            Event::Action(Action::Observe(operation, label)) => {
                let active = Active {
                    label,
                    ..operation.clone().into()
                };
                {
                    let Ok(mut state) = view.lock() else { return };
                    if !state.select(&active) {
                        return;
                    }
                    state.busy = false;
                }
                let reply = Reply::Enrollment {
                    phase: operation.phase,
                    generation: operation.generation,
                };
                self.alab_enrollment_event(weak, thread_id, Event::Progress { active, reply });
            }
            Event::Action(action) => {
                if let Ok(mut state) = view.lock() {
                    state.rediscover();
                }
                self.alab_enrollment_show(
                    view,
                    None,
                    "Request pending · closing this view does not cancel enrollment",
                    vec![],
                );
                self.alab_enrollment_fetch(weak, thread_id, action);
            }
            Event::Status {
                status,
                label,
                display_accounts,
            } => {
                let retained = codex_alab_tui::enrollment_ui::retained(&status.session);
                let conflicts = Conflicts::new(&status, &retained);
                {
                    let Ok(mut state) = view.lock() else { return };
                    if !state.expects(None) {
                        return;
                    }
                    state.busy = false;
                    state.conflicts = conflicts.clone();
                }
                codex_alab_tui::enrollment_ui::reconcile(&status.session, &status.operations);
                let named = label
                    .as_deref()
                    .and_then(|label| named_target(&status, label));
                let named_conflict =
                    named.is_some_and(|target| conflicts.affects(&target.account_id));
                if let Some(target) = named.filter(|_| !named_conflict) {
                    if let Some(action) = conflicts.target_action(target, &status, &retained) {
                        if let Ok(mut state) = view.lock() {
                            state.busy = true;
                        }
                        self.alab_enrollment_event(weak, thread_id, Event::Action(action));
                        return;
                    }
                    self.alab_enrollment_show(
                        view,
                        None,
                        "Enrollment is unavailable on this aterm version",
                        vec![],
                    );
                    return;
                }
                let missing: Vec<_> = retained
                    .iter()
                    .filter(|active| {
                        !status.operations.iter().any(|op| {
                            op.operation == active.operation
                                && op.account_id == active.account_id
                                && op.revision == active.revision
                        })
                    })
                    .map(|active| {
                        codex_alab_tui::enrollment_ui::retry_action(active.clone(), &status)
                    })
                    .collect();
                let mut rows: Vec<Row> = status
                    .operations
                    .iter()
                    .filter(|operation| conflicts.original(&operation.operation).is_none())
                    .map(|operation| {
                        let label = codex_alab_tui::enrollment_ui::operation_label(
                            operation, &status, &retained,
                        );
                        (
                            label.unwrap_or("Retained account").to_owned(),
                            Some(operation.phase.label().into()),
                            Some(Action::Observe(operation.clone(), label.map(str::to_owned))),
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
                let mut masks = HashSet::new();
                let mut shared_mask = false;
                for target in status
                    .targets
                    .iter()
                    .filter(|target| !conflicts.affects(&target.account_id))
                {
                    let action = conflicts.target_action(target, &status, &retained);
                    let description = match &action {
                        Some(Action::Observe(_, _) | Action::Poll(_) | Action::Start(_)) => {
                            "View existing enrollment operation"
                        }
                        Some(_) => "Enroll this configured account",
                        None => "Enrollment is unavailable on this aterm version",
                    };
                    let row = display_accounts
                        .iter()
                        .find(|row| row.id == target.account_id && row.label == target.label);
                    let name = match row.and_then(|row| row.masked_email.as_ref()) {
                        Some(email) => {
                            shared_mask |= !masks.insert(email);
                            format!("{} · {email}", target.label)
                        }
                        None => target.label.clone(),
                    };
                    let description = match row {
                        Some(row) if row.condition == Condition::SignedOut => {
                            format!("Signed out · {description}")
                        }
                        Some(row) if !row.enrolled => format!("No login file · {description}"),
                        Some(_) => format!("Login file present · {description}"),
                        None => description.into(),
                    };
                    rows.push((name, Some(description), action));
                }
                let subtitle = if rows.is_empty() {
                    rows.extend([
                        (
                            "aterm harness accounts sync alab --cli codex".into(),
                            None,
                            None,
                        ),
                        (
                            "aterm harness accounts add oai <email> --cli codex".into(),
                            None,
                            None,
                        ),
                    ]);
                    "No enrollment targets. In a shell, sync the Registry or add an account."
                } else if named_conflict {
                    CONFLICT
                } else if label.is_some() && !status.targets.is_empty() {
                    "No unique match for that label. Choose a configured account."
                } else {
                    "Configured accounts · enrollment does not switch this session."
                };
                let mut subtitle = subtitle.to_owned();
                if shared_mask {
                    subtitle.push_str(
                        " Some targets have the same masked email. To match sign-in addresses to slot labels, \
                         run bash alab/account-labels.sh \
                         from a codex-alab checkout.",
                    );
                }
                self.alab_enrollment_show(view, None, &subtitle, rows);
            }
            Event::Progress { active, reply } => {
                let conflicted = {
                    let Ok(mut state) = view.lock() else { return };
                    if !state.expects(Some(&active)) {
                        return;
                    }
                    state.busy = false;
                    state.conflicts.original(&active.operation).is_some()
                };
                if conflicted {
                    self.alab_enrollment_show(
                        view,
                        None,
                        CONFLICT,
                        vec![(
                            "Refresh this operation".into(),
                            None,
                            Some(Action::Poll(active)),
                        )],
                    );
                    return;
                }
                if matches!(
                    reply,
                    Reply::Enrollment {
                        phase: enrollment::Phase::Refused,
                        ..
                    }
                ) {
                    self.alab_enrollment_show(
                        view,
                        active.label.as_deref(),
                        "Enrollment refused. In a terminal on this Mac, run aterm harness accounts list --cli codex. \
                         Check the slot's sign-in needed line, if present.",
                        vec![("Open /accounts".into(), None, Some(Action::OpenAccounts))],
                    );
                    return;
                }
                let (description, can_cancel) = match reply {
                    Reply::Waiting => ("Request pending; enrollment has not been confirmed", true),
                    Reply::Enrollment { phase, .. } => (phase.label(), phase.can_cancel()),
                };
                let label = active.label.clone();
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
                self.alab_enrollment_show(view, label.as_deref(), description, rows);
            }
            Event::Failed(message, active) => {
                let label = {
                    let Ok(mut state) = view.lock() else { return };
                    if !state.expects(active.as_ref()) {
                        return;
                    }
                    state.busy = false;
                    state
                        .selected()
                        .filter(|active| state.conflicts.original(&active.operation).is_none())
                        .and_then(|active| active.label.clone())
                };
                self.alab_enrollment_show(view, label.as_deref(), message, vec![]);
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
            let operation = action.request_operation().cloned();
            let result: Result<Event, Error> = async {
                let client = Client::connect_accounts_self()
                    .await?
                    .ok_or(Error::Closed)?;
                let birth = codex_alab_tui::identity::current()
                    .ok()
                    .and_then(|id| codex_alab_tui::identity::proc_start_utc(&id))
                    .ok_or_else(|| Error::BadRequest("process identity unavailable".into()))?;
                match action {
                    Action::Discover { label } => {
                        let status = client.enrollment_status(&birth).await?;
                        let display_accounts = picker_accounts(
                            &status,
                            label.as_deref(),
                            client.accounts_snapshot_for_display(&birth),
                        )
                        .await;
                        Ok(Event::Status {
                            status,
                            label,
                            display_accounts,
                        })
                    }
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
                    Action::OpenAccounts | Action::Observe(_, _) | Action::Enroll { .. } => {
                        unreachable!("the enrollment view handles this action locally")
                    }
                }
            }
            .await;
            tx.send(AppEvent::AlabEnrollment {
                view,
                thread_id,
                event: result.unwrap_or_else(|error| Event::Failed(failure(&error), operation)),
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

    fn display_snapshot(status: &Status) -> Snapshot {
        use codex_alab_tui::aterm::accounts::Assignment;
        use codex_alab_tui::aterm::accounts::Capabilities;
        use codex_alab_tui::aterm::accounts::Quota;
        let accounts = status
            .targets
            .iter()
            .map(|target| Account {
                id: target.account_id.clone(),
                label: target.label.clone(),
                masked_email: Some("w***@example.test".into()),
                condition: Condition::SignedOut,
                signed_out_reason: None,
                selection_ref: None,
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
            })
            .collect();
        Snapshot {
            schema: 1,
            observed_at: 1,
            session: status.session.clone(),
            revision: status.revision.clone(),
            request: status.request.clone(),
            assignment: Assignment { account_id: None },
            pool: None,
            capabilities: Capabilities {
                select_bound: false,
                add: false,
                select: false,
                auto: false,
            },
            accounts,
        }
    }

    #[tokio::test]
    async fn named_enrollment_skips_display_and_only_current_snapshots_describe_choices() {
        let status = status();
        let unused = async { panic!("a named enrollment must not wait on the display read") };
        assert!(
            picker_accounts(&status, Some("Work"), unused)
                .await
                .is_empty()
        );
        assert!(
            picker_accounts(&status, None, async { Err(Error::Closed) })
                .await
                .is_empty()
        );
        let snapshot = display_snapshot(&status);
        let mut changed = snapshot.clone();
        changed.revision.push('a');
        assert!(
            picker_accounts(&status, None, async { Ok(changed) })
                .await
                .is_empty()
        );
        let mut restarted = snapshot.clone();
        restarted.session.epoch.push('x');
        assert!(
            picker_accounts(&status, None, async { Ok(restarted) })
                .await
                .is_empty()
        );
        let rows = picker_accounts(&status, Some("unknown"), async { Ok(snapshot) }).await;
        assert_eq!(rows[0].label, "Work");
    }

    #[tokio::test]
    async fn enrollment_choices_identify_shared_masks_and_only_an_explicit_pick_starts() {
        for (requested, first_label, shared_mask) in [
            (None, "oai-other", true),
            (Some("unknown"), "oai-other", true),
            (Some("oai-slot"), "oai-slot", false),
        ] {
            let (mut chat, _tx, mut rx, mut ops) = make_chatwidget_manual_with_sender().await;
            while rx.try_recv().is_ok() {}
            while ops.try_recv().is_ok() {}
            let mut status = status();
            status.session.sid = format!("s-picker-{}", requested.unwrap_or("all"));
            status.operations.clear();
            status.targets[0].label = first_label.into();
            let selected_id = "f".repeat(64);
            status.targets.push(Target {
                account_id: selected_id.clone(),
                label: "oai-slot".into(),
            });
            let mut snapshot = display_snapshot(&status);
            if !shared_mask {
                snapshot.accounts[0].masked_email = Some("a***@example.test".into());
            }
            snapshot.accounts[0].condition = Condition::NotEnrolled;
            snapshot.accounts[0].enrolled = false;
            // A display row matching only a label or only an ID must never describe this target.
            let mut wrong_id = snapshot.accounts[0].clone();
            wrong_id.id = "e".repeat(64);
            wrong_id.masked_email = Some("hidden***@example.test".into());
            let mut wrong_label = wrong_id.clone();
            wrong_label.id = selected_id.clone();
            wrong_label.label = "unrelated".into();
            snapshot.accounts.splice(0..0, [wrong_id, wrong_label]);
            let display_accounts =
                picker_accounts(&status, requested, async { Ok(snapshot) }).await;
            let revision = status.revision.clone();
            let view = Arc::new(Mutex::new(View::default()));
            let weak = Arc::downgrade(&view);
            chat.alab_enrollment_show(view, None, "Loading", vec![]);
            chat.alab_enrollment_event(
                weak.clone(),
                chat.thread_id(),
                Event::Status {
                    status,
                    label: requested.map(str::to_owned),
                    display_accounts,
                },
            );
            let popup = render_bottom_popup(&chat, 145);
            assert_eq!(
                popup.contains("No unique match"),
                requested.is_some(),
                "{popup}"
            );
            assert_eq!(popup.contains("a***@example.test"), !shared_mask, "{popup}");
            for clue in ["w***@example.test", "Signed out", "No login file"] {
                assert!(popup.contains(clue), "{popup}");
            }
            assert!(!popup.contains("hidden***"), "{popup}");
            for width in [52, 120] {
                let popup = render_bottom_popup(&chat, width)
                    .split_whitespace()
                    .join(" ");
                assert_eq!(
                    popup.contains("bash alab/account-labels.sh"),
                    shared_mask,
                    "{popup}"
                );
            }
            assert!(rx.try_recv().is_err());
            chat.handle_key_event(crossterm::event::KeyCode::Down.into());
            chat.handle_key_event(crossterm::event::KeyCode::Enter.into());
            let AppEvent::AlabEnrollment {
                event: Event::Action(Action::Start(start)),
                ..
            } = rx.try_recv().unwrap()
            else {
                panic!("explicit enrollment selection");
            };
            assert_eq!(start.account_id, selected_id);
            assert_eq!(start.revision, revision);
            assert!(rx.try_recv().is_err());
            assert!(ops.try_recv().is_err());
            chat.alab_enrollment_event(
                weak.clone(),
                chat.thread_id(),
                Event::Action(Action::Start(start.clone())),
            );
            let title = "Account enrollment · oai-slot";
            let popup = render_bottom_popup(&chat, 120);
            assert!(popup.contains(title), "{popup}");
            assert!(
                popup.contains("Request pending · closing this view"),
                "{popup}"
            );
            let mut stale = start.clone();
            stale.operation = "older-operation".into();
            chat.alab_enrollment_event(
                weak.clone(),
                chat.thread_id(),
                Event::Failed("older operation failed", Some(stale.clone())),
            );
            chat.alab_enrollment_event(
                weak.clone(),
                chat.thread_id(),
                Event::Progress {
                    active: stale,
                    reply: Reply::Waiting,
                },
            );
            assert_eq!(render_bottom_popup(&chat, 120), popup);
            chat.alab_enrollment_event(
                weak.clone(),
                chat.thread_id(),
                Event::Failed("Current request unavailable", Some(start.clone())),
            );
            let error = render_bottom_popup(&chat, 120);
            assert!(
                error.contains(title) && error.contains("Current request unavailable"),
                "{error}"
            );
            for (reply, subtitle) in [
                (
                    Reply::Waiting,
                    "Request pending; enrollment has not been confirmed",
                ),
                (
                    Reply::Enrollment {
                        phase: Phase::Running,
                        generation: None,
                    },
                    "Enrollment running",
                ),
                (
                    Reply::Enrollment {
                        phase: Phase::AuthenticationRequired,
                        generation: None,
                    },
                    "Complete sign-in in aterm",
                ),
                (
                    Reply::Enrollment {
                        phase: Phase::Applied,
                        generation: Some(1),
                    },
                    "Enrollment complete",
                ),
            ] {
                chat.alab_enrollment_event(
                    weak.clone(),
                    chat.thread_id(),
                    Event::Progress {
                        active: start.clone(),
                        reply,
                    },
                );
                let popup = render_bottom_popup(&chat, 120);
                assert!(popup.contains(title), "{popup}");
                assert!(popup.contains(subtitle), "{popup}");
            }
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
        chat.alab_enrollment_show(view, None, "Loading", vec![]);
        chat.alab_enrollment_event(
            weak,
            thread,
            Event::Status {
                status: status.clone(),
                label: None,
                display_accounts: vec![],
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
                    event: Event::Failed(_, _),
                    ..
                })
            ),
            "accepted effect must run even though its display closed"
        );
        assert_ne!(chat.bottom_pane.active_view_id(), Some(VIEW_ID));

        let pane = Arc::new(Mutex::new(View::default()));
        let weak = Arc::downgrade(&pane);
        assert!(pane.lock().unwrap().select(&active));
        chat.alab_enrollment_show(pane, None, "Loading", vec![]);
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
                event: Event::Failed(_, _),
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
            status.targets[0].clone(),
            status.revision.clone(),
        )
        .unwrap() else {
            panic!("new intent");
        };
        let view = Arc::new(Mutex::new(View::default()));
        let weak = Arc::downgrade(&view);
        let thread = chat.thread_id();
        chat.alab_enrollment_show(view, None, "Request pending", vec![]);
        chat.alab_enrollment_event(
            weak.clone(),
            thread,
            Event::Failed(failure(&Error::Unanswered), None),
        );
        chat.bottom_pane.dismiss_view_by_id(VIEW_ID);
        assert!(weak.upgrade().is_none());

        let reopened = Arc::new(Mutex::new(View::default()));
        let weak = Arc::downgrade(&reopened);
        chat.alab_enrollment_show(reopened, None, "Loading", vec![]);
        chat.alab_enrollment_event(
            weak,
            thread,
            Event::Status {
                status,
                label: None,
                display_accounts: vec![],
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
    async fn a_reopened_nonce_with_two_bindings_allows_only_the_original_poll() {
        for (named, phase) in [
            (None, Phase::Running),
            (Some("moon-reported"), Phase::Running),
            (Some("moon-reported"), Phase::Refused),
        ] {
            let (mut chat, _tx, mut rx, _ops) = make_chatwidget_manual_with_sender().await;
            while rx.try_recv().is_ok() {}
            let mut status = status();
            status.session.sid = format!("s-reopen-contradiction-{}-{phase:?}", named.is_some());
            status.targets[0].label = "moon-retained".into();
            let source = status.targets[0].account_id.clone();
            let other = "f".repeat(64);
            status.targets.push(Target {
                account_id: other.clone(),
                label: "moon-reported".into(),
            });
            let independent = "9".repeat(64);
            status.targets.push(Target {
                account_id: independent.clone(),
                label: "moon-independent".into(),
            });
            let Action::Start(original) = codex_alab_tui::enrollment_ui::admit_intent(
                &status.session,
                status.targets[0].clone(),
                status.revision.clone(),
            )
            .unwrap() else {
                panic!("expected original admission");
            };
            let record = &mut status.operations[0];
            record.operation.clone_from(&original.operation);
            record.revision.clone_from(&original.revision);
            record.account_id.clone_from(&other);
            record.phase = phase;
            let reported = Active::from(record.clone());
            let retained = std::slice::from_ref(&original);
            let conflicts = Conflicts::new(&status, retained);
            assert!(conflicts.permits(&Action::Poll(original.clone())));
            for action in [
                Action::Start(original.clone()),
                Action::Cancel(original.clone()),
                Action::Start(reported.clone()),
                Action::Cancel(reported.clone()),
                Action::Poll(reported.clone()),
                Action::Observe(status.operations[0].clone(), None),
            ] {
                assert!(!conflicts.permits(&action));
            }
            for target in &status.targets[..2] {
                assert!(conflicts.target_action(target, &status, retained).is_none());
            }
            assert!(matches!(
                conflicts.target_action(&status.targets[2], &status, retained),
                Some(Action::Enroll { .. })
            ));
            let view = Arc::new(Mutex::new(View::default()));
            let weak = Arc::downgrade(&view);
            let thread = chat.thread_id();
            chat.alab_enrollment_show(view, None, "Loading", vec![]);
            chat.alab_enrollment_event(
                weak.clone(),
                thread,
                Event::Status {
                    status: status.clone(),
                    label: named.map(str::to_owned),
                    display_accounts: vec![],
                },
            );
            let pane = render_bottom_popup(&chat, 120);
            assert!(
                pane.contains("Check pending enrollment request")
                    && pane.contains("moon-independent"),
                "{pane}"
            );
            for unexpected in [
                "Check pending enrollment request ·",
                "moon-retained",
                "moon-reported",
                phase.label(),
            ] {
                assert!(!pane.contains(unexpected), "{pane}");
            }
            assert!(rx.try_recv().is_err());
            chat.handle_key_event(crossterm::event::KeyCode::Enter.into());
            let AppEvent::AlabEnrollment {
                event: Event::Action(Action::Poll(poll)),
                ..
            } = rx.try_recv().unwrap()
            else {
                panic!("only the explicit read-only poll is selectable for this admission");
            };
            assert!(poll.same_binding(&original));
            chat.alab_enrollment_event(weak.clone(), thread, Event::Action(Action::Poll(poll)));
            for reply in [
                Reply::Waiting,
                Reply::Enrollment {
                    phase: Phase::Running,
                    generation: None,
                },
            ] {
                chat.alab_enrollment_event(
                    weak.clone(),
                    thread,
                    Event::Progress {
                        active: original.clone(),
                        reply,
                    },
                );
                let pane = render_bottom_popup(&chat, 120);
                assert!(
                    pane.contains(CONFLICT) && pane.contains("Refresh this operation"),
                    "{pane}"
                );
                for unexpected in [
                    "Account enrollment ·",
                    "Cancel enrollment",
                    "Enrollment running",
                ] {
                    assert!(!pane.contains(unexpected), "{pane}");
                }
            }
            // Even a delayed native observation for the other tuple cannot replace this check.
            chat.alab_enrollment_event(
                weak.clone(),
                thread,
                Event::Action(Action::Observe(status.operations[0].clone(), None)),
            );
            chat.alab_enrollment_event(
                weak.clone(),
                thread,
                Event::Failed("wrong binding", Some(reported)),
            );
            let pane = render_bottom_popup(&chat, 120);
            assert!(
                pane.contains(CONFLICT) && !pane.contains("wrong binding"),
                "{pane}"
            );
            let still_retained = codex_alab_tui::enrollment_ui::retained(&status.session);
            assert_eq!(still_retained.len(), 1);
            assert!(still_retained[0].same_binding(&original));
            chat.alab_enrollment_event(
                weak.clone(),
                thread,
                Event::Action(Action::Discover { label: None }),
            );
            status.operations[0].account_id = source;
            status.operations[0].phase = Phase::Running;
            chat.alab_enrollment_event(
                weak,
                thread,
                Event::Status {
                    status,
                    label: Some("moon-retained".into()),
                    display_accounts: vec![],
                },
            );
            let pane = render_bottom_popup(&chat, 120);
            assert!(
                pane.contains("Account enrollment · moon-retained")
                    && pane.contains("Cancel enrollment")
                    && !pane.contains(CONFLICT),
                "{pane}"
            );
        }
    }

    #[tokio::test]
    async fn named_retry_observes_a_finished_intent_before_another_enrollment() {
        let (mut chat, _tx, mut rx, _ops) = make_chatwidget_manual_with_sender().await;
        while rx.try_recv().is_ok() {}
        let mut status = status();
        status.session.sid = "s-named-terminal".into();
        let record = &mut status.operations[0];
        let Action::Start(active) = codex_alab_tui::enrollment_ui::admit_intent(
            &status.session,
            Target {
                account_id: record.account_id.clone(),
                label: "Originally selected".into(),
            },
            record.revision.clone(),
        )
        .unwrap() else {
            panic!("expected first intent");
        };
        record.operation = active.operation;
        record.phase = Phase::Applied;
        record.generation = Some(1);
        let view = Arc::new(Mutex::new(View::default()));
        let weak = Arc::downgrade(&view);
        let thread = chat.thread_id();
        chat.alab_enrollment_show(view, None, "Loading", vec![]);
        chat.alab_enrollment_event(
            weak,
            thread,
            Event::Status {
                status,
                label: Some("Work".into()),
                display_accounts: vec![],
            },
        );
        let text = render_bottom_popup(&chat, 110);
        assert!(text.contains("Enrollment complete"));
        assert!(text.contains("Account enrollment · Originally selected"));
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn empty_enrollment_picker_shows_registry_sync_and_individual_setup() {
        let (mut chat, _tx, _rx, _ops) = make_chatwidget_manual_with_sender().await;
        let mut status = status();
        status.session.sid = "s-empty-roster".into();
        status.targets.clear();
        status.operations.clear();
        let view = Arc::new(Mutex::new(View::default()));
        let weak = Arc::downgrade(&view);
        let thread = chat.thread_id();
        chat.alab_enrollment_show(view, None, "Loading", vec![]);
        chat.alab_enrollment_event(
            weak,
            thread,
            Event::Status {
                status,
                label: None,
                display_accounts: vec![],
            },
        );
        for width in [80, 120] {
            let text = render_bottom_popup(&chat, width);
            assert!(text.contains("No enrollment targets."), "{text}");
            let sync = text.find("aterm harness accounts sync alab --cli codex");
            let add = text.find("aterm harness accounts add oai <email> --cli codex");
            assert!(
                matches!((sync, add), (Some(sync), Some(add)) if sync < add),
                "{text}"
            );
            assert!(text.contains("Refresh enrollment status"), "{text}");
        }
    }

    #[tokio::test]
    async fn reopening_discovers_existing_operation_without_starting_another() {
        let (mut chat, _tx, mut rx, mut ops) = make_chatwidget_manual_with_sender().await;
        while rx.try_recv().is_ok() {}
        while ops.try_recv().is_ok() {}
        chat.apply_external_edit("original work".into());
        let view = Arc::new(Mutex::new(View::default()));
        let weak = Arc::downgrade(&view);
        chat.alab_enrollment_show(view, None, "Loading", vec![]);
        let thread = chat.thread_id();
        chat.alab_enrollment_event(
            weak.clone(),
            thread,
            Event::Status {
                status: status(),
                label: None,
                display_accounts: vec![],
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
            event: Event::Action(Action::Observe(operation, label)),
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
            Event::Action(Action::Observe(operation, label)),
        );
        assert!(render_bottom_popup(&chat, 100).contains("Complete sign-in in aterm"));
        assert!(
            !render_bottom_popup(&chat, 100).contains("Account enrollment ·"),
            "an unsolicited operation from another revision cannot take the current row's label"
        );
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
        chat.alab_enrollment_show(view, None, "original", vec![]);
        let thread = chat.thread_id();
        chat.thread_id = Some(ThreadId::new());
        chat.alab_enrollment_event(weak.clone(), thread, Event::Failed("wrong thread", None));
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
    async fn confirmed_refusal_offers_the_native_sign_in_step_and_opens_accounts() {
        // Product activation is process-wide. Exercise the ordinary accounts entry in a
        // child without a host session, just as the accounts view's own activation test does.
        const CHILD: &str = "ALAB_ENROLLMENT_REFUSAL_FIXTURE";
        if std::env::var_os(CHILD).is_none() {
            let name = concat!(
                module_path!(),
                "::confirmed_refusal_offers_the_native_sign_in_step_and_opens_accounts"
            )
            .split_once("::")
            .unwrap()
            .1;
            let result = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", name])
                .env(CHILD, "1")
                .env_remove(codex_alab_core::session::ATERM_SESSION_ENV)
                .output()
                .unwrap();
            let stdout = String::from_utf8_lossy(&result.stdout);
            assert!(
                result.status.success() && stdout.contains("1 passed"),
                "{stdout}{}",
                String::from_utf8_lossy(&result.stderr)
            );
            return;
        }
        codex_alab_core::activate();
        let (mut chat, _tx, mut rx, mut ops) = make_chatwidget_manual_with_sender().await;
        while rx.try_recv().is_ok() {}
        while ops.try_recv().is_ok() {}
        let view = Arc::new(Mutex::new(View::default()));
        let weak = Arc::downgrade(&view);
        let thread = chat.thread_id();
        chat.alab_enrollment_show(view, None, "Loading", vec![]);
        let mut active = Active::from(status().operations.remove(0));
        active.label = Some("Work".into());
        assert!(weak.upgrade().unwrap().lock().unwrap().select(&active));

        for phase in [Phase::AuthenticationRequired, Phase::Refused] {
            chat.alab_enrollment_event(
                weak.clone(),
                thread,
                Event::Progress {
                    active: active.clone(),
                    reply: Reply::Enrollment {
                        phase,
                        generation: None,
                    },
                },
            );
            for width in [52, 120] {
                let text = render_bottom_popup(&chat, width)
                    .split_whitespace()
                    .join(" ");
                assert!(text.contains("Account enrollment · Work"), "{text}");
                let refused = phase == Phase::Refused;
                for clue in [
                    "Enrollment refused",
                    "aterm harness accounts list --cli codex",
                    "sign-in needed line, if present",
                    "Open /accounts",
                ] {
                    assert_eq!(text.contains(clue), refused, "{text}");
                }
                assert_eq!(text.contains("Refresh this operation"), !refused, "{text}");
                assert_eq!(
                    text.contains("Complete sign-in in aterm"),
                    !refused,
                    "{text}"
                );
            }
        }
        assert!(rx.try_recv().is_err());
        chat.handle_key_event(crossterm::event::KeyCode::Enter.into());
        let AppEvent::AlabEnrollment {
            view,
            thread_id,
            event: Event::Action(Action::OpenAccounts),
        } = rx.try_recv().unwrap()
        else {
            panic!("Enter should open the current account list");
        };
        chat.alab_enrollment_event(view, thread_id, Event::Action(Action::OpenAccounts));
        assert_eq!(chat.bottom_pane.active_view_id(), Some("alab-accounts"));
        assert!(render_bottom_popup(&chat, 100).contains("Loading subscription accounts"));
        assert!(ops.try_recv().is_err());
    }

    #[tokio::test]
    async fn cancellation_and_transport_wait_do_not_claim_terminal_completion() {
        let (mut chat, _tx, _rx, _ops) = make_chatwidget_manual_with_sender().await;
        let view = Arc::new(Mutex::new(View::default()));
        let weak = Arc::downgrade(&view);
        chat.alab_enrollment_show(view, None, "Loading", vec![]);
        let thread = chat.thread_id();
        let active: Active = status().operations.remove(0).into();
        assert!(weak.upgrade().unwrap().lock().unwrap().select(&active));
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
