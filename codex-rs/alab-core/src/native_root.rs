// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Observations of native-retained tool cohorts. None of these data values
//! construct SDK membership, native child custody or complete-work authority.
use std::future::Future;
use std::io;
use std::pin::Pin;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolAction {
    Observe,
    Stop,
}

/// Delivery correlation, not native execution authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IoOffer {
    Rejected,
    Pending { nonce: String },
    Retained { birth: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolState {
    Running { birth: String },
    Finished { birth: String, wait_status: i32 },
}

pub type KeyFuture<'a> = Pin<Box<dyn Future<Output = io::Result<String>> + Send + 'a>>;
pub type StateFuture<'a> = Pin<Box<dyn Future<Output = io::Result<ToolState>> + Send + 'a>>;
pub type IoFuture<'a> = Pin<Box<dyn Future<Output = io::Result<IoOffer>> + Send + 'a>>;

/// The original helper's target-exec observation, separate from birth custody.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolExecution {
    NotSpawned { attempt: String, errno: Option<i32> },
    Refused { errno: Option<i32> },
    AwaitingBirth,
    Pending { birth: String },
    NoExecError { birth: String },
    Failed { birth: String, errno: Option<i32> },
}
pub type ExecutionFuture<'a> = Pin<Box<dyn Future<Output = io::Result<ToolExecution>> + Send + 'a>>;

/// Private RPC credential text is wiped when its owning frontend drops it.
/// Its bytes are not native Root, Source or handoff permission.
pub type TransportSecret = zeroize::Zeroizing<String>;
