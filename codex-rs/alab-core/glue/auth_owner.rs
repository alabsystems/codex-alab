// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

impl super::AuthOwner {
    /// Renewal records retain the generation of the request's captured owner.
    pub fn alab_generation(&self) -> u64 {
        self.generation
    }
}
