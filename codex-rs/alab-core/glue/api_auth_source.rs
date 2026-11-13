// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Source leases precede credential selection and outlive actual derived I/O.
use crate::AuthError;
use crate::AuthProvider;
use codex_client::HttpTransport;
use codex_client::Request;
use codex_client::Response;
use codex_client::StreamResponse;
use codex_client::TransportError;
use codex_http_client::CredentialSourceUse;
use codex_http_client::HttpClientFactory;
use codex_http_client::NetworkPermit;
use codex_http_client::NetworkPolicy;
use codex_http_client::NetworkPolicyDenied;
use futures::StreamExt;
use futures::stream;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
enum Admission {
    Destination(NetworkPermit),
    Credential(CredentialSourceUse),
}
impl Admission {
    fn check(&self) -> Result<(), NetworkPolicyDenied> {
        match self {
            Self::Destination(permit) => permit.check(),
            Self::Credential(permit) => permit.check(),
        }
    }
    fn destination(&self) -> Result<&NetworkPermit, NetworkPolicyDenied> {
        match self {
            Self::Destination(permit) => Ok(permit),
            Self::Credential(_) => Err(NetworkPolicyDenied::UnsupportedTransport),
        }
    }
}
struct Guard {
    policy: NetworkPolicy,
    permit: Admission,
}
tokio::task_local! { static CURRENT: Arc<Guard>; }
fn current(policy: &NetworkPolicy) -> Option<Arc<Guard>> {
    CURRENT.try_with(Arc::clone).ok().filter(|guard| {
        guard
            .policy
            .clone()
            .with_account_binding(policy)
            .is_some_and(|both| both == guard.policy)
    })
}
async fn prepare(
    source: NetworkPolicy,
    caller: Option<NetworkPolicy>,
    url: &str,
) -> Result<Arc<Guard>, NetworkPolicyDenied> {
    let policy = match caller {
        Some(caller) => caller
            .with_account_binding(&source)
            .ok_or(NetworkPolicyDenied::Revoked)?,
        None if source.alab_requires_credential_destination() => {
            return Err(NetworkPolicyDenied::UnsupportedTransport);
        }
        // Explicit legacy nonactivated transport contract; no native coverage proof.
        None => source,
    };
    let permit = policy.acquire(&url.parse().map_err(|_| NetworkPolicyDenied::Unavailable)?)?;
    permit.alab_prepare_source().await?;
    Ok(Arc::new(Guard {
        policy,
        permit: Admission::Destination(permit),
    }))
}
async fn prepare_credential(policy: NetworkPolicy) -> Result<Arc<Guard>, NetworkPolicyDenied> {
    let permit = policy.alab_admit_credential_use()?;
    permit.prepare().await?;
    Ok(Arc::new(Guard {
        policy,
        permit: Admission::Credential(permit),
    }))
}
pub fn bind_factory(
    auth: &dyn AuthProvider,
    factory: HttpClientFactory,
) -> Result<HttpClientFactory, AuthError> {
    let Some(source) = auth.alab_source_policy()? else {
        return Ok(factory);
    };
    let policy = factory
        .network_policy()
        .clone()
        .with_account_binding(&source)
        .ok_or(NetworkPolicyDenied::Revoked)?;
    Ok(factory.with_network_policy(policy))
}

