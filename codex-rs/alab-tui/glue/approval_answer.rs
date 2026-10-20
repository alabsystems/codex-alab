// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! ALAB(answer) glue, compiled inside codex-tui as `bottom_pane::approval_overlay::
//! alab_answer` (one `#[path]` line at the end of `approval_overlay.rs`, and the overlay's
//! one-line `pre_draw_tick`): an approval box (a command or an edit) that codex-alab's
//! approval contributor reviewed and cleared, but could not answer at the moment it was
//! asked (`Shared::set_preapproval`: the grace after a claim, a person's recent key), is
//! reported to codex-alab's shared state while it is the TUI's active view; an answer posted
//! for it selects the box's one-shot yes ("Yes, proceed") through the overlay's own
//! `apply_selection`, as a person's Enter does. Never a standing yes ("don't ask again", an
//! execpolicy amendment), never a box the contributor did not clear. Every other box is
//! untouched.

use super::ApprovalDecision;
use super::ApprovalOverlay;
use super::ApprovalRequest;
use codex_alab_tui::boxes::BoxAnswer;
use codex_alab_tui::boxes::BoxKind;
use codex_alab_tui::boxes::BoxOutcome;
use codex_alab_tui::boxes::OpenBox;
use codex_app_server_protocol::CommandExecutionApprovalDecision;
use codex_app_server_protocol::FileChangeApprovalDecision;

/// The overlay's frame: report a cleared box, then apply a posted answer.
pub(super) fn tick(view: &mut ApprovalOverlay) -> bool {
    let Some(state) = codex_alab_tui::boxes::tick_shared() else {
        return false;
    };
    if view.current_complete {
        return false;
    }
    let Some((thread, key)) = box_key(view) else {
        return false;
    };
    let Some(what) = state.preapproval(&key) else {
        return false;
    };
    let id = format!("approval-{key}");
    if !state.touch_box(&id, None) {
        state.report_box(OpenBox {
            id: id.clone(),
            kind: BoxKind::Approval {
                thread,
                key: key.clone(),
                what,
            },
            opened_ms: codex_alab_tui::boxes::now_ms(),
        });
    }
    let Some(answer) = state.take_box_answer(&id) else {
        return false;
    };
    let applied = apply(view, answer, &key);
    let taken = applied.is_ok();
    if taken {
        let _ = state.take_preapproval(&key);
        state.close_box(&id);
    }
    state.set_box_outcome(BoxOutcome { id, applied });
    taken
}

/// The current request's thread and id, for a command or an edit.
fn box_key(view: &ApprovalOverlay) -> Option<(String, String)> {
    match view.current_request.as_ref()? {
        ApprovalRequest::Exec(request) => Some((request.thread_id.to_string(), request.id.clone())),
        ApprovalRequest::ApplyPatch(request) => {
            Some((request.thread_id.to_string(), request.id.clone()))
        }
        ApprovalRequest::Permissions(_) | ApprovalRequest::McpElicitation(_) => None,
    }
}

/// The one-shot yes among the box's options.
fn once(decision: &ApprovalDecision) -> bool {
    matches!(
        decision,
        ApprovalDecision::Command(CommandExecutionApprovalDecision::Accept)
            | ApprovalDecision::FileChange(FileChangeApprovalDecision::Accept)
    )
}

