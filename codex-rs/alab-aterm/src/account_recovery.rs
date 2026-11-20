// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Preserve both ends of account recovery while sharing the native continuation
//! lifecycle for interactive and headless callers. Each purpose still uses its
//! own host protocol and policy. This client adapter grants no execution authority.

pub use crate::exec_selection::Asked as ExecAsked;
pub use crate::exec_selection::Selection as ExecSelection;
pub use crate::exec_selection::Step as ExecStep;
pub use crate::exec_selection::ask as ask_exec;
pub use crate::exec_selection::prepare as prepare_exec;

use crate::Client;
use crate::Error;
use crate::renewal;
use crate::rotation;
use codex_alab_core::account_handoff::AccountHandoff;
use codex_alab_core::account_handoff::Purpose;
use codex_alab_core::quota::SpentWall;
use codex_alab_core::quota::SpentWindow;
use codex_alab_core::wall_kind::WallKind;
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

/// The policy word by which a host says it moves a session off a lost login to another
/// account already signed in (`codex-login-rotation`; `alab/design/accounts-lost-login.md`).
/// A host that prints no such word (aterm 0.199.0 among them) is never asked.
pub const LOGIN_ROTATION: &str = "codex_rotation_login";

/// Shared policy gate; a successful check alone does not authorize execution.
pub fn policy_allows(policy: &crate::policy::HarnessPolicy, purpose: Purpose) -> bool {
    policy.effective
        && match purpose {
            Purpose::Login => policy.get("logins") == Some("renew"),
            Purpose::WeeklyQuota => {
                policy.get("codex_rotation") == Some("1")
                    && matches!(policy.get("accounts"), Some("early" | "wall"))
            }
            Purpose::LoginRotation => {
                policy.get(LOGIN_ROTATION) == Some("1")
                    && matches!(policy.get("accounts"), Some("early" | "wall"))
            }
        }
        && policy.switch("relaunch")
        && policy.switch("resume_limits")
}