/// Bind an auxiliary connection pool to the same immutable source as its auth.
pub fn bind_pool(
    auth: &dyn AuthProvider,
    pool: &codex_http_client::RouteAwareClientPool,
) -> Result<codex_http_client::RouteAwareClientPool, AuthError> {
    let Some(source) = auth.alab_source_policy()? else {
        return Ok(pool.clone());
    };
    pool.with_account_binding(&source)
        .ok_or_else(|| NetworkPolicyDenied::Revoked.into())
}
fn destination_conflict(policy: &NetworkPolicy) -> bool {
    CURRENT
        .try_with(|guard| {
            matches!(guard.permit, Admission::Destination(_))
                && !guard
                    .policy
                    .clone()
                    .with_account_binding(policy)
                    .is_some_and(|both| both == guard.policy)
        })
        .unwrap_or(false)
}
fn verify_selection(auth: &dyn AuthProvider, guard: &Guard) -> Result<(), TransportError> {
    let source = auth.alab_source_policy().map_err(TransportError::from)?;
    match source {
        Some(source)
            if guard
                .policy
                .clone()
                .with_account_binding(&source)
                .is_some_and(|both| both == guard.policy) =>
        {
            guard.permit.check().map_err(Into::into)
        }
        _ => Err(NetworkPolicyDenied::Revoked.into()),
    }
}
pub fn with_policy_sync<T>(
    policy: NetworkPolicy,
    operation: impl FnOnce() -> Result<T, AuthError>,
) -> Result<T, AuthError> {
    if destination_conflict(&policy) {
        return Err(NetworkPolicyDenied::Revoked.into());
    }
    if let Some(guard) = current(&policy) {
        guard.permit.check()?;
        return operation();
    }
    if policy.alab_requires_credential_destination() {
        return Err(NetworkPolicyDenied::Unavailable.into());
    }
    // Legacy synchronous callers have no native admission. They cannot claim it.
    let permit = policy.alab_admit_credential_use()?;
    permit.check()?;
    operation()
}
pub fn execute<'a, T: HttpTransport>(
    auth: &'a dyn AuthProvider,
    transport: &'a T,
    req: Request,
) -> Pin<Box<dyn Future<Output = Result<Response, TransportError>> + Send + 'a>> {
    Box::pin(async move {
        let Some(policy) = auth.alab_source_policy().map_err(TransportError::from)? else {
            let req = auth.apply_auth(req).await.map_err(TransportError::from)?;
            return transport.execute(req).await;
        };
        let guard = prepare(policy, transport.alab_source_policy(), &req.url).await?;
        CURRENT
            .scope(
                guard.clone(),
                guard.permit.destination()?.run(async {
                    let req = auth.apply_auth(req).await.map_err(TransportError::from)?;
                    verify_selection(auth, &guard)?;
                    transport.execute(req).await
                }),
            )
            .await?
    })
}
pub fn stream<'a, T: HttpTransport>(
    auth: &'a dyn AuthProvider,
    transport: &'a T,
    req: Request,
) -> Pin<Box<dyn Future<Output = Result<StreamResponse, TransportError>> + Send + 'a>> {
    Box::pin(async move {
        let Some(policy) = auth.alab_source_policy().map_err(TransportError::from)? else {
            let req = auth.apply_auth(req).await.map_err(TransportError::from)?;
            return transport.stream(req).await;
        };
        let guard = prepare(policy, transport.alab_source_policy(), &req.url).await?;
        let mut response = CURRENT
            .scope(
                guard.clone(),
                guard.permit.destination()?.run(async {
                    let req = auth.apply_auth(req).await.map_err(TransportError::from)?;
                    verify_selection(auth, &guard)?;
                    transport.stream(req).await
                }),
            )
            .await??;
        // Field order also closes the real body before its source guard on cancellation.
        struct BodyLease {
            body: Option<codex_client::ByteStream>,
            guard: Option<Arc<Guard>>,
        }
        response.bytes = Box::pin(stream::unfold(
            BodyLease {
                body: Some(response.bytes),
                guard: Some(guard),
            },
            |mut state| async move {
                let guard = state.guard.as_ref()?;
                let next = match guard.permit.destination() {
                    Ok(permit) => permit.run(state.body.as_mut()?.next()).await,
                    Err(error) => Err(error),
                };
                match next {
                    Ok(Some(chunk)) => Some((chunk, state)),
                    Ok(None) => {
                        drop(state.body.take());
                        drop(state.guard.take());
                        None
                    }
                    Err(error) => {
                        drop(state.body.take());
                        drop(state.guard.take());
                        Some((Err(error.into()), state))
                    }
                }
            },
        ));
        Ok(response)
    })
}
/// Once provider work starts, the actual source lease and response custody stay
/// owned even if its waiter is cancelled. Revocation is checked before/after;
/// it does not discard a response to an accepted credential action midway.
struct PreparedOutput<T> {
    value: T,
    source: Arc<Guard>,
}
async fn owned_with_policy<T: Send + 'static>(
    policy: NetworkPolicy,
    operation: impl Future<Output = T> + Send + 'static,
    check_after: bool,
) -> Result<T, AuthError> {
    if destination_conflict(&policy) {
        return Err(NetworkPolicyDenied::Revoked.into());
    }
    let inherited = current(&policy);
    let parent = codex_alab_core::assignment::current_activity();
    let runtime =
        tokio::runtime::Handle::try_current().map_err(|_| NetworkPolicyDenied::Unavailable)?;
    let task = runtime.spawn(async move {
        let work = async move {
            let guard = match inherited {
                Some(guard) => guard,
                None => prepare_credential(policy).await?,
            };
            guard.permit.check()?;
            let result = CURRENT.scope(guard.clone(), operation).await;
            if check_after {
                guard.permit.check()?;
            }
            Ok::<_, AuthError>(PreparedOutput {
                value: result,
                source: guard,
            })
        };
        match parent {
            Some(parent) => parent.scope(work).await,
            None => work.await,
        }
    });
    let output = task
        .await
        .map_err(|_| AuthError::Policy(NetworkPolicyDenied::Unavailable))??;
    if check_after {
        output.source.permit.check()?;
    }
    Ok(output.value)
}

