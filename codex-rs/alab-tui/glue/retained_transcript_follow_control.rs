// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Native pointer gesture state only. The enclosing whole-view transfer owns
//! activation and first paint; this leaf never reads input or creates a grant.
use super::*;
use serde::Deserialize;
use serde::Serialize;
use serde_json::value::RawValue;
use std::io;

fn invalid(message: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::transcript_view) struct Data {
    schema: u32,
    area: Option<[u16; 4]>,
    pointer: Option<[u16; 2]>,
    pressed: bool,
}

impl Data {
    pub(in crate::transcript_view) fn capture(view: &TranscriptView) -> Self {
        let native = &view.follow_control;
        Self {
            schema: 1,
            area: native.area.map(|r| [r.x, r.y, r.width, r.height]),
            pointer: native.pointer.map(|p| [p.x, p.y]),
            pressed: native.pressed,
        }
    }

    pub(in crate::transcript_view) fn decode(raw: &RawValue, max: usize) -> io::Result<Self> {
        if raw.get().len() > max {
            return Err(invalid("native follow control exceeds whole-view bound"));
        }
        let data: Self = serde_json::from_str(raw.get()).map_err(invalid)?;
        if data.schema != 1
            || (data.area.is_none() && (data.pointer.is_some() || data.pressed))
            || (data.pressed && data.pointer.is_none())
            || serde_json::to_value(&data).map_err(invalid)?
                != serde_json::from_str::<serde_json::Value>(raw.get()).map_err(invalid)?
        {
            return Err(invalid(
                "native follow control has omitted or foreign fields",
            ));
        }
        Ok(data)
    }

    pub(in crate::transcript_view) fn restore(self, view: &mut TranscriptView) {
        view.follow_control = FollowControl {
            area: self.area.map(|[x, y, width, height]| Rect {
                x,
                y,
                width,
                height,
            }),
            pointer: self.pointer.map(|[x, y]| ScreenPosition::new(x, y)),
            pressed: self.pressed,
        };
    }
}

#[cfg(test)]
#[path = "retained_transcript_follow_control_tests.rs"]
mod tests;

impl Data {
    pub(in crate::transcript_view) fn into_native(self) -> FollowControl {
        FollowControl {
            area: self.area.map(|[x, y, width, height]| Rect {
                x,
                y,
                width,
                height,
            }),
            pointer: self.pointer.map(|[x, y]| ScreenPosition::new(x, y)),
            pressed: self.pressed,
        }
    }
}
