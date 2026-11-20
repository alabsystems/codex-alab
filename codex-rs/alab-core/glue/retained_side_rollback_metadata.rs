// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Original attached rollback selectors are DATA, not permission or settlement.
use serde::Deserialize;
use serde::Serialize;
pub(crate) const KEY: &str = "alab.sideRollback";
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum Selection {
    Rollback {
        schema: u8,
        claim: String,
        operation: String,
    },
    Observe {
        schema: u8,
        claim: String,
        operation: String,
    },
}
fn token(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
}
impl Selection {
    pub(crate) fn claim(&self) -> &str {
        match self {
            Self::Rollback { claim, .. } | Self::Observe { claim, .. } => claim,
        }
    }
    pub(crate) fn operation(&self) -> &str {
        match self {
            Self::Rollback { operation, .. } | Self::Observe { operation, .. } => operation,
        }
    }
    pub(crate) fn valid(&self) -> bool {
        let schema = match self {
            Self::Rollback { schema, .. } | Self::Observe { schema, .. } => schema,
        };
        *schema == 1 && token(self.claim()) && token(self.operation())
    }
}

pub(crate) const OBSERVATION_KEY: &str = "alab.sideRollbackObservation";
pub(crate) use codex_alab_core::retained_side_cleanup as observation;
