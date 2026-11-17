// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Exact original fenced-block scanner, including a pending opener.
use super::*;
use crate::streaming::alab_retained::invalid;
use crate::streaming::alab_retained::required;
use serde::Deserialize;
use serde::Serialize;
use std::io;
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Kind {
    Outside,
    Markdown,
    Other,
}
impl Kind {
    pub(crate) fn capture(x: FenceKind) -> Self {
        match x {
            FenceKind::Outside => Self::Outside,
            FenceKind::Markdown => Self::Markdown,
            FenceKind::Other => Self::Other,
        }
    }
    pub(crate) fn restore(self) -> FenceKind {
        match self {
            Self::Outside => FenceKind::Outside,
            Self::Markdown => FenceKind::Markdown,
            Self::Other => FenceKind::Other,
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Data {
    #[serde(deserialize_with = "required")]
    state: Option<(char, usize, Kind)>,
}
impl Data {
    pub(crate) fn capture(actual: &FenceTracker) -> Self {
        let FenceTracker { state } = actual;
        Self {
            state: state.map(|(marker, len, kind)| (marker, len, Kind::capture(kind))),
        }
    }
    pub(crate) fn restore(self) -> io::Result<FenceTracker> {
        if self
            .state
            .is_some_and(|(marker, len, _)| !matches!(marker, '`' | '~') || len < 3)
        {
            return Err(invalid("invalid original streaming fence opener"));
        }
        Ok(FenceTracker {
            state: self
                .state
                .map(|(marker, len, kind)| (marker, len, kind.restore())),
        })
    }
}
