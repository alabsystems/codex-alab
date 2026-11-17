// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Rich provisional editor DATA without inventing a primary Core identity.
//! Child of the existing checkpoint codec; no input, Source or Native grant.
use super::*;

#[derive(Serialize)]
struct NoPrimaryRef<'a> {
    version: u32,
    visible: VisibleRef<'a>,
    stashed: &'a HashMap<ThreadId, SavedPrompt>,
}
#[cfg(test)]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NoPrimary {
    version: u32,
    visible: Visible,
    #[serde(deserialize_with = "unique_slots")]
    stashed: HashMap<ThreadId, SavedPrompt>,
}
#[cfg(test)]
struct PreparedNoPrimary {
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
            version: VERSION,
            visible: VisibleRef {
                draft: &composer.draft,
                attachments: &composer.attachments,
                register: &composer.vim_register,
                history: &composer.vim_history,
                pending_slash_history: &composer.pending_slash_command_history,
                sparkle: composer.sparkle.draft.get(),
            },
            stashed,
        },
        max_bytes,
    )
    .map_err(invalid)
}
#[cfg(test)]
impl PreparedNoPrimary {
    fn install_into(
        mut self,
        pane: &mut BottomPane,
        stashed: &mut HashMap<ThreadId, SavedPrompt>,
    ) -> io::Result<()> {
        if pane.thread_id.is_some() || !stashed.is_empty() {
            return Err(invalid(
                "provisional editor destination changed or owns stash slots",
            ));
        }
        configure(&mut self.checkpoint.visible.draft.textarea, &pane.keymap);
        for saved in self.checkpoint.stashed.values_mut() {
            configure(&mut saved.draft.textarea, &pane.keymap);
        }
        let visible = self.checkpoint.visible;
        pane.composer.draft = visible.draft;
        pane.composer.attachments = visible.attachments;
        pane.composer.vim_register = visible.register;
        pane.composer.vim_history = visible.history;
        pane.composer.pending_slash_command_history = visible.pending_slash_history;
        pane.composer.sparkle.draft.set(visible.sparkle);
        *stashed = self.checkpoint.stashed;
        Ok(())
    }
}
impl BottomPane {
    pub(crate) fn alab_capture_no_primary_checkpoint(
        &self,
        max_bytes: usize,
    ) -> io::Result<Vec<u8>> {
        let stashed = prompts()
            .lock()
            .map_err(|_| invalid("native stash owner is poisoned"))?;
        capture_no_primary(self, &stashed, max_bytes)
    }
    #[cfg(test)]
    fn alab_prepare_no_primary_checkpoint(
        bytes: &[u8],
        max_bytes: usize,
        max_generations: usize,
    ) -> io::Result<PreparedNoPrimary> {
        let checkpoint: NoPrimary =
            graph::decode::<Vec<TextElementSnapshot>, _>(bytes, max_bytes, max_generations)
                .map_err(invalid)?;
        if checkpoint.version != VERSION {
            return Err(invalid("provisional editor checkpoint version differs"));
        }
        Ok(PreparedNoPrimary { checkpoint })
    }
}

#[cfg(test)]
#[path = "editor_no_primary_checkpoint_tests.rs"]
mod tests;
