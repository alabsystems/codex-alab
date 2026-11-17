// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual original ConfigBuilder allocation incarnation. A weak marker neither
//! retains its reader/credential resources nor reconstructs one from metadata.
use crate::legacy_core::config::ConfigBuilder;
use std::ops::Deref;
use std::sync::Arc;
use std::sync::Weak;
pub(crate) struct Owned {
    inner: Box<ConfigBuilder>,
    identity: Arc<()>,
}
impl Owned {
    pub(crate) fn new(actual: ConfigBuilder) -> Self {
        Self {
            inner: Box::new(actual),
            identity: Arc::new(()),
        }
    }
    pub(crate) fn identity(&self) -> Weak<()> {
        Arc::downgrade(&self.identity)
    }
}
impl Clone for Owned {
    // Preserve the original Box<ConfigBuilder>::clone behavior, while a new
    // allocation can never inherit a prior resource incarnation through ABA.
    fn clone(&self) -> Self {
        Self::new(self.inner.as_ref().clone())
    }
}
impl Deref for Owned {
    type Target = ConfigBuilder;
    fn deref(&self) -> &Self::Target {
        self.inner.as_ref()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replacement_and_original_clone_have_fresh_live_incarnations() {
        // Pure original native value only; never built/applied as C2 Config.
        let original = Owned::new(ConfigBuilder::default());
        let old = original.identity();
        let cloned = original.clone();
        assert!(!Weak::ptr_eq(&old, &cloned.identity()));
        drop(original);
        assert!(old.upgrade().is_none());
        let replacement = Owned::new(ConfigBuilder::default());
        assert!(!Weak::ptr_eq(&old, &replacement.identity()));
        assert!(old.upgrade().is_none());
    }
}
