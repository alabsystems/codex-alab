// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! WHICH BUCKET A USAGE WALL STANDS IN: the one classifier codex-alab's glue and its Autopilot
//! share (gap 4 of `alab/PARALLEL-LOGINS.md`, step S3).
//!
//! A wall is ordinary (the `codex` bucket, or Reserve, its fallback), a workspace's (its credits
//! depleted, its spend cap or a member's reached, or spend control), or a model's own limit (a
//! bucket of its own, named by its limit id). A workspace's reached type is read first: the 429
//! states it on the `codex` bucket's snapshot, whose windows have room. This crate names no codex
//! type, so a caller states the wall's facts ([`WallFacts`]); `codex-alab-quota-recovery`'s
//! `wall_kind` reads them off upstream's `UsageLimitReachedError`.

/// The limit id of the ordinary bucket. A wall that names no limit id is this bucket's too.
pub const ORDINARY_LIMIT: &str = "codex";

/// Classification of a usage refusal for interactive account rotation. The quota watcher
/// can still measure the recorded bucket when the refusal's classification is unverified.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LimitClass {
    Ordinary,
    Model,
    Unverified,
}

/// The host's wire grammar for a model's own bucket in a quota rotation request.
pub fn valid_limit_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id != ORDINARY_LIMIT
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// The limit name of Reserve, the fallback upstream serves once the ordinary bucket is spent.
/// Upstream's copy for a usage limit treats it as ordinary usage (`UsageLimitReachedError`'s
/// `Display`, which compares the trimmed name without regard to case), and so does this.
pub const RESERVE: &str = "gpt-reserve";

/// A workspace's wall. Only a refill or a person's change ends one: no reset of the account does,
/// and no other seat of the workspace is chosen (both hosts refuse a destination with the
/// source's account id, and the workspace is that id).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Workspace {
    /// `workspace_owner_credits_depleted`.
    OwnerCreditsDepleted,
    /// `workspace_member_credits_depleted`.
    MemberCreditsDepleted,
    /// `workspace_owner_usage_limit_reached`: the workspace's own spend cap.
    OwnerUsageLimitReached,
    /// `workspace_member_usage_limit_reached`: a per-seat cap the owner set.
    MemberUsageLimitReached,
    /// The bucket's `spend_control_reached`. No 429 states it today (codex-api's header parse
    /// leaves it unread); a usage read does, and a 429 that did would be this wall.
    SpendControl,
}

/// A reached type, as a 429 or its bucket's snapshot names it (upstream's
/// `RateLimitReachedType`, in this crate's words).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Reached {
    /// `rate_limit_reached`: the ordinary wall's own word, never a workspace's.
    RateLimit,
    /// One of the four workspace words.
    Workspace(Workspace),
}

/// Which bucket a usage wall stands in.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum WallKind {
    /// The ordinary `codex` bucket, or Reserve. An earned reset or another account with ordinary
    /// room answers it.
    Ordinary,
    /// A workspace's wall.
    Workspace(Workspace),
    /// A model's own limit bucket, by its limit id (as codex-api normalized it). Another account
    /// answers it only where that account's own read lists the same limit with room.
    Limit(String),
}

impl WallKind {
    /// The model bucket named by the provider, including a malformed id that no host may be asked about.
    pub fn limit_id(&self) -> Option<&str> {
        match self {
            Self::Limit(id) => Some(id),
            Self::Ordinary | Self::Workspace(_) => None,
        }
    }

    /// Whether this refusal can be represented on the native wire. Host policy is checked separately.
    pub fn asked(&self) -> bool {
        self.limit_id().is_none_or(valid_limit_id)
    }

    /// Whether an exec run keeps its request at this wall only while a host says a move may come
    /// ([`crate::exec_wall`]). A workspace's wall has no reset this account reaches, so a run
    /// that waited it out would wait for a person; it waits only while a host looks for an
    /// account outside the workspace.
    pub fn host_kept(&self) -> bool {
        matches!(self, WallKind::Workspace(_))
    }
}

/// What a usage wall states of its bucket.
#[derive(Clone, Copy, Debug, Default)]
pub struct WallFacts<'a> {
    /// The reached type the refusal itself names.
    pub reached: Option<Reached>,
    /// The reached type its bucket's snapshot names.
    pub bucket_reached: Option<Reached>,
    /// The snapshot's `spend_control_reached`.
    pub spend_control: Option<bool>,
    /// The snapshot's limit id (`None`: the ordinary bucket).
    pub limit_id: Option<&'a str>,
    /// The snapshot's limit name.
    pub limit_name: Option<&'a str>,
}

