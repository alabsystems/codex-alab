// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! ALAB(context) glue, compiled inside codex-tui as `chatwidget::ide_context::alab_context`
//! (one `#[path]` line at the end of `tui/src/chatwidget/ide_context.rs`): `/ide` inside aterm
//! takes its context from aterm (`codex_alab_tui::context`). Two hooks reach it, each the
//! first line of the function it may take over: [`apply`] in `maybe_apply_ide_context` and
//! [`status`] in `add_ide_context_status_message`. Each answers `false`, leaving upstream's
//! IDE path to run, unless `/ide` is on and aterm can give the context now.

use codex_alab_tui::context;
use codex_app_server_protocol::ByteRange;
use codex_app_server_protocol::TextElement;
use codex_app_server_protocol::UserInput;

use super::super::ChatWidget;

/// Upstream's marker between the context and the person's request: transcripts and replays
/// show only what follows it (`ide_context/prompt.rs`, `PROMPT_REQUEST_BEGIN`).
const PROMPT_REQUEST_BEGIN: &str = "## My request for Codex:";

/// The outgoing turn takes aterm's context: `true` when this hook handled `/ide` for it.
pub(super) fn apply(chat: &mut ChatWidget, items: &mut Vec<UserInput>) -> bool {
    if !chat.ide_context.is_enabled() {
        return false;
    }
    // A turn the autopilot knows by its exact text gets no context at all, aterm's or an
    // IDE's, whether or not aterm can give one now: a prefix would cost it its receipt (review
    // of the inbox, finding 12). codex-alab's own turn (the plan box's yes): no person asked.
    // A mail turn (the native inbox): the sender's, not the person's.
    if codex_alab_tui::boxes::tick_shared().is_some_and(|state| state.is_own_turn(&text_of(items)))
        || codex_alab_tui::mail::is_mail_turn(&text_of(items))
    {
        chat.sync_ide_context_status_indicator();
        return true;
    }
    if !context::available() {
        return false;
    }
    chat.sync_ide_context_status_indicator();
    match context::read() {
        Some(ctx) if !ctx.prompt.is_empty() => {
            prefix(items, &ctx.prompt);
            chat.add_info_message(context::attached(&ctx), /*hint*/ None);
        }
        Some(_) => {}
        None => chat.add_info_message(context::SKIPPED.to_string(), /*hint*/ None),
    }
    true
}

