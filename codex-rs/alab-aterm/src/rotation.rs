// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Cross-account handoff: a usage wall's (`codex-rotation`) and a lost login's
//! (`codex-login-rotation`, `alab/design/accounts-lost-login.md`), one grammar under two verbs
//! ([`Protocol`]). The host selects a destination from its own roster; neither protocol ever
//! accepts a credential path or restart command. Same-account credential renewal remains a
//! separate protocol. A usage wall's move alone also takes a completion report
//! ([`Client::rotation_complete`]), which lets the host prove the destination for every
//! session walled behind it.

use crate::Client;
use crate::Error;
use crate::renewal::Identity;
use std::collections::BTreeMap;

/// Which move the host is asked for: the verb, and the prefix every reply to it carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Protocol {
    /// Another account at a usage wall (`codex-rotation`).
    Quota,
    /// Another already-signed-in account at a lost login (`codex-login-rotation`).
    Login,
}

impl Protocol {
    pub fn verb(self) -> &'static str {
        match self {
            Self::Quota => "codex-rotation",
            Self::Login => "codex-login-rotation",
        }
    }
}

/// THE CLOSED WORDS A HOST'S `ERR <verb> …` REFUSAL IS READ AS ([`Error::Refused`]): aterm's
/// own refusals of the account-handoff verbs. A refusal in no other words is [`OTHER`]; the
/// host's line itself is never kept, as it may carry an identity.
pub const REFUSALS: &[&str] = &[
    "invalid request",
    "denied",
    "wall changed",
    "native wall changed",
    "wrong recovery purpose",
    "record unavailable",
    "expired or interrupted",
    "claim changed",
    "selection changed",
    "receipt changed",
    "invalid receipt",
    "capacity",
    "unavailable",
    "unknown receipt",
    "worker unavailable",
    "wrong session",
    "identity changed",
    "wrong process",
    "acceptance changed",
];

/// A host refusal in words none of [`REFUSALS`] are.
pub const OTHER: &str = "other";

/// A host that knows no such verb (aterm's `ERR unknown verb …`), as [`Error::Refused`] says it.
pub const UNKNOWN_VERB: &str = "unknown verb";

#[derive(Clone, PartialEq, Eq)]
pub struct Origin {
    pub thread: String,
    pub turn: String,
    pub identity: Identity,
}

impl Origin {
    pub fn valid(&self) -> bool {
        uuid(&self.thread) && uuid(&self.turn) && identity(&self.identity)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct Receipt {
    pub id: String,
    pub origin: Origin,
    pub destination: Identity,
}

#[derive(Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The source is at no usage wall the host would leave for another account
    /// (none at all, or one too near its own reset); ordinary quota recovery
    /// still owns it.
    NotNeeded,
    Waiting(Option<Receipt>),
    Ready(Receipt),
    Committed(Receipt),
    Consumed(Receipt),
    Retired(Receipt),
}

impl Outcome {
    pub fn receipt(&self) -> Option<&Receipt> {
        match self {
            Self::NotNeeded => None,
            Self::Waiting(receipt) => receipt.as_ref(),
            Self::Ready(receipt)
            | Self::Committed(receipt)
            | Self::Consumed(receipt)
            | Self::Retired(receipt) => Some(receipt),
        }
    }
}

/// What a host says of a usage wall's completion report ([`Client::rotation_complete`]): the
/// destination's proof is a fact of the host's pool, so the report grants this process
/// nothing and changes nothing of its receipt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Completion {
    /// The host holds the destination proven for this receipt: recorded now, or already.
    Recorded,
    /// The host could not record it yet: a receipt writer, sign-in or auth lock was busy, or
    /// the host could not yet read this claimed process (its environment, its record or its
    /// resume) before it looked at the report at all. Asking again shortly may.
    Waiting,
    /// The host takes no such report: an aterm that knows `codex-rotation` but not its
    /// `complete` refuses it as `invalid request` before it reads any claim, and one with no
    /// `codex-rotation` knows no such verb. Nothing was recorded, and the host's canary hold
    /// lapses as it would have without the report.
    Unsupported,
}

fn hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
            }
        })
}

fn identity(value: &Identity) -> bool {
    hex(&value.account, 64) && hex(&value.user, 64)
}

fn malformed() -> Error {
    // Never echo an untrusted reply; it may contain credentials or raw identities.
    Error::Malformed("invalid cross-account rotation reply".into())
}

/// A reply to `protocol`'s verb that is no `OK` of it: a host that knows no such verb or
/// refuses the request (in a closed word, [`REFUSALS`]), one too busy to answer
/// ([`Error::Busy`]), or one this reader cannot take ([`Error::Malformed`]). Never the line.
fn refusal(line: &str, protocol: Protocol) -> Error {
    if line.starts_with("ERR busy") || line.starts_with("ERR control server busy") {
        return Error::Busy {
            line: "ERR busy".into(),
            retry_ms: crate::wire::retry_ms(line),
        };
    }
    if line == "ERR unknown verb" || line.starts_with("ERR unknown verb ") {
        return Error::Refused(UNKNOWN_VERB.into());
    }
    match line
        .strip_prefix("ERR ")
        .and_then(|rest| rest.strip_prefix(protocol.verb()))
        .and_then(|rest| rest.strip_prefix(' '))
    {
        Some(words) => Error::Refused(
            REFUSALS
                .iter()
                .find(|known| **known == words.trim_end())
                .copied()
                .unwrap_or(OTHER)
                .into(),
        ),
        None => malformed(),
    }
}

