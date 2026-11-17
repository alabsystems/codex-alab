// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Process-local custody traveling with one actual accepted socket/request.
//! Presence, runtime type or downcasting is NEVER an admission/permission check.
//! Native authorization requires the actual concrete resource's SDK checker.
use std::any::Any;
use std::sync::Arc;

#[derive(Clone)]
pub struct ConnectionResource(Arc<dyn Any + Send + Sync>);
impl ConnectionResource {
    pub fn new<T: Any + Send + Sync>(actual: T) -> Self {
        Self(Arc::new(actual))
    }
    pub fn downcast<T: Any + Send + Sync>(&self) -> Option<Arc<T>> {
        self.0.clone().downcast().ok()
    }
}
impl std::fmt::Debug for ConnectionResource {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("ConnectionResource(<owned; no authority>)")
    }
}