/// The turn's text as the autopilot reads it back: its text items, joined.
fn text_of(items: &[UserInput]) -> String {
    items
        .iter()
        .filter_map(|item| match item {
            UserInput::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

/// `/ide on` or `/ide status` inside aterm: say what each message will carry. `true` when
/// this hook said it (and so `/ide` stays on, where upstream would find no IDE and turn off).
pub(super) fn status(chat: &mut ChatWidget) -> bool {
    if !chat.ide_context.is_enabled() || !context::available() {
        return false;
    }
    chat.sync_ide_context_status_indicator();
    chat.add_info_message(
        context::STATUS.to_string(),
        Some(context::STATUS_HINT.to_string()),
    );
    true
}

/// Put `context` and upstream's marker ahead of the first text item (its elements moved with
/// it), or as a text item of its own ahead of the rest, as upstream's IDE path does.
fn prefix(items: &mut Vec<UserInput>, context: &str) {
    let prefix = format!("{context}\n{PROMPT_REQUEST_BEGIN}\n");
    let Some(at) = items
        .iter()
        .position(|item| matches!(item, UserInput::Text { .. }))
    else {
        items.insert(
            0,
            UserInput::Text {
                text: prefix,
                text_elements: Vec::new(),
            },
        );
        return;
    };
    if let UserInput::Text {
        text,
        text_elements,
    } = &mut items[at]
    {
        let shift = prefix.len();
        *text = format!("{prefix}{text}");
        *text_elements = text_elements
            .iter()
            .map(|element| {
                let range = element.byte_range.clone();
                TextElement::new(
                    ByteRange {
                        start: range.start + shift,
                        end: range.end + shift,
                    },
                    element.placeholder().map(str::to_string),
                )
            })
            .collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_context_leads_the_text_and_the_request_survives_the_transcript() {
        let mut items = vec![UserInput::Text {
            text: "fix [Image #1]".to_string(),
            text_elements: vec![TextElement::new(
                ByteRange { start: 4, end: 14 },
                Some("[Image #1]".to_string()),
            )],
        }];
        prefix(&mut items, "# Context from my terminal (aterm)\n");
        let UserInput::Text {
            text,
            text_elements,
        } = &items[0]
        else {
            panic!("a text item");
        };
        assert!(text.starts_with("# Context from my terminal (aterm)\n"));
        let element = &text_elements[0];
        assert_eq!(
            &text[element.byte_range.start..element.byte_range.end],
            "[Image #1]"
        );
        // Upstream's transcript reader shows only the person's words.
        let (request, _) = crate::user_prompt::extract_prompt_request_with_offset(text);
        assert_eq!(request, "fix [Image #1]");
    }

    #[test]
    fn a_turn_without_text_gets_a_text_item_ahead() {
        let mut items = vec![UserInput::Audio {
            audio: codex_app_server_protocol::AudioReference::Inline {
                url: "data:audio/wav;base64,AAAA".to_string(),
            },
        }];
        prefix(&mut items, "ctx\n");
        assert!(matches!(&items[0], UserInput::Text { text, .. } if text.starts_with("ctx\n")));
        assert_eq!(items.len(), 2);
    }

    /// Review of the inbox, finding 12: with `/ide` on and aterm's context not available
    /// (no proven claim, an older aterm), a mail turn still goes exactly as the autopilot
    /// knows it, no IDE context ahead of it, so its end is receipted and reported. Negative
    /// control: a person's own turn is left to upstream's IDE path.
    #[tokio::test]
    async fn a_mail_turn_takes_no_context_even_when_aterm_gives_none() {
        let (mut chat, _tx, _rx, _op_rx) =
            crate::chatwidget::tests::make_chatwidget_manual_with_sender().await;
        chat.ide_context.enable();
        let state: &'static codex_alab_tui::boxes::Shared =
            Box::leak(Box::new(codex_alab_tui::boxes::Shared::new()));
        let _scope = codex_alab_tui::boxes::scoped(state);
        let text = "# Mail from aterm (task @7)\n…\n## My request for Codex:\nInbox: task @7";
        state.expect_mail_turn(codex_alab_tui::mail::MailTurn {
            id: 1,
            off: 7,
            from: "s-lead@n-lab".to_string(),
            kind: "task".to_string(),
            text: text.to_string(),
        });
        assert!(!codex_alab_tui::context::available());
        let mut items = vec![UserInput::Text {
            text: text.to_string(),
            text_elements: Vec::new(),
        }];
        assert!(apply(&mut chat, &mut items), "handled: nothing attached");
        assert_eq!(
            items,
            vec![UserInput::Text {
                text: text.to_string(),
                text_elements: Vec::new(),
            }]
        );
        assert!(state.is_mail_turn(text), "still known to the autopilot");
        let mut items = vec![UserInput::Text {
            text: "fix it".to_string(),
            text_elements: Vec::new(),
        }];
        assert!(
            !apply(&mut chat, &mut items),
            "upstream's IDE path takes it"
        );
    }

    #[tokio::test]
    async fn outside_codex_alab_ide_is_upstreams() {
        let (mut chat, _tx, _rx, _op_rx) =
            crate::chatwidget::tests::make_chatwidget_manual_with_sender().await;
        let mut items = vec![UserInput::Text {
            text: "hi".to_string(),
            text_elements: Vec::new(),
        }];
        assert!(!apply(&mut chat, &mut items));
        assert!(!status(&mut chat));
        assert_eq!(
            items,
            vec![UserInput::Text {
                text: "hi".to_string(),
                text_elements: Vec::new(),
            }]
        );
    }
}
