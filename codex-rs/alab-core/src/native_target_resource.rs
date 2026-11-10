// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Exact destination resource and ACK correlation. Metadata cannot mint authority.

use super::*;
use crate::assignment::Binding;
use crate::assignment::Generations;

/// A report captured by the SDK after real resource initialization. The native
/// endpoint independently joins its exact member/registration/Ready record.
#[derive(Clone, PartialEq, Eq)]
pub struct ResourceReport {
    member: String,
    registration: String,
    boundary: String,
    target_ref: String,
    source: Box<Source>,
    generations: Generations,
}
impl ResourceReport {
    pub fn from_loaded(
        context: &Context,
        member: String,
        registration: String,
        boundary: String,
        source: Source,
        generations: Generations,
    ) -> io::Result<Self> {
        context.validate_source(&source)?;
        if !valid_hex(&member, 32) || !valid_hex(&registration, 32) || !valid_hex(&boundary, 32) {
            return Err(unavailable());
        }
        Ok(Self {
            member,
            registration,
            boundary,
            target_ref: context.target_ref().into(),
            source: Box::new(source),
            generations,
        })
    }
    pub fn validate_context(&self, context: &Context) -> io::Result<()> {
        context.validate_source(&self.source)?;
        if self.target_ref != context.target_ref() {
            return Err(unavailable());
        }
        Ok(())
    }
    pub fn member(&self) -> &str {
        &self.member
    }
    pub fn registration(&self) -> &str {
        &self.registration
    }
    pub fn boundary(&self) -> &str {
        &self.boundary
    }
    pub fn source(&self) -> &Source {
        &self.source
    }
    pub fn binding(&self) -> Binding {
        Binding {
            account: self.source.account.clone(),
            user: self.source.user.clone(),
            digest: self.source.revision.digest.clone(),
            credential_generation: self.source.revision.generation,
            assignment_generation: self.source.assignment_generation,
            generations: self.generations.clone(),
        }
    }
}

/// Opaque native receipt. It retains the original RESOURCE attempt and report
/// through uncertain ACK replies; it cannot be constructed from public Status.
pub struct ResourceReceipt {
    operation: String,
    capability: zeroize::Zeroizing<String>,
    report: ResourceReport,
}
impl ResourceReceipt {
    /// Only the private adapter calls this after exact authenticated correlation.
    pub fn from_native(
        context: &Context,
        operation: String,
        capability: String,
        report: ResourceReport,
    ) -> io::Result<Self> {
        let capability = zeroize::Zeroizing::new(capability);
        report.validate_context(context)?;
        if !valid_hex(&operation, 32) || !valid_hex(&capability, 64) {
            return Err(unavailable());
        }
        Ok(Self {
            operation,
            capability,
            report,
        })
    }
    pub fn operation(&self) -> &str {
        &self.operation
    }
    pub fn report(&self) -> &ResourceReport {
        &self.report
    }
    /// Private transport only; never persist or display this capability.
    pub fn private_capability(&self) -> &str {
        self.capability.as_str()
    }
}

/// Result of the exact private ACK, compared with the original resource receipt.
/// This is SDK correlation, not a native kernel normalizer or source permit.
pub struct Applied {
    operation: String,
    report: ResourceReport,
}
impl Applied {
    pub fn from_native(
        receipt: &ResourceReceipt,
        boundary: &str,
        observed: &Binding,
    ) -> io::Result<Self> {
        if boundary != receipt.report.boundary || observed != &receipt.report.binding() {
            return Err(unavailable());
        }
        Ok(Self {
            operation: receipt.operation.clone(),
            report: receipt.report.clone(),
        })
    }
    pub fn report(&self) -> &ResourceReport {
        &self.report
    }
    pub fn operation(&self) -> &str {
        &self.operation
    }
}

pub type ResourceFuture<'a> =
    Pin<Box<dyn Future<Output = io::Result<ResourceReceipt>> + Send + 'a>>;
pub type AppliedFuture<'a> = Pin<Box<dyn Future<Output = io::Result<Applied>> + Send + 'a>>;

macro_rules! private_debug {
    ($name:ident) => {
        impl std::fmt::Debug for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(concat!(stringify!($name), "(<private>)"))
            }
        }
    };
}
private_debug!(ResourceReport);
private_debug!(ResourceReceipt);
private_debug!(Applied);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_bindings_do_not_substitute_another_original_report() {
        // Metadata only: no native permit or applied authority is constructed.
        let report = ResourceReport {
            member: "1".repeat(32),
            registration: "2".repeat(32),
            boundary: "3".repeat(32),
            target_ref: "4".repeat(64),
            source: Box::new(Source {
                operation: "5".repeat(32),
                slot: "fixture-slot".into(),
                path: "/fixture/source".into(),
                account: "fixture-account".into(),
                user: "fixture-user".into(),
                revision: crate::managed_auth::Revision {
                    generation: 7,
                    digest: "6".repeat(64),
                },
                assignment_generation: 8,
                native_intent: Some("7".repeat(32)),
            }),
            generations: Generations {
                auth: 1,
                owner: 2,
                source: 3,
                login: 4,
            },
        };
        assert_eq!(report, report.clone());
        let mut different_operation = report.clone();
        different_operation.source.operation = "8".repeat(32);
        assert_eq!(report.binding(), different_operation.binding());
        assert_ne!(report, different_operation);
        let mut different_target = report.clone();
        different_target.target_ref = "9".repeat(64);
        assert_eq!(report.binding(), different_target.binding());
        assert_ne!(report, different_target);
    }
}
