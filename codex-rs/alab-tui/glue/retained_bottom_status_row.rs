// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Exact displayed native status; actual sender/frame are supplied by enclosing graphs.
use super::*;
use crate::copy_input_guard::alab_retained_graph as senders;
use crate::history_cell::alab_retained_cells as cells;
use crate::key_hint::alab_retained_keys::Hint;
use codex_alab_tui::upgrade_editor_clock::Snapshot;
use serde::Deserialize;
use serde::Serialize;
use std::io;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Effects {
    starfield: bool,
    shimmer: bool,
    welcome: bool,
    effort: bool,
    progress: bool,
    title: bool,
}
impl Effects {
    pub(crate) fn capture(v: codex_config::types::TuiEffects) -> Self {
        Self {
            starfield: v.starfield,
            shimmer: v.shimmer,
            welcome: v.welcome,
            effort: v.effort,
            progress: v.progress,
            title: v.title,
        }
    }
    pub(crate) fn restore(self) -> codex_config::types::TuiEffects {
        codex_config::types::TuiEffects {
            starfield: self.starfield,
            shimmer: self.shimmer,
            welcome: self.welcome,
            effort: self.effort,
            progress: self.progress,
            title: self.title,
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Data {
    header: String,
    started: Snapshot,
    #[serde(deserialize_with = "required")]
    details: Option<String>,
    max_lines: usize,
    #[serde(deserialize_with = "required")]
    inline: Option<String>,
    #[serde(deserialize_with = "required")]
    hook: Option<String>,
    interrupt: bool,
    #[serde(deserialize_with = "required")]
    binding: Option<Hint>,
    sender: senders::SenderRef,
    frame: cells::FrameRef,
    animations: bool,
    effects: Effects,
}
impl Data {
    pub(crate) fn capture(
        actual: &StatusIndicatorWidget,
        senders: &mut senders::Capture,
        cells: &mut cells::Capture,
    ) -> io::Result<Self> {
        Ok(Self {
            header: actual.header.clone(),
            started: Snapshot::capture(actual.header_started_at).map_err(io::Error::other)?,
            details: actual.details.clone(),
            max_lines: actual.details_max_lines,
            inline: actual.inline_message.clone(),
            hook: actual.hook_status_message.clone(),
            interrupt: actual.show_interrupt_hint,
            binding: actual.interrupt_binding.map(Hint::capture),
            sender: senders.sender(&actual.app_event_tx)?,
            frame: cells.frame(&actual.frame_requester)?,
            animations: actual.animations_enabled,
            effects: Effects::capture(actual.effects),
        })
    }
    pub(crate) fn restore(
        self,
        senders: &senders::Restored,
        cells: &cells::RestoreContext,
    ) -> io::Result<StatusIndicatorWidget> {
        if self.max_lines == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "native status details max lines is zero",
            ));
        }
        Ok(StatusIndicatorWidget {
            header: self.header,
            header_started_at: self.started.restore().map_err(io::Error::other)?,
            details: self.details,
            details_max_lines: self.max_lines,
            inline_message: self.inline,
            hook_status_message: self.hook,
            show_interrupt_hint: self.interrupt,
            interrupt_binding: self.binding.map(Hint::restore).transpose()?,
            app_event_tx: senders.sender(self.sender)?,
            frame_requester: cells.frame(self.frame)?,
            animations_enabled: self.animations,
            effects: self.effects.restore(),
        })
    }
}
fn required<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Option<T>, D::Error> {
    Option::<T>::deserialize(d)
}
#[cfg(test)]
mod controls {
    use super::*;
    use crate::app_command::AppCommand;
    use crate::app_event::AppEvent;
    use std::sync::Arc;
    #[test]
    fn original_status_fields_and_interrupt_use_same_bound_channel_voice_and_frame()
    -> io::Result<()> {
        let (source_tx, mut source_events) = tokio::sync::mpsc::unbounded_channel();
        let source_sender = AppEventSender::new(source_tx);
        let (source_frame, _source_frames) = FrameRequester::test_channel();
        let mut original = StatusIndicatorWidget::new(
            source_sender,
            source_frame,
            true,
            codex_config::types::TuiEffects::default(),
        );
        original.update_header("Exact original status".into());
        original.update_details(
            Some("  exact lower-case details".into()),
            StatusDetailsCapitalization::Preserve,
            7,
        );
        original.update_inline_message(Some(" background process".into()));
        original.update_hook_status_message(Some("same running hook".into()));
        original.set_interrupt_hint_visible(false);
        let mut sender_capture = senders::Capture::default();
        let mut cell_capture = cells::Capture::default();
        let data = Data::capture(&original, &mut sender_capture, &mut cell_capture)?;
        let sender_ref = data.sender;
        let frame_ref = data.frame;
        let wire = serde_json::to_vec(&data).map_err(io::Error::other)?;
        let data: Data = serde_json::from_slice(&wire).map_err(io::Error::other)?;
        let (target_tx, mut target_events) = tokio::sync::mpsc::unbounded_channel();
        let actual_sender = AppEventSender::new(target_tx);
        let (actual_frame, _target_frames) = FrameRequester::test_channel();
        let mut senders = senders::Prepared::decode(&sender_capture.encode(1_048_576)?, 1_048_576)?;
        senders.bind(sender_ref, &actual_sender)?;
        let senders = senders.restore()?;
        let mut text = crate::terminal_hyperlinks::alab_retained_text::Capture::default();
        let bytes = cell_capture.encode(&mut text, 1_048_576)?;
        let mut text =
            crate::terminal_hyperlinks::alab_retained_text::Restored::new(text.export()?)?;
        let mut context = cells::RestoreContext::default();
        context.bind_frame(frame_ref, &actual_frame)?;
        let _cells = cells::Restored::decode(&bytes, 1_048_576, &mut text, &mut context)?;
        let restored = data.restore(&senders, &context)?;
        assert_eq!(restored.header, original.header);
        assert_eq!(restored.details, original.details);
        assert_eq!(restored.details_max_lines, 7);
        assert_eq!(restored.inline_message, original.inline_message);
        assert_eq!(restored.hook_status_message, original.hook_status_message);
        assert!(!restored.show_interrupt_hint);
        assert_eq!(restored.interrupt_binding, original.interrupt_binding);
        assert_eq!(restored.effects, original.effects);
        assert!(restored.frame_requester.alab_same_scheduler(&actual_frame));
        assert!(
            restored
                .app_event_tx
                .app_event_tx
                .same_channel(&actual_sender.app_event_tx)
        );
        assert!(Arc::ptr_eq(
            &restored.app_event_tx.voice_only,
            &actual_sender.voice_only
        ));
        assert!(target_events.try_recv().is_err());
        restored.interrupt();
        assert!(matches!(
            target_events.try_recv(),
            Ok(AppEvent::CodexOp(AppCommand::Interrupt))
        ));
        assert!(source_events.try_recv().is_err());
        Ok(())
    }
}
