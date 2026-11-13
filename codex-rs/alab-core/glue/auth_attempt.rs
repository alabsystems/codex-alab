// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Record the actual credential selected for each foreground model attempt.
//! Background compaction deliberately cannot replace a foreground wall's proof.

pub(super) fn record(
    client: &super::ModelClient,
    setup: &super::CurrentClientSetup,
    turn: Option<&str>,
) {
    use codex_alab_core::renewal::RequestAuth;
    if !codex_alab_core::active() {
        return;
    }
    let (Some(turn), Some(manager)) = (turn, client.auth_manager()) else {
        return;
    };
    let provider = client.state.provider.info();
    let kind = setup
        .auth
        .as_ref()
        .filter(|_| {
            provider.is_openai()
                && provider.requires_openai_auth
                && provider.env_key.is_none()
                && provider.experimental_bearer_token.is_none()
                && provider.auth.is_none()
                && provider.aws.is_none()
        })
        .and_then(|auth| {
            if auth.auth_mode() == codex_protocol::auth::AuthMode::Chatgpt {
                auth.get_account_id()
                    .zip(auth.get_chatgpt_user_id())
                    .map(|(account, user)| RequestAuth::ChatGpt { account, user })
            } else if auth.is_api_key_auth() {
                Some(RequestAuth::OpenAiApiKey)
            } else {
                None
            }
        });
    let selected_owner = setup.auth_owner.clone();
    let owner = selected_owner
        .as_ref()
        .map(codex_login::AuthOwner::alab_generation);
    let generation = setup.auth_revision;
    // Freeze only the actual Authorization bearer, not refresh/ID tokens or
    // mutable login metadata. No value or fingerprint leaves this RAM closure.
    let attempted_bearer = kind.as_ref().and_then(|_| {
        let mut headers = http::HeaderMap::new();
        setup.api_auth.add_auth_headers(&mut headers).ok()?;
        headers
            .get(http::header::AUTHORIZATION)?
            .to_str()
            .ok()?
            .strip_prefix("Bearer ")
            .map(str::to_owned)
    });
    // An unobservable Authorization value cannot prove an attempted bearer.
    let kind = kind.filter(|_| attempted_bearer.is_some());
    let api_key = matches!(kind, Some(RequestAuth::OpenAiApiKey));
    let changed_manager = std::sync::Arc::clone(&manager);
    let key = std::sync::Arc::as_ptr(&manager) as usize;
    codex_alab_core::renewal::record_auth_attempt(
        key,
        turn,
        kind,
        owner.unwrap_or_default(),
        generation.unwrap_or_default(),
        move || {
            selected_owner
                .as_ref()
                .is_some_and(codex_login::AuthOwner::is_current)
                && manager
                    .auth_cached()
                    .and_then(|auth| manager.auth_change_state_for(&auth))
                    .is_some_and(|now| {
                        Some(now.owner_generation) == owner
                            && (!api_key || Some(now.generation) == generation)
                    })
        },
        move || {
            attempted_bearer
                .as_ref()
                .zip(
                    changed_manager
                        .auth_cached()
                        .and_then(|auth| auth.get_token().ok())
                        .as_ref(),
                )
                .is_some_and(|(attempted, current)| attempted != current)
        },
    );
}
