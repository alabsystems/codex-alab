// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Preserve both ends of account recovery while sharing the native continuation
//! lifecycle for interactive and headless callers. Each purpose still uses its
//! own host protocol and policy. This client adapter grants no execution authority.

pub use crate::exec_selection::Selection as ExecSelection;
pub use crate::exec_selection::prepare as prepare_exec;

use crate::Client;
use crate::Error;
use crate::renewal;
use crate::rotation;
use codex_alab_core::account_handoff::AccountHandoff;
use codex_alab_core::account_handoff::Purpose;
use codex_alab_core::quota::SpentWall;
use codex_alab_core::quota::SpentWindow;
use std::time::Duration;

#[derive(Clone, PartialEq, Eq)]
pub struct Receipt {
    pub id: String,
    pub thread: String,
    pub wall: String,
    pub purpose: Purpose,
    pub identity: renewal::Identity,
    pub destination: renewal::Identity,
}

impl Receipt {
    /// Match recovery metadata to the original request and selected accounts.
    /// This does not replace the caller's live host and execution-admission checks.
    pub fn matches_handoff(&self, thread: &str, turn: &str, handoff: &AccountHandoff) -> bool {
        self.thread == thread
            && self.wall == turn
            && self.purpose == handoff.purpose
            && self.identity.account == handoff.account
            && self.identity.user == handoff.user
            && handoff.target().is_none_or(|target| {
                target
                    == (
                        self.destination.account.as_str(),
                        self.destination.user.as_str(),
                    )
            })
            && handoff.receipt.as_ref().is_none_or(|id| id == &self.id)
    }
}

/// Shared policy gate; a successful check alone does not authorize execution.
pub fn policy_allows(policy: &crate::policy::HarnessPolicy, purpose: Purpose) -> bool {
    policy.effective
        && match purpose {
            Purpose::Login => policy.get("logins") == Some("renew"),
            Purpose::WeeklyQuota => {
                policy.get("codex_rotation") == Some("1")
                    && matches!(policy.get("accounts"), Some("early" | "wall"))
            }
        }
        && policy.switch("relaunch")
        && policy.switch("resume_limits")
}

/// The policy word by which a host says it also leaves a five-hour wall: the
/// seconds a spent five-hour window's reset must be further away than for the
/// host to move the session (aterm prints its own bound beside
/// `codex_rotation=1`). A host that prints no such word leaves only a spent
/// week, and answers `not-needed` at every five-hour wall.
pub const FIVE_HOUR_HANDOFF: &str = "codex_rotation_five_hour";

/// How much nearer its reset a five-hour wall may be when the host reads the
/// account, after the turn has ended, than it is when this question is
/// answered. The time left at the answer must clear the host's bound by this,
/// or the host could find the wall inside it and decline a turn that has
/// already ended. It covers only what comes after the answer (the turn's end
/// and the host's own read): the time left is measured at `now`, not when the
/// walled request read its usage.
const HANDOFF_SKEW: Duration = Duration::from_secs(60);

/// Whether this host, by its own policy reply, would move a session that
/// stands at `wall` to another account, asked at `now` (seconds since the
/// Unix epoch). A local mirror of the host's rule, never wider than it: a yes
/// ends a running turn, so it is given only where the host's own later reading
/// should agree. It selects nothing and shows nothing about where the session
/// could go.
///
/// A spent week is left under the rotation policy alone. A spent five-hour
/// window also needs the host's advertised bound, cleared at `now` by
/// [`HANDOFF_SKEW`], and `limit_wait`: without that switch a turn the host
/// then declines raises attention at once, where the request it ended would
/// have waited the wall out.
pub fn leaves_wall(policy: &crate::policy::HarnessPolicy, wall: SpentWall, now: i64) -> bool {
    policy_allows(policy, Purpose::WeeklyQuota)
        && match wall.window {
            SpentWindow::Week => true,
            SpentWindow::FiveHour => {
                policy.switch("limit_wait")
                    && five_hour_bound(policy)
                        .and_then(|bound| bound.checked_add(HANDOFF_SKEW))
                        .is_some_and(|least| wall.resets_in(now) > least)
            }
        }
}

/// The bound a host's policy reply advertises for leaving a five-hour wall
/// ([`FIVE_HOUR_HANDOFF`], a canonical count of seconds); `None` for a reply
/// that prints no such word, or one this reader cannot take. A reply with no
/// bound leaves no five-hour wall, whatever else it says.
pub fn five_hour_bound(policy: &crate::policy::HarnessPolicy) -> Option<Duration> {
    policy
        .get(FIVE_HOUR_HANDOFF)
        .and_then(crate::wire::canonical_u64)
        .map(Duration::from_secs)
}

