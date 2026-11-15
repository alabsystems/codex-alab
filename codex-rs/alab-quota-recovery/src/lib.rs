// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Earned-reset recovery under a cross-process custody lock.
#[cfg(unix)]
mod unix;

/// What a walled request knows about its own wall: the provider's named reset,
/// and the host that would continue on another account if the request ended now.
#[derive(Clone, Copy, Default)]
pub struct Wall<'a> {
    pub reset_at: Option<i64>,
    pub handoff: Option<&'a codex_alab_core::quota::WallHandoff>,
}
pub use codex_alab_core::wall_kind::WallKind;

/// The bucket a usage limit's refusal stands in, as alab-core's one classifier
/// ([`codex_alab_core::wall_kind::wall_kind`]) reads the facts upstream's error states.
pub fn wall_kind(details: &codex_protocol::error::UsageLimitReachedError) -> WallKind {
    use codex_alab_core::wall_kind::Reached;
    use codex_alab_core::wall_kind::WallFacts;
    use codex_alab_core::wall_kind::Workspace;
    use codex_protocol::protocol::RateLimitReachedType;
    let reached = |kind| match kind {
        RateLimitReachedType::RateLimitReached => Reached::RateLimit,
        RateLimitReachedType::WorkspaceOwnerCreditsDepleted => {
            Reached::Workspace(Workspace::OwnerCreditsDepleted)
        }
        RateLimitReachedType::WorkspaceMemberCreditsDepleted => {
            Reached::Workspace(Workspace::MemberCreditsDepleted)
        }
        RateLimitReachedType::WorkspaceOwnerUsageLimitReached => {
            Reached::Workspace(Workspace::OwnerUsageLimitReached)
        }
        RateLimitReachedType::WorkspaceMemberUsageLimitReached => {
            Reached::Workspace(Workspace::MemberUsageLimitReached)
        }
    };
    let bucket = details.rate_limits.as_deref();
    codex_alab_core::wall_kind::wall_kind(&WallFacts {
        reached: details.rate_limit_reached_type.map(reached),
        bucket_reached: bucket
            .and_then(|bucket| bucket.rate_limit_reached_type)
            .map(reached),
        spend_control: bucket.and_then(|bucket| bucket.spend_control_reached),
        limit_id: bucket.and_then(|bucket| bucket.limit_id.as_deref()),
        limit_name: bucket.and_then(|bucket| bucket.limit_name.as_deref()),
    })
}

#[cfg(unix)]
pub use unix::State;
#[cfg(unix)]
pub use unix::ordinary_weekly_wall;
#[cfg(unix)]
pub use unix::preflight;
#[cfg(unix)]
pub use unix::recover_weekly;

#[cfg(not(unix))]
#[derive(Default)]
pub struct State;
#[cfg(not(unix))]
impl State {
    pub async fn bind(_: &codex_login::AuthManager) -> Self {
        Self
    }
}
#[cfg(not(unix))]
pub fn ordinary_weekly_wall(_: &codex_protocol::error::UsageLimitReachedError) -> bool {
    false
}
#[cfg(not(unix))]
pub async fn recover_weekly(
    _: &codex_login::AuthManager,
    _: &std::path::Path,
    _: &str,
    _: codex_http_client::HttpClientFactory,
    _: &str,
    _: &State,
    _: Wall<'_>,
) -> bool {
    false
}

#[cfg(not(unix))]
pub async fn preflight(
    _: &codex_login::AuthManager,
    _: &std::path::Path,
    _: &str,
    _: codex_http_client::HttpClientFactory,
    _: &str,
    _: &State,
    _: Option<&codex_protocol::protocol::RateLimitSnapshot>,
) {
}

#[cfg(test)]
mod wall_kind_tests {
    use super::*;
    use codex_alab_core::wall_kind::Workspace;
    use codex_protocol::error::UsageLimitReachedError;
    use codex_protocol::protocol::RateLimitReachedType;
    use codex_protocol::protocol::RateLimitSnapshot;

    fn refusal(
        reached: Option<RateLimitReachedType>,
        bucket: Option<RateLimitSnapshot>,
    ) -> UsageLimitReachedError {
        UsageLimitReachedError {
            plan_type: None,
            resets_at: None,
            limit_window_minutes: None,
            rate_limits: bucket.map(Box::new),
            promo_message: None,
            rate_limit_reached_type: reached,
        }
    }

    fn bucket(
        limit_id: &str,
        limit_name: Option<&str>,
        reached: Option<RateLimitReachedType>,
        spend_control: Option<bool>,
    ) -> RateLimitSnapshot {
        RateLimitSnapshot {
            limit_id: Some(limit_id.into()),
            limit_name: limit_name.map(Into::into),
            normal_model_slug: None,
            primary: None,
            secondary: None,
            credits: None,
            individual_limit: None,
            spend_control_reached: spend_control,
            plan_type: None,
            rate_limit_reached_type: reached,
        }
    }

    /// EACH FACT OF A WALL IS READ WHERE UPSTREAM'S ERROR STATES IT: a workspace's reached type
    /// on the refusal or on its bucket alone, the bucket's spend control, and its limit id and
    /// name. The fixture's goldens state a workspace's word in both places and no spend control
    /// at all, so these are read here one at a time.
    #[test]
    fn each_fact_of_a_wall_is_read_where_upstream_states_it() {
        use RateLimitReachedType::*;
        let ordinary = Some(RateLimitReached);
        for (reached, workspace) in [
            (
                WorkspaceOwnerCreditsDepleted,
                Workspace::OwnerCreditsDepleted,
            ),
            (
                WorkspaceMemberCreditsDepleted,
                Workspace::MemberCreditsDepleted,
            ),
            (
                WorkspaceOwnerUsageLimitReached,
                Workspace::OwnerUsageLimitReached,
            ),
            (
                WorkspaceMemberUsageLimitReached,
                Workspace::MemberUsageLimitReached,
            ),
        ] {
            let kind = WallKind::Workspace(workspace);
            assert_eq!(wall_kind(&refusal(Some(reached), None)), kind);
            for top in [None, ordinary] {
                let on_bucket = bucket("codex", Some("codex"), Some(reached), None);
                assert_eq!(wall_kind(&refusal(top, Some(on_bucket))), kind, "{top:?}");
            }
        }
        for (spend_control, kind) in [
            (Some(true), WallKind::Workspace(Workspace::SpendControl)),
            (Some(false), WallKind::Ordinary),
            (None, WallKind::Ordinary),
        ] {
            let codex = bucket("codex", Some("codex"), ordinary, spend_control);
            assert_eq!(wall_kind(&refusal(ordinary, Some(codex))), kind);
        }
        let other = bucket("codex_other", Some("codex_other"), ordinary, None);
        assert_eq!(
            wall_kind(&refusal(ordinary, Some(other))),
            WallKind::Limit("codex_other".into())
        );
        for name in ["gpt-reserve", " GPT-Reserve "] {
            let reserve = bucket("base_model_inference", Some(name), ordinary, None);
            assert_eq!(
                wall_kind(&refusal(ordinary, Some(reserve))),
                WallKind::Ordinary,
                "{name:?}"
            );
        }
        assert_eq!(wall_kind(&refusal(None, None)), WallKind::Ordinary);
        assert_eq!(wall_kind(&refusal(ordinary, None)), WallKind::Ordinary);
    }
}
