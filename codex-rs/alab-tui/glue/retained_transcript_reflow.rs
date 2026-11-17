// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Preserve scheduling facts; decoding never repairs or emits terminal history.
use super::*;
use crate::streaming::alab_retained::invalid;
use crate::streaming::alab_retained::required;
use codex_alab_tui::upgrade_editor_clock::Snapshot;
use serde::Deserialize;
use serde::Serialize;
use std::io;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Data {
    #[serde(deserialize_with = "required")]
    last_observed_width: Option<u16>,
    #[serde(deserialize_with = "required")]
    last_reflow_width: Option<u16>,
    #[serde(deserialize_with = "required")]
    pending_reflow_width: Option<u16>,
    #[serde(deserialize_with = "required")]
    pending_until: Option<Snapshot>,
    #[serde(deserialize_with = "required")]
    visible_history_rows: Option<u16>,
    ran_during_stream: bool,
    resize_requested_during_stream: bool,
}
impl Data {
    pub(crate) fn capture(actual: &TranscriptReflowState) -> io::Result<Self> {
        let TranscriptReflowState {
            last_observed_width,
            last_reflow_width,
            pending_reflow_width,
            pending_until,
            visible_history_rows,
            ran_during_stream,
            resize_requested_during_stream,
        } = actual;
        Ok(Self {
            last_observed_width: *last_observed_width,
            last_reflow_width: *last_reflow_width,
            pending_reflow_width: *pending_reflow_width,
            pending_until: pending_until
                .map(Snapshot::capture)
                .transpose()
                .map_err(invalid)?,
            visible_history_rows: *visible_history_rows,
            ran_during_stream: *ran_during_stream,
            resize_requested_during_stream: *resize_requested_during_stream,
        })
    }
    pub(crate) fn restore(self) -> io::Result<TranscriptReflowState> {
        Ok(TranscriptReflowState {
            last_observed_width: self.last_observed_width,
            last_reflow_width: self.last_reflow_width,
            pending_reflow_width: self.pending_reflow_width,
            pending_until: self
                .pending_until
                .map(|x| x.restore())
                .transpose()
                .map_err(invalid)?,
            visible_history_rows: self.visible_history_rows,
            ran_during_stream: self.ran_during_stream,
            resize_requested_during_stream: self.resize_requested_during_stream,
        })
    }
}
#[cfg(test)]
mod controls {
    use super::*;
    #[test]
    fn retained_original_resize_debounce_and_post_stream_repair_survive_without_rendering()
    -> io::Result<()> {
        let mut actual = TranscriptReflowState::default();
        actual.note_width(80);
        actual.mark_reflowed_width(80);
        actual.set_visible_history_rows(17);
        actual.schedule_debounced(Some(43));
        actual.mark_resize_requested_during_stream();
        let data = Data::capture(&actual)?;
        let raw = serde_json::to_vec(&data).map_err(invalid)?;
        let decoded: Data = serde_json::from_slice(&raw).map_err(invalid)?;
        let mut restored = decoded.restore()?;
        assert_eq!(restored.visible_history_rows(), Some(17));
        assert!(restored.has_pending_reflow());
        assert_eq!(
            restored.reflow_needed_for_width(43),
            actual.reflow_needed_for_width(43)
        );
        let now = Instant::now() + Duration::from_secs(2);
        assert_eq!(restored.pending_is_due(now), actual.pending_is_due(now));
        assert!(restored.take_stream_finish_reflow_needed());
        assert!(!restored.take_stream_finish_reflow_needed());
        let mut wire = serde_json::to_value(data).map_err(invalid)?;
        wire.as_object_mut().unwrap().remove("pending_until");
        assert!(serde_json::from_value::<Data>(wire).is_err());
        Ok(())
    }
}
