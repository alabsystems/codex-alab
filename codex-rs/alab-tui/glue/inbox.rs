// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! ALAB(inbox) glue, compiled inside codex-tui as `chatwidget::input_submission::alab_inbox`
//! (one `#[path]` line at the end of `tui/src/chatwidget/input_submission.rs`): a typed
//! `Inbox: task @<off>` is resolved natively (`codex_alab_tui::mail`). The hook sits just
//! before `/ide`'s, once the turn's items are built: [`intercept`] either leaves them as
//! they are (the line goes as typed), puts the mail ahead of the typed line, or holds mail
//! aterm does not vouch for in the composer for a person, in which case the turn is not sent.

use codex_alab_tui::mail;
use codex_app_server_protocol::UserInput;

use super::super::ChatWidget;
use super::super::UserMessage;

/// `true` when this turn must not be sent: its mail now waits in the composer for a person.
pub(super) fn intercept(chat: &mut ChatWidget, items: &mut [UserInput]) -> bool {
    let [UserInput::Text { text, .. }] = &*items else {
        return false;
    };
    let Some((kind, off)) = mail::trigger(text) else {
        return false;
    };
    let typed = text.clone();
    let held = mail::claim_held();
    let mail = match mail::read(kind, off) {
        Ok(mail) => mail,
        Err(why) => {
            // Fail closed: the line goes as typed. Say so only where codex-alab could have
            // read it (a proven claim held), never outside aterm.
            if held {
                chat.add_info_message(
                    format!("aterm mail @{off} was not read ({why}): the line goes as typed."),
                    /*hint*/ None,
                );
            }
            return false;
        }
    };
    if !mail.sendable() {
        let words = mail::context(&mail);
        mail::expect(&mail, &words);
        chat.restore_user_message_to_composer(UserMessage::from(words));
        chat.add_info_message(mail::held_for_person(&mail), /*hint*/ None);
        return true;
    }
    let full = mail::turn_text(&mail, &typed);
    mail::expect(&mail, &full);
    items[0] = UserInput::Text {
        text: full,
        text_elements: Vec::new(),
    };
    chat.add_info_message(mail::info(&mail), /*hint*/ None);
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[tokio::test]
    async fn outside_codex_alab_the_line_goes_as_typed() {
        let (mut chat, _tx, _rx, _op_rx) =
            crate::chatwidget::tests::make_chatwidget_manual_with_sender().await;
        let mut items = vec![UserInput::Text {
            text: "Inbox: task @12".to_string(),
            text_elements: Vec::new(),
        }];
        assert!(!intercept(&mut chat, &mut items));
        assert_eq!(
            items,
            vec![UserInput::Text {
                text: "Inbox: task @12".to_string(),
                text_elements: Vec::new(),
            }]
        );
    }
}
