// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! ALAB(answer) glue, compiled inside codex-tui as `bottom_pane::request_user_input::
//! alab_answer` (one `#[path]` line at the end of `request_user_input/mod.rs`, and one line
//! in the overlay's `pre_draw_tick`): the `request_user_input` overlay, while it is the
//! TUI's active view, is reported to codex-alab's shared state each frame; an answer the
//! autopilot posted for THIS request is applied through the overlay's own state and its own
//! `submit_answers`, so it leaves on the TUI's own connection as a person's Enter sends it,
//! the history cell is written, and the overlay closes (or moves to the next queued request).
//!
//! It refuses (and says why) when a person has begun answering, or when the answer does not
//! name every question of this request with a choice it has. Inert unless
//! `codex_alab_tui::boxes::tick_shared` names a state: the codex binary, or an ALab test.

use super::ComposerDraft;
use super::RequestUserInputOverlay;
use codex_alab_tui::boxes::BoxAnswer;
use codex_alab_tui::boxes::BoxKind;
use codex_alab_tui::boxes::BoxOption;
use codex_alab_tui::boxes::BoxOutcome;
use codex_alab_tui::boxes::BoxQuestion;
use codex_alab_tui::boxes::Choice;
use codex_alab_tui::boxes::OpenBox;

/// The overlay's frame: report it, then apply a posted answer. `true` when one was applied
/// (the frame redraws, and the pane pops a completed overlay).
pub(super) fn tick(overlay: &mut RequestUserInputOverlay) -> bool {
    let Some(state) = codex_alab_tui::boxes::tick_shared() else {
        return false;
    };
    if overlay.done {
        return false;
    }
    let begun = begun(overlay);
    if !state.touch_box(&overlay.request.item_id, begun) {
        state.report_box(snapshot(overlay, begun));
    }
    let Some(answer) = state.take_box_answer(&overlay.request.item_id) else {
        return false;
    };
    let id = overlay.request.item_id.clone();
    let applied = apply(overlay, answer);
    let taken = applied.is_ok();
    if taken {
        codex_alab_tui::boxes::close_answered(state, &id, std::ptr::from_ref(overlay).addr());
    }
    state.set_box_outcome(BoxOutcome { id, applied });
    taken
}

/// Whether, and how, a person has begun answering: theirs to finish.
fn begun(overlay: &RequestUserInputOverlay) -> Option<&'static str> {
    if overlay.confirm_unanswered.is_some() {
        Some("a person opened its unanswered-questions confirmation")
    } else if overlay.answers.iter().any(|a| a.answer_committed) {
        Some("a person answered part of it")
    } else if !overlay.composer.is_empty()
        || overlay.composer.is_in_paste_burst()
        || overlay
            .answers
            .iter()
            .any(|a| !a.draft.text.trim().is_empty())
    {
        Some("a person typed in its notes")
    } else {
        None
    }
}

fn snapshot(overlay: &RequestUserInputOverlay, begun: Option<&str>) -> OpenBox {
    let request = &overlay.request;
    OpenBox {
        id: request.item_id.clone(),
        kind: BoxKind::Question {
            thread: request.thread_id.clone(),
            questions: request
                .questions
                .iter()
                .map(|q| BoxQuestion {
                    id: q.id.clone(),
                    header: q.header.clone(),
                    question: q.question.clone(),
                    options: q
                        .options
                        .iter()
                        .flatten()
                        .map(|o| BoxOption {
                            label: o.label.clone(),
                            description: o.description.clone(),
                        })
                        .collect(),
                    is_other: q.is_other,
                    is_secret: q.is_secret,
                })
                .collect(),
            blocking: request.is_blocking,
            begun: begun.map(str::to_owned),
        },
        opened_ms: codex_alab_tui::boxes::now_ms(),
    }
}

