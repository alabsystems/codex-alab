// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use codex_alab_core::native_source_lifetime::NetworkScope;
impl AuthRouteConfig {
    /// Compare actual configuration owners independently of endpoint narrowing.
    /// This proves no credential, native use or network destination admission.
    pub fn alab_has_configuration_owners(
        &self,
        application: &NetworkPolicy,
        local: &NetworkPolicy,
    ) -> std::io::Result<bool> {
        let same_owner = |actual: &NetworkPolicy, expected: &NetworkPolicy| {
            actual.alab_managed_configuration()?;
            expected.alab_managed_configuration()?;
            Ok::<_, std::io::Error>(
                actual.clone().restrict_to_endpoints(Default::default())
                    == expected.clone().restrict_to_endpoints(Default::default()),
            )
        };
        let Some(bootstrap) = self.local_bootstrap_factory.as_ref() else {
            return Ok(false);
        };
        Ok(
            same_owner(self.http_client_factory.network_policy(), application)?
                && same_owner(&self.application_network_policy, application)?
                && same_owner(bootstrap.network_policy(), local)?,
        )
    }

    /// Allocate an unavailable, separately owned destination account policy.
    /// The resource owner must load B's requirements and retain the returned
    /// controller for subsequent updates. Bootstrap keeps the live local rules;
    /// no source-account policy snapshot, cookie jar or native capability moves.
    pub fn alab_fresh_destination_routes(
        &self,
    ) -> std::io::Result<(Self, codex_http_client::NetworkPolicyController)> {
        let managed = self
            .application_network_policy
            .alab_managed_configuration()?;
        let bootstrap = match &self.local_bootstrap_factory {
            Some(factory) => factory,
            None if !managed => &self.http_client_factory,
            None => {
                return Err(std::io::Error::other(
                    "local bootstrap configuration unavailable",
                ));
            }
        };
        bootstrap.network_policy().alab_managed_configuration()?;
        let controller = codex_http_client::NetworkPolicyController::default();
        let application_network_policy = self
            .application_network_policy
            .alab_for_destination_controller(&controller)?;
        let content = self
            .http_client_factory
            .network_policy()
            .alab_for_destination_controller(&controller)?
            .with_account_binding(&application_network_policy)
            .ok_or_else(|| std::io::Error::other("destination configuration constraints differ"))?;
        let clear_cookies =
            |factory: &HttpClientFactory| factory.clone().with_chatgpt_cookies(std::iter::empty());
        Ok((
            Self {
                http_client_factory: clear_cookies(&self.http_client_factory)
                    .with_network_policy(content),
                application_network_policy,
                local_bootstrap_factory: Some(clear_cookies(bootstrap)),
            },
            controller,
        ))
    }

    /// Refresh local transport options while retaining these exact destination
    /// and local controllers plus every prior endpoint restriction. No policy
    /// is published and no credential or cookie state is transferred.
    pub fn alab_rebase_destination_options(&self, local: &Self) -> std::io::Result<Self> {
        let unavailable = || std::io::Error::other("destination local route options unavailable");
        let original_bootstrap = self
            .local_bootstrap_factory
            .as_ref()
            .ok_or_else(unavailable)?;
        let next_bootstrap = local
            .local_bootstrap_factory
            .as_ref()
            .unwrap_or(&local.http_client_factory);
        for policy in [
            self.http_client_factory.network_policy(),
            &self.application_network_policy,
            original_bootstrap.network_policy(),
        ] {
            if !policy.alab_managed_configuration()? {
                return Err(unavailable());
            }
        }
        // Local ConfigBuilder options have no account controller. A foreign
        // managed controller must not be stripped to make a join succeed.
        for policy in [
            local.http_client_factory.network_policy(),
            &local.application_network_policy,
            next_bootstrap.network_policy(),
        ] {
            if policy.alab_managed_configuration()? {
                return Err(unavailable());
            }
        }
        let application = self
            .application_network_policy
            .clone()
            .with_account_binding(&local.application_network_policy)
            .ok_or_else(unavailable)?;
        let content = self
            .http_client_factory
            .network_policy()
            .clone()
            .with_account_binding(local.http_client_factory.network_policy())
            .ok_or_else(unavailable)?
            .with_account_binding(&application)
            .ok_or_else(unavailable)?;
        let bootstrap = original_bootstrap
            .network_policy()
            .clone()
            .with_account_binding(next_bootstrap.network_policy())
            .ok_or_else(unavailable)?;
        Ok(Self {
            http_client_factory: local
                .http_client_factory
                .clone()
                .with_chatgpt_cookies(std::iter::empty())
                .with_network_policy(content),
            application_network_policy: application,
            local_bootstrap_factory: Some(
                next_bootstrap
                    .clone()
                    .with_chatgpt_cookies(std::iter::empty())
                    .with_network_policy(bootstrap),
            ),
        })
    }

