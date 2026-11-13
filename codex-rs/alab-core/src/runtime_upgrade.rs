// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Fresh-constructor native runtime handover. Data identifies the retained
//! snapshot; only the actual admitted member supplies this host connection.
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::Arc;

pub type LoadFuture<'a> =
    Pin<Box<dyn Future<Output = io::Result<Arc<dyn Continuation>>> + Send + 'a>>;
pub trait Host: Send + Sync {
    fn load<'a>(&'a self, primary: &'a str, state: &'a [u8]) -> LoadFuture<'a>;
}
pub trait Continuation: Send + Sync {
    /// Check this exact native operation before each restored member's admission.
    fn check(&self) -> crate::native_source::UnitFuture<'_>;
    /// Called only after every root and its goal accounting have been restored.
    fn activate(&self) -> crate::native_source::UnitFuture<'_>;
    /// Release retained recovery DATA only after the actual UI attachment succeeds.
    fn settle(&self) -> crate::native_source::UnitFuture<'_>;
}
