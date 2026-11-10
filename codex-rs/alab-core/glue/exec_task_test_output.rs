// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Native task completions for the restored-child and private-review fixtures.

pub(crate) fn completed(message: impl Into<String>) -> super::regular::RegularTaskOutput {
    super::regular::RegularTaskOutput {
        last_agent_message: Some(message.into()),
        mcp_startup_requirements: Default::default(),
    }
}
