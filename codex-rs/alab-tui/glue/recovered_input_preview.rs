// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::bottom_pane::BottomPane;
use crate::key_hint::ShortcutHint;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum Recovery {
    #[default]
    Ordinary,
    Held {
        recallable: bool,
        preparing: bool,
    },
    // Older saved panes contain the texts and bindings but no delivery state.
    #[cfg(unix)]
    Unknown,
}

impl BottomPane {
    pub(crate) fn alab_set_recovered_preview(
        &mut self,
        recovered: bool,
        recallable: bool,
        preparing: bool,
    ) {
        self.pending_input_preview.alab_recovery = if recovered {
            Recovery::Held {
                recallable,
                preparing,
            }
        } else {
            Recovery::Ordinary
        };
    }
}

pub(super) struct Labels<'a> {
    pub(super) pending: &'static str,
    pub(super) interrupt: Option<ShortcutHint>,
    pub(super) rejected: &'static str,
    pub(super) queued_title: &'static str,
    pub(super) queued: &'a [String],
    pub(super) recall: bool,
    preparation: Option<&'a String>,
}

impl PendingInputPreview {
    pub(super) fn alab_queue_labels(&self) -> Labels<'_> {
        let mut labels = Labels {
            pending: "Steer messages",
            interrupt: None,
            rejected: "Returned messages",
            queued_title: "Queued follow-up inputs",
            queued: &self.queued_messages,
            recall: false,
            preparation: None,
        };
        match self.alab_recovery {
            Recovery::Ordinary => {
                labels.pending = "Messages to be submitted after next tool call";
                labels.interrupt = self.interrupt_binding;
                labels.rejected = "Messages to be submitted at end of turn";
                labels.recall = !labels.queued.is_empty();
            }
            Recovery::Held {
                recallable,
                preparing,
            } => {
                labels.pending = "Messages awaiting delivery confirmation";
                labels.rejected = "Held messages (automatic sending paused)";
                labels.recall = recallable;
                if preparing && let Some((first, rest)) = labels.queued.split_first() {
                    labels.preparation = Some(first);
                    labels.queued = rest;
                }
                if !labels.queued.is_empty() {
                    labels.queued_title = "Follow-up inputs (automatic sending paused)";
                }
            }
            #[cfg(unix)]
            Recovery::Unknown => {}
        }
        labels
    }
}

impl Labels<'_> {
    pub(super) fn show_preparation(&self, lines: &mut Vec<HyperlinkLine>, width: u16) {
        if let Some(preparation) = self.preparation {
            if !lines.is_empty() {
                lines.push(HyperlinkLine::new(Line::default()));
            }
            let wrapped = wrap_preview_lines(
                preparation
                    .lines()
                    .take(PREVIEW_LINE_LIMIT + 1)
                    .map(|line| Line::from(line.to_owned().dim())),
                RtOptions::new(width as usize),
            );
            PendingInputPreview::push_truncated_preview_lines(
                lines,
                wrapped,
                Line::from("…".dim()),
            );
        }
    }
}
