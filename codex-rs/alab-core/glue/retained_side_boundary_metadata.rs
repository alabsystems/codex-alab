// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Selection/occurrence DATA only; actual Native A and Scope/Core ownership
//! are independently checked by the accepted Source producer.
use serde::Deserialize;
use serde::Serialize;
pub(crate) const KEY: &str = "alab.sideInjection";
pub(crate) const RESULT_KEY: &str = "alab.sideInjectionResult";
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Selection {
    pub schema: u8,
    pub claim: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Completion {
    pub schema: u8,
    pub claim: String,
    pub thread_id: String,
    pub occurrence: String,
}
pub(crate) fn token(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
}
impl Selection {
    pub(crate) fn valid(&self) -> bool {
        self.schema == 1 && token(&self.claim)
    }
}
impl Completion {
    pub(crate) fn valid(&self) -> bool {
        self.schema == 1
            && token(&self.claim)
            && token(&self.occurrence)
            && !self.thread_id.is_empty()
    }
}
