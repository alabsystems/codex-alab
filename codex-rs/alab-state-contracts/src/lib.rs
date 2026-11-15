// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Persistence contracts independent of application and UI test harnesses.

#[cfg(test)]
mod endpoint_diagnostic_tests;
#[cfg(test)]
mod queue_loss_contract_tests;
#[cfg(test)]
mod recovery_diagnostic_tests;
#[cfg(test)]
#[path = "../../alab-core/glue/state_log_filter.rs"]
mod state_log_filter;
#[cfg(test)]
#[path = "../../alab-core/glue/test_child_process.rs"]
mod test_child_process;
#[cfg(test)]
mod test_process;
#[cfg(test)]
mod websocket_handshake_diagnostic_tests;
