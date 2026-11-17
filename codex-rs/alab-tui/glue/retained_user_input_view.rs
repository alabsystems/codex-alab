// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Editing DATA for an actual request_user_input view. Install requires the
//! exact original request+queued requests on a genuinely replayed destination;
//! its AppEventSender and Source callback recipients remain the original ones.
use super::*;
use serde::Deserialize;
use serde::Serialize;
use std::io;

crate::alab_editor_upgrade::native_state!(
    Focus,
    FocusWire,
    #[serde(remote = "Focus", deny_unknown_fields)]
    enum FocusWire {
        Options,
        Notes,
    }
);
crate::alab_editor_upgrade::native_state!(
    ComposerDraft,
    DraftWire,
    #[serde(remote = "ComposerDraft", deny_unknown_fields)]
    struct DraftWire {
        text: String,
        text_elements: Vec<TextElement>,
        local_image_paths: Vec<PathBuf>,
        pending_pastes: Vec<(String, String)>,
    }
);
crate::alab_editor_upgrade::native_state!(
    AnswerState,
    AnswerWire,
    #[serde(remote = "AnswerState", deny_unknown_fields)]
    struct AnswerWire {
        #[serde(with = "scroll")]
        options_state: ScrollState,
        draft: ComposerDraft,
        answer_committed: bool,
        notes_visible: bool,
    }
);
mod scroll {
    use super::*;
    #[derive(Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Data {
        selected_idx: Option<usize>,
        scroll_top: usize,
    }
    pub(super) fn serialize<S: serde::Serializer>(
        value: &ScrollState,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        Data {
            selected_idx: value.selected_idx,
            scroll_top: value.scroll_top,
        }
        .serialize(serializer)
    }
    pub(super) fn deserialize<'de, D: serde::Deserializer<'de>>(
        decoder: D,
    ) -> Result<ScrollState, D::Error> {
        let data = Data::deserialize(decoder)?;
        Ok(ScrollState {
            selected_idx: data.selected_idx,
            scroll_top: data.scroll_top,
        })
    }
}

#[derive(Serialize)]
struct DataRef<'a> {
    schema: u32,
    request: &'a ToolRequestUserInputParams,
    queue: &'a VecDeque<ToolRequestUserInputParams>,
    answers: &'a [AnswerState],
    current_idx: usize,
    focus: Focus,
    pending_submission_draft: &'a Option<ComposerDraft>,
    #[serde(with = "optional_scroll")]
    confirm_unanswered: Option<ScrollState>,
    #[serde(with = "codex_alab_tui::upgrade_editor_clock::instant")]
    request_started_at: Instant,
    auto_resolution_snoozed: bool,
    editor: Vec<u8>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Data {
    schema: u32,
    request: ToolRequestUserInputParams,
    queue: VecDeque<ToolRequestUserInputParams>,
    answers: Vec<AnswerState>,
    current_idx: usize,
    focus: Focus,
    pending_submission_draft: Option<ComposerDraft>,
    #[serde(with = "optional_scroll")]
    confirm_unanswered: Option<ScrollState>,
    #[serde(with = "codex_alab_tui::upgrade_editor_clock::instant")]
    request_started_at: Instant,
    auto_resolution_snoozed: bool,
    editor: Vec<u8>,
}
mod optional_scroll {
    use super::*;
    #[derive(Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Data {
        selected_idx: Option<usize>,
        scroll_top: usize,
    }
    pub(super) fn serialize<S: serde::Serializer>(
        value: &Option<ScrollState>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        value
            .map(|state| Data {
                selected_idx: state.selected_idx,
                scroll_top: state.scroll_top,
            })
            .serialize(serializer)
    }
    pub(super) fn deserialize<'de, D: serde::Deserializer<'de>>(
        decoder: D,
    ) -> Result<Option<ScrollState>, D::Error> {
        Ok(
            Option::<Data>::deserialize(decoder)?.map(|data| ScrollState {
                selected_idx: data.selected_idx,
                scroll_top: data.scroll_top,
            }),
        )
    }
}