/// The `key=value` words of an `OK` of `protocol`'s verb, each key once; `None` for a reply
/// that is no such `OK` (read by [`refusal`]).
fn ok_fields(line: &str, protocol: Protocol) -> Option<Result<BTreeMap<&str, &str>, Error>> {
    let rest = line
        .strip_prefix("OK ")
        .and_then(|rest| rest.strip_prefix(protocol.verb()))
        .and_then(|rest| rest.strip_prefix(' '))?;
    let mut fields = BTreeMap::new();
    for word in rest.split_ascii_whitespace() {
        let Some((key, value)) = word.split_once('=') else {
            return Some(Err(malformed()));
        };
        if fields.insert(key, value).is_some() {
            return Some(Err(malformed()));
        }
    }
    Some(Ok(fields))
}

fn parse(line: &str, protocol: Protocol) -> Result<Outcome, Error> {
    let fields = ok_fields(line, protocol).ok_or_else(|| refusal(line, protocol))??;
    let state = fields.get("state").copied().ok_or_else(malformed)?;
    if state == "not-needed" && fields.len() == 1 {
        return Ok(Outcome::NotNeeded);
    }
    if state == "waiting" && fields.len() == 1 {
        return Ok(Outcome::Waiting(None));
    }
    let receipt = receipt(&fields)?;
    match state {
        "waiting" => Ok(Outcome::Waiting(Some(receipt))),
        "ready" => Ok(Outcome::Ready(receipt)),
        "committed" => Ok(Outcome::Committed(receipt)),
        "consumed" => Ok(Outcome::Consumed(receipt)),
        "retired" => Ok(Outcome::Retired(receipt)),
        _ => Err(malformed()),
    }
}

/// THE ONE RECEIPT READER every receipt-bearing reply of either verb goes through: the state
/// and the seven receipt fields, and nothing else, each well formed, with a destination that
/// is not the source's account.
fn receipt(fields: &BTreeMap<&str, &str>) -> Result<Receipt, Error> {
    if fields.len() != 8 || !fields.contains_key("state") {
        return Err(malformed());
    }
    let get = |name| fields.get(name).copied().ok_or_else(malformed);
    let receipt = Receipt {
        id: get("id")?.into(),
        origin: Origin {
            thread: get("thread")?.into(),
            turn: get("turn")?.into(),
            identity: Identity {
                account: get("source_account")?.into(),
                user: get("source_user")?.into(),
            },
        },
        destination: Identity {
            account: get("target_account")?.into(),
            user: get("target_user")?.into(),
        },
    };
    if !hex(&receipt.id, 32)
        || !receipt.origin.valid()
        || !identity(&receipt.destination)
        || receipt.origin.identity.account == receipt.destination.account
    {
        return Err(malformed());
    }
    Ok(receipt)
}

/// A HOST'S ANSWER TO A COMPLETION REPORT, READ FAIL-CLOSED. `state=completed` with the
/// receipt it proves ([`receipt`], the reader every receipt reply goes through), naming the
/// receipt `id` and the resumed turn `turn` the report was for, is [`Completion::Recorded`];
/// a bare `state=waiting` is [`Completion::Waiting`]; `invalid request` under the verb and an
/// unknown verb are [`Completion::Unsupported`]. Every other refusal is [`refusal`]'s (a
/// closed word, busy, or unreadable), and every other `OK` (another receipt or turn, another
/// state, a waiting reply that names a receipt, a ninth field) is unreadable: never recorded.
fn parse_completion(line: &str, id: &str, turn: &str) -> Result<Completion, Error> {
    let protocol = Protocol::Quota;
    let Some(fields) = ok_fields(line, protocol) else {
        return match refusal(line, protocol) {
            Error::Refused(word) if word == UNKNOWN_VERB || word == "invalid request" => {
                Ok(Completion::Unsupported)
            }
            error => Err(error),
        };
    };
    let fields = fields?;
    match fields.get("state").copied() {
        Some("waiting") if fields.len() == 1 => Ok(Completion::Waiting),
        Some("completed") => {
            let receipt = receipt(&fields)?;
            if receipt.id == id && receipt.origin.turn == turn {
                Ok(Completion::Recorded)
            } else {
                Err(malformed())
            }
        }
        _ => Err(malformed()),
    }
}

impl Client {
    /// One request of `protocol`. Transport failures pass through as they are (the host was
    /// not reached, did not answer, closed, or was busy); every other failure of the exchange
    /// is unreadable, and a reply is read only as `protocol`'s ([`parse`]).
    async fn rotation_call(&self, protocol: Protocol, request: &str) -> Result<Outcome, Error> {
        parse(&self.rotation_line(request).await?, protocol)
    }

