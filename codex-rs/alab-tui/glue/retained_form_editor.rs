// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual plain-form ChatComposer editor/search graph. AppEvent sender, keymap,
//! callback, frame owner and form request stay with the real replayed view.
//! Child of the original rich stash codec; shares its native generation graph.
use super::*;

#[derive(Serialize)]
struct DataRef<'a> {
    version: u32,
    visible: VisibleRef<'a>,
    history: &'a crate::bottom_pane::chat_composer_history::ChatComposerHistory,
    search: &'a Option<super::super::super::super::history_search::HistorySearchSession>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Data {
    version: u32,
    visible: Visible,
    history: crate::bottom_pane::chat_composer_history::ChatComposerHistory,
    search: Option<super::super::super::super::history_search::HistorySearchSession>,
}

pub(crate) struct PreparedFormEditor(Data);

impl ChatComposer {
    pub(crate) fn alab_capture_retained_form_editor(
        &self,
        max_bytes: usize,
    ) -> io::Result<Vec<u8>> {
        if self.config.popups_enabled
            || self.config.slash_commands_enabled
            || !matches!(self.popups.active, ActivePopup::None)
        {
            return Err(invalid("form editor still owns a non-form popup"));
        }
        graph::encode::<Vec<TextElementSnapshot>>(
            &DataRef {
                version: VERSION,
                visible: VisibleRef {
                    draft: &self.draft,
                    attachments: &self.attachments,
                    register: &self.vim_register,
                    history: &self.vim_history,
                    pending_slash_history: &self.pending_slash_command_history,
                    sparkle: self.sparkle.draft.get(),
                },
                history: &self.history,
                search: &self.history_search,
            },
            max_bytes,
        )
        .map_err(invalid)
    }
    pub(crate) fn alab_prepare_retained_form_editor(
        bytes: &[u8],
        max_bytes: usize,
        max_generations: usize,
    ) -> io::Result<PreparedFormEditor> {
        let data: Data =
            graph::decode::<Vec<TextElementSnapshot>, _>(bytes, max_bytes, max_generations)
                .map_err(invalid)?;
        if data.version != VERSION {
            return Err(invalid("retained form editor version differs"));
        }
        data.history.alab_validate_retained()?;
        Ok(PreparedFormEditor(data))
    }
}
impl PreparedFormEditor {
    pub(crate) fn validate_destination(&self, composer: &ChatComposer) -> io::Result<()> {
        if composer.config.popups_enabled
            || composer.config.slash_commands_enabled
            || !matches!(composer.popups.active, ActivePopup::None)
        {
            return Err(invalid(
                "retained editor destination is not the original plain form",
            ));
        }
        Ok(())
    }
    pub(crate) fn install(
        mut self,
        composer: &mut ChatComposer,
        actual_keymap: &RuntimeKeymap,
    ) -> io::Result<()> {
        self.validate_destination(composer)?;
        configure(&mut self.0.visible.draft.textarea, actual_keymap);
        let visible = self.0.visible;
        composer.draft = visible.draft;
        composer.attachments = visible.attachments;
        composer.vim_register = visible.register;
        composer.vim_history = visible.history;
        composer.pending_slash_command_history = visible.pending_slash_history;
        composer.sparkle.draft.set(visible.sparkle);
        composer.history = self.0.history;
        composer.history_search = self.0.search;
        Ok(())
    }
}
