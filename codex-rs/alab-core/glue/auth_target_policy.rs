// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Compare private preview policy with the actual loaded document and routing.
//! Metadata hashes do not issue a source, select a destination, or install a grant.
use super::*;
use codex_alab_core::assignment_service::{Target, TargetPolicy};

#[derive(Clone, Copy)]
pub(super) enum Side { Source, Destination }

fn provider(tokens: &TokenData) -> std::io::Result<(String, String)> {
    let provenance = tokens.oauth_provenance.as_ref().ok_or_else(managed_auth::unavailable)?;
    if provenance.issuer.is_empty() || provenance.issuer.len() > 2048
        || provenance.client_id.is_empty() || provenance.client_id.len() > 1024
        || provenance.issuer.chars().any(char::is_control)
        || provenance.client_id.chars().any(char::is_control) {
        return Err(managed_auth::unavailable());
    }
    let endpoint = format!("{}/oauth/token", provenance.issuer.trim_end_matches('/'));
    let parsed = url::Url::parse(&endpoint).map_err(|_| managed_auth::unavailable())?;
    if parsed.scheme() != "https" || parsed.host_str().is_none() || !parsed.username().is_empty()
        || parsed.password().is_some() || parsed.query().is_some() || parsed.fragment().is_some() {
        return Err(managed_auth::unavailable());
    }
    let mut hash = Sha256::new();
    hash.update(b"aterm/codex-assignment-provider/v1\0");
    for field in [endpoint.as_bytes(), provenance.client_id.as_bytes()] {
        let length = u32::try_from(field.len()).map_err(|_| managed_auth::unavailable())?;
        hash.update(length.to_be_bytes());
        hash.update(field);
    }
    Ok((format!("{:x}", hash.finalize()), endpoint))
}

