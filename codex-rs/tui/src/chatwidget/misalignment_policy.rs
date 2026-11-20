//! Show misalignment findings in history when review is disabled. With `misalignment_review` enabled, block
//! ordinary input for explicit review; review identity prevents stale UI actions from authorizing
//! a turn.

use super::*;
use codex_app_server_protocol::MisalignmentErrorDetails;

const PRECAUTION_VIEW: &str = "misalignment_precaution";

#[derive(Debug)]
pub(crate) struct MisalignmentReview {
    pub(crate) thread_id: ThreadId,
    pub(crate) turn_id: String,
    pub(crate) details: MisalignmentErrorDetails,
}

impl MisalignmentReview {
    pub(crate) fn continuation_message(&self) -> Option<&str> {
        self.details
            .steer
            .as_ref()
            .map(|steer| steer.message.as_str())
            .filter(|message| !message.trim().is_empty() && message.len() <= 1024)
    }
}

pub(super) struct MisalignmentViolation {
    turn_id: Option<String>,
    review: Option<Arc<MisalignmentReview>>,
    retired_voice: bool,
    passive: bool,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum MisalignmentTurnSource {
    ServerNotification,
    AcknowledgedContinuation,
}

const MISALIGNMENT_POLICY_TITLE: &str = "Chat stopped as a precaution";
const MISALIGNMENT_POLICY_DESCRIPTION: &str = "We couldn’t confirm the agent was acting safely and following your instructions. To continue working, start or resume another chat.";
const MISALIGNMENT_POLICY_NOTICE: &str = "We couldn’t confirm the agent was acting safely and following your instructions. The request was stopped; you can continue with a new request.";

impl ChatWidget {
    fn add_passive_misalignment_findings(&mut self, review: Option<&MisalignmentReview>) {
        if let Some(review) = review {
            let explanation = review
                .details
                .detailed_explanation
                .as_deref()
                .unwrap_or(MISALIGNMENT_POLICY_NOTICE);
            self.add_to_history(history_cell::new_visible_warning_event(format!(
                "Misalignment policy finding: {explanation}"
            )));
            if let Some(message) = review.continuation_message() {
                self.add_to_history(history_cell::new_info_event(
                    format!("Server-suggested continuation (not submitted): {message:?}"),
                    // ALAB(misalignment-review): Enabling review applies to future launches.
                    Some(
                        "Interactive review is off in this session. To review future findings, \
                         run `codex features enable misalignment_review` in a shell before your next Codex launch."
                            .to_string(),
                    ),
                ));
            }
        } else {
            self.add_to_history(history_cell::new_visible_warning_event(
                MISALIGNMENT_POLICY_NOTICE.to_string(),
            ));
        }
    }

    pub(crate) fn has_misalignment_policy_violation(&self) -> bool {
        self.misalignment_policy_violation
            .as_ref()
            .is_some_and(|violation| !violation.passive)
    }

    pub(super) fn alab_refused_turn(&self, turn: &str) -> bool {
        self.misalignment_policy_violation.as_ref().is_some_and(|finding| finding.turn_id.as_deref() == Some(turn))
    }

    pub(crate) fn bind_live_misalignment_turn(&mut self, turn: &str) -> bool {
        let lifecycle = &mut self.turn_lifecycle;
        if lifecycle.admitted_turn_id.as_deref().is_some_and(|id| id != turn)
            || (lifecycle.agent_turn_running
                && lifecycle.last_turn_id.as_deref().is_some_and(|id| id != turn))
        {
            return false;
        }
        if lifecycle.last_turn_id.as_deref() != Some(turn) {
            lifecycle.last_turn_id = Some(turn.to_owned());
        }
        true
    }

    pub(crate) fn rejects_misalignment_policy_op(&self, op: &AppCommand) -> bool {
        self.has_misalignment_policy_violation()
            && !matches!(
                op,
                AppCommand::Interrupt
                    | AppCommand::RealtimeConversationStop { .. }
                    | AppCommand::CleanBackgroundTerminals
                    | AppCommand::OverrideTurnContext { .. }
                    | AppCommand::ReloadUserConfig
                    | AppCommand::ListSkills { .. }
                    | AppCommand::SetThreadName { .. }
            )
    }

    pub(crate) fn on_misalignment_policy_violation(&mut self) {
        self.on_misalignment_error(/*turn_id*/ None, /*details*/ None);
    }

    pub(crate) fn on_side_thread_misalignment_policy_violation(&mut self) {
        if self.config.features.enabled(Feature::MisalignmentReview) {
            self.on_misalignment_policy_violation();
        } else {
            // ALAB(misalignment-review): A child finding must not finalize the visible parent turn.
            self.add_passive_misalignment_findings(/*review*/ None);
            self.request_redraw();
        }
    }