/// WHETHER A HOST'S ANSWER TO A LOST LOGIN'S MOVE IS A DEFINITE NO: it knows no such verb, it
/// refused the request as invalid or for a native wall or purpose that is no longer the
/// one asked about (none of which another ask can change), or it answered nothing this
/// reader can take. Every other refusal (a hold, a claim or record in motion, capacity) and
/// every transport failure is no answer yet, asked again.
pub fn login_rotation_declined(error: &Error) -> bool {
    match error {
        Error::Refused(word) => matches!(
            word.as_str(),
            rotation::UNKNOWN_VERB
                | "invalid request"
                | "native wall changed"
                | "wrong recovery purpose"
        ),
        Error::Malformed(_) => true,
        _ => false,
    }
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

/// The policy word by which a host says it leaves a wall its fresh usage read shows refusing
/// ordinary use with no spent window to explain it: a workspace's credits or spend cap, spend
/// control, a credit-only plan out of credits (gap 4 of `alab/PARALLEL-LOGINS.md`, aterm's step
/// S2). A host that prints no such word answers `not-needed` at such a wall, so an exec run
/// keeps its request at a workspace's wall only for a host that prints it
/// ([`crate::exec_selection::ask`]).
pub const ROTATION_REFUSAL: &str = "codex_rotation_refusal";

/// Whether this host, by its own policy reply, leaves a wall of refused ordinary use
/// ([`ROTATION_REFUSAL`] `=1`, beside the rotation policy itself).
pub fn leaves_refusals(policy: &crate::policy::HarnessPolicy) -> bool {
    policy_allows(policy, Purpose::WeeklyQuota) && policy.get(ROTATION_REFUSAL) == Some("1")
}

/// Whether the host accepts a model's own bucket on an interactive quota rotation prepare.
pub fn leaves_limits(policy: &crate::policy::HarnessPolicy) -> bool {
    policy_allows(policy, Purpose::WeeklyQuota) && policy.get("codex_rotation_limits") == Some("1")
}

/// A named exec move requires both the general model rotation contract and the exec arm contract.
pub(crate) fn leaves_exec_limits(policy: &crate::policy::HarnessPolicy) -> bool {
    leaves_limits(policy) && policy.get("codex_exec_rotation_limits") == Some("1")
}

pub fn leaves_exec_wall(policy: &crate::policy::HarnessPolicy, kind: &WallKind) -> bool {
    match kind {
        WallKind::Ordinary => policy_allows(policy, Purpose::WeeklyQuota),
        WallKind::Workspace(_) => leaves_refusals(policy),
        WallKind::Limit(id) => {
            codex_alab_core::wall_kind::valid_limit_id(id) && leaves_exec_limits(policy)
        }
    }
}

/// Apply the same wire grammar and advertised policy at prepare, arm, and replay of a lost arm.
pub(crate) fn append_exec_limit(
    client: &Client,
    request: &mut String,
    limit: Option<&str>,
) -> Result<(), Error> {
    if let Some(id) = limit {
        if !codex_alab_core::wall_kind::valid_limit_id(id)
            || !client
                .policy()
                .is_some_and(|policy| leaves_exec_limits(&policy))
        {
            return Err(Error::BadRequest("unsupported exec rotation limit".into()));
        }
        request.push_str(" limit=");
        request.push_str(id);
    }
    Ok(())
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

impl Receipt {
    /// A move's receipt, for the purpose whose protocol answered it.
    fn moved(value: rotation::Receipt, purpose: Purpose) -> Self {
        Self {
            id: value.id,
            thread: value.origin.thread,
            wall: value.origin.turn,
            purpose,
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

impl Outcome {
    /// A move's outcome, for the purpose whose protocol answered it.
    fn moved(value: rotation::Outcome, purpose: Purpose) -> Self {
        let receipt = |value| Receipt::moved(value, purpose);
        match value {
            rotation::Outcome::NotNeeded => Self::NotNeeded,
            rotation::Outcome::Waiting(value) => Self::Waiting(value.map(receipt)),
            rotation::Outcome::Ready(value) => Self::Ready(receipt(value)),
            rotation::Outcome::Committed(value) => Self::Committed(receipt(value)),
            rotation::Outcome::Consumed(value) => Self::Consumed(receipt(value)),
            rotation::Outcome::Retired(value) => Self::Retired(receipt(value)),
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
    prepare_with_limit(client, purpose, thread, wall, identity, None).await
}

/// Prepare a quota rotation for the named model bucket, or ordinary recovery when omitted.
pub async fn prepare_with_limit(
    client: &Client,
    purpose: Purpose,
    thread: &str,
    wall: &str,
    identity: &renewal::Identity,
    limit: Option<&str>,
) -> Result<Outcome, Error> {
    if limit.is_some() && purpose != Purpose::WeeklyQuota {
        return Err(Error::BadRequest(
            "model limit requires quota rotation".into(),
        ));
    }
    let origin = || rotation::Origin {
        thread: thread.into(),
        turn: wall.into(),
        identity: identity.clone(),
    };
    let moved = |outcome| Outcome::moved(outcome, purpose);
    match purpose {
        Purpose::Login => client
            .renewal_prepare(thread, wall, identity)
            .await
            .map(Into::into),
        Purpose::WeeklyQuota => client
            .prepare_move(rotation::Protocol::Quota, &origin(), limit)
            .await
            .map(moved),
        Purpose::LoginRotation => client.login_rotation_prepare(&origin()).await.map(moved),
    }
}

pub async fn status(client: &Client, purpose: Purpose, id: &str) -> Result<Outcome, Error> {
    let moved = |outcome| Outcome::moved(outcome, purpose);
    match purpose {
        Purpose::Login => client.renewal_status(id).await.map(Into::into),
        Purpose::WeeklyQuota => client.rotation_status(id).await.map(moved),
        Purpose::LoginRotation => client.login_rotation_status(id).await.map(moved),
    }
}

pub async fn commit(client: &Client, purpose: Purpose, id: &str) -> Result<Outcome, Error> {
    let moved = |outcome| Outcome::moved(outcome, purpose);
    match purpose {
        Purpose::Login => client.renewal_commit(id).await.map(Into::into),
        Purpose::WeeklyQuota => client.rotation_commit(id).await.map(moved),
        Purpose::LoginRotation => client.login_rotation_commit(id).await.map(moved),
    }
}

pub async fn consume(
    client: &Client,
    purpose: Purpose,
    id: &str,
    turn: &str,
) -> Result<Outcome, Error> {
    let moved = |outcome| Outcome::moved(outcome, purpose);
    match purpose {
        Purpose::Login => client.renewal_consume(id, turn).await.map(Into::into),
        Purpose::WeeklyQuota => client.rotation_consume(id, turn).await.map(moved),
        Purpose::LoginRotation => client.login_rotation_consume(id, turn).await.map(moved),
    }
}

/// REPORT THAT A MOVE'S RESUMED TURN WAS SERVED ([`Client::rotation_complete`]). Only a usage
/// wall's move puts a pool canary on its destination for the host to prove, so only
/// [`Purpose::WeeklyQuota`] sends one; a same-account renewal and a lost login's move have
/// nothing to prove and answer [`rotation::Completion::Unsupported`] without asking the host.
pub async fn complete(
    client: &Client,
    purpose: Purpose,
    id: &str,
    turn: &str,
) -> Result<rotation::Completion, Error> {
    match purpose {
        Purpose::WeeklyQuota => client.rotation_complete(id, turn).await,
        Purpose::Login | Purpose::LoginRotation => Ok(rotation::Completion::Unsupported),
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
            limit: None,
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
        for purpose in [Purpose::Login, Purpose::WeeklyQuota, Purpose::LoginRotation] {
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
                Purpose::WeeklyQuota | Purpose::LoginRotation => {
                    let verb = if purpose == Purpose::WeeklyQuota {
                        "codex-rotation"
                    } else {
                        "codex-login-rotation"
                    };
                    (
                        verb,
                        format!(
                            "{verb} prepare thread={thread} turn={wall} source_account={} source_user={}",
                            identity.account, identity.user
                        ),
                        format!(
                            "OK {verb} state=ready id={id} thread={thread} turn={wall} source_account={} source_user={} target_account={} target_user={}\n",
                            identity.account,
                            identity.user,
                            "c".repeat(64),
                            "d".repeat(64)
                        ),
                    )
                }
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
            for other in ["codex-renewal", "codex-rotation", "codex-login-rotation"]
                .into_iter()
                .filter(|other| *other != protocol)
            {
                host.reply_once(
                    &format!("@{} {protocol} status id={id}", host.sid()),
                    &reply.replace(protocol, other),
                );
                assert!(
                    matches!(
                        status(&client, purpose, &id).await,
                        Err(Error::Malformed(_))
                    ),
                    "{purpose:?} read {other}'s reply"
                );
            }
            // Each purpose's commit and consume name its own verb.
            host.reply_once(
                &format!("@{} {protocol} commit id={id}", host.sid()),
                &reply.replace("state=ready", "state=committed"),
            );
            assert!(matches!(
                commit(&client, purpose, &id).await,
                Ok(Outcome::Committed(receipt)) if receipt.purpose == purpose
            ));
        }
    }

    #[tokio::test]
    async fn a_named_prepare_requires_the_advertised_bucket_and_binds_the_source() {
        let directory = tempfile::tempdir().unwrap();
        let host = crate::fake::FakeAterm::start(directory.path(), crate::fake::Mode::Current)
            .await
            .unwrap();
        host.set_policy_key("accounts", "wall");
        host.set_policy_key("codex_rotation", "1");
        let client = Client::connect(host.endpoint(), crate::Options::default())
            .await
            .unwrap();
        client.harness_policy().await.unwrap();
        client.claim().await.unwrap();
        let identity = renewal::Identity {
            account: "a".repeat(64),
            user: "b".repeat(64),
        };
        let thread = "00000000-0000-0000-0000-000000000001";
        let turn = "00000000-0000-0000-0000-000000000002";
        let limit = "codex_other";
        let ask = |purpose, limit| {
            prepare_with_limit(&client, purpose, thread, turn, &identity, Some(limit))
        };
        let requests = host.requests();
        assert!(matches!(
            ask(Purpose::WeeklyQuota, limit).await,
            Err(Error::BadRequest(_))
        ));
        assert_eq!(host.requests(), requests);

        host.set_policy_key("codex_rotation_limits", "1");
        client.harness_policy().await.unwrap();
        let requests = host.requests();
        for (purpose, id) in [
            (Purpose::Login, limit),
            (Purpose::LoginRotation, limit),
            (Purpose::WeeklyQuota, "codex"),
            (Purpose::WeeklyQuota, "x target_account=y"),
        ] {
            assert!(matches!(ask(purpose, id).await, Err(Error::BadRequest(_))));
        }
        assert_eq!(host.requests(), requests);
        let command = format!(
            "@{} codex-rotation prepare thread={thread} turn={turn} source_account={} source_user={} limit={limit}",
            host.sid(),
            identity.account,
            identity.user
        );
        let reply = format!(
            "OK codex-rotation state=ready id={} thread={thread} turn={turn} source_account={} source_user={} target_account={} target_user={}\n",
            "1".repeat(32),
            identity.account,
            identity.user,
            "c".repeat(64),
            "d".repeat(64)
        );
        host.reply_once(&command, &reply);
        assert!(
            matches!(ask(Purpose::WeeklyQuota, limit).await, Ok(Outcome::Ready(receipt)) if receipt.identity == identity)
        );
        host.reply_once(&command, &reply.replace(&identity.user, &"e".repeat(64)));
        assert!(matches!(
            ask(Purpose::WeeklyQuota, limit).await,
            Err(Error::Malformed(_))
        ));
        assert_eq!(
            &host.requests()[requests.len()..],
            [command.clone(), command]
        );
    }

    /// ONLY A USAGE WALL'S MOVE REPORTS ITS COMPLETION: a renewal and a lost login's move answer
    /// unsupported and send nothing, whatever the id; a usage wall's move sends its one
    /// `codex-rotation complete` line (which the fake, knowing no such verb, does not take).
    #[tokio::test]
    async fn only_a_usage_walls_move_reports_its_completion() {
        let directory = tempfile::tempdir().expect("scratch host");
        let host = crate::fake::FakeAterm::start(directory.path(), crate::fake::Mode::Current)
            .await
            .expect("host");
        let client = Client::connect(host.endpoint(), crate::Options::default())
            .await
            .expect("client");
        client.harness_policy().await.expect("policy");
        client.claim().await.expect("claim");
        let id = "1".repeat(32);
        let turn = "00000000-0000-0000-0000-000000000002";
        let before = host.requests();
        for purpose in [Purpose::Login, Purpose::LoginRotation] {
            for id in [id.as_str(), "not-a-receipt"] {
                assert_eq!(
                    complete(&client, purpose, id, turn).await.ok(),
                    Some(rotation::Completion::Unsupported),
                    "{purpose:?}"
                );
            }
        }
        assert_eq!(host.requests(), before, "nothing was sent");
        assert_eq!(
            complete(&client, Purpose::WeeklyQuota, &id, turn)
                .await
                .ok(),
            Some(rotation::Completion::Unsupported)
        );
        let sent: Vec<_> = host.requests().split_off(before.len());
        assert_eq!(
            sent,
            [format!(
                "@{} codex-rotation complete id={id} turn={turn}",
                host.sid()
            )]
        );
    }

    /// A LOST LOGIN'S MOVE IS ASKED ONLY OF A HOST THAT SAYS IT MOVES ONE: `codex_rotation_login=1`,
    /// an `accounts` policy that moves sessions (`early` or `wall`), and the relaunch and resume
    /// switches. aterm 0.199.0 prints no such word, so it is never asked; the usage wall's own
    /// `codex_rotation` word is not that word, nor is it needed.
    #[test]
    fn a_login_rotation_is_asked_only_of_a_host_that_advertises_it() {
        let login = |keys: &[(&str, &str)]| {
            let mut policy = rotating(keys);
            policy.keys.remove("codex_rotation");
            policy
        };
        assert!(policy_allows(
            &login(&[(LOGIN_ROTATION, "1")]),
            Purpose::LoginRotation
        ));
        assert!(policy_allows(
            &login(&[(LOGIN_ROTATION, "1"), ("accounts", "early")]),
            Purpose::LoginRotation
        ));
        // aterm 0.199.0: no word. Its usage wall's rotation stays as it was.
        assert!(!policy_allows(&rotating(&[]), Purpose::LoginRotation));
        assert!(policy_allows(&rotating(&[]), Purpose::WeeklyQuota));
        for keys in [
            &[(LOGIN_ROTATION, "0")][..],
            &[(LOGIN_ROTATION, "true")],
            &[(LOGIN_ROTATION, "1"), ("accounts", "off")],
            &[(LOGIN_ROTATION, "1"), ("accounts", "manual")],
            &[(LOGIN_ROTATION, "1"), ("relaunch", "false")],
            &[(LOGIN_ROTATION, "1"), ("resume_limits", "false")],
        ] {
            assert!(
                !policy_allows(&login(keys), Purpose::LoginRotation),
                "{keys:?}"
            );
        }
        let mut accountless = login(&[(LOGIN_ROTATION, "1")]);
        accountless.keys.remove("accounts");
        assert!(!policy_allows(&accountless, Purpose::LoginRotation));
        let mut ineffective = login(&[(LOGIN_ROTATION, "1")]);
        ineffective.effective = false;
        assert!(!policy_allows(&ineffective, Purpose::LoginRotation));
        // The word moves no other purpose.
        let only_login = login(&[(LOGIN_ROTATION, "1")]);
        assert!(!policy_allows(&only_login, Purpose::WeeklyQuota));
        assert!(!policy_allows(&only_login, Purpose::Login));
    }

    /// A WALL OF REFUSED ORDINARY USE IS LEFT ONLY BY A HOST THAT SAYS SO: `codex_rotation_refusal=1`
    /// beside the rotation policy. Today's aterm prints no such word, so it leaves none.
    #[test]
    fn a_refused_wall_is_left_only_by_a_host_that_advertises_it() {
        assert!(leaves_refusals(&rotating(&[(ROTATION_REFUSAL, "1")])));
        assert!(!leaves_refusals(&rotating(&[])), "today's aterm");
        for word in ["0", "true", "", "01", " 1"] {
            assert!(
                !leaves_refusals(&rotating(&[(ROTATION_REFUSAL, word)])),
                "{word:?}"
            );
        }
        // The word is no rotation policy of its own: each key rotation needs.
        for (key, off) in [
            ("codex_rotation", "0"),
            ("accounts", "off"),
            ("relaunch", "false"),
            ("resume_limits", "false"),
        ] {
            let mut host = rotating(&[(ROTATION_REFUSAL, "1")]);
            host.keys.insert(key.into(), off.into());
            assert!(!leaves_refusals(&host), "{key}={off}");
        }
        let mut standby = rotating(&[(ROTATION_REFUSAL, "1")]);
        standby.effective = false;
        assert!(!leaves_refusals(&standby));
        // It moves nothing else.
        let only = rotating(&[(ROTATION_REFUSAL, "1")]);
        assert!(!policy_allows(&only, Purpose::LoginRotation));
        assert_eq!(five_hour_bound(&only), None);
        // An exec run's wall, by kind: an ordinary one under the rotation policy alone, a
        // workspace's only with the workspace word, a model's own limit with separate grants.
        use codex_alab_core::wall_kind::Workspace;
        let credits = WallKind::Workspace(Workspace::MemberCreditsDepleted);
        let model = WallKind::Limit("codex_other".into());
        let today = rotating(&[]);
        assert!(leaves_exec_wall(&today, &WallKind::Ordinary));
        assert!(!leaves_exec_wall(&today, &credits), "today's aterm");
        assert!(!leaves_exec_wall(&today, &model));
        assert!(leaves_exec_wall(&only, &WallKind::Ordinary));
        assert!(leaves_exec_wall(&only, &credits));
        assert!(!leaves_exec_wall(&only, &model));
        let mut off = rotating(&[(ROTATION_REFUSAL, "1")]);
        off.keys.insert("codex_rotation".into(), "0".into());
        for kind in [&WallKind::Ordinary, &credits, &model] {
            assert!(!leaves_exec_wall(&off, kind), "{kind:?}");
        }
    }

    #[test]
    fn model_buckets_require_the_exact_capability_and_effective_quota_rotation() {
        let key = "codex_rotation_limits";
        let enabled = rotating(&[(key, "1")]);
        assert!(leaves_limits(&enabled));
        assert!(!leaves_limits(&rotating(&[])));
        for value in ["0", "true", "", "01", " 1"] {
            assert!(!leaves_limits(&rotating(&[(key, value)])), "{value:?}");
        }
        for (key, off) in [
            ("codex_rotation", "0"),
            ("accounts", "off"),
            ("relaunch", "false"),
            ("resume_limits", "false"),
        ] {
            let mut host = enabled.clone();
            host.keys.insert(key.into(), off.into());
            assert!(!leaves_limits(&host), "{key}={off}");
        }
        let mut standby = enabled;
        standby.effective = false;
        assert!(!leaves_limits(&standby));
    }

    /// A DEFINITE NO, OR NO ANSWER YET: a host that knows no such verb, an invalid request, a
    /// native wall or purpose that is no longer the one asked about, or an unreadable answer is
    /// a decline; a hold, a record or claim in motion, capacity and every transport failure are
    /// asked again.
    #[test]
    fn only_a_definite_answer_declines_a_login_rotation() {
        for word in [
            rotation::UNKNOWN_VERB,
            "invalid request",
            "native wall changed",
            "wrong recovery purpose",
        ] {
            assert!(
                login_rotation_declined(&Error::Refused(word.into())),
                "{word}"
            );
        }
        assert!(login_rotation_declined(&Error::Malformed(
            "unreadable".into()
        )));
        for word in rotation::REFUSALS
            .iter()
            .copied()
            .chain([rotation::OTHER])
            .filter(|word| {
                !matches!(
                    *word,
                    "invalid request" | "native wall changed" | "wrong recovery purpose"
                )
            })
        {
            assert!(
                !login_rotation_declined(&Error::Refused(word.into())),
                "{word}"
            );
        }
        for error in [
            Error::Unanswered,
            Error::Closed,
            Error::Busy {
                line: "ERR busy".into(),
                retry_ms: None,
            },
            Error::Io(std::io::Error::other("unreachable")),
            Error::BadRequest("no claim".into()),
        ] {
            assert!(!login_rotation_declined(&error), "{error}");
        }
    }
}
