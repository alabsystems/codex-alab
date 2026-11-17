// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! One whole-UI marker graph shared by selection and worker weak scopes.
//! These are local alias DATA, never clipboard leases or a worker constructor.
use codex_alab_tui::upgrade_editor_graph::Capture;
use codex_alab_tui::upgrade_editor_graph::GenerationId;
use codex_alab_tui::upgrade_editor_graph::Restore;
use serde::Deserialize;
use serde::Serialize;
use std::io;
use std::sync::Arc;
use std::sync::Weak;
const MAX_SCOPES: usize = 1_000_000;
fn invalid(message: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct ScopeRef(GenerationId);
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ScopeData {
    schema: u8,
    count: u32,
}
impl ScopeData {
    pub(crate) fn validate_data(&self) -> io::Result<()> {
        if self.schema != 1 || usize::try_from(self.count).map_err(invalid)? > MAX_SCOPES {
            return Err(invalid("clipboard UI scope schema/capacity"));
        }
        Ok(())
    }
}
#[derive(Default)]
pub(crate) struct ScopeCapture {
    actual: Capture<()>,
}
impl ScopeCapture {
    pub(crate) fn strong(&mut self, actual: &Arc<()>) -> io::Result<ScopeRef> {
        if let Ok(Some(existing)) = self.actual.weak(&Arc::downgrade(actual)) {
            return Ok(ScopeRef(existing));
        }
        if self.actual.values().len() >= MAX_SCOPES {
            return Err(invalid("clipboard UI scope capacity"));
        }
        Ok(ScopeRef(self.actual.strong(actual)))
    }
    /// Register all real strong UI owners before translating worker weak scopes.
    pub(crate) fn weak(&self, actual: &Weak<()>) -> io::Result<Option<ScopeRef>> {
        self.actual
            .weak(actual)
            .map(|value| value.map(ScopeRef))
            .map_err(invalid)
    }
    pub(crate) fn export(&self) -> io::Result<ScopeData> {
        Ok(ScopeData {
            schema: 1,
            count: u32::try_from(self.actual.values().len()).map_err(invalid)?,
        })
    }
}
pub(crate) struct ScopeRestored {
    actual: Restore<()>,
}
impl ScopeRestored {
    pub(crate) fn new(data: ScopeData) -> io::Result<Self> {
        data.validate_data()?;
        let count = usize::try_from(data.count).map_err(invalid)?;
        Ok(Self {
            actual: Restore::new(vec![(); count]),
        })
    }
    pub(crate) fn strong(&self, reference: ScopeRef) -> io::Result<Arc<()>> {
        self.actual.strong(reference.0).map_err(invalid)
    }
    pub(crate) fn weak(&self, reference: Option<ScopeRef>) -> io::Result<Weak<()>> {
        match reference {
            Some(reference) => self.actual.weak(reference.0).map_err(invalid),
            None => Ok(Weak::new()),
        }
    }
    pub(super) fn weak_index(&self) -> Vec<Weak<()>> {
        self.actual.weak_values().collect()
    }
    /// Validate after all actual UI strong fields were staged, before installation.
    pub(crate) fn validate_roots(&self) -> io::Result<()> {
        self.actual.validate_roots().map_err(invalid)
    }
}
#[cfg(test)]
#[path = "retained_clipboard_scope_tests.rs"]
mod tests;
