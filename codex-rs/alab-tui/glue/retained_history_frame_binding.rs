// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! A frame selector is UI DATA. Only the actual restored Tui supplies its live scheduler handle.
use super::*;
use crate::terminal_hyperlinks::alab_retained_text as native;
use serde::Deserialize;
use serde::Serialize;
use std::io;
impl FrameRequester {
    /// Resource identity only; no scheduler or permission is constructed.
    pub(crate) fn alab_same_scheduler(&self, other: &Self) -> bool {
        self.frame_schedule_tx
            .same_channel(&other.frame_schedule_tx)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct FrameRef(u32);
#[derive(Default)]
pub(crate) struct Capture {
    held: Vec<FrameRequester>,
}
impl Capture {
    pub(crate) fn reference(&mut self, actual: &FrameRequester) -> io::Result<FrameRef> {
        if actual.frame_schedule_tx.is_closed() {
            return Err(native::invalid("actual frame scheduler is closed"));
        }
        if let Some(index) = self.held.iter().position(|held| {
            held.frame_schedule_tx
                .same_channel(&actual.frame_schedule_tx)
        }) {
            return u32::try_from(index).map(FrameRef).map_err(native::invalid);
        }
        if self.held.len() >= native::MAX_NODES {
            return Err(native::invalid("frame resource graph capacity"));
        }
        let reference = FrameRef(u32::try_from(self.held.len()).map_err(native::invalid)?);
        self.held.push(actual.clone());
        Ok(reference)
    }
    pub(crate) fn count(&self) -> io::Result<u32> {
        u32::try_from(self.held.len()).map_err(native::invalid)
    }
}
#[derive(Default)]
pub(crate) struct Bindings {
    actual: Vec<(FrameRef, FrameRequester)>,
    count: Option<u32>,
}
impl Bindings {
    pub(crate) fn bind(&mut self, reference: FrameRef, actual: &FrameRequester) -> io::Result<()> {
        if actual.frame_schedule_tx.is_closed() {
            return Err(native::invalid("restored Tui scheduler is closed"));
        }
        if self.actual.len() >= native::MAX_NODES {
            return Err(native::invalid("frame resource binding capacity"));
        }
        for (existing, held) in &self.actual {
            let same = held
                .frame_schedule_tx
                .same_channel(&actual.frame_schedule_tx);
            if *existing == reference {
                return if same {
                    Ok(())
                } else {
                    Err(native::invalid(
                        "frame reference already bound to another actual Tui scheduler",
                    ))
                };
            }
            if same {
                return Err(native::invalid(
                    "distinct original schedulers cannot be merged by DATA",
                ));
            }
        }
        if self.count.is_some_and(|count| reference.0 >= count) {
            return Err(native::invalid("frame binding has no original graph entry"));
        }
        self.actual.push((reference, actual.clone()));
        Ok(())
    }
    pub(crate) fn install(&mut self, count: u32) -> io::Result<()> {
        if self.count.is_some()
            || count as usize > native::MAX_NODES
            || self
                .actual
                .iter()
                .any(|(reference, _)| reference.0 >= count)
        {
            return Err(native::invalid("frame graph count or binding differs"));
        }
        self.count = Some(count);
        Ok(())
    }
    pub(crate) fn requester(&self, reference: FrameRef) -> io::Result<FrameRequester> {
        if !self.count.is_some_and(|count| reference.0 < count) {
            return Err(native::invalid(
                "frame resource reference has no graph entry",
            ));
        }
        let actual = self
            .actual
            .iter()
            .find(|(id, _)| *id == reference)
            .map(|(_, actual)| actual)
            .ok_or_else(|| native::invalid("actual restored Tui frame resource is required"))?;
        if actual.frame_schedule_tx.is_closed() {
            return Err(native::invalid(
                "actual restored Tui scheduler closed during restore",
            ));
        }
        Ok(actual.clone())
    }
}
#[cfg(test)]
mod controls {
    use super::*;
    #[test]
    fn actual_frame_channels_require_explicit_binding_and_preserve_clone_identity() -> io::Result<()>
    {
        let (first, _first_rx) = FrameRequester::test_channel();
        let (second, _second_rx) = FrameRequester::test_channel();
        let mut capture = Capture::default();
        let a = capture.reference(&first)?;
        assert_eq!(capture.reference(&first.clone())?, a);
        let b = capture.reference(&second)?;
        assert_ne!(a, b);
        let mut bindings = Bindings::default();
        bindings.install(capture.count()?)?;
        assert!(bindings.requester(a).is_err());
        bindings.bind(a, &first)?;
        assert!(bindings.bind(a, &second).is_err());
        assert!(bindings.bind(b, &first).is_err());
        bindings.bind(b, &second)?;
        assert!(
            bindings
                .requester(a)?
                .frame_schedule_tx
                .same_channel(&first.frame_schedule_tx)
        );
        assert!(
            bindings
                .requester(b)?
                .frame_schedule_tx
                .same_channel(&second.frame_schedule_tx)
        );
        Ok(())
    }
}
