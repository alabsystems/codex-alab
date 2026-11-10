// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Exact native retirement of an OPEN which has admitted no credential read.

use super::*;

pub type OpenRetirementFuture<'a> =
    Pin<Box<dyn Future<Output = io::Result<OpenRetirement>> + Send + 'a>>;

/// A private native Closed receipt, not a source-drain or assignment permit.
/// Once PRELOAD or member work was admitted, its separate owned retirement
/// must settle that work; OPEN cleanup cannot hide it.
pub struct OpenRetirement {
    assignment: String,
    operation: String,
    open_operation: String,
    member: Option<String>,
}

impl OpenRetirement {
    /// The authenticated adapter validates native instance, complete immutable
    /// request, assignment/context and explicit Closed before constructing this.
    /// Shape validation alone supplies no native retirement evidence.
    pub fn from_native(
        context: &Context,
        operation: String,
        open_operation: String,
        member: Option<String>,
    ) -> io::Result<Self> {
        if !valid_hex(&operation, 32)
            || !valid_hex(&open_operation, 32)
            || member
                .as_deref()
                .is_some_and(|member| !valid_hex(member, 32))
        {
            return Err(unavailable());
        }
        Ok(Self {
            assignment: context.assignment().into(),
            operation,
            open_operation,
            member,
        })
    }

    pub fn assignment(&self) -> &str {
        &self.assignment
    }
    pub fn operation(&self) -> &str {
        &self.operation
    }
    pub fn open_operation(&self) -> &str {
        &self.open_operation
    }
    pub fn member(&self) -> Option<&str> {
        self.member.as_deref()
    }
}

impl std::fmt::Debug for OpenRetirement {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("NativeOpenRetirement(<private>)")
    }
}
