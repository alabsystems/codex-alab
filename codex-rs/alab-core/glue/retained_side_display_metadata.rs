// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Response-owned display classification DATA only. The private Source
//! occurrence, exact writer/Core and real S2/Native/UI resources construct the
//! response; decoding this DATA never constructs any of those resources.
use serde::Deserialize;
use serde::Serialize;
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SideDisplayItem {
    pub turn_id: String,
    pub item_id: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SideDisplayBoundary {
    pub schema: u8,
    pub occurrence: String,
    pub before: Vec<SideDisplayItem>,
}
impl SideDisplayBoundary {
    pub fn valid(&self) -> bool {
        self.schema == 1
            && self.occurrence.len() == 32
            && self
                .occurrence
                .bytes()
                .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
            && self.before.len() <= 131072
            && self
                .before
                .iter()
                .all(|key| !key.turn_id.is_empty() && !key.item_id.is_empty())
            && self.before.windows(2).all(|pair| pair[0] < pair[1])
    }
}