    pub(crate) fn alab_with_native_source(&self, scope: Result<NetworkScope, ()>) -> Self {
        let bind = |factory: &HttpClientFactory| {
            factory.clone().with_network_policy(
                factory
                    .network_policy()
                    .clone()
                    .with_native_source(scope.clone()),
            )
        };
        Self {
            http_client_factory: bind(&self.http_client_factory),
            application_network_policy: self
                .application_network_policy
                .clone()
                .with_native_source(scope.clone()),
            local_bootstrap_factory: self.local_bootstrap_factory.as_ref().map(bind),
        }
    }
}

#[cfg(test)]
mod destination_tests {
    use super::*;
    use codex_http_client::DestinationPolicy;
    use codex_http_client::NetworkPolicyController;
    use codex_http_client::NetworkPolicyDenied;
    use codex_http_client::OutboundProxyPolicy;
    use std::collections::BTreeSet;

    fn factory(policy: NetworkPolicy) -> HttpClientFactory {
        HttpClientFactory::new(OutboundProxyPolicy::RespectSystemProxy)
            .with_sensitive_header_protection()
            .with_network_policy(policy)
            .with_chatgpt_cookies(["sid=source-account".parse().unwrap()])
    }

    #[test]
    fn destination_policy_starts_unavailable_and_never_invalidates_source() {
        let source = NetworkPolicyController::default();
        let local = NetworkPolicyController::default();
        let url = url::Url::parse("https://auth.example.test/oauth/token").unwrap();
        let limited = DestinationPolicy::Restricted {
            allowed_hosts: BTreeSet::from(["auth.example.test".into()]),
        };
        assert!(source.publish(source.policy().revision(), limited.clone()));
        assert!(local.publish(local.policy().revision(), limited.clone()));
        let source_permit = source.policy().for_current_account().acquire(&url).unwrap();
        let routes = AuthRouteConfig::from_http_client_factory(factory(source.policy()))
            .with_local_bootstrap_factory(factory(local.policy()));
        let (next, destination) = routes.alab_fresh_destination_routes().unwrap();
        assert!(matches!(
            next.application_network_policy.acquire(&url),
            Err(NetworkPolicyDenied::Unavailable)
        ));
        assert!(source_permit.check().is_ok());
        assert!(!next.http_client_factory.has_chatgpt_cookies());
        assert!(
            !next
                .local_bootstrap_factory
                .as_ref()
                .unwrap()
                .has_chatgpt_cookies()
        );
        assert!(next.http_client_factory.protects_sensitive_headers());
        assert_eq!(
            next.http_client_factory.outbound_proxy_policy(),
            OutboundProxyPolicy::RespectSystemProxy
        );
        assert!(destination.publish(destination.policy().revision(), limited));
        let destination_permit = next
            .application_network_policy
            .clone()
            .for_current_account()
            .acquire(&url)
            .unwrap();
        source.policy().invalidate();
        assert!(destination_permit.check().is_ok());
        assert!(
            next.http_client_factory
                .network_policy()
                .acquire(&url)
                .is_ok()
        );
        assert!(
            next.application_network_policy
                .with_account_binding(&source.policy())
                .is_none()
        );
    }

    #[test]
    fn bootstrap_keeps_live_local_tightening_and_exact_endpoint_limits() {
        let source = NetworkPolicyController::default();
        let local = NetworkPolicyController::default();
        let url = url::Url::parse("https://auth.example.test/oauth/token").unwrap();
        let other = url::Url::parse("https://auth.example.test/other").unwrap();
        let allowed = DestinationPolicy::Restricted {
            allowed_hosts: BTreeSet::from(["auth.example.test".into()]),
        };
        assert!(source.publish(source.policy().revision(), allowed.clone()));
        assert!(local.publish(local.policy().revision(), allowed.clone()));
        let routes = AuthRouteConfig::from_http_client_factory(factory(
            source
                .policy()
                .restrict_to_endpoints(BTreeSet::from([url.clone()])),
        ))
        .with_local_bootstrap_factory(factory(local.policy()));
        let (next, destination) = routes.alab_fresh_destination_routes().unwrap();
        assert!(destination.publish(destination.policy().revision(), allowed));
        assert!(matches!(
            next.http_client_factory.network_policy().acquire(&other),
            Err(NetworkPolicyDenied::Destination)
        ));
        let bootstrap = next.http_client_factory_for_bootstrap([url.clone()]);
        let permit = bootstrap.network_policy().acquire(&url).unwrap();
        assert!(matches!(
            bootstrap.network_policy().acquire(&other),
            Err(NetworkPolicyDenied::Destination)
        ));
        assert!(local.publish(
            local.policy().revision(),
            DestinationPolicy::Restricted {
                allowed_hosts: BTreeSet::new(),
            }
        ));
        assert_eq!(permit.check(), Err(NetworkPolicyDenied::Revoked));
        assert!(matches!(
            bootstrap.network_policy().acquire(&url),
            Err(NetworkPolicyDenied::Destination)
        ));
    }

