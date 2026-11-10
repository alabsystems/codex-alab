// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! ALAB(notices) glue, compiled inside codex-tui as `security_setup::alab_notices` (one
//! `#[path]` line at the end of `tui/src/security_setup.rs`): inside aterm, the account's
//! security-setup notice is one entry of aterm's exceptions digest, not a banner in every tab
//! (`codex_alab_tui::notices`).
//!
//! The fetch observes an entry before HTTP and carries its result without changing notices.
//! App's current-request guard then admits [`accept`]. It verifies the current identity and
//! awaits filing or conditional settlement, so a later App result cannot overtake that action.
//! An unavailable native observation cannot settle anything; an unavailable filing retains
//! upstream's banner fallback. No account identifiers are logged or sent to aterm.

use super::Config;
use super::Identity;
use super::Notice;
use codex_alab_tui::notices::ObservedLater;
use codex_alab_tui::notices::RoutedNotice;
use std::time::Duration;

/// Capture the native observation before app-server performs its account-owned fetch.
/// An empty successful response may settle only that captured identity and revision.
pub(crate) fn prefetch_using(
    config: &Config,
    server: &crate::app_server_session::AppServerSession,
    tx: crate::app_event_sender::AppEventSender,
    request_id: uuid::Uuid,
    delivery: Delivery,
) -> Option<tokio::task::JoinHandle<()>> {
    if config.model_provider_id != "openai" || server.uses_remote_workspace() {
        return None;
    }
    let config = config.clone();
    let request_handle = server.request_handle();
    Some(tokio::spawn(async move {
        let result = tokio::time::timeout(Duration::from_secs(3), async {
            let auth = config.auth_config().load_auth(false).await.ok().flatten()?;
            if !matches!(auth, codex_login::CodexAuth::Chatgpt(_)) || auth.is_fedramp_account() {
                return None;
            }
            let identity = Identity::from_auth(&auth)?;
            let observed = delivery.observe(&identity).await;
            let response: codex_app_server_protocol::GetAccountSecuritySetupResponse =
                request_handle
                    .request_typed(
                        codex_app_server_protocol::ClientRequest::GetAccountSecuritySetup {
                            request_id: codex_app_server_protocol::RequestId::String(
                                uuid::Uuid::new_v4().to_string(),
                            ),
                            params: None,
                        },
                    )
                    .await
                    .ok()?;
            let notice = match response.notice {
                Some(fetched) => {
                    if fetched.account_id != identity.account || fetched.user_id != identity.user {
                        return None;
                    }
                    let notice = Notice {
                        title: fetched.title,
                        description: fetched.description,
                        action: super::Action {
                            label: fetched.action.label,
                            url: fetched.action.url,
                        },
                    };
                    if !notice.valid() {
                        return None;
                    }
                    Some(notice)
                }
                None if observed.is_some()
                    && response
                        .alab_successful_empty
                        .as_ref()
                        .is_some_and(|fetched| {
                            fetched.account_id == identity.account
                                && fetched.user_id == identity.user
                        }) =>
                {
                    None
                }
                None => return None,
            };
            let current = config.auth_config().load_auth(false).await.ok().flatten()?;
            if Identity::from_auth(&current).as_ref() != Some(&identity) {
                return None;
            }
            Some((identity, notice, observed))
        })
        .await
        .ok()
        .flatten();
        if let Some((identity, notice, observed)) = result {
            tx.send(crate::app_event::AppEvent::SecuritySetupLoaded {
                request_id,
                identity,
                notice: Fetched::new(notice, observed, delivery),
            });
        }
    }))
}

#[derive(Clone, Debug)]
pub(crate) struct Fetched {
    pub(crate) notice: Option<Notice>,
    observed: Option<ObservedLater>,
    delivery: Delivery,
}

