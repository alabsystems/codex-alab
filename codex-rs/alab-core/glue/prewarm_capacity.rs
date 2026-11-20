// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A refused speculative handshake can carry a deadline for the first real request.
//! Hand the original clock to the foreground retry loop, which owns notice and cancellation.

use super::ApiHeaderMap;
use super::AuthOwner;
use super::CodexErr;
use super::CodexErrorDetails;
use super::CodexResponsesMetadata;
use super::CurrentClientSetup;
use super::ResponsesConnectionKey;
use super::is_guardian_reviewer;
use codex_alab_core::capacity::CapacityReason;
use codex_http_client::ChatGptRoutingAffinity;
use codex_http_client::RetryAfter;
use std::sync::Mutex;
use std::sync::PoisonError;

#[derive(PartialEq, Eq)]
pub(super) struct Route {
    connection: ResponsesConnectionKey,
    owner: Option<AuthOwner>,
    runtime: Option<String>,
    headers: ApiHeaderMap,
    parent_affinity: Option<ChatGptRoutingAffinity>,
    model: String,
    tier: Option<String>,
}

struct Pending {
    route: Box<Route>,
    reason: CapacityReason,
    advice: RetryAfter,
}

#[derive(Default)]
pub(super) struct PrewarmCapacity(Mutex<Option<Pending>>);

impl PrewarmCapacity {
    pub(super) fn before(
        &self,
        setup: &CurrentClientSetup,
        model: &str,
        tier: Option<&str>,
        headers: &ApiHeaderMap,
        metadata: &CodexResponsesMetadata,
        warmup: bool,
    ) -> Result<Option<Box<Route>>, CodexErr> {
        if !codex_alab_core::active() {
            return Ok(None);
        }
        let mut pending = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if !warmup && pending.is_none() {
            return Ok(None);
        }
        let guardian = is_guardian_reviewer(headers);
        let route = Route {
            connection: ResponsesConnectionKey::new(&setup.api_provider, setup.auth_revision),
            owner: setup.auth_owner.clone(),
            runtime: setup.runtime_id.clone(),
            headers: headers.clone(),
            parent_affinity: if guardian {
                metadata.parent_routing_affinity.clone()
            } else {
                None
            },
            model: model.to_owned(),
            tier: if guardian {
                None
            } else {
                tier.map(str::to_owned)
            },
        };
        // The first real attempt consumes the handoff, even when its route changed.
        // A second prewarm matching live advice also needs to avoid an early handshake.
        if let Some(previous) = pending.take()
            && *previous.route == route
            && previous
                .route
                .owner
                .as_ref()
                .is_none_or(AuthOwner::is_current)
            && !previous.advice.remaining_delay().is_zero()
        {
            let details = match previous.reason {
                CapacityReason::ServerOverloaded => CodexErrorDetails::ServerOverloaded,
                CapacityReason::PolicyCapacityUnavailable => {
                    CodexErrorDetails::PolicyCapacityUnavailable
                }
            };
            let error = CodexErr::new(details).with_retry_after(previous.advice);
            if warmup {
                *pending = Some(previous);
            }
            return Err(error);
        }
        Ok(warmup.then(|| Box::new(route)))
    }

    pub(super) fn remember(&self, route: Option<Box<Route>>, error: CodexErr) -> CodexErr {
        if let Some(route) = route
            && let Some(advice) = error.retry_after()
            && !advice.remaining_delay().is_zero()
        {
            let reason = match error.details() {
                CodexErrorDetails::ServerOverloaded => CapacityReason::ServerOverloaded,
                CodexErrorDetails::PolicyCapacityUnavailable => {
                    CapacityReason::PolicyCapacityUnavailable
                }
                _ => return error,
            };
            let mut pending = self.0.lock().unwrap_or_else(PoisonError::into_inner);
            if !pending
                .as_ref()
                .is_some_and(|old| old.route == route && old.advice.deadline() >= advice.deadline())
            {
                *pending = Some(Pending {
                    route,
                    reason,
                    advice,
                });
            }
        }
        error
    }
}
