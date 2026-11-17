// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Historical result and original monotonic expiry, never a clipboard operation.
use super::*;
use codex_alab_tui::upgrade_editor_clock::Snapshot;
use serde::Deserialize;
use serde::Serialize;
use std::io;
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum ResultData {
    Confirmed {},
    Unconfirmed {},
    Pending { id: u64 },
    Busy {},
    Failed {},
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::transcript_view) struct Data {
    schema: u8,
    result: ResultData,
    characters: u64,
    #[serde(deserialize_with = "required")]
    expires_at: Option<Snapshot>,
}
impl Data {
    pub(in crate::transcript_view) fn capture(actual: &CopyFeedback) -> io::Result<Self> {
        Ok(Self {
            schema: 1,
            result: match actual.result {
                Ok(CopyStatus::Confirmed) => ResultData::Confirmed {},
                Ok(CopyStatus::Unconfirmed) => ResultData::Unconfirmed {},
                Ok(CopyStatus::Pending(id)) => ResultData::Pending { id },
                Ok(CopyStatus::Busy) => ResultData::Busy {},
                Err(()) => ResultData::Failed {},
            },
            characters: u64::try_from(actual.characters).map_err(io::Error::other)?,
            expires_at: actual
                .expires_at
                .map(Snapshot::capture)
                .transpose()
                .map_err(io::Error::other)?,
        })
    }
    pub(in crate::transcript_view) fn pending_id(&self) -> Option<u64> {
        if let ResultData::Pending { id } = self.result {
            Some(id)
        } else {
            None
        }
    }
    pub(in crate::transcript_view) fn restore(self) -> io::Result<CopyFeedback> {
        if self.schema != 1 {
            return Err(io::Error::other("native copy feedback schema"));
        }
        Ok(CopyFeedback {
            result: match self.result {
                ResultData::Confirmed {} => Ok(CopyStatus::Confirmed),
                ResultData::Unconfirmed {} => Ok(CopyStatus::Unconfirmed),
                ResultData::Pending { id } => Ok(CopyStatus::Pending(id)),
                ResultData::Busy {} => Ok(CopyStatus::Busy),
                ResultData::Failed {} => Err(()),
            },
            characters: usize::try_from(self.characters).map_err(io::Error::other)?,
            expires_at: self
                .expires_at
                .map(|stamp| stamp.restore())
                .transpose()
                .map_err(io::Error::other)?,
        })
    }
}
fn required<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Option<T>, D::Error> {
    Option::<T>::deserialize(d)
}
