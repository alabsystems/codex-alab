// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

/// Preserve the human-readable error while giving ALab clients a stable discriminator for the
/// one transient routing failure. Vanilla app-server behavior and every other routing error keep
/// their upstream, unstructured response.
fn alab_workspace_routing_rpc_error(error: WorkspaceRoutingError) -> JSONRPCErrorError {
    alab_workspace_routing_rpc_error_for(error, codex_alab_core::active())
}

fn alab_workspace_routing_rpc_error_for(
    error: WorkspaceRoutingError,
    alab_active: bool,
) -> JSONRPCErrorError {
    let is_timeout = matches!(&error, WorkspaceRoutingError::DiscoveryTimeout);
    let mut response = internal_error(error.to_string());
    if alab_active && is_timeout {
        response.data = Some(serde_json::json!({
            "reason": codex_alab_core::workspace_routing::DISCOVERY_TIMEOUT_REASON,
        }));
    }
    response
}

#[cfg(test)]
mod alab_workspace_routing_error_tests {
    use super::*;

    #[test]
    fn active_timeout_has_a_stable_reason_without_changing_its_legacy_error() {
        let error = alab_workspace_routing_rpc_error_for(
            WorkspaceRoutingError::DiscoveryTimeout,
            /*alab_active*/ true,
        );

        assert_eq!(error.code, -32603);
        assert_eq!(error.message, "workspace routing discovery timed out");
        assert_eq!(
            error.data,
            Some(serde_json::json!({
                "reason": codex_alab_core::workspace_routing::DISCOVERY_TIMEOUT_REASON,
            }))
        );
    }

    #[test]
    fn vanilla_and_non_timeout_errors_remain_unstructured() {
        let vanilla = alab_workspace_routing_rpc_error_for(
            WorkspaceRoutingError::DiscoveryTimeout,
            /*alab_active*/ false,
        );
        let other = alab_workspace_routing_rpc_error_for(
            WorkspaceRoutingError::MissingWorkspace,
            /*alab_active*/ true,
        );

        assert_eq!(vanilla.message, "workspace routing discovery timed out");
        assert_eq!(vanilla.data, None);
        assert_eq!(
            other.message,
            "selected workspace missing from routing discovery"
        );
        assert_eq!(other.data, None);
    }
}