    #[test]
    fn credential_bound_routes_and_ambiguous_managed_bootstrap_are_refused() {
        let source = NetworkPolicyController::default();
        let local = NetworkPolicyController::default();
        let ambiguous = AuthRouteConfig::from_http_client_factory(factory(source.policy()));
        assert!(ambiguous.alab_fresh_destination_routes().is_err());
        let bound = ambiguous
            .clone()
            .with_local_bootstrap_factory(factory(local.policy().for_current_account()));
        assert!(bound.alab_fresh_destination_routes().is_err());
        let native_bound = ambiguous
            .with_local_bootstrap_factory(factory(local.policy().with_native_source(Err(()))));
        assert!(native_bound.alab_fresh_destination_routes().is_err());
        assert!(
            source
                .policy()
                .alab_for_destination_controller(&source)
                .is_err()
        );
    }
    #[test]
    fn rebase_uses_new_proxy_options_and_keeps_old_endpoint_and_live_local_limits() {
        let source = NetworkPolicyController::default();
        let local = NetworkPolicyController::default();
        let endpoint: url::Url = "https://auth.example.test/only".parse().unwrap();
        let other: url::Url = "https://auth.example.test/other".parse().unwrap();
        assert!(local.publish(local.policy().revision(), DestinationPolicy::Unrestricted));
        let routes = AuthRouteConfig::from_http_client_factory(factory(source.policy()))
            .with_local_bootstrap_factory(factory(
                local
                    .policy()
                    .restrict_to_endpoints(BTreeSet::from([endpoint.clone()])),
            ));
        let (destination, controller) = routes.alab_fresh_destination_routes().unwrap();
        let options = AuthRouteConfig::from_http_client_factory(
            HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault)
                .with_chatgpt_cookies(["sid=new-local-option".parse().unwrap()]),
        );
        let rebased = destination
            .alab_rebase_destination_options(&options)
            .unwrap();
        assert_eq!(
            rebased.http_client_factory.outbound_proxy_policy(),
            OutboundProxyPolicy::ReqwestDefault
        );
        assert!(!rebased.http_client_factory.has_chatgpt_cookies());
        assert!(
            !rebased
                .local_bootstrap_factory
                .as_ref()
                .unwrap()
                .has_chatgpt_cookies()
        );
        assert!(
            rebased
                .alab_has_configuration_owners(&controller.policy(), &local.policy())
                .unwrap()
        );
        assert!(matches!(
            rebased
                .http_client_factory
                .network_policy()
                .acquire(&endpoint),
            Err(NetworkPolicyDenied::Unavailable)
        ));
        let bootstrap =
            rebased.http_client_factory_for_bootstrap([endpoint.clone(), other.clone()]);
        let permit = bootstrap.network_policy().acquire(&endpoint).unwrap();
        assert!(matches!(
            bootstrap.network_policy().acquire(&other),
            Err(NetworkPolicyDenied::Destination)
        ));
        local.policy().invalidate();
        assert!(matches!(permit.check(), Err(NetworkPolicyDenied::Revoked)));
        let foreign = AuthRouteConfig::from_http_client_factory(factory(
            NetworkPolicyController::default().policy(),
        ));
        assert!(
            destination
                .alab_rebase_destination_options(&foreign)
                .is_err()
        );
        let bound = AuthRouteConfig::from_http_client_factory(factory(
            source.policy().for_current_account(),
        ));
        assert!(destination.alab_rebase_destination_options(&bound).is_err());
        let native = AuthRouteConfig::from_http_client_factory(factory(
            NetworkPolicy::default().with_native_source(Err(())),
        ));
        assert!(
            destination
                .alab_rebase_destination_options(&native)
                .is_err()
        );
    }
}