pub enum Outcome {
    NotNeeded,
    Waiting(Option<Receipt>),
    Ready(Receipt),
    Committed(Receipt),
    Consumed(Receipt),
    Retired(Receipt),
}

impl From<renewal::Receipt> for Receipt {
    fn from(value: renewal::Receipt) -> Self {
        Self {
            id: value.id,
            thread: value.thread,
            wall: value.wall,
            purpose: Purpose::Login,
            destination: value.identity.clone(),
            identity: value.identity,
        }
    }
}

impl From<rotation::Receipt> for Receipt {
    fn from(value: rotation::Receipt) -> Self {
        Self {
            id: value.id,
            thread: value.origin.thread,
            wall: value.origin.turn,
            purpose: Purpose::WeeklyQuota,
            identity: value.origin.identity,
            destination: value.destination,
        }
    }
}

impl From<renewal::Outcome> for Outcome {
    fn from(value: renewal::Outcome) -> Self {
        match value {
            renewal::Outcome::Waiting(value) => Self::Waiting(value.map(Into::into)),
            renewal::Outcome::Ready(value) => Self::Ready(value.into()),
            renewal::Outcome::Committed(value) => Self::Committed(value.into()),
            renewal::Outcome::Consumed(value) => Self::Consumed(value.into()),
            renewal::Outcome::Retired(value) => Self::Retired(value.into()),
        }
    }
}

impl From<rotation::Outcome> for Outcome {
    fn from(value: rotation::Outcome) -> Self {
        match value {
            rotation::Outcome::NotNeeded => Self::NotNeeded,
            rotation::Outcome::Waiting(value) => Self::Waiting(value.map(Into::into)),
            rotation::Outcome::Ready(value) => Self::Ready(value.into()),
            rotation::Outcome::Committed(value) => Self::Committed(value.into()),
            rotation::Outcome::Consumed(value) => Self::Consumed(value.into()),
            rotation::Outcome::Retired(value) => Self::Retired(value.into()),
        }
    }
}

pub async fn prepare(
    client: &Client,
    purpose: Purpose,
    thread: &str,
    wall: &str,
    identity: &renewal::Identity,
) -> Result<Outcome, Error> {
    match purpose {
        Purpose::Login => client
            .renewal_prepare(thread, wall, identity)
            .await
            .map(Into::into),
        Purpose::WeeklyQuota => client
            .rotation_prepare(&rotation::Origin {
                thread: thread.into(),
                turn: wall.into(),
                identity: identity.clone(),
            })
            .await
            .map(Into::into),
    }
}

pub async fn status(client: &Client, purpose: Purpose, id: &str) -> Result<Outcome, Error> {
    match purpose {
        Purpose::Login => client.renewal_status(id).await.map(Into::into),
        Purpose::WeeklyQuota => client.rotation_status(id).await.map(Into::into),
    }
}

pub async fn commit(client: &Client, purpose: Purpose, id: &str) -> Result<Outcome, Error> {
    match purpose {
        Purpose::Login => client.renewal_commit(id).await.map(Into::into),
        Purpose::WeeklyQuota => client.rotation_commit(id).await.map(Into::into),
    }
}