/// The bucket `facts` places a wall in. A workspace's reached type, on the refusal or on its
/// bucket, comes first, then spend control; a model's own limit is a limit id other than
/// [`ORDINARY_LIMIT`] whose name is not [`RESERVE`]; everything else is ordinary.
pub fn wall_kind(facts: &WallFacts<'_>) -> WallKind {
    for reached in [facts.reached, facts.bucket_reached].into_iter().flatten() {
        if let Reached::Workspace(workspace) = reached {
            return WallKind::Workspace(workspace);
        }
    }
    if facts.spend_control == Some(true) {
        return WallKind::Workspace(Workspace::SpendControl);
    }
    match facts.limit_id {
        Some(id) if id != ORDINARY_LIMIT && !reserve(facts.limit_name) => {
            WallKind::Limit(id.to_owned())
        }
        _ => WallKind::Ordinary,
    }
}

fn reserve(name: Option<&str>) -> bool {
    name.map(str::trim)
        .is_some_and(|name| name.eq_ignore_ascii_case(RESERVE))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Workspace; 5] = [
        Workspace::OwnerCreditsDepleted,
        Workspace::MemberCreditsDepleted,
        Workspace::OwnerUsageLimitReached,
        Workspace::MemberUsageLimitReached,
        Workspace::SpendControl,
    ];

    #[test]
    fn only_canonical_model_bucket_ids_fit_the_native_request() {
        for id in ["a", "codex_other", "model_42", &"z".repeat(64)] {
            assert!(valid_limit_id(id), "{id:?}");
        }
        for id in [
            "",
            "codex",
            "Codex",
            "model-x",
            "é",
            "x\n",
            "x y=z",
            &"z".repeat(65),
        ] {
            assert!(!valid_limit_id(id), "{id:?}");
        }
    }

    #[test]
    fn a_workspace_word_comes_first_wherever_it_is_named() {
        for workspace in ALL.into_iter().filter(|w| *w != Workspace::SpendControl) {
            let word = Some(Reached::Workspace(workspace));
            for facts in [
                WallFacts {
                    reached: word,
                    ..WallFacts::default()
                },
                WallFacts {
                    bucket_reached: word,
                    ..WallFacts::default()
                },
                // Before the ordinary word on the other side, and before a model's limit id.
                WallFacts {
                    reached: Some(Reached::RateLimit),
                    bucket_reached: word,
                    limit_id: Some("codex_other"),
                    ..WallFacts::default()
                },
                WallFacts {
                    reached: word,
                    spend_control: Some(true),
                    ..WallFacts::default()
                },
            ] {
                assert_eq!(
                    wall_kind(&facts),
                    WallKind::Workspace(workspace),
                    "{facts:?}"
                );
            }
        }
        let spend = WallFacts {
            reached: Some(Reached::RateLimit),
            spend_control: Some(true),
            limit_id: Some("codex_other"),
            ..WallFacts::default()
        };
        assert_eq!(
            wall_kind(&spend),
            WallKind::Workspace(Workspace::SpendControl)
        );
        for workspace in ALL {
            let kind = WallKind::Workspace(workspace);
            assert!(kind.asked() && kind.host_kept(), "{kind:?}");
        }
    }

    #[test]
    fn a_limit_of_its_own_is_neither_the_ordinary_bucket_nor_reserve() {
        let limit = |id, name| WallFacts {
            reached: Some(Reached::RateLimit),
            bucket_reached: Some(Reached::RateLimit),
            spend_control: Some(false),
            limit_id: id,
            limit_name: name,
        };
        assert_eq!(wall_kind(&WallFacts::default()), WallKind::Ordinary);
        for (id, name) in [
            (None, None),
            (Some("codex"), None),
            (Some("codex"), Some("codex_other")),
            (Some("base_model_inference"), Some("gpt-reserve")),
            (Some("base_model_inference"), Some(" GPT-Reserve ")),
            (None, Some("codex_other")),
        ] {
            assert_eq!(
                wall_kind(&limit(id, name)),
                WallKind::Ordinary,
                "{id:?} {name:?}"
            );
        }
        for (id, name) in [
            (Some("codex_other"), None),
            (Some("codex_other"), Some("codex_other")),
            (Some("codex_other"), Some("codex")),
            (Some("base_model_inference"), Some("gpt-reserve-2")),
            (Some("Codex"), None),
        ] {
            assert_eq!(
                wall_kind(&limit(id, name)),
                WallKind::Limit(id.unwrap().to_owned()),
                "{id:?} {name:?}"
            );
        }
        let model = WallKind::Limit("codex_other".into());
        assert!(model.asked() && !model.host_kept());
        assert!(!WallKind::Limit("bad limit".into()).asked());
        assert!(WallKind::Ordinary.asked() && !WallKind::Ordinary.host_kept());
    }
}
