// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Reapply native inherited-message admission to already adopted checkpoint entries.
//! This leaves local evidence, accounting, revisions and omission facts intact.

use super::RetainedContext;

impl RetainedContext {
    /// A restricted runtime cannot adopt the parent message prefix from a checkpoint.
    /// Use the same stored provenance as native live recording, never message text.
    pub fn alab_restrict_inherited_messages(&mut self) {
        self.user_messages.retain(|entry| !entry.inherited);
        self.assistant_messages.retain(|entry| !entry.inherited);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RetainedContextEvent;
    use crate::RetainedInputSource;
    use crate::RetainedUserMessage;
    use crate::UserInputOrigin;
    use crate::VerifiedAnswer;
    use crate::VerifiedQuestionAnswer;
    use codex_protocol_test_support::uuid_v7_item_id_generator_for_test;

    #[test]
    fn restricted_checkpoint_keeps_local_evidence_and_existing_uncertainty() {
        let generator = uuid_v7_item_id_generator_for_test();
        let message = |id: &str| RetainedUserMessage {
            turn_id: "original-turn".into(),
            message_id: Some(id.into()),
            text: id.into(),
            complete: true,
            origin: UserInputOrigin::User,
            phase: None,
        };
        let mut history = RetainedContext::default();
        history.record_user_message(
            message("local restriction"),
            RetainedInputSource::Local(Some(7)),
            &generator,
        );
        history.record_assistant_message(
            message("local assistant"),
            RetainedInputSource::Local(Some(8)),
            &generator,
        );
        history.record(
            &RetainedContextEvent::VerifiedAnswer {
                answer: VerifiedAnswer {
                    turn_id: "original-turn".into(),
                    call_id: "original-question".into(),
                    questions: vec![VerifiedQuestionAnswer {
                        question: "Publish?".into(),
                        answer: "No.".into(),
                    }],
                },
                acceptance_order: Some(9),
            },
            &generator,
        );
        let mut expected = history.clone();
        history.record_user_message(
            message("inherited grant"),
            RetainedInputSource::Inherited,
            &generator,
        );
        history.record_assistant_message(
            message("inherited assistant"),
            RetainedInputSource::Inherited,
            &generator,
        );
        // Adopting parent inputs already made their missing answers uncertain.
        // Removing those inputs must never claim that the gap has been repaired.
        expected.verified_answers_incomplete = true;
        let mut checkpoint: RetainedContext =
            serde_json::from_value(serde_json::to_value(history).unwrap()).unwrap();
        checkpoint.alab_restrict_inherited_messages();
        assert_eq!(checkpoint, expected);
        checkpoint.alab_restrict_inherited_messages();
        assert_eq!(checkpoint, expected);
    }
}
