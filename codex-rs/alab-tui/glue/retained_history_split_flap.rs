// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Native split-flap state and exact unique inner cell; real restored Tui supplies frame resource.
use super::*;
use crate::history_cell::alab_retained_cells::CaptureContext;
use crate::history_cell::alab_retained_cells::FrameRef;
use crate::history_cell::alab_retained_cells::RestoreContext;
use crate::history_cell::alab_retained_cells::UniqueBody as Body;
use crate::history_cell::alab_retained_cells::{self as cells};
use crate::terminal_hyperlinks::alab_retained_text as native;
use codex_alab_tui::upgrade_editor_clock::Snapshot;
use serde::Deserialize;
use serde::Serialize;
use std::io;
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Data {
    inner: Box<Body>,
    role: String,
    target: String,
    visible_target: String,
    flap_sample: Vec<u8>,
    tile_arrivals: Vec<Snapshot>,
    started_at: Snapshot,
    phase_started_at: Snapshot,
    motion: u8,
    frame: FrameRef,
}
fn sample(visible: &str) -> Vec<u8> {
    let mut sample = visible
        .bytes()
        .rev()
        .filter(u8::is_ascii_alphabetic)
        .take(FLAP_SAMPLE_WINDOW)
        .collect::<Vec<_>>();
    if sample.is_empty() {
        sample.extend(visible.bytes().rev().take(FLAP_SAMPLE_WINDOW));
    }
    sample.reverse();
    sample
}
impl Data {
    pub(crate) fn capture(
        actual: &dyn HistoryCell,
        context: &mut CaptureContext<'_>,
        nesting: usize,
    ) -> io::Result<Option<Self>> {
        let Some(actual) = actual.as_any().downcast_ref::<SplitFlapTranscriptCell>() else {
            return Ok(None);
        };
        let next = cells::depth(nesting)?;
        Ok(Some(Self {
            inner: Box::new(Body::capture(actual.inner.as_ref(), context, next)?),
            role: actual.role.clone(),
            target: actual.target.clone(),
            visible_target: actual.visible_target.clone(),
            flap_sample: actual.flap_sample.clone(),
            tile_arrivals: actual
                .tile_arrivals
                .iter()
                .map(|instant| Snapshot::capture(*instant).map_err(native::invalid))
                .collect::<io::Result<_>>()?,
            started_at: Snapshot::capture(actual.started_at).map_err(native::invalid)?,
            phase_started_at: Snapshot::capture(actual.phase_started_at)
                .map_err(native::invalid)?,
            motion: match actual.motion_mode {
                MotionMode::Animated => 0,
                MotionMode::Reduced => 1,
            },
            frame: context.frame(&actual.frame_requester)?,
        }))
    }
    pub(crate) fn restore(
        self,
        shared: &mut native::Restored,
        resources: &mut RestoreContext,
        nesting: usize,
    ) -> io::Result<Box<dyn HistoryCell>> {
        let next = cells::depth(nesting)?;
        if self.tile_arrivals.len() > native::MAX_NODES
            || self.tile_arrivals.len() != self.visible_target.len()
            || !self
                .visible_target
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric())
            || self.flap_sample != sample(&self.visible_target)
        {
            return Err(native::invalid(
                "split-flap native tile/sample shape differs",
            ));
        }
        let motion_mode = match self.motion {
            0 => MotionMode::Animated,
            1 => MotionMode::Reduced,
            _ => return Err(native::invalid("unknown native split-flap motion mode")),
        };
        let frame_requester = resources.frame(self.frame)?;
        Ok(Box::new(SplitFlapTranscriptCell {
            inner: self.inner.restore(shared, resources, next)?,
            role: self.role,
            target: self.target,
            visible_target: self.visible_target,
            flap_sample: self.flap_sample,
            tile_arrivals: self
                .tile_arrivals
                .into_iter()
                .map(|instant| instant.restore().map_err(native::invalid))
                .collect::<io::Result<_>>()?,
            started_at: self.started_at.restore().map_err(native::invalid)?,
            phase_started_at: self.phase_started_at.restore().map_err(native::invalid)?,
            motion_mode,
            frame_requester,
        }))
    }
}
#[cfg(test)]
mod controls {
    use super::*;
    use crate::history_cell::AgentMessageCell;
    use crate::history_cell::HistoryCell;
    use std::sync::Arc;
    fn render(text: &str) -> Box<dyn HistoryCell> {
        Box::new(AgentMessageCell::new(
            vec![Line::from(text.to_owned())],
            true,
        ))
    }
    #[test]
    fn native_split_flap_requires_actual_frame_binding_and_keeps_original_state_and_inner_type()
    -> io::Result<()> {
        let (requester, _old_receiver) = FrameRequester::test_channel();
        let original: Arc<dyn HistoryCell> = Arc::new(SplitFlapTranscriptCell::new(
            render,
            "user",
            "Exact native target 42",
            None,
            0,
            MotionMode::Reduced,
            requester.clone(),
        ));
        let mut graph = cells::Capture::default();
        let frame = graph.frame(&requester)?;
        let root = graph.intern(&original)?;
        let mut text = native::Capture::default();
        let bytes = graph.encode(&mut text, 1_048_576)?;
        let mut shared = native::Restored::new(text.export()?)?;
        assert!(
            cells::Restored::decode(
                &bytes,
                1_048_576,
                &mut shared,
                &mut RestoreContext::default()
            )
            .is_err()
        );
        let (actual, _actual_receiver) = FrameRequester::test_channel();
        let mut resources = RestoreContext::default();
        resources.bind_frame(frame, &actual)?;
        let mut shared = native::Restored::new(text.export()?)?;
        let restored =
            cells::Restored::decode(&bytes, 1_048_576, &mut shared, &mut resources)?.cell(root)?;
        let native = restored
            .as_any()
            .downcast_ref::<SplitFlapTranscriptCell>()
            .ok_or_else(|| native::invalid("split-flap native type changed"))?;
        let before = original
            .as_any()
            .downcast_ref::<SplitFlapTranscriptCell>()
            .ok_or_else(|| native::invalid("actual split-flap missing"))?;
        assert_eq!(native.role, before.role);
        assert_eq!(native.target, before.target);
        assert_eq!(native.visible_target, before.visible_target);
        assert_eq!(native.flap_sample, before.flap_sample);
        assert_eq!(native.tile_arrivals.len(), before.tile_arrivals.len());
        assert_eq!(native.motion_mode, before.motion_mode);
        assert!(native.inner.as_any().is::<AgentMessageCell>());
        for width in [0, 5, 24, 97] {
            assert_eq!(original.display_lines(width), restored.display_lines(width));
        }
        assert_eq!(original.raw_lines(), restored.raw_lines());
        Ok(())
    }
    #[test]
    fn native_animated_cell_uses_real_restored_frame_channel_and_rejects_impossible_empty_sample()
    -> io::Result<()> {
        let (old, mut old_receiver) = FrameRequester::test_channel();
        let original: Arc<dyn HistoryCell> = Arc::new(SplitFlapTranscriptCell::new(
            render,
            "assistant",
            "Original animated words",
            None,
            0,
            MotionMode::Animated,
            old.clone(),
        ));
        while old_receiver.try_recv().is_ok() {}
        let mut graph = cells::Capture::default();
        let frame = graph.frame(&old)?;
        let root = graph.intern(&original)?;
        let mut text = native::Capture::default();
        let bytes = graph.encode(&mut text, 1_048_576)?;
        let (actual, mut current_receiver) = FrameRequester::test_channel();
        let mut resources = RestoreContext::default();
        resources.bind_frame(frame, &actual)?;
        let mut shared = native::Restored::new(text.export()?)?;
        let restored =
            cells::Restored::decode(&bytes, 1_048_576, &mut shared, &mut resources)?.cell(root)?;
        let restored = restored
            .as_any()
            .downcast_ref::<SplitFlapTranscriptCell>()
            .ok_or_else(|| native::invalid("split-flap native type changed"))?;
        let _ = restored.animate_lines(
            restored.inner.display_hyperlink_lines(80),
            80,
            Duration::from_millis(250),
        );
        assert!(current_receiver.try_recv().is_ok());
        assert!(old_receiver.try_recv().is_err());
        let mut malformed: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(native::invalid)?;
        malformed["cells"][0]["original"]["flapSample"] = serde_json::json!([]);
        let mut resources = RestoreContext::default();
        resources.bind_frame(frame, &actual)?;
        let mut shared = native::Restored::new(text.export()?)?;
        assert!(
            cells::Restored::decode(
                &serde_json::to_vec(&malformed).map_err(native::invalid)?,
                1_048_576,
                &mut shared,
                &mut resources
            )
            .is_err()
        );
        Ok(())
    }
}