    pub(super) fn on_misalignment_review_feature_changed(&mut self, enabled: bool) {
        if enabled || !self.has_misalignment_policy_violation() {
            return;
        }
        let review = self
            .misalignment_policy_violation
            .as_ref()
            .and_then(|violation| violation.review.clone());
        if let Some(violation) = self.misalignment_policy_violation.as_mut() {
            violation.passive = true;
            violation.retired_voice = false;
        }
        // ALAB(misalignment-review): Opting out immediately releases existing review UI.
        self.bottom_pane.dismiss_view_by_id(PRECAUTION_VIEW);
        self.bottom_pane
            .set_composer_input_enabled(/*enabled*/ true, /*placeholder*/ None);
        self.add_passive_misalignment_findings(review.as_deref());
        self.request_redraw();
        self.alab_recover_finding_input(/*fresh_launch*/ false); // ALAB(misalignment-review): dismissing interactive review never authorizes queued input.
    }

    pub(super) fn on_misalignment_error(
        &mut self,
        turn_id: Option<String>,
        details: Option<MisalignmentErrorDetails>,
    ) {
        self.on_turn_misalignment_error(turn_id, details, None);
    }

    pub(super) fn on_turn_misalignment_error(
        &mut self,
        turn_id: Option<String>,
        details: Option<MisalignmentErrorDetails>,
        replay_kind: Option<ReplayKind>,
    ) {
        // A snapshot selects its latest refusal even when its turn start was evicted.
        if replay_kind.is_none()
            && let (Some(turn_id), Some(latest)) = (&turn_id, &self.turn_lifecycle.last_turn_id)
            && turn_id != latest
        {
            return;
        }
        let details = if replay_kind.is_some() { None } else { details };
        let interactive = self.config.features.enabled(Feature::MisalignmentReview);
        let was_blocking = self
            .misalignment_policy_violation
            .as_ref()
            .is_some_and(|violation| !violation.passive);
        // Bound memory-only findings and newly submitted model input. Never truncate a steer.
        let missing_details = details.is_none();
        let details = details.filter(|details| {
            details
                .detailed_explanation
                .as_ref()
                .is_some_and(|text| !text.trim().is_empty() && text.len() <= 64 * 1024)
        });
        if let Some(current) = &self.misalignment_policy_violation
            && current.passive == !interactive
            // A parent-thread precaution cannot become a continuable side-thread warning.
            && (current.turn_id.is_none()
                || (current.turn_id == turn_id
                    && (missing_details
                        || current.review.as_ref().map(|review| &review.details) == details.as_ref())))
        {
            return;
        }
        let retired_voice = interactive && self.realtime_conversation_is_running();
        if retired_voice {
            self.stop_realtime_conversation();
        }
        let review = self.thread_id.zip(turn_id.clone()).zip(details).map(
            |((thread_id, turn_id), details)| {
                Arc::new(MisalignmentReview {
                    thread_id,
                    turn_id,
                    details,
                })
            },
        );
        let passive_review = (!interactive).then(|| review.clone()).flatten();
        let updating_passive_review = !interactive && self.misalignment_policy_violation.as_ref()
            .is_some_and(|previous| previous.passive && previous.turn_id == turn_id); // ALAB(misalignment-review): detail updates describe the same terminal event.
        self.misalignment_policy_violation = Some(MisalignmentViolation {
            turn_id,
            review,
            retired_voice,
            passive: !interactive,
        });
        // ALAB(misalignment-review): keep fresh input editable and retain earlier work for manual recovery.
        if !interactive {
            if !updating_passive_review {
                self.alab_recover_finding_input(replay_kind == Some(ReplayKind::ResumeInitialMessages));
                self.finalize_turn_after_finding(true); // ALAB(misalignment-review): stop the turn without inventing child process exits.
            }
            if was_blocking {
                self.bottom_pane.dismiss_view_by_id(PRECAUTION_VIEW);
                self.bottom_pane
                    .set_composer_input_enabled(/*enabled*/ true, /*placeholder*/ None);
            }
            self.add_passive_misalignment_findings(passive_review.as_deref());
            self.request_redraw();
            return;
        }
        self.input_queue.clear();
        self.finalize_turn_after_finding(true); // ALAB(misalignment-review): stop the turn without inventing child process exits.
        self.refresh_pending_input_preview();
        self.bottom_pane.drain_pending_submission_state();
        self.bottom_pane
            .set_composer_text(String::new(), Vec::new(), Vec::new());
        self.bottom_pane.set_composer_input_enabled(
            /*enabled*/ false,
            Some(MISALIGNMENT_POLICY_TITLE.to_string()),
        );

        self.show_misalignment_policy_precaution();
    }

