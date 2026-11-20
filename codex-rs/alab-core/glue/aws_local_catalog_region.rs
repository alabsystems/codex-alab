// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use aws_config::environment::EnvironmentVariableRegionProvider;
use aws_config::meta::region::RegionProviderChain;
use aws_config::profile::ProfileFileRegionProvider;
use std::future::Future;
use std::pin::pin;
use std::task::Context;
use std::task::Poll;
use std::task::Waker;

/// Read the local prefix of the SDK region chain. The pinned SDK reads profile
/// files synchronously, including source_profile inheritance. Catalog creation
/// must never start an instance-metadata request or wait for an async resolver.
pub fn locally_available_region(profile: Option<&str>) -> Option<String> {
    let mut files = ProfileFileRegionProvider::builder();
    if let Some(name) = profile {
        files = files.profile_name(name);
    }
    let chain = RegionProviderChain::first_try(EnvironmentVariableRegionProvider::new())
        .or_else(files.build());
    match pin!(chain.region()).poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(region) => region.map(|region| region.to_string()),
        Poll::Pending => None,
    }
}
