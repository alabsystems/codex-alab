// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Production retained rich provisional editor DATA; no fake primary identity.
//! Child of the existing checkpoint codec; no input, Source or Native grant.
use super::*;
use crate::bottom_pane::alab_retained_delayed_approval::PreparedDelayed;
const NO_PRIMARY_VERSION: u32 = 2;

#[derive(Serialize)]
struct NoPrimaryRef<'a> {
    version: u32,
    visible: VisibleRef<'a>,
    stashed: &'a HashMap<ThreadId, SavedPrompt>,
    local_history: &'a [crate::bottom_pane::chat_composer_history::HistoryEntry],
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NoPrimary {
    version: u32,
    visible: Visible,
    #[serde(deserialize_with = "unique_slots")]
    stashed: HashMap<ThreadId, SavedPrompt>,
    local_history: Vec<crate::bottom_pane::chat_composer_history::HistoryEntry>,
}
pub(crate) struct PreparedNoPrimary {
    checkpoint: NoPrimary,
}
fn capture_no_primary(
    pane: &BottomPane,
    stashed: &HashMap<ThreadId, SavedPrompt>,
    max_bytes: usize,
) -> io::Result<Vec<u8>> {
    if pane.thread_id.is_some() {
        return Err(invalid("provisional editor already has a primary"));
    }
    let composer = &pane.composer;
    graph::encode::<Vec<TextElementSnapshot>>(
        &NoPrimaryRef {
            version: NO_PRIMARY_VERSION,
            visible: VisibleRef {
                draft: &composer.draft,
                attachments: &composer.attachments,
                register: &composer.vim_register,
                history: &composer.vim_history,
                pending_slash_history: &composer.pending_slash_command_history,
                sparkle: composer.sparkle.draft.get(),
            },
            stashed,
            local_history: composer.history.startup_local_history(),
        },
        max_bytes,
    )
    .map_err(invalid)
}
impl PreparedNoPrimary {
    pub(crate) fn install_with_clock(
        self,
        pane: &mut BottomPane,
        clock: Option<&PreparedDelayed>,
    ) -> io::Result<()> {
        let mut stashed = prompts()
            .lock()
            .map_err(|_| invalid("native stash owner is poisoned"))?;
        self.install_with_stash(pane, &mut stashed, clock)
    }
    #[cfg(test)]
    fn install_into(
        self,
        pane: &mut BottomPane,
        stashed: &mut HashMap<ThreadId, SavedPrompt>,
    ) -> io::Result<()> {
        self.install_with_stash(pane, stashed, None)
    }
    fn install_with_stash(
        mut self,
        pane: &mut BottomPane,
        stashed: &mut HashMap<ThreadId, SavedPrompt>,
        clock: Option<&PreparedDelayed>,
    ) -> io::Result<()> {
        if pane.thread_id.is_some()
            || !stashed.is_empty()
            || !pane.composer.is_empty()
            || pane.composer.is_in_paste_burst()
            || pane.alab_delay_activity.local_value()? != 0
            || !pane.composer.history.startup_local_history().is_empty()
        {
            return Err(invalid(
                "provisional editor destination changed or owns stash slots",
            ));
        }
        configure(&mut self.checkpoint.visible.draft.textarea, &pane.keymap);
        for saved in self.checkpoint.stashed.values_mut() {
            configure(&mut saved.draft.textarea, &pane.keymap);
        }
        // Hold the validated stash through the clock's final check. From the first
        // clock mutation onward, installing the editor and its stash cannot refuse.
        if let Some(clock) = clock {
            if pane.has_pending_approval() || pane.has_active_modal() {
                return Err(invalid(
                    "provisional editor restore must precede callback replay",
                ));
            }
            clock.prime_before_original_replay(pane)?;
        }
        let visible = self.checkpoint.visible;
        pane.composer.draft = visible.draft;
        pane.composer.attachments = visible.attachments;
        pane.composer.vim_register = visible.register;
        pane.composer.vim_history = visible.history;
        pane.composer.pending_slash_command_history = visible.pending_slash_history;
        pane.composer.sparkle.draft.set(visible.sparkle);
        *stashed = self.checkpoint.stashed;
        pane.composer
            .restore_startup_local_history(self.checkpoint.local_history);
        Ok(())
    }
}
impl BottomPane {
    /// Move the actual provisional editor after its final original input poll.
    /// The destination keeps its event sender, effective policy, callback views
    /// and persistent-history metadata. Every fallible check precedes any move.
    pub(crate) fn alab_handoff_retained_no_primary_editor(
        &mut self,
        source: &mut BottomPane,
    ) -> io::Result<()> {
        if source.thread_id.is_some()
            || self.alab_delay_activity.local_value()? != 0
            || !self.composer.is_empty()
            || self.composer.history_search.is_some()
            || self.composer.popups.active()
            || source.composer.history_search.is_some()
        {
            return Err(invalid(
                "original startup editor needs native recovery before handoff",
            ));
        }
        // This transfer validates the actual shared clock and leaves it live
        // on the widget. It never inspects or replaces the widget view stack.
        self.alab_transfer_delayed_clock_from(source)?;
        configure(&mut source.composer.draft.textarea, &self.keymap);
        // Runtime input policy belongs to the actual installed destination.
        // Exchange only those policy fields before moving the native draft.
        std::mem::swap(
            &mut self.composer.draft.input_enabled,
            &mut source.composer.draft.input_enabled,
        );
        std::mem::swap(
            &mut self.composer.draft.input_disabled_placeholder,
            &mut source.composer.draft.input_disabled_placeholder,
        );
        std::mem::swap(
            &mut self.composer.draft.disable_paste_burst,
            &mut source.composer.draft.disable_paste_burst,
        );
        std::mem::swap(&mut self.composer.draft, &mut source.composer.draft);
        std::mem::swap(
            &mut self.composer.attachments,
            &mut source.composer.attachments,
        );
        std::mem::swap(
            &mut self.composer.vim_register,
            &mut source.composer.vim_register,
        );
        std::mem::swap(
            &mut self.composer.vim_history,
            &mut source.composer.vim_history,
        );
        std::mem::swap(
            &mut self.composer.pending_slash_command_history,
            &mut source.composer.pending_slash_command_history,
        );
        // Original startup recall merges into the destination's actual
        // persistent-history owner through its unchanged canonical policy.
        self.composer.restore_startup_local_history(
            source.composer.history.startup_local_history().to_vec(),
        );
        let sparkle = self
            .composer
            .sparkle
            .draft
            .replace(source.composer.sparkle.draft.get());
        source.composer.sparkle.draft.set(sparkle);
        Ok(())
    }

    pub(crate) fn alab_capture_retained_no_primary_checkpoint(
        &self,
        max_bytes: usize,
    ) -> io::Result<Vec<u8>> {
        let stashed = prompts()
            .lock()
            .map_err(|_| invalid("native stash owner is poisoned"))?;
        capture_no_primary(self, &stashed, max_bytes)
    }
    pub(crate) fn alab_prepare_retained_no_primary_checkpoint(
        bytes: &[u8],
        max_bytes: usize,
        max_generations: usize,
    ) -> io::Result<PreparedNoPrimary> {
        let checkpoint: NoPrimary =
            graph::decode::<Vec<TextElementSnapshot>, _>(bytes, max_bytes, max_generations)
                .map_err(invalid)?;
        if checkpoint.version != NO_PRIMARY_VERSION {
            return Err(invalid("provisional editor checkpoint version differs"));
        }
        Ok(PreparedNoPrimary { checkpoint })
    }
}

#[cfg(test)]
#[path = "retained_no_primary_editor_tests.rs"]
mod tests;