    pub(crate) fn show_misalignment_policy_precaution(&mut self) {
        if !self.has_misalignment_policy_violation() {
            return;
        }
        self.bottom_pane.dismiss_view_by_id(PRECAUTION_VIEW);
        let mut items = vec![
            SelectionItem {
                name: "New chat".to_string(),
                actions: vec![Box::new(|tx| tx.send(AppEvent::NewSession { name: None }))],
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "Resume another chat".to_string(),
                actions: vec![Box::new(|tx| tx.send(AppEvent::OpenResumePicker))],
                ..Default::default()
            },
        ];
        let review = self
            .misalignment_policy_violation
            .as_ref()
            .and_then(|violation| violation.review.clone())
            // ALAB(misalignment-review): Hide a previously created review if the flag is disabled.
            .filter(|_| self.config.features.enabled(Feature::MisalignmentReview));
        if let Some(review) = &review {
            let review = Arc::clone(review);
            items.insert(
                0,
                SelectionItem {
                    name: "Review findings".to_string(),
                    actions: vec![Box::new(move |tx| {
                        tx.send(AppEvent::ReviewMisalignment(Arc::clone(&review)));
                    })],
                    ..Default::default()
                },
            );
        }
        items.insert(
            1,
            SelectionItem {
                name: "Agent command center".to_string(),
                actions: vec![Box::new(|tx| tx.send(AppEvent::OpenAgentsOverview))],
                ..Default::default()
            },
        );
        self.bottom_pane.show_selection_view(SelectionViewParams {
            view_id: Some(PRECAUTION_VIEW),
            header: Box::new(
                Paragraph::new(vec![
                    Line::from(if review.is_some() { "Chat paused as a precaution" } else { MISALIGNMENT_POLICY_TITLE }).bold(),
                    Line::from(if review.is_some() {
                        "We couldn’t confirm the agent was interpreting your instructions correctly. Review what we detected before deciding to continue."
                    } else { MISALIGNMENT_POLICY_DESCRIPTION }).dim(),
                ])
                .wrap(Wrap { trim: false }),
            ),
            items,
            allow_cancel: false,
            ..SelectionViewParams::picker()
        });
    }

    pub(crate) fn is_current_misalignment_review(&self, review: &Arc<MisalignmentReview>) -> bool {
        let current = self
            .misalignment_policy_violation
            .as_ref()
            .and_then(|violation| violation.review.as_ref());
        let turn_id = self.turn_lifecycle.last_turn_id.as_ref();
        // ALAB(misalignment-review): Passive or disabled findings cannot authorize continuation.
        self.has_misalignment_policy_violation()
            && self.config.features.enabled(Feature::MisalignmentReview)
            && self.thread_id == Some(review.thread_id)
            && turn_id.is_none_or(|id| id == &review.turn_id)
            && current.is_some_and(|current| Arc::ptr_eq(current, review))
    }

    pub(crate) fn show_misalignment_review_confirmation(
        &mut self,
        review: Arc<MisalignmentReview>,
    ) {
        if !self.is_current_misalignment_review(&review) {
            return;
        }
        let can_continue = review.continuation_message().is_some();
        self.bottom_pane.dismiss_view_by_id(PRECAUTION_VIEW);
        self.bottom_pane.show_selection_view(SelectionViewParams {
            view_id: Some(PRECAUTION_VIEW),
            title: Some("Chat paused as a precaution".to_string()),
            items: vec![
                SelectionItem {
                    name: "Acknowledge findings and continue".to_string(),
                    is_disabled: !can_continue,
                    actions: vec![Box::new(move |tx| {
                        tx.send(AppEvent::ContinueMisalignment(Arc::clone(&review)));
                    })],
                    require_explicit_confirmation: true,
                    dismiss_on_select: true,
                    ..Default::default()
                },
                SelectionItem {
                    name: "Back".to_string(),
                    actions: vec![Box::new(|tx| tx.send(AppEvent::CloseMisalignmentReview))],
                    ..Default::default()
                },
            ],
            allow_cancel: false,
            ..SelectionViewParams::confirmation()
        });
    }

    pub(crate) fn clear_misalignment_for_new_turn(
        &mut self,
        turn_id: &str,
        source: MisalignmentTurnSource,
    ) {
        if self
            .misalignment_policy_violation
            .as_ref()
            .is_some_and(|violation| {
                violation.passive // ALAB(misalignment-review): passive findings never leak into a later turn.
                    || (violation.turn_id.as_ref().is_some_and(|id| id != turn_id)
                        && (!violation.retired_voice
                            || source == MisalignmentTurnSource::AcknowledgedContinuation))
            })
        {
            self.misalignment_policy_violation = None;
            self.bottom_pane.dismiss_view_by_id(PRECAUTION_VIEW);
            self.bottom_pane
                .set_composer_input_enabled(/*enabled*/ true, /*placeholder*/ None);
        }
    }
}
