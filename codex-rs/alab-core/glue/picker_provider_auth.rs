// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Check the actual last signer for a process resumed onto a confirmed subscription.
//! Informational account metadata can describe a different credential than the HTTP signer.

use codex_alab_core::bound_picker;
use codex_api::AuthError;
use codex_api::AuthHeaderTelemetry;
use codex_api::AuthHeadersFuture;
use codex_api::AuthProvider;
use codex_api::AuthProviderFuture;
use codex_api::AuthRefreshFuture;
use codex_api::SharedAuthProvider;
use codex_http_client::CHATGPT_ACCOUNT_ID_HEADER;
use codex_http_client::NetworkPolicy;
use codex_http_client::Request;
use codex_login::CodexAuth;
use codex_login::TokenData;
use codex_login::default_client::ClientRedirectPolicy;
use codex_model_provider_info::ModelProviderInfo;
use codex_protocol::error::CodexErr;
use codex_runtime_agent_instance::ActorInfo;
use http::HeaderMap;
use http::header::AUTHORIZATION;
use std::sync::Arc;

fn permitted(info: &ModelProviderInfo) -> bool {
    crate::provider_uses_first_party_auth_path(info)
        && info.gateway_oauth.is_none()
        && !info.is_gcp()
        && !info.is_amazon_bedrock()
}

pub(crate) fn admit(info: &ModelProviderInfo) -> codex_protocol::error::Result<()> {
    if bound_picker::expected().is_some() && !permitted(info) {
        return Err(CodexErr::InvalidRequest(
            bound_picker::unavailable().to_string(),
        ));
    }
    Ok(())
}

pub(crate) fn redirect_policy(ordinary: ClientRedirectPolicy) -> ClientRedirectPolicy {
    if bound_picker::expected().is_some() {
        ClientRedirectPolicy::Reject
    } else {
        ordinary
    }
}

pub(crate) fn bind(
    info: &ModelProviderInfo,
    resolved: &mut crate::ResolvedProviderAuth,
) -> codex_protocol::error::Result<()> {
    if let Some(expected) = bound_picker::expected() {
        admit(info)?;
        bind_selected(resolved, expected)?;
    }
    Ok(())
}

fn bind_selected(
    resolved: &mut crate::ResolvedProviderAuth,
    expected: &str,
) -> codex_protocol::error::Result<()> {
    let Some(snapshot @ CodexAuth::Chatgpt(_)) = resolved.credential.as_ref() else {
        return Err(CodexErr::InvalidRequest(
            bound_picker::unavailable().to_string(),
        ));
    };
    if let Some(runtime) = &resolved.runtime
        && !matches!(runtime.runtime().actor().info(), ActorInfo::Chatgpt {account_id,user_id} if bound_picker::principal(account_id,user_id)==expected)
    {
        return Err(CodexErr::InvalidRequest(
            bound_picker::unavailable().to_string(),
        ));
    }
    resolved.auth = Arc::new(BoundSigner {
        signer: resolved.auth.clone(),
        snapshot: snapshot.clone(),
        expected: expected.to_owned(),
        runtime: resolved.runtime.is_some(),
    });
    Ok(())
}

struct BoundSigner {
    signer: SharedAuthProvider,
    snapshot: CodexAuth,
    expected: String,
    runtime: bool,
}

fn unavailable() -> AuthError {
    AuthError::Build(bound_picker::unavailable().to_string())
}

fn one<'a>(headers: &'a HeaderMap, key: &str) -> Result<&'a str, AuthError> {
    let mut values = headers.get_all(key).iter();
    let value = values.next().ok_or_else(unavailable)?;
    if values.next().is_some() {
        return Err(unavailable());
    }
    value.to_str().map_err(|_| unavailable())
}

impl BoundSigner {
    fn check(&self, headers: &HeaderMap) -> Result<(), AuthError> {
        // Extract credentials only after the original signer and its policy admitted this use.
        let tokens = self.snapshot.get_token_data().map_err(|_| unavailable())?;
        // These secondary credential carriers and platform selectors cannot identify a
        // subscription. Only a matching native runtime may add its own signed assertion.
        if [
            "cookie",
            "cookie2",
            "api-key",
            "x-api-key",
            codex_http_client::OPENAI_ORGANIZATION_HEADER,
            codex_http_client::OPENAI_PROJECT_HEADER,
        ]
        .iter()
        .any(|name| headers.contains_key(*name))
            || (!self.runtime && headers.contains_key("x-openai-agent-assertion"))
        {
            return Err(unavailable());
        }
        check(headers, &tokens, &self.expected)
    }
}

fn check(headers: &HeaderMap, tokens: &TokenData, expected: &str) -> Result<(), AuthError> {
    let (scheme, bearer) = one(headers, AUTHORIZATION.as_str())?
        .split_once(' ')
        .ok_or_else(unavailable)?;
    if !scheme.eq_ignore_ascii_case("Bearer")
        || bearer.is_empty()
        || bearer.bytes().any(|b| b.is_ascii_whitespace())
        || bearer != tokens.access_token
    {
        return Err(unavailable());
    }
    let workspace = one(headers, CHATGPT_ACCOUNT_ID_HEADER)?;
    if tokens.account_id.as_deref() != Some(workspace)
        || !CodexAuth::alab_picker_tokens_match(expected, tokens)
    {
        return Err(unavailable());
    }
    Ok(())
}

impl AuthProvider for BoundSigner {
    fn add_auth_headers(&self, headers: &mut HeaderMap) -> Result<(), AuthError> {
        let mut assigned = headers.clone();
        self.signer.add_auth_headers(&mut assigned)?;
        self.check(&assigned)?;
        *headers = assigned;
        Ok(())
    }

    fn apply_auth_headers(&self, headers: HeaderMap) -> AuthHeadersFuture<'_> {
        Box::pin(async move {
            let assigned = self.signer.apply_auth_headers(headers).await?;
            self.check(&assigned)?;
            Ok(assigned)
        })
    }

    fn apply_auth(&self, request: Request) -> AuthProviderFuture<'_> {
        Box::pin(async move {
            let assigned = self.signer.apply_auth(request).await?;
            self.check(&assigned.headers)?;
            Ok(assigned)
        })
    }

    // Policy, telemetry and refresh describe the original signer. A retry is checked at use.
    fn auth_header_telemetry(&self) -> Result<AuthHeaderTelemetry, AuthError> {
        self.signer.auth_header_telemetry()
    }
    fn alab_source_policy(&self) -> Result<Option<NetworkPolicy>, AuthError> {
        self.signer.alab_source_policy()
    }
    fn alab_gateway_recovery(&self) -> Option<AuthRefreshFuture<'_>> {
        self.signer.alab_gateway_recovery()
    }
    fn refresh_after_unauthorized(&self, headers: HeaderMap) -> AuthRefreshFuture<'_> {
        self.signer.refresh_after_unauthorized(headers)
    }
}

#[cfg(test)]
#[path = "picker_provider_auth_tests.rs"]
mod tests;
