// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Message-history batch cursor DATA. File revision/offset remains a hint; the
//! original reader still revalidates the actual file before using its anchor.
//! Compiled as a child of codex-message-history::batch, which owns these types.
use super::*;
use serde::Deserialize;
use serde::Serialize;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Data {
    end_offset: usize,
    byte_anchor: Option<AnchorData>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AnchorData {
    position: u64,
    revision: RevisionData,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RevisionData {
    len: u64,
    modified: Option<SystemTime>,
}

impl Serialize for HistoryBatchCursor {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Data {
            end_offset: self.end_offset,
            byte_anchor: self.byte_anchor.map(|anchor| AnchorData {
                position: anchor.position,
                revision: RevisionData {
                    len: anchor.revision.len,
                    modified: anchor.revision.modified,
                },
            }),
        }
        .serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for HistoryBatchCursor {
    fn deserialize<D: serde::Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        let data = Data::deserialize(decoder)?;
        if data
            .byte_anchor
            .as_ref()
            .is_some_and(|anchor| anchor.position > anchor.revision.len)
        {
            return Err(serde::de::Error::custom(
                "history cursor is outside its captured revision",
            ));
        }
        Ok(Self {
            end_offset: data.end_offset,
            byte_anchor: data.byte_anchor.map(|anchor| HistoryByteAnchor {
                position: anchor.position,
                revision: HistoryFileRevision {
                    len: anchor.revision.len,
                    modified: anchor.revision.modified,
                },
            }),
        })
    }
}
