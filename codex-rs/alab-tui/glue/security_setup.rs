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
