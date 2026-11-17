// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Private phase-policy boundary only. No origin/Native receipt fabricated.
use super::*;

#[test]
fn local_discard_never_consumes_registration_or_accepted_unknown_custody() {
    for phase in [
        Phase::Registering,
        Phase::Registered,
        Phase::Beginning,
        Phase::Held,
        Phase::Entered,
        Phase::Retiring,
        Phase::Selected,
        Phase::Completing,
        Phase::Complete,
        Phase::Unknown,
    ] {
        assert!(!discardable_unregistered(phase));
    }
    for phase in [Phase::Prepared, Phase::Rejected, Phase::Retired] {
        assert!(discardable_unregistered(phase));
    }
}