    /// The status line a handoff request is answered with, sent only under the native
    /// self-claim ([`Error::BadRequest`] without it, and nothing reaches the host).
    async fn rotation_line(&self, request: &str) -> Result<String, Error> {
        if !self.holds_claim() {
            return Err(Error::BadRequest(
                "rotation requires the native self-claim".into(),
            ));
        }
        let reply = self.on_self(request).await.map_err(|error| match error {
            Error::Io(_) | Error::Unanswered | Error::Closed | Error::Busy { .. } => error,
            _ => malformed(),
        })?;
        Ok(reply.status)
    }

    async fn prepare_move(&self, protocol: Protocol, origin: &Origin) -> Result<Outcome, Error> {
        if !origin.valid() {
            return Err(Error::BadRequest("invalid rotation origin".into()));
        }
        let outcome = self
            .rotation_call(
                protocol,
                &format!(
                    "{} prepare thread={} turn={} source_account={} source_user={}",
                    protocol.verb(),
                    origin.thread,
                    origin.turn,
                    origin.identity.account,
                    origin.identity.user,
                ),
            )
            .await?;
        if outcome
            .receipt()
            .is_some_and(|receipt| receipt.origin != *origin)
        {
            return Err(malformed());
        }
        Ok(outcome)
    }

    pub async fn rotation_prepare(&self, origin: &Origin) -> Result<Outcome, Error> {
        self.prepare_move(Protocol::Quota, origin).await
    }

    pub async fn rotation_status(&self, id: &str) -> Result<Outcome, Error> {
        self.rotation_id(Protocol::Quota, "status", id, None).await
    }

    pub async fn rotation_commit(&self, id: &str) -> Result<Outcome, Error> {
        self.rotation_id(Protocol::Quota, "commit", id, None).await
    }

    pub async fn rotation_consume(&self, id: &str, turn: &str) -> Result<Outcome, Error> {
        self.consume_move(Protocol::Quota, id, turn).await
    }

    /// `codex-rotation complete id=<id> turn=<turn>`: as the bound successor of the consumed
    /// usage-wall receipt `id`, report that its resumed turn `turn` (the walled turn's own id)
    /// was served on the destination, so the host can prove that destination for every
    /// session walled behind it ([`parse_completion`] reads the answer). The receipt id, the
    /// turn and the native self-claim are checked before anything is sent
    /// ([`Error::BadRequest`]); transport failures pass through as they are.
    pub async fn rotation_complete(&self, id: &str, turn: &str) -> Result<Completion, Error> {
        if !hex(id, 32) {
            return Err(Error::BadRequest("invalid rotation receipt".into()));
        }
        if !uuid(turn) {
            return Err(Error::BadRequest("invalid rotation completed turn".into()));
        }
        let line = self
            .rotation_line(&format!(
                "{} complete id={id} turn={turn}",
                Protocol::Quota.verb()
            ))
            .await?;
        parse_completion(&line, id, turn)
    }

    /// `codex-login-rotation prepare`: ask the host to move this session off its lost login
    /// to another account already signed in on this Mac.
    pub async fn login_rotation_prepare(&self, origin: &Origin) -> Result<Outcome, Error> {
        self.prepare_move(Protocol::Login, origin).await
    }

    pub async fn login_rotation_status(&self, id: &str) -> Result<Outcome, Error> {
        self.rotation_id(Protocol::Login, "status", id, None).await
    }

    pub async fn login_rotation_commit(&self, id: &str) -> Result<Outcome, Error> {
        self.rotation_id(Protocol::Login, "commit", id, None).await
    }

    pub async fn login_rotation_consume(&self, id: &str, turn: &str) -> Result<Outcome, Error> {
        self.consume_move(Protocol::Login, id, turn).await
    }

    async fn consume_move(
        &self,
        protocol: Protocol,
        id: &str,
        turn: &str,
    ) -> Result<Outcome, Error> {
        if !uuid(turn) {
            return Err(Error::BadRequest("invalid rotation accepted turn".into()));
        }
        self.rotation_id(protocol, "consume", id, Some(turn)).await
    }

