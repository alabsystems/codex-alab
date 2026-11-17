// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Only these real retained observations may preserve their SAME channel after
//! a confirmed Native negative. A negative never grants admission or ACKs a
//! hold. Transport/auth/protocol/unanswered uncertainty after dispatch still
//! closes it. Local selector rejection occurs before dispatch and preserves
//! the original pre-validation behavior; it never grants admission.
use super::*;

pub(crate) enum RetainedQuery {
    Origin,
    InitialActivation,
    Activation,
    ActivationCommit,
    AppCurrent,
    SourceSelectionRegister,
    SourceSelectionObserve,
    SourceSelectionCommit,
}
impl RetainedQuery {
    fn action(&self) -> &'static str {
        match self {
            Self::Origin => "controller-retained-origin",
            Self::InitialActivation => "controller-retained-initial-activation",
            Self::Activation => "controller-retained-activation",
            Self::ActivationCommit => "controller-retained-activation-commit",
            Self::AppCurrent => "controller-retained-app-current",
            Self::SourceSelectionRegister => "controller-source-selection-register",
            Self::SourceSelectionObserve => "controller-source-selection-observe",
            Self::SourceSelectionCommit => "controller-source-selection-commit",
        }
    }
}
#[derive(Debug)]
pub(crate) enum ObservationError {
    Refused(io::Error),
    Uncertain(io::Error),
}
impl ObservationError {
    pub(crate) fn into_io(self) -> io::Error {
        match self {
            Self::Refused(error) | Self::Uncertain(error) => error,
        }
    }
}
impl Transport {
    pub(crate) async fn observe_retained(
        &self,
        query: RetainedQuery,
        operation: &str,
        fields: &str,
        member: &str,
    ) -> Result<String, ObservationError> {
        if !valid_hex(operation, 32) || !valid_hex(member, 32) || !self.conn.is_open() {
            return Err(ObservationError::Uncertain(unavailable()));
        }
        if !self.epoch().is_ok_and(|epoch| epoch.is_some()) {
            self.conn.close();
            return Err(ObservationError::Uncertain(unavailable()));
        }
        let mut confirmed_negative = false;
        let result = self
            .exchange(
                query.action(),
                operation,
                fields,
                Some(member),
                &mut confirmed_negative,
            )
            .await;
        match result {
            Ok(body) => Ok(body),
            Err(error) if confirmed_negative => Err(ObservationError::Refused(error)),
            Err(error) => {
                self.conn.close();
                Err(ObservationError::Uncertain(error))
            }
        }
    }
}

#[cfg(test)]
#[path = "source_retained_observation_tests.rs"]
mod tests;