/// Put `answer` in the overlay's own state and submit it, or say why not (nothing changed).
fn apply(overlay: &mut RequestUserInputOverlay, answer: BoxAnswer) -> Result<(), String> {
    let BoxAnswer::Question { answers, .. } = answer else {
        return Err("a plan box's answer is no question's".to_owned());
    };
    if let Some(why) = begun(overlay) {
        return Err(why.to_owned());
    }
    if answers.len() != overlay.request.questions.len() {
        return Err("the answer does not name every question".to_owned());
    }
    let mut plan = Vec::with_capacity(answers.len());
    for (idx, question) in overlay.request.questions.iter().enumerate() {
        let Some((_, choice)) = answers.iter().find(|(id, _)| *id == question.id) else {
            return Err(format!("no answer for question `{}`", question.id));
        };
        let options = question.options.as_ref().map_or(0, Vec::len);
        if let Choice::Option(at) = choice
            && *at >= options
        {
            return Err(format!(
                "question `{}` has no option {}",
                question.id,
                at + 1
            ));
        }
        plan.push((idx, options > 0, choice.clone()));
    }
    for (idx, has_options, choice) in plan {
        let Some(state) = overlay.answers.get_mut(idx) else {
            return Err("the overlay lost a question".to_owned());
        };
        match choice {
            Choice::Option(at) => {
                state.options_state.selected_idx = Some(at);
                state.draft = ComposerDraft::default();
            }
            Choice::Note(text) => {
                if has_options {
                    state.options_state.selected_idx = None;
                }
                state.draft = ComposerDraft {
                    text,
                    ..ComposerDraft::default()
                };
                state.notes_visible = true;
            }
        }
        state.answer_committed = true;
    }
    overlay.confirm_unanswered = None;
    overlay.pending_submission_draft = None;
    overlay.restore_current_draft();
    overlay.submit_answers();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::RequestUserInputOverlay;
    use crate::app_command::AppCommand;
    use crate::app_event::AppEvent;
    use crate::app_event_sender::AppEventSender;
    use crate::bottom_pane::bottom_pane_view::BottomPaneView;
    use codex_alab_tui::boxes::BoxAnswer;
    use codex_alab_tui::boxes::BoxKind;
    use codex_alab_tui::boxes::Choice;
    use codex_alab_tui::boxes::Shared;
    use codex_app_server_protocol::ToolRequestUserInputOption;
    use codex_app_server_protocol::ToolRequestUserInputParams;
    use codex_app_server_protocol::ToolRequestUserInputQuestion;
    use codex_app_server_protocol::ToolRequestUserInputResponse;
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;
    use pretty_assertions::assert_eq;
    use std::time::Instant;
    use tokio::sync::mpsc::UnboundedReceiver;
    use tokio::sync::mpsc::unbounded_channel;

    fn request() -> ToolRequestUserInputParams {
        ToolRequestUserInputParams {
            thread_id: "00000000-0000-0000-0000-000000000001".to_owned(),
            turn_id: "turn-1".to_owned(),
            item_id: "call-1".to_owned(),
            questions: vec![
                ToolRequestUserInputQuestion {
                    id: "storage".to_owned(),
                    header: "Storage".to_owned(),
                    question: "Where should the cache live?".to_owned(),
                    is_other: true,
                    is_secret: false,
                    options: Some(vec![
                        ToolRequestUserInputOption {
                            label: "On disk".to_owned(),
                            description: "Survives restarts.".to_owned(),
                        },
                        ToolRequestUserInputOption {
                            label: "In memory (Recommended)".to_owned(),
                            description: "Fastest.".to_owned(),
                        },
                    ]),
                },
                ToolRequestUserInputQuestion {
                    id: "name".to_owned(),
                    header: "Name".to_owned(),
                    question: "What should it be called?".to_owned(),
                    is_other: false,
                    is_secret: false,
                    options: None,
                },
            ],
            is_blocking: true,
            auto_resolution_ms: None,
        }
    }

    fn overlay() -> (RequestUserInputOverlay, UnboundedReceiver<AppEvent>) {
        overlay_with(/*disable_paste_burst*/ false)
    }

    fn overlay_with(
        disable_paste_burst: bool,
    ) -> (RequestUserInputOverlay, UnboundedReceiver<AppEvent>) {
        let (tx, rx) = unbounded_channel::<AppEvent>();
        let overlay = RequestUserInputOverlay::new(
            request(),
            AppEventSender::new(tx),
            /*has_input_focus*/ true,
            /*enhanced_keys_supported*/ false,
            disable_paste_burst,
        );
        (overlay, rx)
    }

    fn state() -> &'static Shared {
        Box::leak(Box::new(Shared::new()))
    }

    fn answered(rx: &mut UnboundedReceiver<AppEvent>) -> (String, ToolRequestUserInputResponse) {
        let mut found = None;
        let mut history = 0;
        while let Ok(event) = rx.try_recv() {
            match event {
                AppEvent::SubmitThreadOp {
                    op: AppCommand::UserInputAnswer { id, response },
                    ..
                } => found = Some((id, response)),
                AppEvent::InsertHistoryCell(_) => history += 1,
                _ => {}
            }
        }
        assert_eq!(history, 1, "the answer is written to the transcript once");
        found.unwrap_or_else(|| panic!("no UserInputAnswer was submitted"))
    }

    fn by_codex_alab() -> BoxAnswer {
        BoxAnswer::Question {
            id: "call-1".to_owned(),
            answers: vec![
                ("storage".to_owned(), Choice::Option(1)),
                ("name".to_owned(), Choice::Note("cache".to_owned())),
            ],
        }
    }

    #[test]
    fn a_posted_answer_leaves_as_a_persons_enter_would_send_it_and_the_overlay_closes() {
        let shared = state();
        let _scope = codex_alab_tui::boxes::scoped(shared);
        let (mut ours, mut rx) = overlay();
        assert!(!ours.pre_draw_tick(Instant::now()), "nothing is posted yet");
        let open = shared
            .open_box()
            .unwrap_or_else(|| panic!("the overlay is reported"));
        assert_eq!(open.id, "call-1");
        let BoxKind::Question {
            questions, begun, ..
        } = &open.kind
        else {
            panic!("a question box: {open:?}");
        };
        assert_eq!(questions.len(), 2);
        assert_eq!(questions[0].options[1].label, "In memory (Recommended)");
        assert_eq!(*begun, None);

        assert!(shared.post_box_answer(by_codex_alab()));
        assert!(ours.pre_draw_tick(Instant::now()), "the answer is applied");
        assert!(ours.is_complete(), "the overlay closes");
        assert_eq!(shared.take_box_outcome().map(|o| o.applied), Some(Ok(())));
        let ours = answered(&mut rx);

        // The same answer by a person's keys: down to option 2, Enter, the name, Enter
        // (typed slowly: no paste burst gathers the keys).
        let (mut theirs, mut rx) = overlay_with(/*disable_paste_burst*/ true);
        theirs.handle_key_event(KeyEvent::from(KeyCode::Down));
        theirs.handle_key_event(KeyEvent::from(KeyCode::Enter));
        for ch in "cache".chars() {
            theirs.handle_key_event(KeyEvent::from(KeyCode::Char(ch)));
        }
        theirs.handle_key_event(KeyEvent::from(KeyCode::Enter));
        assert!(theirs.is_complete());
        assert_eq!(ours, answered(&mut rx));
        assert_eq!(ours.0, "turn-1", "it answers the request's own turn");
        assert_eq!(
            ours.1.answers["storage"].answers,
            vec!["In memory (Recommended)".to_owned()]
        );
        assert_eq!(
            ours.1.answers["name"].answers,
            vec!["user_note: cache".to_owned()]
        );
    }

    #[test]
    fn a_policy_text_goes_in_the_notes_with_no_option_chosen() {
        let shared = state();
        let _scope = codex_alab_tui::boxes::scoped(shared);
        let (mut overlay, mut rx) = overlay();
        overlay.pre_draw_tick(Instant::now());
        assert!(shared.post_box_answer(BoxAnswer::Question {
            id: "call-1".to_owned(),
            answers: vec![
                (
                    "storage".to_owned(),
                    Choice::Note("Decide yourself.".to_owned())
                ),
                (
                    "name".to_owned(),
                    Choice::Note("Decide yourself.".to_owned())
                ),
            ],
        }));
        assert!(overlay.pre_draw_tick(Instant::now()));
        let (_, response) = answered(&mut rx);
        assert_eq!(
            response.answers["storage"].answers,
            vec!["user_note: Decide yourself.".to_owned()]
        );
    }

    #[test]
    fn a_person_who_began_answering_keeps_the_overlay() {
        let shared = state();
        let _scope = codex_alab_tui::boxes::scoped(shared);
        let (mut overlay, mut rx) = overlay();
        overlay.pre_draw_tick(Instant::now());
        assert!(shared.post_box_answer(by_codex_alab()));
        overlay.handle_key_event(KeyEvent::from(KeyCode::Tab));
        for ch in "mine".chars() {
            overlay.handle_key_event(KeyEvent::from(KeyCode::Char(ch)));
        }
        assert!(!overlay.pre_draw_tick(Instant::now()));
        assert!(!overlay.is_complete());
        let outcome = shared.take_box_outcome().map(|o| o.applied);
        assert!(
            matches!(&outcome, Some(Err(why)) if why.contains("a person")),
            "{outcome:?}"
        );
        assert!(rx.try_recv().is_err(), "nothing was submitted");
        let Some(BoxKind::Question { begun, .. }) = shared.open_box().map(|b| b.kind) else {
            panic!("still open");
        };
        assert_eq!(begun.as_deref(), Some("a person typed in its notes"));
    }

    #[test]
    fn an_answer_for_another_request_or_without_every_question_is_refused() {
        let shared = state();
        let _scope = codex_alab_tui::boxes::scoped(shared);
        let (mut overlay, mut rx) = overlay();
        overlay.pre_draw_tick(Instant::now());
        assert!(!shared.post_box_answer(BoxAnswer::Question {
            id: "call-2".to_owned(),
            answers: Vec::new(),
        }));
        assert!(shared.post_box_answer(BoxAnswer::Question {
            id: "call-1".to_owned(),
            answers: vec![("storage".to_owned(), Choice::Option(1))],
        }));
        assert!(!overlay.pre_draw_tick(Instant::now()));
        assert!(!overlay.is_complete());
        assert!(rx.try_recv().is_err());
        assert!(shared.post_box_answer(BoxAnswer::Question {
            id: "call-1".to_owned(),
            answers: vec![
                ("storage".to_owned(), Choice::Option(5)),
                ("name".to_owned(), Choice::Note("x".to_owned())),
            ],
        }));
        assert!(!overlay.pre_draw_tick(Instant::now()));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn without_alab_the_overlay_ticks_as_upstreams() {
        let (mut overlay, mut rx) = overlay();
        assert!(!overlay.pre_draw_tick(Instant::now()));
        assert!(!overlay.is_complete());
        assert!(rx.try_recv().is_err());
    }
}