fn apply(view: &mut ApprovalOverlay, answer: BoxAnswer, key: &str) -> Result<(), String> {
    let BoxAnswer::Approval { .. } = answer else {
        return Err("a question's or a plan's answer is no approval box's".to_owned());
    };
    if box_key(view).is_none_or(|(_, current)| current != key) {
        return Err("the approval box changed".to_owned());
    }
    let Some(at) = view
        .options
        .iter()
        .position(|option| once(&option.decision))
    else {
        return Err("the box offers no one-shot yes".to_owned());
    };
    view.apply_selection(at);
    if !view.current_complete && box_key(view).is_some_and(|(_, current)| current == key) {
        return Err("the box did not take the one-shot yes".to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::ExecApprovalRequest;
    use super::*;
    use crate::app_command::AppCommand;
    use crate::app_event::AppEvent;
    use crate::app_event_sender::AppEventSender;
    use crate::bottom_pane::BottomPaneView;
    use codex_alab_tui::boxes::Shared;
    use codex_features::Features;
    use codex_protocol::ThreadId;
    use pretty_assertions::assert_eq;
    use std::time::Instant;
    use tokio::sync::mpsc::UnboundedReceiver;
    use tokio::sync::mpsc::unbounded_channel;

    fn overlay() -> (ApprovalOverlay, UnboundedReceiver<AppEvent>) {
        let (tx, rx) = unbounded_channel::<AppEvent>();
        let keymap = crate::keymap::RuntimeKeymap::defaults();
        let view = ApprovalOverlay::new(
            ApprovalRequest::Exec(ExecApprovalRequest {
                kind: Default::default(),
                thread_id: ThreadId::new(),
                thread_label: None,
                id: "call-7".to_owned(),
                environment_id: None,
                command: vec!["rm".to_owned(), "-f".to_owned(), "junk.tmp".to_owned()],
                reason: None,
                available_decisions: vec![
                    CommandExecutionApprovalDecision::Accept,
                    CommandExecutionApprovalDecision::AcceptForSession,
                    CommandExecutionApprovalDecision::Cancel,
                ],
                network_approval_context: None,
                additional_permissions: None,
            }),
            AppEventSender::new(tx),
            Features::with_defaults(),
            keymap.approval,
            keymap.list,
        );
        (view, rx)
    }

    fn state() -> &'static Shared {
        Box::leak(Box::new(Shared::new()))
    }

    fn decisions(rx: &mut UnboundedReceiver<AppEvent>) -> Vec<CommandExecutionApprovalDecision> {
        let mut found = Vec::new();
        while let Ok(event) = rx.try_recv() {
            if let AppEvent::SubmitThreadOp {
                op: AppCommand::ExecApproval { decision, .. },
                ..
            } = event
            {
                found.push(decision);
            }
        }
        found
    }

    #[test]
    fn a_box_codex_alab_cleared_takes_the_one_shot_yes() {
        let shared = state();
        let _scope = codex_alab_tui::boxes::scoped(shared);
        let (mut view, mut rx) = overlay();
        shared.set_preapproval(&["call-7"], Some("exec_command".to_owned()));
        assert!(!view.pre_draw_tick(Instant::now()), "nothing is posted yet");
        let open = shared
            .open_box()
            .unwrap_or_else(|| panic!("the cleared box is reported"));
        assert_eq!(open.id, "approval-call-7");
        assert!(matches!(&open.kind, BoxKind::Approval { key, .. } if key == "call-7"));
        assert!(shared.post_box_answer(BoxAnswer::Approval {
            id: "approval-call-7".to_owned(),
        }));
        assert!(view.pre_draw_tick(Instant::now()), "the answer is applied");
        assert_eq!(shared.take_box_outcome().map(|o| o.applied), Some(Ok(())));
        assert_eq!(
            decisions(&mut rx),
            vec![CommandExecutionApprovalDecision::Accept],
            "once, never for the session"
        );
        assert_eq!(shared.preapproval("call-7"), None, "the review is used up");
        assert!(view.is_complete());
    }

    #[test]
    fn a_box_codex_alab_did_not_clear_is_never_reported() {
        let shared = state();
        let _scope = codex_alab_tui::boxes::scoped(shared);
        let (mut view, mut rx) = overlay();
        // Another box's review, or none: this one stays a person's.
        shared.set_preapproval(&["call-8"], Some("exec_command".to_owned()));
        assert!(!view.pre_draw_tick(Instant::now()));
        assert_eq!(shared.open_box(), None);
        assert!(decisions(&mut rx).is_empty());
        assert!(!view.is_complete());
    }
}
