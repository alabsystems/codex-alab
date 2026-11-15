// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Typed native assignment status and prepared-target metadata, which the native source and
//! destination epochs check. These carry no credential path or token.

use crate::managed_auth::valid_hex;
use std::io;

fn unavailable() -> io::Error {
    io::Error::other("Native account assignment must be reconciled.")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Manual,
    Auto,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Waiting,
    Prepared,
    Installing,
    AwaitingAck,
    Applied,
    Cancelled,
    Refused,
    Reconciling,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    Busy,
    Unavailable,
    Denied,
    StaleRevision,
    SourceChanged,
    TargetUnavailable,
    Policy,
    Cancelled,
    Superseded,
    NeedsReconciliation,
    UnsupportedConsumer,
    AuthenticationRequired,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub operation: String,
    pub phase: Phase,
    pub boundary: Option<String>,
    pub assignment_generation: Option<u64>,
    pub credential_generation: Option<u64>,
    pub target: Option<String>,
    pub mode: Option<Mode>,
    pub reason: Option<Reason>,
}
impl Status {
    pub fn validate(&self, operation: &str) -> io::Result<()> {
        let opaque_terminal = matches!(self.phase, Phase::Cancelled | Phase::Refused)
            && self.target.is_none()
            && self.boundary.is_none()
            && self.assignment_generation.is_none()
            && self.credential_generation.is_none();
        if self.operation != operation
            || !valid_hex(operation, 32)
            || self
                .boundary
                .as_ref()
                .is_some_and(|value| !valid_hex(value, 32))
            || self
                .target
                .as_ref()
                .is_some_and(|value| !valid_hex(value, 64))
            || self.assignment_generation == Some(0)
            || self.credential_generation == Some(0)
            || self.assignment_generation.is_some() != self.credential_generation.is_some()
            || (self.mode.is_none() && self.phase != Phase::Waiting && !opaque_terminal)
            || (self.phase == Phase::Prepared
                && (self.target.is_none()
                    || self.boundary.is_some()
                    || self.assignment_generation.is_some()))
            || (matches!(self.phase, Phase::AwaitingAck | Phase::Applied)
                && (self.boundary.is_none()
                    || self.assignment_generation.is_none()
                    || self.target.is_none()))
        {
            return Err(unavailable());
        }
        Ok(())
    }
}

/// Private prepared-target policy input, authenticated by the owning native transport.
/// This is not a credential source or permission to install a grant.
#[derive(Clone, PartialEq, Eq)]
pub struct TargetPolicy {
    /// Domain-separated bindings of the actual validated endpoint and client ID.
    pub source_provider: String,
    pub target_provider: String,
    /// Classifications read from the authenticated source and destination documents.
    pub source_fedramp: bool,
    pub target_fedramp: bool,
    /// Exact native endpoint against which each manager checks its effective override.
    pub target_refresh_endpoint: String,
}
impl TargetPolicy {
    fn validate(&self) -> io::Result<()> {
        let endpoint = &self.target_refresh_endpoint;
        if !valid_hex(&self.source_provider, 64)
            || !valid_hex(&self.target_provider, 64)
            || self.source_provider != self.target_provider
            || self.source_fedramp != self.target_fedramp
            || endpoint.len() > 2048
            || !endpoint.starts_with("https://")
            || !endpoint.ends_with("/oauth/token")
            || endpoint.bytes().any(|byte| {
                byte <= 0x20 || byte >= 0x7f || matches!(byte, b'\\' | b'?' | b'#' | b'@')
            })
            || endpoint[8..].split('/').next().is_none_or(str::is_empty)
        {
            return Err(unavailable());
        }
        Ok(())
    }
}
impl std::fmt::Debug for TargetPolicy {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PreparedTargetPolicy(<private>)")
    }
}

/// Exact prepared metadata; every policy field is required, with no inferred defaults.
#[derive(Clone)]
pub struct Target {
    pub operation: String,
    pub pool_revision: String,
    pub expected_assignment: u64,
    pub target_ref: String,
    pub account: String,
    pub user: String,
    pub source_grant: String,
    pub target_grant: String,
    pub policy: TargetPolicy,
}
impl Target {
    pub fn validate(&self, operation: &str) -> io::Result<()> {
        if self.operation != operation
            || !valid_hex(operation, 32)
            || [
                &self.account,
                &self.user,
                &self.source_grant,
                &self.target_grant,
                &self.pool_revision,
                &self.target_ref,
            ]
            .into_iter()
            .any(|value| !valid_hex(value, 64))
        {
            return Err(unavailable());
        }
        self.policy.validate()
    }
}
impl std::fmt::Debug for Target {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PreparedTarget(<private>)")
    }
}

#[cfg(test)]
mod target_policy_tests {
    use super::*;

    fn target() -> Target {
        Target {
            operation: "a".repeat(32),
            pool_revision: "1".repeat(64),
            expected_assignment: 1,
            target_ref: "2".repeat(64),
            account: "b".repeat(64),
            user: "c".repeat(64),
            source_grant: "d".repeat(64),
            target_grant: "e".repeat(64),
            policy: TargetPolicy {
                source_provider: "f".repeat(64),
                target_provider: "f".repeat(64),
                source_fedramp: false,
                target_fedramp: false,
                target_refresh_endpoint: "https://auth.example/oauth/token".into(),
            },
        }
    }

    #[test]
    fn policy_requires_same_admitted_provider_and_realm() {
        let mut target = target();
        assert!(target.validate(&target.operation).is_ok());
        target.policy.target_provider = "1".repeat(64);
        assert!(target.validate(&target.operation).is_err());
        target.policy.target_provider = target.policy.source_provider.clone();
        target.policy.target_fedramp = true;
        assert!(target.validate(&target.operation).is_err());
        target.policy.source_fedramp = true;
        assert!(target.validate(&target.operation).is_ok());
        assert_eq!(format!("{target:?}"), "PreparedTarget(<private>)");
        assert_eq!(
            format!("{:?}", target.policy),
            "PreparedTargetPolicy(<private>)"
        );
    }

    #[test]
    fn policy_refuses_missing_binding_and_unsafe_endpoint_metadata() {
        for endpoint in [
            "",
            "http://auth.example/oauth/token",
            "https:///oauth/token",
            "https://user@auth.example/oauth/token",
            "https://auth.example/oauth/token?x",
            "https://auth.example/\noauth/token",
            "https://auth.example/other",
        ] {
            let mut target = target();
            target.policy.target_refresh_endpoint = endpoint.into();
            assert!(target.validate(&target.operation).is_err());
        }
        let mut target = target();
        target.policy.source_provider.clear();
        assert!(target.validate(&target.operation).is_err());
    }
}