    async fn rotation_id(
        &self,
        protocol: Protocol,
        verb: &str,
        id: &str,
        turn: Option<&str>,
    ) -> Result<Outcome, Error> {
        if !hex(id, 32) {
            return Err(Error::BadRequest("invalid rotation receipt".into()));
        }
        let suffix = turn.map(|turn| format!(" turn={turn}")).unwrap_or_default();
        let outcome = self
            .rotation_call(
                protocol,
                &format!("{} {verb} id={id}{suffix}", protocol.verb()),
            )
            .await?;
        if outcome.receipt().is_none_or(|receipt| receipt.id != id) {
            return Err(malformed());
        }
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(state: &str) -> String {
        format!(
            "OK codex-rotation state={state} id={} thread=00000000-0000-0000-0000-000000000001 turn=00000000-0000-0000-0000-000000000002 source_account={} source_user={} target_account={} target_user={}",
            "1".repeat(32),
            "a".repeat(64),
            "b".repeat(64),
            "c".repeat(64),
            "d".repeat(64),
        )
    }

    #[test]
    fn every_phase_retains_both_identities_and_the_failed_native_turn() {
        for phase in ["waiting", "ready", "committed", "consumed", "retired"] {
            assert!(parse(&reply(phase), Protocol::Quota).is_ok_and(|outcome| {
                outcome.receipt().is_some_and(|r| {
                    r.origin.identity.account == "a".repeat(64)
                        && r.destination.account == "c".repeat(64)
                        && r.origin.turn.ends_with('2')
                })
            }));
        }
        assert!(matches!(
            parse("OK codex-rotation state=waiting", Protocol::Quota),
            Ok(Outcome::Waiting(None))
        ));
        assert!(matches!(
            parse("OK codex-rotation state=not-needed", Protocol::Quota),
            Ok(Outcome::NotNeeded)
        ));
        assert!(parse(&reply("not-needed"), Protocol::Quota).is_err());
    }

    #[test]
    fn renewal_or_unbound_and_ambiguous_replies_cannot_be_rotation_receipts() {
        let good = reply("ready");
        for bad in [
            good.replace("codex-rotation", "codex-renewal"),
            good.replace(&"c".repeat(64), &"a".repeat(64)),
            good.replace(&"b".repeat(64), "raw-user"),
            good.replace("state=ready", "state=unknown"),
            good.replace("00000000-0000-0000-0000-000000000002", "invalid"),
            format!("{good} state=committed"),
            format!("{good} path=/credentials"),
        ] {
            assert!(parse(&bad, Protocol::Quota).is_err());
        }
        let failure = parse("ERR secret-token-value", Protocol::Quota)
            .err()
            .map(|e| e.to_string());
        assert!(failure.is_some_and(|error| !error.contains("secret-token-value")));
    }

    /// ONE GRAMMAR UNDER TWO VERBS, NEVER SUBSTITUTED: a lost login's move reads the same eight
    /// fields under its own `OK codex-login-rotation` prefix, and a reply under the other verb's
    /// prefix is unreadable for either. The target is never the source, and a ninth field is
    /// refused.
    #[test]
    fn a_login_rotation_reply_is_read_only_under_its_own_verb() {
        let login = |state: &str| reply(state).replace("codex-rotation", "codex-login-rotation");
        for phase in ["waiting", "ready", "committed", "consumed", "retired"] {
            let outcome = parse(&login(phase), Protocol::Login);
            assert!(
                outcome.as_ref().is_ok_and(|outcome| outcome
                    .receipt()
                    .is_some_and(|r| r.destination.account == "c".repeat(64))),
                "{phase}"
            );
            assert!(
                matches!(
                    parse(&login(phase), Protocol::Quota),
                    Err(Error::Malformed(_))
                ),
                "a login rotation's {phase} is no quota rotation's"
            );
            assert!(
                matches!(
                    parse(&reply(phase), Protocol::Login),
                    Err(Error::Malformed(_))
                ),
                "a quota rotation's {phase} is no login rotation's"
            );
        }
        assert!(matches!(
            parse("OK codex-login-rotation state=not-needed", Protocol::Login),
            Ok(Outcome::NotNeeded)
        ));
        assert!(matches!(
            parse("OK codex-login-rotation state=waiting", Protocol::Login),
            Ok(Outcome::Waiting(None))
        ));
        let good = login("ready");
        for bad in [
            good.replace(&"c".repeat(64), &"a".repeat(64)),
            format!("{good} wall=login"),
            good.replace("codex-login-rotation", "codex-renewal"),
            good.replace("OK codex-login-rotation ", "OK codex-login-rotationstate"),
        ] {
            assert!(
                matches!(parse(&bad, Protocol::Login), Err(Error::Malformed(_))),
                "{bad}"
            );
        }
    }

    /// A HOST'S REFUSAL IS READ AS A CLOSED WORD, NEVER ITS LINE: `ERR <verb> <words>` is
    /// [`Error::Refused`] with aterm's own words, or `other`; a host that knows no such verb
    /// is `unknown verb`; a busy host is busy; anything else, another verb's refusal included,
    /// is unreadable. No error's text holds what the host said beyond the closed word.
    #[test]
    fn a_hosts_refusal_is_read_as_a_closed_word() {
        let refused = |line: &str, protocol| match parse(line, protocol) {
            Err(Error::Refused(word)) => Some(word),
            _ => None,
        };
        assert_eq!(
            refused("ERR codex-login-rotation denied", Protocol::Login).as_deref(),
            Some("denied")
        );
        assert_eq!(
            refused("ERR codex-rotation native wall changed", Protocol::Quota).as_deref(),
            Some("native wall changed")
        );
        for word in REFUSALS {
            assert_eq!(
                refused(&format!("ERR codex-login-rotation {word}"), Protocol::Login).as_deref(),
                Some(*word)
            );
        }
        for line in [
            "ERR unknown verb (try: help)",
            "ERR unknown verb 'codex-login-rotation' (help lists them)",
        ] {
            assert_eq!(
                refused(line, Protocol::Login).as_deref(),
                Some(UNKNOWN_VERB),
                "{line}"
            );
        }
        let secret = "sk-secret-account-a@example.invalid";
        assert_eq!(
            refused(
                &format!("ERR codex-login-rotation {secret}"),
                Protocol::Login
            )
            .as_deref(),
            Some(OTHER)
        );
        for line in [
            format!("ERR codex-rotation denied {secret}"),
            format!("ERR codex-renewal {secret}"),
            format!("ERR {secret}"),
            format!("OK codex-renewal state=ready {secret}"),
            "ERR codex-login-rotationdenied".to_owned(),
        ] {
            let error = parse(&line, Protocol::Login)
                .err()
                .unwrap_or_else(|| panic!("{line} is no outcome"));
            assert!(!error.to_string().contains(secret), "{line}: {error}");
            assert!(matches!(error, Error::Malformed(_)), "{line}: {error}");
        }
        assert!(matches!(
            parse("ERR busy retry_ms=250", Protocol::Login),
            Err(Error::Busy {
                retry_ms: Some(250),
                ..
            })
        ));
    }

    /// The receipt id and resumed turn [`reply`]'s receipt names.
    const ID: &str = "11111111111111111111111111111111";
    const TURN: &str = "00000000-0000-0000-0000-000000000002";

    /// A COMPLETION IS RECORDED ONLY FOR THE RECEIPT AND TURN IT REPORTED: `state=completed`
    /// with the same eight-field receipt every other reply carries, naming the report's id and
    /// turn, is recorded; a bare `waiting` waits. A completed reply for another receipt or
    /// turn, with a ninth field, without its receipt, under the other verb's prefix, or with a
    /// receipt that fails the shared reader, and every other state, is unreadable.
    #[test]
    fn a_completion_is_recorded_only_for_the_receipt_and_turn_it_reported() {
        assert_eq!(
            parse_completion(&reply("completed"), ID, TURN).ok(),
            Some(Completion::Recorded)
        );
        assert_eq!(
            parse_completion("OK codex-rotation state=waiting", ID, TURN).ok(),
            Some(Completion::Waiting)
        );
        let good = reply("completed");
        let mut bad = vec![
            // Another receipt, or another turn, than the one reported.
            good.replace(ID, &"2".repeat(32)),
            good.replace(TURN, "00000000-0000-0000-0000-000000000003"),
            // A ninth field, a field twice, a word that is no field.
            format!("{good} path=/credentials"),
            format!("{good} state=completed"),
            format!("{good} stray"),
            // No receipt at all, or part of one.
            "OK codex-rotation state=completed".into(),
            good.replace(&format!(" target_user={}", "d".repeat(64)), ""),
            // The shared reader's own refusals: a destination that is the source's account,
            // a raw identity, an id or turn that is not well formed.
            good.replace(&"c".repeat(64), &"a".repeat(64)),
            good.replace(&"b".repeat(64), "raw-user"),
            good.replace(ID, &"1".repeat(31)),
            good.replace(TURN, "invalid"),
            // Another verb's reply.
            good.replace("codex-rotation", "codex-login-rotation"),
            good.replace("codex-rotation", "codex-renewal"),
            good.replace("OK codex-rotation ", "OK codex-rotationstate"),
            // A waiting reply that names a receipt, and the states that are no completion.
            reply("waiting"),
            "OK codex-rotation state=not-needed".into(),
            "OK codex-rotation state=recorded".into(),
            "OK codex-rotation".into(),
            "OK".into(),
            String::new(),
        ];
        bad.extend(
            ["ready", "committed", "consumed", "retired", "unknown"]
                .into_iter()
                .map(reply),
        );
        for line in &bad {
            assert!(
                matches!(parse_completion(line, ID, TURN), Err(Error::Malformed(_))),
                "{line}"
            );
        }
        // The id and turn asked about are the ones compared: the same reply for another
        // report is unreadable.
        assert!(matches!(
            parse_completion(&good, &"2".repeat(32), TURN),
            Err(Error::Malformed(_))
        ));
        assert!(matches!(
            parse_completion(&good, ID, "00000000-0000-0000-0000-000000000001"),
            Err(Error::Malformed(_))
        ));
    }

    /// AN OLD HOST IS UNSUPPORTED, A REFUSAL IS ITS CLOSED WORD: `invalid request` under the
    /// verb (an aterm with `codex-rotation` but no `complete`) and an unknown verb are
    /// [`Completion::Unsupported`]; every other refusal word is [`Error::Refused`] with that
    /// word, or `other`; a busy host is busy; another verb's refusal is unreadable. No error's
    /// text holds what the host said beyond the closed word.
    #[test]
    fn a_completion_refusal_is_a_closed_word_and_an_old_host_is_unsupported() {
        for line in [
            "ERR codex-rotation invalid request",
            "ERR codex-rotation invalid request ",
            "ERR unknown verb",
            "ERR unknown verb (try: help)",
            "ERR unknown verb 'codex-rotation' (help lists them)",
        ] {
            assert_eq!(
                parse_completion(line, ID, TURN).ok(),
                Some(Completion::Unsupported),
                "{line}"
            );
        }
        for word in REFUSALS.iter().filter(|word| **word != "invalid request") {
            assert!(
                matches!(
                    parse_completion(&format!("ERR codex-rotation {word}"), ID, TURN),
                    Err(Error::Refused(refused)) if refused == *word
                ),
                "{word}"
            );
        }
        let secret = "sk-secret-account-a@example.invalid";
        let other = parse_completion(&format!("ERR codex-rotation {secret}"), ID, TURN);
        assert!(
            matches!(&other, Err(Error::Refused(word)) if word == OTHER),
            "{other:?}"
        );
        for line in [
            format!("ERR codex-rotation invalid request {secret}"),
            format!("ERR codex-rotation denied {secret}"),
        ] {
            assert!(
                matches!(parse_completion(&line, ID, TURN), Err(Error::Refused(word)) if word == OTHER),
                "{line}"
            );
        }
        for line in [
            "ERR codex-login-rotation invalid request".to_owned(),
            "ERR codex-renewal invalid request".to_owned(),
            format!("ERR codex-login-rotation {secret}"),
            format!("ERR {secret}"),
            format!("OK codex-renewal state=completed {secret}"),
            "ERR codex-rotationinvalid request".to_owned(),
        ] {
            let error = parse_completion(&line, ID, TURN)
                .err()
                .unwrap_or_else(|| panic!("{line} is no completion"));
            assert!(!error.to_string().contains(secret), "{line}: {error}");
            assert!(matches!(error, Error::Malformed(_)), "{line}: {error}");
        }
        assert!(
            parse_completion(&format!("ERR codex-rotation {secret}"), ID, TURN)
                .err()
                .is_some_and(|error| !error.to_string().contains(secret))
        );
        assert!(matches!(
            parse_completion("ERR busy retry_ms=250", ID, TURN),
            Err(Error::Busy {
                retry_ms: Some(250),
                ..
            })
        ));
    }

    /// EVERY ANSWER THE CONTRACT GIVES A1 (`alab/PARALLEL-LOGINS.md`, gap 5) IS READ AS IT
    /// SAYS: aterm's shared `codex-rotation` lane answers a well-formed report `denied` before
    /// it reads a claim, a bare `waiting` while it cannot read the claimed process, and `wrong
    /// recovery purpose` for another purpose's receipt; A1's own checks answer the rest. Each
    /// refusal is its own closed word (never `other`, and never the `invalid request` that
    /// means a host without the report), each waiting is waiting, and none is recorded.
    #[test]
    fn every_answer_the_contract_gives_a1_is_read_as_it_says() {
        for word in [
            "denied",
            "wrong recovery purpose",
            "unknown receipt",
            "wrong session",
            "receipt changed",
            "wrong process",
            "selection changed",
        ] {
            assert!(REFUSALS.contains(&word), "{word}");
            assert!(
                matches!(
                    parse_completion(&format!("ERR codex-rotation {word}"), ID, TURN),
                    Err(Error::Refused(refused)) if refused == word
                ),
                "{word}"
            );
        }
        assert_eq!(
            parse_completion("OK codex-rotation state=waiting", ID, TURN).ok(),
            Some(Completion::Waiting)
        );
        for line in [
            "ERR busy",
            "ERR busy retry_ms=250",
            "ERR control server busy",
        ] {
            assert!(
                matches!(parse_completion(line, ID, TURN), Err(Error::Busy { .. })),
                "{line}"
            );
        }
    }

    /// THE COMPLETION REPORT AGAINST A HOST: the exact `codex-rotation complete id= turn=` line
    /// is sent; the fake, which knows no `codex-rotation` verb, is unsupported, and so is a host
    /// that answers `invalid request`; `waiting` then `completed` is read in order; a completed
    /// reply for another receipt or turn is unreadable; a refusal is its closed word; and a
    /// host that does not answer is [`Error::Unanswered`].
    #[tokio::test]
    async fn the_completion_report_names_its_receipt_and_turn_and_reads_each_answer() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let host = crate::fake::FakeAterm::start(dir.path(), crate::fake::Mode::Current)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let options = crate::Options {
            timeout: std::time::Duration::from_millis(500),
            ..crate::Options::default()
        };
        let client = Client::connect(host.endpoint(), options)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        client
            .harness_policy()
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        client
            .claim()
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let command = format!(
            "@{} codex-rotation complete id={ID} turn={TURN}",
            host.sid()
        );
        let asked = || {
            host.requests()
                .iter()
                .filter(|request| **request == command)
                .count()
        };
        assert_eq!(
            client.rotation_complete(ID, TURN).await.ok(),
            Some(Completion::Unsupported),
            "the fake knows no codex-rotation verb"
        );
        assert_eq!(asked(), 1, "{:?}", host.requests());
        host.reply_once(&command, "ERR codex-rotation invalid request\n");
        assert_eq!(
            client.rotation_complete(ID, TURN).await.ok(),
            Some(Completion::Unsupported),
            "an aterm with codex-rotation but no complete"
        );
        host.reply_sequence(
            &command,
            vec![
                "OK codex-rotation state=waiting\n".into(),
                format!("{}\n", reply("completed")),
            ],
        );
        assert_eq!(
            client.rotation_complete(ID, TURN).await.ok(),
            Some(Completion::Waiting)
        );
        assert_eq!(
            client.rotation_complete(ID, TURN).await.ok(),
            Some(Completion::Recorded)
        );
        for other in [
            reply("completed").replace(ID, &"2".repeat(32)),
            reply("completed").replace(TURN, "00000000-0000-0000-0000-000000000003"),
            reply("consumed"),
        ] {
            host.reply_once(&command, &format!("{other}\n"));
            assert!(
                matches!(
                    client.rotation_complete(ID, TURN).await,
                    Err(Error::Malformed(_))
                ),
                "{other}"
            );
        }
        host.reply_once(&command, "ERR codex-rotation wrong process\n");
        assert!(matches!(
            client.rotation_complete(ID, TURN).await,
            Err(Error::Refused(word)) if word == "wrong process"
        ));
        assert_eq!(asked(), 8, "every report reached the host once");
        host.stall("codex-rotation");
        assert!(matches!(
            client.rotation_complete(ID, TURN).await,
            Err(Error::Unanswered)
        ));
    }