fn material_is_scoped(policy: &NetworkPolicy) -> bool {
    !destination_conflict(policy)
        && (!policy.alab_requires_credential_destination()
            || current(policy)
                .is_some_and(|guard| matches!(guard.permit, Admission::Destination(_))))
}
/// Raw header introspection for an explicitly captured nonactivated legacy
/// snapshot only. A destination context must always use normal source admission.
/// This does not grant permission to send the returned material.
pub fn legacy_snapshot_export_available(policy: &NetworkPolicy) -> bool {
    !policy.alab_requires_credential_destination() && CURRENT.try_with(|_| ()).is_err()
}
pub async fn select_headers(
    policy: NetworkPolicy,
    operation: impl Future<Output = Result<http::HeaderMap, AuthError>> + Send + 'static,
) -> Result<http::HeaderMap, AuthError> {
    if !material_is_scoped(&policy) {
        return Err(NetworkPolicyDenied::UnsupportedTransport.into());
    }
    owned_with_policy(policy, operation, true).await?
}
pub async fn select_request(
    policy: NetworkPolicy,
    operation: impl Future<Output = Result<Request, AuthError>> + Send + 'static,
) -> Result<Request, AuthError> {
    if !material_is_scoped(&policy) {
        return Err(NetworkPolicyDenied::UnsupportedTransport.into());
    }
    owned_with_policy(policy, operation, true).await?
}
pub async fn credential_action(
    policy: NetworkPolicy,
    operation: impl Future<Output = bool> + Send + 'static,
) -> bool {
    owned_with_policy(policy, operation, false)
        .await
        .unwrap_or(false)
}
/// Keep a selected credential operation and its source alive through actual completion.
/// This grants no destination permission; the operation must use its admitted runner.
pub async fn credential_operation<T: Send + 'static>(
    policy: NetworkPolicy,
    operation: impl Future<Output = T> + Send + 'static,
) -> Result<T, AuthError> {
    owned_with_policy(policy, operation, true).await
}
/// A concrete destination lease for a handshake. The bound factory's real
/// socket permit takes over before this scope ends; this is not a source seal.
pub struct DestinationLease(Option<Arc<Guard>>);
impl DestinationLease {
    pub async fn scope<T>(
        &self,
        operation: impl Future<Output = T>,
    ) -> Result<T, NetworkPolicyDenied> {
        match &self.0 {
            Some(guard) => {
                CURRENT
                    .scope(guard.clone(), guard.permit.destination()?.run(operation))
                    .await
            }
            None => Ok(operation.await),
        }
    }
}
pub async fn prepare_factory(
    auth: &dyn AuthProvider,
    factory: &HttpClientFactory,
    url: &str,
) -> Result<(HttpClientFactory, DestinationLease), AuthError> {
    let bound = bind_factory(auth, factory.clone())?;
    let Some(policy) = auth.alab_source_policy()? else {
        return Ok((bound, DestinationLease(None)));
    };
    let guard = prepare(policy, Some(bound.network_policy().clone()), url).await?;
    Ok((bound, DestinationLease(Some(guard))))
}

#[cfg(test)]
#[path = "api_auth_source_tests.rs"]
mod tests;
