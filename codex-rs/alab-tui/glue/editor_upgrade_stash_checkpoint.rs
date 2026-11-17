// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! One data checkpoint for the visible native draft and every per-thread stash.
//! Call at the real synchronous UI capture boundary. This is no SDK/input grant.
use super::super::super::*;
use super::SavedPrompt;
use super::prompts;
use crate::bottom_pane::BottomPane;
use crate::bottom_pane::textarea::TextArea;
use crate::bottom_pane::textarea::TextElementSnapshot;
use codex_alab_tui::upgrade_editor_graph_scope as graph;
use codex_protocol::ThreadId;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::io;

const VERSION: u32 = 1;

#[derive(Serialize)]
struct VisibleRef<'a> {
    draft: &'a DraftState,
    attachments: &'a AttachmentState,
    register: &'a vim_register::VimRegister,
    history: &'a vim_history::VimHistory,
    pending_slash_history: &'a Option<crate::bottom_pane::chat_composer_history::HistoryEntry>,
    sparkle: sparkle::SparkleDraft,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Visible {
    draft: DraftState,
    attachments: AttachmentState,
    register: vim_register::VimRegister,
    history: vim_history::VimHistory,
    pending_slash_history: Option<crate::bottom_pane::chat_composer_history::HistoryEntry>,
    sparkle: sparkle::SparkleDraft,
}
#[derive(Serialize)]
struct CheckpointRef<'a> {
    version: u32,
    primary: ThreadId,
    visible: VisibleRef<'a>,
    stashed: &'a HashMap<ThreadId, SavedPrompt>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Checkpoint {
    version: u32,
    primary: ThreadId,
    visible: Visible,
    #[serde(deserialize_with = "unique_slots")]
    stashed: HashMap<ThreadId, SavedPrompt>,
}
fn unique_slots<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<HashMap<ThreadId, SavedPrompt>, D::Error> {
    struct Slots;
    impl<'de> serde::de::Visitor<'de> for Slots {
        type Value = HashMap<ThreadId, SavedPrompt>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("unique per-thread native draft slots")
        }
        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            mut map: A,
        ) -> Result<Self::Value, A::Error> {
            let mut slots = HashMap::new();
            while let Some((thread, draft)) = map.next_entry::<ThreadId, SavedPrompt>()? {
                if slots.insert(thread, draft).is_some() {
                    return Err(serde::de::Error::custom(
                        "duplicate native draft thread slot",
                    ));
                }
            }
            Ok(slots)
        }
    }
    deserializer.deserialize_map(Slots)
}