    /// A REPORT THE CLIENT WILL NOT SEND NEVER REACHES THE HOST: a receipt id that is not 32
    /// lowercase hex digits or a turn that is not a uuid is refused before anything is sent,
    /// under the claim as without it.
    #[tokio::test]
    async fn a_malformed_completion_report_is_never_sent() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let host = crate::fake::FakeAterm::start(dir.path(), crate::fake::Mode::Current)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let client = Client::connect(host.endpoint(), crate::Options::default())
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        client
            .harness_policy()
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        client
            .claim()
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(client.holds_claim());
        let before = host.requests();
        for (id, turn) in [
            ("1".repeat(31), TURN.to_owned()),
            ("1".repeat(33), TURN.to_owned()),
            ("A".repeat(32), TURN.to_owned()),
            (format!("{} turn=x", "1".repeat(32)), TURN.to_owned()),
            (String::new(), TURN.to_owned()),
            (ID.to_owned(), "00000000-0000-0000-0000-00000000000".into()),
            (ID.to_owned(), "00000000-0000-0000-0000-00000000000G".into()),
            (ID.to_owned(), "00000000x0000-0000-0000-000000000002".into()),
            (ID.to_owned(), format!("{TURN} id={}", "2".repeat(32))),
            (ID.to_owned(), String::new()),
        ] {
            assert!(
                matches!(
                    client.rotation_complete(&id, &turn).await,
                    Err(Error::BadRequest(_))
                ),
                "{id} {turn}"
            );
        }
        assert_eq!(host.requests(), before);
    }

    /// The fake's standing reply ([`crate::fake::FakeAterm::reply_always`]): a host with no
    /// destination answers `waiting` at every ask, a one-time reply goes first, and once the
    /// standing reply is cleared the fake answers the verb itself (it knows none).
    #[tokio::test]
    async fn a_host_whose_answer_stands_gives_it_at_every_ask() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let host = crate::fake::FakeAterm::start(dir.path(), crate::fake::Mode::Current)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let client = Client::connect(host.endpoint(), crate::Options::default())
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        client
            .harness_policy()
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        client
            .claim()
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let origin = Origin {
            thread: "00000000-0000-0000-0000-000000000001".into(),
            turn: "00000000-0000-0000-0000-000000000002".into(),
            identity: Identity {
                account: "a".repeat(64),
                user: "b".repeat(64),
            },
        };
        let command = format!(
            "@{} codex-rotation prepare thread={} turn={} source_account={} source_user={}",
            host.sid(),
            origin.thread,
            origin.turn,
            origin.identity.account,
            origin.identity.user
        );
        host.reply_always(&command, "OK codex-rotation state=waiting\n");
        for _ in 0..3 {
            assert!(matches!(
                client.rotation_prepare(&origin).await,
                Ok(Outcome::Waiting(None))
            ));
        }
        host.reply_once(&command, "OK codex-rotation state=not-needed\n");
        assert!(matches!(
            client.rotation_prepare(&origin).await,
            Ok(Outcome::NotNeeded)
        ));
        assert!(matches!(
            client.rotation_prepare(&origin).await,
            Ok(Outcome::Waiting(None))
        ));
        // Another command is not answered by it.
        let other = Origin {
            turn: "00000000-0000-0000-0000-000000000003".into(),
            ..origin.clone()
        };
        assert!(client.rotation_prepare(&other).await.is_err());
        assert!(host.reply_always_clear(&command));
        assert!(!host.reply_always_clear(&command));
        assert!(
            client.rotation_prepare(&origin).await.is_err(),
            "the fake knows no codex-rotation verb of its own"
        );
        let asked = host
            .requests()
            .iter()
            .filter(|request| request.contains("codex-rotation prepare"))
            .count();
        assert_eq!(asked, 7, "every ask reached the host and was recorded");
    }

    /// THE LOGIN PROTOCOL AGAINST A HOST: the exact `codex-login-rotation prepare` line is sent;
    /// a host that knows no such verb (the fake, as aterm 0.199.0) is `unknown verb`, a refusal
    /// is its closed word, a receipt under the verb is read, and a host that does not answer is
    /// [`Error::Unanswered`], not an unreadable reply. The status, commit and consume lines name
    /// the receipt under the same verb.
    #[tokio::test]
    async fn the_login_protocol_names_its_verb_and_keeps_transport_failures() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let host = crate::fake::FakeAterm::start(dir.path(), crate::fake::Mode::Current)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let options = crate::Options {
            timeout: std::time::Duration::from_millis(500),
            ..crate::Options::default()
        };
        let client = Client::connect(host.endpoint(), options)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        client
            .harness_policy()
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        client
            .claim()
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let origin = Origin {
            thread: "00000000-0000-0000-0000-000000000001".into(),
            turn: "00000000-0000-0000-0000-000000000002".into(),
            identity: Identity {
                account: "a".repeat(64),
                user: "b".repeat(64),
            },
        };
        let command = format!(
            "@{} codex-login-rotation prepare thread={} turn={} source_account={} source_user={}",
            host.sid(),
            origin.thread,
            origin.turn,
            origin.identity.account,
            origin.identity.user
        );
        assert!(matches!(
            client.login_rotation_prepare(&origin).await,
            Err(Error::Refused(word)) if word == UNKNOWN_VERB
        ));
        assert!(host.requests().contains(&command), "{:?}", host.requests());
        host.reply_once(&command, "ERR codex-login-rotation denied\n");
        assert!(matches!(
            client.login_rotation_prepare(&origin).await,
            Err(Error::Refused(word)) if word == "denied"
        ));
        let id = "1".repeat(32);
        let receipt = format!(
            "id={id} thread={} turn={} source_account={} source_user={} target_account={} target_user={}",
            origin.thread,
            origin.turn,
            origin.identity.account,
            origin.identity.user,
            "c".repeat(64),
            "d".repeat(64)
        );
        host.reply_once(
            &command,
            &format!("OK codex-login-rotation state=ready {receipt}\n"),
        );
        assert!(matches!(
            client.login_rotation_prepare(&origin).await,
            Ok(Outcome::Ready(ready)) if ready.destination.account == "c".repeat(64)
        ));
        for (verb, call) in ["status", "commit", "consume"].into_iter().enumerate() {
            let line = match verb {
                2 => format!(
                    "@{} codex-login-rotation consume id={id} turn={}",
                    host.sid(),
                    origin.turn
                ),
                _ => format!("@{} codex-login-rotation {call} id={id}", host.sid()),
            };
            host.reply_once(
                &line,
                &format!("OK codex-login-rotation state=committed {receipt}\n"),
            );
            let outcome = match verb {
                0 => client.login_rotation_status(&id).await,
                1 => client.login_rotation_commit(&id).await,
                _ => client.login_rotation_consume(&id, &origin.turn).await,
            };
            assert!(matches!(outcome, Ok(Outcome::Committed(_))), "{call}");
            assert!(host.requests().contains(&line), "{call}");
        }
        // A quota rotation's reply to the login verb is refused.
        host.reply_once(
            &command,
            &format!("OK codex-rotation state=ready {receipt}\n"),
        );
        assert!(matches!(
            client.login_rotation_prepare(&origin).await,
            Err(Error::Malformed(_))
        ));
        host.stall("codex-login-rotation");
        assert!(matches!(
            client.login_rotation_prepare(&origin).await,
            Err(Error::Unanswered)
        ));
    }

    #[tokio::test]
    async fn rotation_calls_without_native_custody_never_reach_the_host() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let host = crate::fake::FakeAterm::start(dir.path(), crate::fake::Mode::Current)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let client = Client::connect(host.endpoint(), crate::Options::default())
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let before = host.requests();
        let origin = Origin {
            thread: "00000000-0000-0000-0000-000000000001".into(),
            turn: "00000000-0000-0000-0000-000000000002".into(),
            identity: Identity {
                account: "a".repeat(64),
                user: "b".repeat(64),
            },
        };
        assert!(matches!(
            client.rotation_prepare(&origin).await,
            Err(Error::BadRequest(_))
        ));
        let id = "1".repeat(32);
        assert!(matches!(
            client.rotation_status(&id).await,
            Err(Error::BadRequest(_))
        ));
        assert!(matches!(
            client.rotation_commit(&id).await,
            Err(Error::BadRequest(_))
        ));
        assert!(matches!(
            client.rotation_consume(&id, &origin.turn).await,
            Err(Error::BadRequest(_))
        ));
        assert!(matches!(
            client.login_rotation_prepare(&origin).await,
            Err(Error::BadRequest(_))
        ));
        assert!(matches!(
            client.login_rotation_status(&id).await,
            Err(Error::BadRequest(_))
        ));
        assert!(matches!(
            client.rotation_complete(&id, &origin.turn).await,
            Err(Error::BadRequest(_))
        ));
        assert_eq!(host.requests(), before);
    }
}