/// Sanitized policy witness from the actual verified loaded document. It owns
/// no token, path, manager, native capability, or fabricated manager counters.
pub(crate) struct Witness { provider: String, endpoint: String, fedramp: bool, provenance: crate::token_data::AccountOAuthProvenance }
impl Witness {
    pub(crate) fn from_tokens(tokens: &TokenData) -> std::io::Result<Self> {
        let (provider, endpoint) = provider(tokens)?;
        let provenance = tokens.oauth_provenance.clone().ok_or_else(managed_auth::unavailable)?;
        Ok(Self { provider, endpoint, fedramp: tokens.id_token.chatgpt_account_is_fedramp, provenance })
    }
    fn check_policy(&self, policy: &TargetPolicy, side: Side, effective_override: Option<&str>) -> std::io::Result<()> {
        let (expected, fedramp) = match side {
            Side::Source => (&policy.source_provider, policy.source_fedramp),
            Side::Destination => (&policy.target_provider, policy.target_fedramp),
        };
        if &self.provider != expected || self.fedramp != fedramp
            || self.endpoint != policy.target_refresh_endpoint
            || effective_override.is_some_and(|configured| configured != self.endpoint) {
            return Err(managed_auth::unavailable());
        }
        Ok(())
    }
    pub(crate) fn source_target(&self, target: &Target) -> std::io::Result<()> {
        target.validate(&target.operation)?;
        let effective = super::super::refresh_token_endpoint(Some(&self.provenance));
        self.check_policy(&target.policy, Side::Source, Some(&effective))
    }
}
#[cfg(test)]
fn check(tokens: &TokenData, policy: &TargetPolicy, side: Side, effective_override: Option<&str>) -> std::io::Result<()> {
    Witness::from_tokens(tokens)?.check_policy(policy, side, effective_override)
}
pub(super) fn document(tokens: &TokenData, target: &Target, side: Side) -> std::io::Result<()> {
    target.validate(&target.operation)?;
    // Proven documents use their recorded client_id; the environment/default
    // client fallback is not effective in this branch.
    let witness = Witness::from_tokens(tokens)?;
    let effective = super::super::refresh_token_endpoint(Some(&witness.provenance));
    witness.check_policy(&target.policy, side, Some(&effective))?;
    if matches!(side, Side::Destination) {
        let account = tokens.account_id.as_deref().or(tokens.id_token.chatgpt_account_id.as_deref())
            .ok_or_else(managed_auth::unavailable)?;
        let user = tokens.id_token.chatgpt_user_id.as_deref().ok_or_else(managed_auth::unavailable)?;
        if principal(b"aterm/codex-renewal/account/v1\0", account) != target.account
            || principal(b"aterm/codex-renewal/user/v1\0", user) != target.user {
            return Err(managed_auth::unavailable());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (TokenData, TargetPolicy) {
        let tokens = TokenData { oauth_provenance: Some(crate::token_data::AccountOAuthProvenance {
            issuer: "https://auth.example.invalid".into(), client_id: "recorded-client".into(),
        }), ..Default::default() };
        let (binding, endpoint) = provider(&tokens).unwrap();
        let policy = TargetPolicy { source_provider: binding.clone(), target_provider: binding,
            source_fedramp: false, target_fedramp: false, target_refresh_endpoint: endpoint };
        (tokens, policy)
    }
    #[test]
    fn target_policy_uses_actual_provenance_client_realm_and_effective_route() {
        let (tokens, policy) = fixture();
        assert!(check(&tokens, &policy, Side::Source, None).is_ok());
        assert!(check(&tokens, &policy, Side::Destination, Some(&policy.target_refresh_endpoint)).is_ok());
        for route in ["", "https://different.example.invalid/oauth/token", "https://auth.example.invalid/oauth/token/"] {
            assert!(check(&tokens, &policy, Side::Source, Some(route)).is_err());
        }
        let mut changed = tokens.clone();
        changed.oauth_provenance.as_mut().unwrap().client_id = "different-client".into();
        assert!(check(&changed, &policy, Side::Source, None).is_err());
        changed = tokens.clone();
        changed.id_token.chatgpt_account_is_fedramp = true;
        assert!(check(&changed, &policy, Side::Destination, None).is_err());
        changed = tokens;
        changed.oauth_provenance = None;
        changed.id_token.raw_jwt = "unverified.issuer.claim".into();
        assert!(check(&changed, &policy, Side::Source, None).is_err(), "JWT claims/default client are not provenance");
    }
    #[test]
    fn provider_hash_is_domain_separated_and_length_delimited() {
        let (tokens, policy) = fixture();
        let mut expected = Sha256::new();
        expected.update(b"aterm/codex-assignment-provider/v1\0");
        for value in ["https://auth.example.invalid/oauth/token", "recorded-client"] {
            expected.update(u32::try_from(value.len()).unwrap().to_be_bytes());
            expected.update(value.as_bytes());
        }
        assert_eq!(policy.source_provider, format!("{:x}", expected.finalize()));
        let mut slash = tokens;
        slash.oauth_provenance.as_mut().unwrap().issuer.push('/');
        assert_eq!(provider(&slash).unwrap().0, policy.source_provider);
    }
    const ROUTE_CHILD: &str = "ALAB_TARGET_POLICY_ROUTE_CHILD";
    #[test]
    fn target_policy_observes_actual_effective_refresh_helper() {
        let cases = vec![
            (std::ffi::OsString::from("https://auth.example.invalid/oauth/token"), true),
            (std::ffi::OsString::from("https://other.example.invalid/oauth/token"), false),
        ];
        #[cfg(unix)]
        let cases = {
            use std::os::unix::ffi::OsStringExt;
            let mut values = cases;
            values.push((std::ffi::OsString::from_vec(vec![0xff]), true));
            values
        };
        let test = concat!(module_path!(), "::effective_route_child").split_once("::").unwrap().1;
        for (endpoint, accepted) in cases {
            let output = std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact", test])
                .env(ROUTE_CHILD, if accepted { "accept" } else { "refuse" })
                .env(REFRESH_TOKEN_URL_OVERRIDE_ENV_VAR, endpoint)
                .env(super::super::super::CLIENT_ID_OVERRIDE_ENV_VAR, "unused-environment-client")
                .output().unwrap();
            assert!(output.status.success(), "{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
            assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
        }
    }
    #[test]
    fn effective_route_child() {
        let Ok(expected) = std::env::var(ROUTE_CHILD) else { return; };
        let (tokens, policy) = fixture();
        let target = Target { operation: "a".repeat(32), pool_revision: "b".repeat(64), expected_assignment: 0,
            target_ref: "c".repeat(64), account: "d".repeat(64), user: "e".repeat(64), source_grant: "f".repeat(64),
            target_grant: "a".repeat(64), policy };
        assert_eq!(document(&tokens, &target, Side::Source).is_ok(), expected == "accept");
        assert_eq!(Witness::from_tokens(&tokens).unwrap().source_target(&target).is_ok(), expected == "accept");
    }

}
