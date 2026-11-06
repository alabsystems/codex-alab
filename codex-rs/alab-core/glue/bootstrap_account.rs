// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

/// Account display is also the TUI's entry to sign-in. A rejected saved login
/// must be presented as needing authentication, rather than killing that entry.
/// Do not clear the shared credential store: a different process may have renewed
/// it. Model routing still calls read_account directly and fails closed.
fn alab_bootstrap_account(
    result: Result<AccountRead, AccountReadError>,
) -> Result<AccountRead, AccountReadError> {
    match result {
        Err(AccountReadError::Routing(
            WorkspaceRoutingError::DiscoveryUnauthorized
            | WorkspaceRoutingError::RefreshTokenRejected(_),
        )) => Ok(AccountRead {
            account_state: ProviderAccountState {
                account: None,
                requires_openai_auth: true,
            },
            workspace_routing: None,
            workspace_name: None,
        }),
        result => result,
    }
}

#[cfg(test)]
mod alab_bootstrap_account_unit_tests {
    use super::*;

    #[test]
    fn unauthorized_routing_requests_sign_in() -> Result<(), AccountReadError> {
        let account =
            alab_bootstrap_account(Err(WorkspaceRoutingError::DiscoveryUnauthorized.into()))?;
        assert!(account.account_state.account.is_none());
        assert!(account.account_state.requires_openai_auth);
        assert!(account.workspace_routing.is_none());
        Ok(())
    }

    #[test]
    fn routing_failures_other_than_authentication_still_fail_closed() {
        for error in [
            WorkspaceRoutingError::BackendConflict,
            WorkspaceRoutingError::DiscoveryFailed,
            WorkspaceRoutingError::AccountChanged,
            WorkspaceRoutingError::DiscoveryTimeout,
        ] {
            let expected = error.to_string();
            assert_eq!(
                alab_bootstrap_account(Err(error.into()))
                    .err()
                    .map(|error| error.to_string()),
                Some(expected)
            );
        }
    }
}