impl Fetched {
    pub(crate) fn new(
        notice: Option<Notice>,
        observed: Option<ObservedLater>,
        delivery: Delivery,
    ) -> Self {
        Self {
            notice,
            observed,
            delivery,
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct Delivery {
    #[cfg(test)]
    pub(crate) transport: Option<std::sync::Arc<dyn TestTransport>>,
}

impl std::fmt::Debug for Delivery {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Delivery").finish_non_exhaustive()
    }
}

#[cfg(test)]
pub(crate) trait TestTransport: Send + Sync {
    fn observe<'a>(
        &'a self,
        identity: Identity,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<ObservedLater>> + Send + 'a>>;
    fn route<'a>(
        &'a self,
        notice: RoutedNotice,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send + 'a>>;
    fn settle<'a>(
        &'a self,
        observed: ObservedLater,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send + 'a>>;
}

impl Delivery {
    pub(super) async fn observe(&self, identity: &Identity) -> Option<ObservedLater> {
        #[cfg(test)]
        if let Some(transport) = &self.transport {
            return transport.observe(identity.clone()).await;
        }
        codex_alab_tui::notices::observe(&identity.account, &identity.user).await
    }

    async fn route(&self, notice: RoutedNotice) -> bool {
        #[cfg(test)]
        if let Some(transport) = &self.transport {
            return transport.route(notice).await;
        }
        codex_alab_tui::notices::route(&notice).await
    }

    async fn settle(&self, observed: ObservedLater) {
        #[cfg(test)]
        if let Some(transport) = &self.transport {
            transport.settle(observed).await;
            return;
        }
        codex_alab_tui::notices::settle_observed(&observed).await;
    }
}

/// Called only after App admits the current request. The mutable App event owner awaits
/// the entire action; neither a stale event nor a detached mutation can end a newer notice.
pub(crate) async fn accept(
    config: &Config,
    identity: Identity,
    fetched: Fetched,
) -> Option<(Identity, Notice)> {
    let current = tokio::time::timeout(
        Duration::from_secs(3),
        config
            .auth_config()
            .load_auth(/*enable_codex_api_key_env*/ false),
    )
    .await
    .ok()?
    .ok()
    .flatten()?;
    if Identity::from_auth(&current).as_ref() != Some(&identity) {
        return None;
    }
    if let Some(notice) = fetched.notice {
        if notice.valid() && !fetched.delivery.route(routed(&identity, &notice)).await {
            return Some((identity, notice));
        }
    } else if let Some(observed) = fetched.observed {
        fetched.delivery.settle(observed).await;
    }
    None
}

/// The notice as ALab's router reads it. The page is the action's URL as parsed
/// (`Notice::valid` already held it to `https://chatgpt.com` with no user or port), so it
/// reaches aterm in ASCII, its escapes normalized.
fn routed(identity: &Identity, notice: &Notice) -> RoutedNotice {
    RoutedNotice {
        account: identity.account.clone(),
        user: identity.user.clone(),
        title: notice.title.clone(),
        description: notice.description.clone(),
        url: url::Url::parse(&notice.action.url)
            .map_or_else(|_| notice.action.url.clone(), String::from),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security_setup::Action;

    /// The router gets the server's words and page and the ids it hashes into the key; the
    /// page as parsed (negative control: the raw form differs, and is not what is sent).
    #[test]
    fn the_notice_maps_onto_the_router() {
        let identity = Identity {
            account: "acct-0123".to_string(),
            user: "user-abcd".to_string(),
        };
        let notice = Notice {
            title: "Set up security for Daybreak mode".to_string(),
            description: "Finish setup by October 15.".to_string(),
            action: Action {
                label: "Set up security".to_string(),
                url: "https://chatgpt.com/cyber?from=cli#é".to_string(),
            },
        };
        assert!(notice.valid());
        let routed = routed(&identity, &notice);
        assert_eq!(routed.account, "acct-0123");
        assert_eq!(routed.user, "user-abcd");
        assert_eq!(routed.title, notice.title);
        assert_eq!(routed.description, notice.description);
        assert_eq!(routed.url, "https://chatgpt.com/cyber?from=cli#%C3%A9");
        assert_ne!(routed.url, notice.action.url);
        assert!(codex_alab_tui::notices::later(&routed).line().is_ok());
    }
}