pub async fn consume(
    client: &Client,
    purpose: Purpose,
    id: &str,
    turn: &str,
) -> Result<Outcome, Error> {
    match purpose {
        Purpose::Login => client.renewal_consume(id, turn).await.map(Into::into),
        Purpose::WeeklyQuota => client.rotation_consume(id, turn).await.map(Into::into),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A host policy under which a spent week rotates, with these keys besides.
    fn rotating(keys: &[(&str, &str)]) -> crate::policy::HarnessPolicy {
        crate::policy::HarnessPolicy {
            generation: 1,
            effective: true,
            why: "enabled".into(),
            keys: [
                ("codex_rotation", "1"),
                ("accounts", "wall"),
                ("relaunch", "true"),
                ("resume_limits", "true"),
            ]
            .iter()
            .chain(keys)
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect(),
        }
    }

    #[test]
    fn a_five_hour_wall_is_left_only_under_the_hosts_advertised_bound() {
        // The moment the host is asked, and walls that many seconds from
        // their reset then.
        const NOW: i64 = 1_800_000_000;
        let wall = |window, seconds: u64| SpentWall {
            window,
            resets_at: NOW.saturating_add_unsigned(seconds),
        };
        let leaves_wall =
            |policy: &crate::policy::HarnessPolicy, wall: SpentWall| leaves_wall(policy, wall, NOW);
        let week = wall(SpentWindow::Week, 3 * 24 * 3600);
        let five_hours = |seconds| wall(SpentWindow::FiveHour, seconds);
        let advertised: &[(&str, &str)] = &[(FIVE_HOUR_HANDOFF, "900"), ("limit_wait", "true")];

        // A host that prints no word leaves a spent week and no five-hour wall.
        let silent = rotating(&[("limit_wait", "true")]);
        assert!(leaves_wall(&silent, week));
        assert!(!leaves_wall(&silent, five_hours(4 * 3600)));
        assert_eq!(five_hour_bound(&silent), None);

        // The bound, cleared by the skew: 900 s + 60 s is not far enough, one
        // second more is.
        let host = rotating(advertised);
        assert_eq!(five_hour_bound(&host), Some(Duration::from_secs(900)));
        assert!(leaves_wall(&host, week));
        assert!(leaves_wall(&host, five_hours(961)));
        assert!(leaves_wall(&host, five_hours(18_000)));
        for near in [960, 959, 900, 60, 0] {
            assert!(!leaves_wall(&host, five_hours(near)), "{near}");
        }
        // The host's own number is followed, whatever it is.
        let wider = rotating(&[(FIVE_HOUR_HANDOFF, "3600"), ("limit_wait", "true")]);
        assert!(!leaves_wall(&wider, five_hours(3660)));
        assert!(leaves_wall(&wider, five_hours(3661)));
        let zero = rotating(&[(FIVE_HOUR_HANDOFF, "0"), ("limit_wait", "true")]);
        assert!(!leaves_wall(&zero, five_hours(60)));
        assert!(leaves_wall(&zero, five_hours(61)));

        // Without `limit_wait` a declined five-hour turn would alert a person
        // hours early; a spent week does not depend on it.
        for limit_wait in [None, Some("false"), Some("1"), Some("")] {
            let mut keys = vec![(FIVE_HOUR_HANDOFF, "900")];
            keys.extend(limit_wait.map(|value| ("limit_wait", value)));
            let host = rotating(&keys);
            // The bound is the word's alone: `limit_wait` is the rule's.
            assert_eq!(five_hour_bound(&host), Some(Duration::from_secs(900)));
            assert!(
                !leaves_wall(&host, five_hours(4 * 3600)),
                "limit_wait={limit_wait:?}"
            );
            assert!(leaves_wall(&host, week), "limit_wait={limit_wait:?}");
        }

        // A word that is not a canonical count of seconds is no word.
        for word in [
            "",
            "-",
            "soon",
            "900s",
            "15m",
            "+900",
            "0900",
            "9e2",
            "900.0",
            " 900",
            "-1",
            "99999999999999999999999",
        ] {
            let host = rotating(&[(FIVE_HOUR_HANDOFF, word), ("limit_wait", "true")]);
            assert_eq!(five_hour_bound(&host), None, "{word:?}");
            assert!(!leaves_wall(&host, five_hours(4 * 3600)), "{word:?}");
            assert!(leaves_wall(&host, week), "{word:?}");
        }
        // A bound no duration can add the skew to is never cleared.
        let huge = rotating(&[
            (FIVE_HOUR_HANDOFF, &u64::MAX.to_string()),
            ("limit_wait", "true"),
        ]);
        assert!(!leaves_wall(&huge, five_hours(u64::MAX)));

        // The word is no rotation policy of its own: each key rotation needs.
        for (key, off) in [
            ("codex_rotation", "0"),
            ("accounts", "off"),
            ("relaunch", "false"),
            ("resume_limits", "false"),
        ] {
            let mut host = rotating(advertised);
            host.keys.insert(key.into(), off.into());
            assert!(!leaves_wall(&host, five_hours(4 * 3600)), "{key}={off}");
            assert!(!leaves_wall(&host, week), "{key}={off}");
        }
        let mut standby = rotating(advertised);
        standby.effective = false;
        assert!(!leaves_wall(&standby, five_hours(4 * 3600)));
        assert!(!leaves_wall(&standby, week));
    }

    /// The bound is judged with the time left when the host is asked. The
    /// requests between the usage read and the question (the credit list, the
    /// host's policy) take time, and a wall that cleared the bound when it was
    /// read may be inside it by then.
    #[test]
    fn a_five_hour_walls_bound_is_judged_when_the_host_is_asked() {
        const READ: i64 = 1_800_000_000;
        let host = rotating(&[(FIVE_HOUR_HANDOFF, "900"), ("limit_wait", "true")]);
        // 965 s from its reset when the usage was read: clear of 900 s + 60 s.
        let wall = SpentWall {
            window: SpentWindow::FiveHour,
            resets_at: READ + 965,
        };
        assert!(leaves_wall(&host, wall, READ));
        assert!(leaves_wall(&host, wall, READ + 4));
        // Asked after a slow credit list, the same wall is inside the bound.
        for later in [5, 40, 965, 966, 86_400] {
            assert!(!leaves_wall(&host, wall, READ + later), "{later}");
        }
        // A clock read before the usage was (it cannot be, but it is not this
        // rule's to assume) only makes the wall look further away.
        assert!(leaves_wall(&host, wall, READ - 30));
        // A spent week is left whenever it is asked about.
        let week = SpentWall {
            window: SpentWindow::Week,
            resets_at: READ + 965,
        };
        for now in [READ, READ + 40, READ + 86_400] {
            assert!(leaves_wall(&host, week, now), "{now}");
        }
    }

    #[test]
    fn quota_handoff_cannot_change_its_origin_destination_or_receipt() {
        let receipt = Receipt {
            id: "1".repeat(32),
            thread: "thread".into(),
            wall: "turn".into(),
            purpose: Purpose::WeeklyQuota,
            identity: renewal::Identity {
                account: "a".repeat(64),
                user: "b".repeat(64),
            },
            destination: renewal::Identity {
                account: "c".repeat(64),
                user: "d".repeat(64),
            },
        };
        let handoff = AccountHandoff {
            purpose: Purpose::WeeklyQuota,
            openai_scope: true,
            local: false,
            account: receipt.identity.account.clone(),
            user: receipt.identity.user.clone(),
            destination: Some(codex_alab_core::account_handoff::Destination {
                account: receipt.destination.account.clone(),
                user: receipt.destination.user.clone(),
            }),
            receipt: Some(receipt.id.clone()),
            turn: None,
            accepted: false,
            starts: 1,
            goal_id: None,
        };
        assert!(receipt.matches_handoff("thread", "turn", &handoff));
        assert!(!receipt.matches_handoff("other-thread", "turn", &handoff));
        assert!(!receipt.matches_handoff("thread", "other-turn", &handoff));
        for field in 0..5 {
            let mut changed = handoff.clone();
            match field {
                0 => changed.account = "e".repeat(64),
                1 => changed.user = "e".repeat(64),
                2 => changed.destination.as_mut().unwrap().account = "e".repeat(64),
                3 => changed.receipt = Some("2".repeat(32)),
                _ => changed.purpose = Purpose::Login,
            }
            assert!(!receipt.matches_handoff("thread", "turn", &changed));
        }
    }

    #[tokio::test]
    async fn shared_recovery_dispatch_preserves_scope_and_rejects_protocol_substitution() {
        let directory = tempfile::tempdir().expect("scratch host");
        let host = crate::fake::FakeAterm::start(directory.path(), crate::fake::Mode::Current)
            .await
            .expect("host");
        let client = Client::connect(host.endpoint(), crate::Options::default())
            .await
            .expect("client");
        client.harness_policy().await.expect("policy");
        client.claim().await.expect("claim");
        let identity = renewal::Identity {
            account: "a".repeat(64),
            user: "b".repeat(64),
        };
        let thread = "00000000-0000-0000-0000-000000000001";
        let wall = "00000000-0000-0000-0000-000000000002";
        let id = "1".repeat(32);
        for purpose in [Purpose::Login, Purpose::WeeklyQuota] {
            let (protocol, command, reply) = match purpose {
                Purpose::Login => (
                    "codex-renewal",
                    format!(
                        "codex-renewal prepare thread={thread} wall={wall} account={} user={}",
                        identity.account, identity.user
                    ),
                    format!(
                        "OK codex-renewal state=ready id={id} thread={thread} wall={wall} account={} user={}\n",
                        identity.account, identity.user
                    ),
                ),
                Purpose::WeeklyQuota => (
                    "codex-rotation",
                    format!(
                        "codex-rotation prepare thread={thread} turn={wall} source_account={} source_user={}",
                        identity.account, identity.user
                    ),
                    format!(
                        "OK codex-rotation state=ready id={id} thread={thread} turn={wall} source_account={} source_user={} target_account={} target_user={}\n",
                        identity.account,
                        identity.user,
                        "c".repeat(64),
                        "d".repeat(64)
                    ),
                ),
            };
            host.reply_once(&format!("@{} {command}", host.sid()), &reply);
            let Outcome::Ready(receipt) = prepare(&client, purpose, thread, wall, &identity)
                .await
                .expect("prepared")
            else {
                panic!("ready receipt")
            };
            assert!(receipt.purpose == purpose && receipt.identity == identity);
            assert_eq!(
                (&*receipt.id, &*receipt.thread, &*receipt.wall),
                (&*id, thread, wall)
            );
            assert_eq!(
                receipt.destination.account,
                if purpose == Purpose::Login {
                    identity.account.clone()
                } else {
                    "c".repeat(64)
                }
            );
            let other = if purpose == Purpose::Login {
                "codex-rotation"
            } else {
                "codex-renewal"
            };
            host.reply_once(
                &format!("@{} {protocol} status id={id}", host.sid()),
                &reply.replace(protocol, other),
            );
            assert!(status(&client, purpose, &id).await.is_err());
        }
    }
}
