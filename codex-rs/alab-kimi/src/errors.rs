// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use codex_api::ApiError;
use codex_api::TransportError;
use codex_model_provider_info::ModelProviderInfo;
use codex_protocol::error::CodexErr;
use serde_json::Value;

#[derive(Clone, Copy)]
enum RateLimitKind {
    Quota,
    Transient,
}

pub fn map_api_error(active: bool, provider: &ModelProviderInfo, error: ApiError) -> CodexErr {
    if !active || !is_kimi_provider(provider) {
        return codex_api::map_api_error(error);
    }
    let retry_after = match &error {
        ApiError::Retryable { retry_after, .. }
        | ApiError::RateLimitExceeded { retry_after, .. }
        | ApiError::Transport(TransportError::Http { retry_after, .. }) => *retry_after,
        _ => None,
    };
    match rate_limit_kind(&error) {
        Some(RateLimitKind::Quota) => match retry_after {
            Some(retry_after) => CodexErr::QuotaExceeded.with_retry_after(retry_after),
            None => CodexErr::QuotaExceeded,
        },
        Some(RateLimitKind::Transient) => {
            codex_api::map_api_error(ApiError::ServerOverloaded { retry_after })
        }
        None => codex_api::map_api_error(error),
    }
}

fn is_kimi_provider(provider: &ModelProviderInfo) -> bool {
    matches!(provider.name.as_str(), "Kimi Code" | "Kimi Code (China)")
        || provider.env_key.as_deref() == Some("KIMI_API_KEY")
}

fn rate_limit_kind(error: &ApiError) -> Option<RateLimitKind> {
    match error {
        ApiError::Transport(TransportError::Http {
            status,
            body: Some(body),
            ..
        }) if status.as_u16() == 429 => {
            let message = serde_json::from_str::<Value>(body)
                .ok()?
                .get("error")?
                .get("message")?
                .as_str()?
                .to_owned();
            classify(&message)
        }
        ApiError::RateLimitExceeded { message, .. } | ApiError::Retryable { message, .. } => {
            classify(message)
        }
        _ => None,
    }
}

fn classify(message: &str) -> Option<RateLimitKind> {
    let message = message.to_ascii_lowercase();
    if [
        "5-hour usage limit",
        "weekly (7-day) usage limit",
        "monthly usage limit",
        "credit balance is insufficient",
        "extra usage balance",
        "extra usage limit",
    ]
    .iter()
    .any(|needle| message.contains(needle))
    {
        return Some(RateLimitKind::Quota);
    }
    if [
        "concurrent request limit",
        "we're receiving too many requests",
        "engine is currently overloaded",
    ]
    .iter()
    .any(|needle| message.contains(needle))
    {
        return Some(RateLimitKind::Transient);
    }
    None
}
