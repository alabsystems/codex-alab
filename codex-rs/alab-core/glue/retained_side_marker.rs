// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Canonical occurrence correlation only. Deserialized DATA has no local
//! identity and cannot construct a Core receipt, permission or UI witness.
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;
use std::sync::Arc;

#[derive(Clone, schemars::JsonSchema)]
pub struct RetainedSideMarker {
    schema: u8,
    token: String,
    #[schemars(skip)]
    local: Option<Arc<()>>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Data {
    schema: u8,
    token: String,
}
impl RetainedSideMarker {
    /// The Core registers this SAME local allocation before using it. Merely
    /// constructing a marker establishes no Core/SDK/currentness authority.
    pub fn new_local(token: String) -> Result<Self, &'static str> {
        if !valid_token(&token) {
            return Err("invalid retained Side occurrence token");
        }
        Ok(Self {
            schema: 1,
            token,
            local: Some(Arc::new(())),
        })
    }
    pub fn token_data(&self) -> &str {
        &self.token
    }
    /// Local IO correlation only; deserialized DATA has no such allocation.
    pub fn local_delivery_identity(&self) -> Option<Arc<()>> {
        self.local.clone()
    }
    pub fn same_local(&self, other: &Self) -> bool {
        self.token == other.token
            && match (&self.local, &other.local) {
                (Some(left), Some(right)) => Arc::ptr_eq(left, right),
                _ => false,
            }
    }
}
impl std::fmt::Debug for RetainedSideMarker {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        output.write_str("RetainedSideMarker(<occurrence DATA>)")
    }
}
impl PartialEq for RetainedSideMarker {
    fn eq(&self, other: &Self) -> bool {
        self.token == other.token
    }
}
impl Eq for RetainedSideMarker {}
impl Serialize for RetainedSideMarker {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Data {
            schema: self.schema,
            token: self.token.clone(),
        }
        .serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for RetainedSideMarker {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;
        let data = Data::deserialize(deserializer)?;
        if data.schema != 1 || !valid_token(&data.token) {
            return Err(D::Error::custom("invalid retained Side occurrence DATA"));
        }
        Ok(Self {
            schema: data.schema,
            token: data.token,
            local: None,
        })
    }
}
fn valid_token(token: &str) -> bool {
    token.len() == 32
        && token
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
#[path = "retained_side_marker_tests.rs"]
mod tests;