fn invalid(message: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

pub(crate) struct PreparedUserInput(Data);

/// This helper changes only saved editing DATA. Production's App consumer
/// supplies its mask exclusively from authenticated exact Source callback facts.
pub(crate) fn retain_request_editing(
    original: &serde_json::value::RawValue,
    keep: &[bool],
    max_bytes: usize,
    max_generations: usize,
) -> io::Result<Option<Box<serde_json::value::RawValue>>> {
    let prepared = PreparedUserInput::decode(original, max_bytes, max_generations)?;
    if keep.len() != prepared.0.queue.len() + 1 {
        return Err(invalid("callback mask changed native queue identity count"));
    }
    if !keep[0] {
        return Ok(None);
    }
    let mut value: serde_json::Value = serde_json::from_str(original.get()).map_err(invalid)?;
    let queue = value
        .get_mut("queue")
        .and_then(serde_json::Value::as_array_mut)
        .ok_or_else(|| invalid("captured native request queue is missing"))?;
    let mut index = 1usize;
    queue.retain(|_| {
        let retained = keep[index];
        index += 1;
        retained
    });
    let raw = serde_json::value::to_raw_value(&value).map_err(invalid)?;
    drop(PreparedUserInput::decode(&raw, max_bytes, max_generations)?);
    Ok(Some(raw))
}

pub(super) fn matches_requests(
    actual: &RequestUserInputOverlay,
    requests: &[ToolRequestUserInputParams],
) -> bool {
    let Some((first, rest)) = requests.split_first() else {
        return false;
    };
    let same = |left: &ToolRequestUserInputParams, right: &ToolRequestUserInputParams| matches!((serde_json::to_value(left), serde_json::to_value(right)), (Ok(left), Ok(right)) if left == right);
    !actual.done
        && rest.len() <= actual.queue.len()
        && same(first, &actual.request)
        && rest
            .iter()
            .zip(&actual.queue)
            .all(|(left, right)| same(left, right))
}

pub(super) fn capture(
    actual: &RequestUserInputOverlay,
    max_bytes: usize,
) -> io::Result<crate::bottom_pane::alab_retained_views::ViewData> {
    actual
        .alab_capture_retained_view(max_bytes)
        .map(|original| crate::bottom_pane::alab_retained_views::ViewData::UserInput { original })
}
pub(super) fn install(
    actual: &mut RequestUserInputOverlay,
    data: crate::bottom_pane::alab_retained_views::ViewData,
    owners: &[crate::app::app_server_requests::alab_retained_owners::UserInputOwner<'_>],
    keymap: &RuntimeKeymap,
    max_bytes: usize,
    max_generations: usize,
) -> io::Result<()> {
    match data {
        crate::bottom_pane::alab_retained_views::ViewData::Approval { .. }
        | crate::bottom_pane::alab_retained_views::ViewData::DelayedApproval { .. } => {
            Err(invalid("question installer cannot consume approval DATA"))
        }
        crate::bottom_pane::alab_retained_views::ViewData::UserInput { original } => {
            PreparedUserInput::decode(&original, max_bytes, max_generations)?.install_bound(
                actual,
                owners,
                keymap,
                max_bytes,
                max_generations,
            )
        }
    }
}

impl RequestUserInputOverlay {
    pub(crate) fn alab_capture_retained_view(
        &self,
        max_bytes: usize,
    ) -> io::Result<Box<serde_json::value::RawValue>> {
        if self.done {
            return Err(invalid(
                "completed request view is not a pending callback owner",
            ));
        }
        let editor = self.composer.alab_capture_retained_form_editor(max_bytes)?;
        let raw = serde_json::value::to_raw_value(&DataRef {
            schema: 1,
            request: &self.request,
            queue: &self.queue,
            answers: &self.answers,
            current_idx: self.current_idx,
            focus: self.focus,
            pending_submission_draft: &self.pending_submission_draft,
            confirm_unanswered: self.confirm_unanswered,
            request_started_at: self.request_started_at,
            auto_resolution_snoozed: self.auto_resolution_snoozed,
            editor,
        })
        .map_err(invalid)?;
        if raw.get().len() > max_bytes {
            return Err(invalid("retained request view exceeds E1 budget"));
        }
        Ok(raw)
    }
}

impl PreparedUserInput {
    pub(crate) fn decode(
        raw: &serde_json::value::RawValue,
        max_bytes: usize,
        max_generations: usize,
    ) -> io::Result<Self> {
        if raw.get().len() > max_bytes {
            return Err(invalid("retained request view exceeds E1 budget"));
        }
        let data: Data = serde_json::from_str(raw.get()).map_err(invalid)?;
        if data.schema != 1
            || data.answers.len() != data.request.questions.len()
            || (!data.answers.is_empty() && data.current_idx >= data.answers.len())
            || (data.answers.is_empty() && data.current_idx != 0)
            || data
                .confirm_unanswered
                .as_ref()
                .is_some_and(|state| state.selected_idx.is_some_and(|index| index > 1))
        {
            return Err(invalid(
                "retained form selection differs from its original questions",
            ));
        }
        for (answer, question) in data.answers.iter().zip(&data.request.questions) {
            let count = RequestUserInputOverlay::options_len_for_question(question);
            if answer
                .options_state
                .selected_idx
                .is_some_and(|index| index >= count)
            {
                return Err(invalid(
                    "retained question option is outside original choices",
                ));
            }
        }
        drop(ChatComposer::alab_prepare_retained_form_editor(
            &data.editor,
            max_bytes,
            max_generations,
        )?);
        Ok(Self(data))
    }

    /// The caller supplies the actual view delivered through original listener
    /// replay. This function never constructs a callback/reply/request handle.
    /// All operations are synchronous within the original exclusive UI event.
    fn install(
        self,
        actual: &mut RequestUserInputOverlay,
        actual_keymap: &RuntimeKeymap,
        max_bytes: usize,
        max_generations: usize,
    ) -> io::Result<()> {
        // Actual replay may append new requests after the captured prefix.
        // They and their real callbacks remain owned by the destination queue.
        let original_requests: Vec<_> = std::iter::once(self.0.request.clone())
            .chain(self.0.queue.iter().cloned())
            .collect();
        let same_requests = matches_requests(actual, &original_requests);
        if !same_requests
            || actual.done
            || !actual.composer.is_empty()
            || actual.pending_submission_draft.is_some()
            || actual.confirm_unanswered.is_some()
            || actual
                .answers
                .iter()
                .any(|answer| answer.answer_committed || answer.draft != ComposerDraft::default())
        {
            return Err(invalid(
                "actual replayed request view changed or already owns input",
            ));
        }
        let editor = ChatComposer::alab_prepare_retained_form_editor(
            &self.0.editor,
            max_bytes,
            max_generations,
        )?;
        editor.validate_destination(&actual.composer)?;
        editor.install(&mut actual.composer, actual_keymap)?;
        // Real callback, request queue, sender, keymaps and original recipient
        // registration are untouched; only their matched editing DATA moves.
        actual.answers = self.0.answers;
        actual.current_idx = self.0.current_idx;
        actual.focus = self.0.focus;
        actual.pending_submission_draft = self.0.pending_submission_draft;
        actual.confirm_unanswered = self.0.confirm_unanswered;
        actual.request_started_at = self.0.request_started_at;
        actual.auto_resolution_snoozed = self.0.auto_resolution_snoozed;
        Ok(())
    }

    /// Production consumes real borrowed callback owners, issued only by the
    /// actual App registry after original replay registered the new connection.
    pub(crate) fn install_bound(
        self,
        actual: &mut RequestUserInputOverlay,
        owners: &[crate::app::app_server_requests::alab_retained_owners::UserInputOwner<'_>],
        actual_keymap: &RuntimeKeymap,
        max_bytes: usize,
        max_generations: usize,
    ) -> io::Result<()> {
        let requests: Vec<_> = std::iter::once(&self.0.request)
            .chain(&self.0.queue)
            .collect();
        if owners.len() != requests.len()
            || !owners
                .iter()
                .zip(requests)
                .all(|(owner, request)| owner.matches(request))
        {
            return Err(invalid(
                "retained form lacks its actual replayed callback owners",
            ));
        }
        actual.alab_inputs.validate_owners(owners)?;
        self.install(actual, actual_keymap, max_bytes, max_generations)
    }
}

#[cfg(test)]
#[path = "retained_user_input_view_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "retained_callback_form_tests.rs"]
mod callback_tests;