pub(crate) struct PreparedDraftCheckpoint {
    checkpoint: Checkpoint,
}
fn invalid(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}
fn capture(
    pane: &BottomPane,
    stashed: &HashMap<ThreadId, SavedPrompt>,
    max_bytes: usize,
) -> io::Result<Vec<u8>> {
    let primary = pane
        .thread_id
        .ok_or_else(|| invalid("draft capture has no native primary thread"))?;
    let composer = &pane.composer;
    graph::encode::<Vec<TextElementSnapshot>>(
        &CheckpointRef {
            version: VERSION,
            primary,
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
fn prepare(
    bytes: &[u8],
    expected_primary: ThreadId,
    max_bytes: usize,
    max_generations: usize,
) -> io::Result<PreparedDraftCheckpoint> {
    let checkpoint: Checkpoint =
        graph::decode::<Vec<TextElementSnapshot>, _>(bytes, max_bytes, max_generations)
            .map_err(invalid)?;
    if checkpoint.version != VERSION || checkpoint.primary != expected_primary {
        return Err(invalid(
            "draft checkpoint version or primary routing differs",
        ));
    }
    Ok(PreparedDraftCheckpoint { checkpoint })
}
fn configure(editor: &mut TextArea, keymap: &RuntimeKeymap) {
    editor.set_keymap_bindings(keymap);
    if let Some(query) = editor.vim_query_mut() {
        configure(&mut query.editor, keymap);
    }
}
impl PreparedDraftCheckpoint {
    fn install_into(
        mut self,
        pane: &mut BottomPane,
        stashed: &mut HashMap<ThreadId, SavedPrompt>,
    ) -> io::Result<()> {
        // Every fallible condition precedes mutation of either native owner.
        if pane.thread_id != Some(self.checkpoint.primary) || !stashed.is_empty() {
            return Err(invalid(
                "draft destination changed or already owns stash slots",
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

    pub(crate) fn install(self, pane: &mut BottomPane) -> io::Result<()> {
        let mut stashed = prompts()
            .lock()
            .map_err(|_| invalid("native stash owner is poisoned"))?;
        self.install_into(pane, &mut stashed)
    }
}
impl BottomPane {
    pub(crate) fn alab_draft_has_layout(&self) -> bool {
        self.composer.draft.textarea.alab_upgrade_has_layout()
    }

    pub(crate) fn alab_capture_draft_checkpoint(&self, max_bytes: usize) -> io::Result<Vec<u8>> {
        let stashed = prompts()
            .lock()
            .map_err(|_| invalid("native stash owner is poisoned"))?;
        capture(self, &stashed, max_bytes)
    }
    pub(crate) fn alab_prepare_draft_checkpoint(
        bytes: &[u8],
        expected_primary: ThreadId,
        max_bytes: usize,
        max_generations: usize,
    ) -> io::Result<PreparedDraftCheckpoint> {
        prepare(bytes, expected_primary, max_bytes, max_generations)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_event_sender::AppEventSender;
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;
    use std::time::Duration;

    const BUDGET: usize = 1024 * 1024;
    const GENERATIONS: usize = 1024;
    fn pane(thread: ThreadId) -> BottomPane {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut pane = crate::bottom_pane::tests::test_pane(AppEventSender::new(tx));
        pane.set_history_metadata(thread, 0, 0);
        pane.composer.set_disable_paste_burst(true);
        pane.composer.set_vim_enabled(true);
        pane
    }
    fn fill(pane: &mut BottomPane, text: &str) {
        let composer = &mut pane.composer;
        composer.set_text_content(format!("{text} "), Vec::new(), Vec::new());
        composer.move_cursor_to_end();
        composer.insert_selected_mention(0..0, "$skill", Some("/original/skill/SKILL.md"));
        composer.attach_image(PathBuf::from(format!("{text}.png")));
        composer.handle_paste(format!("{text}\n").repeat(400));
        composer.popups.active = ActivePopup::None;
        composer.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        composer.handle_key_event(KeyEvent::new(KeyCode::Char('0'), KeyModifiers::NONE));
    }
    fn snapshot(pane: &BottomPane) -> serde_json::Value {
        serde_json::to_value(pane.composer.snapshot_draft()).unwrap()
    }
    fn source(
        primary: ThreadId,
        secondary: ThreadId,
    ) -> (BottomPane, HashMap<ThreadId, SavedPrompt>) {
        let mut pane = pane(primary);
        fill(&mut pane, "first");
        for ch in ['y', '$'] {
            pane.composer
                .handle_key_event(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        let first = pane.composer.alab_take_prompt(&pane.keymap);
        pane.set_history_metadata(secondary, 0, 0);
        fill(&mut pane, "second");
        let second = pane.composer.alab_take_prompt(&pane.keymap);
        pane.set_history_metadata(primary, 0, 0);
        fill(&mut pane, "current");
        (pane, HashMap::from([(primary, first), (secondary, second)]))
    }

    #[test]
    fn native_draft_checkpoint_stages_all_slots_and_refuses_conflicts_without_mutation() {
        let primary = ThreadId::new();
        let secondary = ThreadId::new();
        let (source, slots) = source(primary, secondary);
        let before = snapshot(&source);
        let bytes = capture(&source, &slots, BUDGET).unwrap();
        assert_eq!(snapshot(&source), before);
        assert_eq!(slots.len(), 2);
        assert!(prepare(&bytes, secondary, BUDGET, GENERATIONS).is_err());

        let mut wrong = pane(secondary);
        let wrong_before = snapshot(&wrong);
        let mut empty = HashMap::new();
        assert!(
            prepare(&bytes, primary, BUDGET, GENERATIONS)
                .unwrap()
                .install_into(&mut wrong, &mut empty)
                .is_err()
        );
        assert_eq!(snapshot(&wrong), wrong_before);
        assert!(empty.is_empty());

        let mut destination = pane(primary);
        let mut occupied_pane = pane(ThreadId::new());
        fill(&mut occupied_pane, "untouched");
        let occupied = occupied_pane
            .composer
            .alab_take_prompt(&occupied_pane.keymap);
        let mut occupied_slots = HashMap::from([(secondary, occupied)]);
        let occupied_before = serde_json::to_value(&occupied_slots[&secondary]).unwrap();
        let destination_before = snapshot(&destination);
        assert!(
            prepare(&bytes, primary, BUDGET, GENERATIONS)
                .unwrap()
                .install_into(&mut destination, &mut occupied_slots)
                .is_err()
        );
        assert_eq!(snapshot(&destination), destination_before);
        assert_eq!(
            serde_json::to_value(&occupied_slots[&secondary]).unwrap(),
            occupied_before
        );

        let wire: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let mut invalid_slot = wire.clone();
        invalid_slot["body"]["stashed"][secondary.to_string()]["draft"]["textarea"]["cursor_pos"] =
            u64::MAX.into();
        assert!(
            prepare(
                &serde_json::to_vec(&invalid_slot).unwrap(),
                primary,
                BUDGET,
                GENERATIONS
            )
            .is_err()
        );
        let original_slots = serde_json::to_string(&wire["body"]["stashed"]).unwrap();
        let (thread, draft) = wire["body"]["stashed"]
            .as_object()
            .unwrap()
            .iter()
            .next()
            .unwrap();
        let thread = serde_json::to_string(thread).unwrap();
        let duplicate = format!("{{{thread}:{draft},{thread}:{draft}}}");
        let body = serde_json::to_string(&wire["body"]).unwrap().replace(
            &format!("\"stashed\":{original_slots}"),
            &format!("\"stashed\":{duplicate}"),
        );
        let duplicate = format!(
            "{{\"generations\":{},\"body\":{body}}}",
            wire["generations"]
        );
        let error = prepare(duplicate.as_bytes(), primary, BUDGET, GENERATIONS)
            .err()
            .expect("duplicate native thread slot accepted");
        assert!(
            error.to_string().contains("duplicate native draft"),
            "{error}"
        );
        assert_eq!(snapshot(&destination), destination_before);

        prepare(&bytes, primary, BUDGET, GENERATIONS)
            .unwrap()
            .install_into(&mut destination, &mut empty)
            .unwrap();
        assert_eq!(snapshot(&destination), before);
        assert_eq!(empty.len(), 2);
        assert!(empty[&primary].draft.textarea.text().contains("first"));
        assert!(empty[&secondary].draft.textarea.text().contains("second"));
        // The visible weak payload map must still reach the generation owned only by a stash.
        let saved = empty.remove(&primary).unwrap();
        destination
            .composer
            .alab_restore_prompt(saved, &destination.keymap);
        destination
            .composer
            .handle_key_event(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE));
        assert_eq!(destination.composer.attachments.local_images.len(), 2);
        assert_eq!(destination.composer.draft.pending_pastes.len(), 2);
        assert_eq!(destination.composer.draft.mention_bindings.len(), 2);
    }

    #[tokio::test]
    async fn native_draft_and_all_stash_slots_survive_an_actual_same_pid_exec() {
        use std::io::Read;
        use std::io::Seek;
        use std::io::Write;
        use std::os::fd::AsRawFd;
        use std::os::fd::FromRawFd;
        use std::os::unix::process::CommandExt;
        const STAGE: &str = "ALAB_DRAFT_CHECKPOINT_EXEC_STAGE";
        const FD: &str = "ALAB_DRAFT_CHECKPOINT_EXEC_FD";
        const PRIMARY: &str = "ALAB_DRAFT_CHECKPOINT_EXEC_PRIMARY";
        const SECONDARY: &str = "ALAB_DRAFT_CHECKPOINT_EXEC_SECONDARY";
        const PID: &str = "ALAB_DRAFT_CHECKPOINT_EXEC_PID";
        const TEST: &str = "bottom_pane::chat_composer::inline_input::alab_stash::draft_checkpoint::tests::native_draft_and_all_stash_slots_survive_an_actual_same_pid_exec";
        const SUCCESS: &str = "ALAB_DRAFT_CHECKPOINT_EXEC_OK";
        match std::env::var(STAGE).ok().as_deref() {
            None => {
                let output = tokio::time::timeout(
                    Duration::from_secs(30),
                    tokio::process::Command::new(std::env::current_exe().unwrap())
                        .args(["--exact", TEST, "--nocapture"])
                        .env(STAGE, "source")
                        .kill_on_drop(true)
                        .output(),
                )
                .await
                .unwrap()
                .unwrap();
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                assert!(String::from_utf8_lossy(&output.stdout).contains(SUCCESS));
            }
            Some("source") => {
                let primary = ThreadId::new();
                let secondary = ThreadId::new();
                let (pane, slots) = source(primary, secondary);
                *prompts().lock().unwrap() = slots;
                let bytes = pane.alab_capture_draft_checkpoint(BUDGET).unwrap();
                assert_eq!(prompts().lock().unwrap().len(), 2);
                let mut file = tempfile::tempfile().unwrap();
                file.write_all(&bytes).unwrap();
                file.rewind().unwrap();
                let fd = file.as_raw_fd();
                // SAFETY: this fixture owns the open, unlinked data file. Keep only this
                // descriptor across exec; it carries bytes, never native continuation authority.
                let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
                assert!(flags >= 0);
                assert_eq!(
                    unsafe { libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) },
                    0
                );
                let error = std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", TEST, "--nocapture"])
                    .env(STAGE, "destination")
                    .env(FD, fd.to_string())
                    .env(PID, std::process::id().to_string())
                    .env(PRIMARY, primary.to_string())
                    .env(SECONDARY, secondary.to_string())
                    .exec();
                panic!("native draft fixture exec failed: {error}");
            }
            Some("destination") => {
                assert_eq!(std::env::var(PID).unwrap(), std::process::id().to_string());
                assert!(prompts().lock().unwrap().is_empty());
                let fd = std::env::var(FD).unwrap().parse().unwrap();
                // SAFETY: the source fixture transferred sole ownership of this inherited fd.
                let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
                let mut bytes = Vec::new();
                file.read_to_end(&mut bytes).unwrap();
                let primary: ThreadId =
                    ThreadId::from_string(&std::env::var(PRIMARY).unwrap()).unwrap();
                let secondary: ThreadId =
                    ThreadId::from_string(&std::env::var(SECONDARY).unwrap()).unwrap();
                let mut pane = pane(primary);
                BottomPane::alab_prepare_draft_checkpoint(&bytes, primary, BUDGET, GENERATIONS)
                    .unwrap()
                    .install(&mut pane)
                    .unwrap();
                assert!(pane.composer.current_text().contains("current"));
                let saved = {
                    let mut slots = prompts().lock().unwrap();
                    assert_eq!(slots.len(), 2);
                    assert!(slots[&secondary].draft.textarea.text().contains("second"));
                    slots.remove(&primary).unwrap()
                };
                pane.composer.alab_restore_prompt(saved, &pane.keymap);
                assert!(pane.composer.current_text().contains("first"));
                pane.composer
                    .handle_key_event(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE));
                assert_eq!(pane.composer.attachments.local_images.len(), 2);
                assert_eq!(pane.composer.draft.pending_pastes.len(), 2);
                assert_eq!(pane.composer.draft.mention_bindings.len(), 2);
                std::io::stdout()
                    .write_all(format!("{SUCCESS}\n").as_bytes())
                    .unwrap();
            }
            _ => panic!("unexpected draft exec fixture stage"),
        }
    }
}

#[path = "editor_no_primary_checkpoint.rs"]
mod no_primary; // ALAB(retained-ui): reuse the actual rich editor codec without a fabricated primary identity.

#[cfg(unix)]
#[path = "retained_no_primary_editor.rs"]
mod retained_no_primary;

#[cfg(unix)]
#[path = "retained_form_editor.rs"]
mod retained_form_editor;
