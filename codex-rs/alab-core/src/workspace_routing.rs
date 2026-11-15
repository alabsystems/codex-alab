// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Stable wire discriminators shared by ALab's workspace-routing producer and consumer.

/// The only workspace-routing failure the TUI may retry automatically.
pub const DISCOVERY_TIMEOUT_REASON: &str = "workspaceRoutingDiscoveryTimeout";
