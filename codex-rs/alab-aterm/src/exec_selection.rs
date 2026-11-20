// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Exec preparation through the shared account adapter. A selection has no
//! receipt or capability and cannot authorize suspension or process replacement.

use crate::Client;
use crate::Error;
use crate::exec_route::Miss;
use crate::exec_route::NoRoute;
use crate::exec_route::plain_word;
use crate::renewal::Identity;
use crate::rotation::OTHER;
use crate::rotation::Origin;
use crate::rotation::REFUSALS;
use crate::rotation::UNKNOWN_VERB;
use codex_alab_core::exec_wall::Verdict;
use codex_alab_core::wall_kind::WallKind;
use std::collections::BTreeMap;

#[derive(Clone, PartialEq, Eq)]
pub enum Selection {
    Waiting,
    NotNeeded,
    Selected {
        origin: Origin,
        destination: Identity,
    },
    /// aterm's definite refusal of this run, `state=refused why=<word>`, by its word: a plain
    /// one, or empty for a word that is not ([`plain_word`]), which is a word this reader does
    /// not know. No aterm sends one yet.
    Refused(String),
}

impl Selection {
    /// Why the run does not move on this answer, as its line tells it: `not-needed`, or a
    /// refusal by its word ([`NoRoute::refused`]). `None`: a move may still come of it
    /// (`waiting`, `selected`), or it is no answer yet (a refusal that names a suspension).
    pub fn route(&self) -> Option<NoRoute> {
        match self {
            Selection::Waiting | Selection::Selected { .. } => None,
            Selection::NotNeeded => Some(NoRoute::NotNeeded),
            Selection::Refused(why) => match NoRoute::refused(why) {
                Miss::Route(route) => Some(route),
                Miss::Transient => None,
            },
        }
    }
}

fn malformed() -> Error {
    Error::Malformed("invalid exec account selection".into())
}

/// A reply that is a refusal, as its closed word: busy ([`Error::Busy`]), a host that knows no
/// such verb ([`UNKNOWN_VERB`]), aterm's native-operation denial (`denied`), the exec handoff's
/// `unsupported request` (`invalid request`, as the rotation verbs say it), or another
/// `ERR codex-exec-handoff …` refusal in [`REFUSALS`]' words, else [`OTHER`]. The line itself is
/// never kept: it may carry an identity. `None`: not a refusal.
fn refusal(line: &str) -> Option<Error> {
    let line = line.trim_end();
    if !(line == "ERR" || line.starts_with("ERR ")) {
        return None;
    }
    if line.starts_with("ERR busy") || line.starts_with("ERR control server busy") {
        return Some(Error::Busy {
            line: "ERR busy".into(),
            retry_ms: crate::wire::retry_ms(line),
        });
    }
    if line == "ERR unknown verb" || line.starts_with("ERR unknown verb ") {
        return Some(Error::Refused(UNKNOWN_VERB.into()));
    }
    if crate::client::denied(line) {
        return Some(Error::Refused("denied".into()));
    }
    let word = match line.strip_prefix("ERR codex-exec-handoff ") {
        Some("unsupported request") => "invalid request",
        Some(words) => REFUSALS
            .iter()
            .find(|known| **known == words)
            .copied()
            .unwrap_or(OTHER),
        None => OTHER,
    };
    Some(Error::Refused(word.into()))
}

fn parse(line: &str, expected: &Origin) -> Result<Selection, Error> {
    if let Some(refused) = refusal(line) {
        return Err(refused);
    }
    let rest = line
        .strip_prefix("OK codex-exec-handoff ")
        .ok_or_else(malformed)?;
    let mut fields = BTreeMap::new();
    for word in rest.split_ascii_whitespace() {
        let (key, value) = word.split_once('=').ok_or_else(malformed)?;
        if fields.insert(key, value).is_some() {
            return Err(malformed());
        }
    }
    match fields.get("state").copied() {
        Some("waiting") if fields.len() == 1 => return Ok(Selection::Waiting),
        Some("not-needed") if fields.len() == 1 => return Ok(Selection::NotNeeded),
        // Exactly `why=<word>` beside the state: anything more is no refusal this reader takes.
        // A word that is not plain is one it does not know, kept empty, so its line never
        // repeats it and the run never asks again on the 5 s clock for it.
        Some("refused") if fields.len() == 2 => {
            return fields
                .get("why")
                .map(|why| Selection::Refused(plain_word(why)))
                .ok_or_else(malformed);
        }
        Some("selected") if fields.len() == 7 => {}
        _ => return Err(malformed()),
    }
    let field = |key| {
        fields
            .get(key)
            .map(|s| (*s).to_owned())
            .ok_or_else(malformed)
    };
    let origin = Origin {
        thread: field("thread")?,
        turn: field("turn")?,
        identity: Identity {
            account: field("source_account")?,
            user: field("source_user")?,
        },
    };
    let destination = Identity {
        account: field("target_account")?,
        user: field("target_user")?,
    };
    let target = Origin {
        identity: destination.clone(),
        ..origin.clone()
    };
    if origin != *expected
        || !origin.valid()
        || !target.valid()
        || destination.account == origin.identity.account
    {
        return Err(malformed());
    }
    Ok(Selection::Selected {
        origin,
        destination,
    })
}

pub async fn prepare(client: &Client, origin: &Origin) -> Result<Selection, Error> {
    prepare_for(client, origin, None).await
}

async fn prepare_for(
    client: &Client,
    origin: &Origin,
    limit: Option<&str>,
) -> Result<Selection, Error> {
    if !origin.valid() || !client.holds_claim() {
        return Err(Error::BadRequest(
            "exec account selection requires a valid native origin and self-claim".into(),
        ));
    }
    let mut request = format!(
        "codex-exec-handoff prepare thread={} turn={} source_account={} source_user={}",
        origin.thread, origin.turn, origin.identity.account, origin.identity.user,
    );
    crate::account_recovery::append_exec_limit(client, &mut request, limit)?;
    let reply = client.on_self(&request).await?;
    parse(&reply.status, origin)
}

/// What an exec run's ask about one wall came to (gap 4 of `alab/PARALLEL-LOGINS.md`, step S3).
pub enum Asked {
    /// No prepare was sent. The bucket cannot be represented or the host lacks its contract.
    Unasked,
    /// A reused claim saw rotation disabled. Fresh admission owns the passive retry clock.
    Readmit,
    /// Nothing was sent: the host's policy leaves no wall of refused ordinary use
    /// ([`crate::account_recovery::leaves_refusals`]), so it would answer `not-needed`.
    NotLeft,
    /// The host's answer to `prepare`.
    Answered(Result<Selection, Error>),
}

/// Ask for this refusal's exact bucket only when the host advertises the matching contract.
pub async fn ask(client: &Client, origin: &Origin, kind: &WallKind) -> Asked {
    if !kind.asked() {
        return Asked::Unasked;
    }
    if kind.limit_id().is_some() {
        match client.harness_policy().await {
            Ok(crate::policy::PolicyAnswer::Supported(policy))
                if crate::account_recovery::leaves_exec_wall(&policy, kind) => {}
            Ok(crate::policy::PolicyAnswer::Supported(policy))
                if !crate::account_recovery::policy_allows(
                    &policy,
                    codex_alab_core::account_handoff::Purpose::WeeklyQuota,
                ) =>
            {
                return Asked::Readmit;
            }
            Ok(crate::policy::PolicyAnswer::Unsupported(line))
                if !crate::exec_route::not_served(&line) =>
            {
                return Asked::Answered(Err(Error::PolicyInvalidated));
            }
            Ok(_) => return Asked::Unasked,
            Err(error) => return Asked::Answered(Err(error)),
        }
    }
    if kind.host_kept()
        && !client
            .policy()
            .is_some_and(|policy| crate::account_recovery::leaves_exec_wall(&policy, kind))
    {
        return Asked::NotLeft;
    }
    Asked::Answered(prepare_for(client, origin, kind.limit_id()).await)
}

impl Asked {
    /// The verdict this ask gives a host-kept wall ([`codex_alab_core::exec_wall`]): a move may
    /// come at `waiting` and `selected`; none will at `not-needed`, at aterm's `refused` for any
    /// word that names a route (an off one included: the run keeps such a wall only while a move
    /// may come), at a policy that leaves no such wall, or at a definite refusal ([`declined`]).
    /// `None`: no answer yet (a refusal that names a suspension among them), or nothing asked.
    pub fn verdict(&self) -> Option<Verdict> {
        match self {
            Asked::Unasked | Asked::Readmit => None,
            Asked::NotLeft => Some(Verdict::Declined),
            Asked::Answered(Ok(Selection::Waiting | Selection::Selected { .. })) => {
                Some(Verdict::Keep)
            }
            Asked::Answered(Ok(selection)) => selection.route().map(|_| Verdict::Declined),
            Asked::Answered(Err(error)) => declined(error).then_some(Verdict::Declined),
        }
    }

    /// What the runtime does with this ask at a wall it keeps only while a host may move it
    /// (`kept`, [`WallKind::host_kept`]) or at an ordinary one. Each answer's handling is decided
    /// here, and the runtime's loop (`alab-core/glue/exec_handoff_runtime.rs`) does each step
    /// as it is named: at a host-kept wall the [`verdict`](Asked::verdict) decides first, so only
    /// an ordinary wall tells a route.
    pub fn step(self, kept: bool) -> Step {
        let verdict = if kept { self.verdict() } else { None };
        if verdict == Some(Verdict::Declined) {
            return Step::Decline;
        }
        let keep = verdict == Some(Verdict::Keep);
        match self {
            Asked::Readmit => Step::Readmit,
            Asked::Unasked | Asked::NotLeft => Step::Decline,
            Asked::Answered(Ok(Selection::Selected { destination, .. })) => {
                Step::Move { destination, keep }
            }
            Asked::Answered(Ok(selection)) => {
                selection.route().map_or(Step::Again { keep }, Step::Tell)
            }
            Asked::Answered(Err(_)) => Step::Again { keep },
        }
    }
}

/// What the runtime of an exec run does with one ask at its wall ([`Asked::step`]).
#[derive(Clone, PartialEq, Eq)]
pub enum Step {
    /// Drop the reused claim and check normal admission again for the same live wall.
    Readmit,
    /// No move will come of this wall before it changes, and nothing is told: a host-kept wall
    /// was answered a definite no, or nothing was asked. The runtime declines a host-kept wall
    /// ([`Verdict::Declined`]), so the run ends at its next refusal there, and asks nothing more
    /// until the wall changes.
    Decline,
    /// No move comes of this answer at an ordinary wall: tell the route once for the wall
    /// ([`crate::exec_route::Latch`]), then ask again on its clock, or never
    /// ([`NoRoute::again`]).
    Tell(NoRoute),
    /// The host selected `destination`: arm the move. `keep`: as at [`Step::Again`].
    Move { destination: Identity, keep: bool },
    /// No answer yet, or a move may still come of it (`waiting`): ask again on the 5 s clock,
    /// telling nothing. `keep`: the host-kept wall waits on for this answer, so the runtime
    /// records [`Verdict::Keep`] first.
    Again { keep: bool },
}

impl Step {
    /// Whether the runtime records [`Verdict::Keep`] for its host-kept wall before this step.
    pub fn keeps(&self) -> bool {
        matches!(
            self,
            Step::Move { keep: true, .. } | Step::Again { keep: true }
        )
    }
}

/// A destination names an account, so it is never printed.
impl std::fmt::Debug for Step {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Step::Readmit => f.write_str("Readmit"),
            Step::Decline => f.write_str("Decline"),
            Step::Tell(route) => f.debug_tuple("Tell").field(route).finish(),
            Step::Move { keep, .. } => f
                .debug_struct("Move")
                .field("keep", keep)
                .finish_non_exhaustive(),
            Step::Again { keep } => f.debug_struct("Again").field("keep", keep).finish(),
        }
    }
}

/// Whether `error`, a host's answer to `prepare`, is a definite no that another ask cannot
/// change: a host that knows no such verb, aterm's denial (its policy moves no exec run, or the
/// asker is not the tab's proven claimant), a request it calls unsupported, or an answer this
/// reader cannot take. Busy, unanswered, closed and every other refusal are no answer yet.
pub fn declined(error: &Error) -> bool {
    match error {
        Error::Refused(word) => {
            matches!(word.as_str(), UNKNOWN_VERB | "denied" | "invalid request")
        }
        Error::Malformed(_) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origin() -> Origin {
        Origin {
            thread: "00000000-0000-0000-0000-000000000001".into(),
            turn: "00000000-0000-0000-0000-000000000002".into(),
            identity: Identity {
                account: "a".repeat(64),
                user: "b".repeat(64),
            },
        }
    }

    /// The host's `selected` answer for `origin`, moving it to another account.
    fn selected(origin: &Origin) -> String {
        format!(
            "OK codex-exec-handoff state=selected thread={} turn={} source_account={} source_user={} target_account={} target_user={}\n",
            origin.thread,
            origin.turn,
            origin.identity.account,
            origin.identity.user,
            "c".repeat(64),
            "d".repeat(64)
        )
    }

    #[tokio::test]
    async fn exec_account_selection_preserves_origin_and_is_not_a_rotation_receipt() {
        let directory = tempfile::tempdir().unwrap();
        let host = crate::fake::FakeAterm::start(directory.path(), crate::fake::Mode::Current)
            .await
            .unwrap();
        let client = Client::connect(host.endpoint(), crate::Options::default())
            .await
            .unwrap();
        let origin = origin();
        assert!(prepare(&client, &origin).await.is_err());
        client.harness_policy().await.unwrap();
        client.claim().await.unwrap();
        let request = format!(
            "@{} codex-exec-handoff prepare thread={} turn={} source_account={} source_user={}",
            host.sid(),
            origin.thread,
            origin.turn,
            origin.identity.account,
            origin.identity.user
        );
        let selected = selected(&origin);
        host.reply_once(&request, &selected);
        assert!(matches!(
            prepare(&client, &origin).await.unwrap(),
            Selection::Selected { .. }
        ));
        for altered in [
            selected.replace("codex-exec-handoff", "codex-rotation"),
            selected.replace("state=selected", "state=ready"),
            selected.replace(&origin.turn, "00000000-0000-0000-0000-000000000003"),
            selected.replace(&"c".repeat(64), &origin.identity.account),
            format!("{} cap={}\n", selected.trim(), "e".repeat(64)),
        ] {
            host.reply_once(&request, &altered);
            assert!(prepare(&client, &origin).await.is_err());
        }
    }

    /// A REFUSAL OF `prepare` IS READ AS ITS CLOSED WORD, never as the line: aterm's native
    /// denial, the handoff's unsupported request and an unknown verb are definite; busy and any
    /// other word are no answer yet; an `OK` this reader cannot take is unreadable, and definite.
    #[test]
    fn a_refused_selection_is_read_as_its_word() {
        let origin = origin();
        // A selection names identities, so it has no `Debug` for `expect_err`.
        let refused = |line: &str| {
            parse(line, &origin)
                .err()
                .unwrap_or_else(|| panic!("{line} was taken"))
        };
        let secret = "a".repeat(64);
        for (line, word) in [
            ("ERR denied native operation authority", Some("denied")),
            ("ERR denied", Some("denied")),
            (
                "ERR codex-exec-handoff unsupported request",
                Some("invalid request"),
            ),
            ("ERR unknown verb codex-exec-handoff", Some(UNKNOWN_VERB)),
            ("ERR unknown verb", Some(UNKNOWN_VERB)),
            ("ERR codex-exec-handoff capacity", Some("capacity")),
            ("ERR codex-exec-handoff something new", Some(OTHER)),
            (&format!("ERR deniedx {secret}"), Some(OTHER)),
            ("ERR", Some(OTHER)),
        ] {
            let error = refused(&format!("{line}\n"));
            match (&error, word) {
                (Error::Refused(read), Some(word)) => assert_eq!(read, word, "{line}"),
                _ => panic!("{line}: {error}"),
            }
            assert!(!error.to_string().contains(&secret), "{line}");
            assert_eq!(
                declined(&error),
                matches!(
                    word,
                    Some("denied" | "invalid request") | Some(UNKNOWN_VERB)
                ),
                "{line}"
            );
        }
        for line in [
            "ERR busy",
            "ERR busy retry_ms=250",
            "ERR control server busy",
        ] {
            let error = refused(line);
            assert!(matches!(error, Error::Busy { .. }), "{line}");
            assert!(!declined(&error), "{line}");
        }
        for line in ["OK codex-exec-handoff state=ready", "OK other", "ERRx"] {
            let error = refused(line);
            assert!(matches!(error, Error::Malformed(_)), "{line}");
            assert!(declined(&error), "{line}");
        }
        for error in [
            Error::Unanswered,
            Error::Closed,
            Error::BadRequest("no claim".into()),
            Error::Io(std::io::Error::other("gone")),
        ] {
            assert!(!declined(&error), "{error}");
        }
    }

    /// A REFUSAL IS EXACTLY `state=refused why=<word>`: a plain word is kept, and a reply with any
    /// other field, no `why=` or two of one key is malformed, so a definite no that another ask
    /// cannot change, and its line is never kept. A word that is not plain is a refusal in a word
    /// this reader does not know, kept empty: never malformed, and never repeated.
    #[test]
    fn a_refused_reply_with_other_fields_is_malformed() {
        let origin = origin();
        let read = |line: &str| parse(&format!("OK codex-exec-handoff {line}\n"), &origin);
        for word in [
            "not-in-tab",
            "hold",
            "rotation-off",
            "unsupported-run",
            "a-new-word",
        ] {
            // The order of the two fields is the host's.
            for line in [
                format!("state=refused why={word}"),
                format!("why={word} state=refused"),
            ] {
                assert!(
                    matches!(read(&line), Ok(Selection::Refused(kept)) if kept == word),
                    "{line}"
                );
            }
        }
        let secret = "a".repeat(64);
        for line in [
            "state=refused".to_owned(),
            "state=refused reason=not-in-tab".to_owned(),
            "state=refused why=not-in-tab why=hold".to_owned(),
            "state=refused state=refused why=hold".to_owned(),
            "state=refused why=not-in-tab detail=x".to_owned(),
            format!("state=refused why=not-exec source_account={secret}"),
            format!("state=refused why=not-exec cap={}", "e".repeat(64)),
            format!(
                "state=refused why=not-in-tab thread={} turn={}",
                origin.thread, origin.turn
            ),
        ] {
            let error = read(&line)
                .err()
                .unwrap_or_else(|| panic!("{line} was taken"));
            assert!(matches!(error, Error::Malformed(_)), "{line}: {error}");
            assert!(declined(&error), "{line}");
            assert!(!error.to_string().contains(&secret), "{line}");
        }
        for line in [
            "state=refused why=".to_owned(),
            "state=refused why=Not-In-Tab".to_owned(),
            "state=refused why=x@y.z".to_owned(),
            "state=refused why=a/b".to_owned(),
            "state=refused why=not_in_tab".to_owned(),
            "state=refused why=rotation_off".to_owned(),
            format!("state=refused why={}", "w".repeat(33)),
            format!("state=refused why={secret}x"),
            "state=refused why=not-in-tab=1".to_owned(),
        ] {
            assert!(
                matches!(read(&line), Ok(Selection::Refused(kept)) if kept.is_empty()),
                "{line}"
            );
        }
        // Another verb's refusal is not this one.
        assert!(matches!(
            parse("OK codex-rotation state=refused why=hold\n", &origin),
            Err(Error::Malformed(_))
        ));
    }

    /// A REFUSED PREPARE DECLINES A HOST-KEPT WALL, whatever its word: a structural one, an off
    /// word or a word this reader does not know, plain or not. The run then ends at its next
    /// refusal with the provider's own message, as at `not-needed`, where `waiting` before it kept
    /// the run. A refusal that names a suspension is no answer yet: like busy, it leaves the
    /// earlier `waiting` standing.
    #[tokio::test]
    async fn a_refused_prepare_declines_a_host_kept_wall() {
        use codex_alab_core::exec_wall::ExecWalls;
        use codex_alab_core::exec_wall::UNKNOWN_RESET_WAIT;
        use codex_alab_core::wall_kind::Workspace;
        use std::sync::Arc;
        use std::time::Instant;
        let directory = tempfile::tempdir().unwrap();
        let host = crate::fake::FakeAterm::start(directory.path(), crate::fake::Mode::Current)
            .await
            .unwrap();
        host.set_policy_key("codex_rotation", "1");
        host.set_policy_key("accounts", "wall");
        host.set_policy_key(crate::account_recovery::ROTATION_REFUSAL, "1");
        let client = Client::connect(host.endpoint(), crate::Options::default())
            .await
            .unwrap();
        client.harness_policy().await.unwrap();
        client.claim().await.unwrap();
        let origin = origin();
        let request = format!(
            "@{} codex-exec-handoff prepare thread={} turn={} source_account={} source_user={}",
            host.sid(),
            origin.thread,
            origin.turn,
            origin.identity.account,
            origin.identity.user
        );
        let credits = WallKind::Workspace(Workspace::MemberCreditsDepleted);
        let start = Instant::now();
        // The word sent, and the word kept: empty for one that is not plain.
        for (word, kept_word, declines) in [
            ("not-in-tab", "not-in-tab", true),
            ("nested-aterm", "nested-aterm", true),
            ("not-alab-image", "not-alab-image", true),
            ("not-exec", "not-exec", true),
            ("unsupported-run", "unsupported-run", true),
            ("no-pool-row", "no-pool-row", true),
            ("login-names-another", "login-names-another", true),
            ("hold", "hold", true),
            ("autopilot-off", "autopilot-off", true),
            ("agent-off", "agent-off", true),
            ("rotation-off", "rotation-off", true),
            ("a-new-word", "a-new-word", true),
            ("rotation_off", "", true),
            ("suspended", "suspended", false),
        ] {
            let run = Arc::new(ExecWalls::default());
            run.root_turn(&origin.turn);
            assert_eq!(run.keep(&credits, true, start), Some(UNKNOWN_RESET_WAIT));
            run.waited(&credits, start);
            host.reply_once(&request, "OK codex-exec-handoff state=waiting\n");
            let asked = ask(&client, &origin, &credits).await;
            run.decide(&origin.turn, &credits, asked.verdict().expect("waiting"));
            host.reply_once(
                &request,
                &format!("OK codex-exec-handoff state=refused why={word}\n"),
            );
            let asked = ask(&client, &origin, &credits).await;
            assert!(
                matches!(&asked, Asked::Answered(Ok(Selection::Refused(kept))) if kept == kept_word),
                "{word}"
            );
            let verdict = asked.verdict();
            assert_eq!(verdict, declines.then_some(Verdict::Declined), "{word}");
            if let Some(verdict) = verdict {
                run.decide(&origin.turn, &credits, verdict);
            }
            assert_eq!(
                run.keep(&credits, true, start + UNKNOWN_RESET_WAIT),
                (!declines).then_some(UNKNOWN_RESET_WAIT),
                "{word}"
            );
        }
    }

    /// EACH ANSWER IS ONE STEP OF THE RUNTIME, read off the wire as the runtime asks: at an
    /// ordinary wall, `not-needed` and every refusal that names a route are told on that route's
    /// clock (an unknown word, plain or not, a passive Off), `waiting`, a suspension and every
    /// `ERR` are asked again on the 5 s clock, and `selected` moves. At a host-kept wall nothing
    /// is told: `waiting` and `selected` keep the wall, and `not-needed`, a refusal that names a
    /// route and a definite `ERR` decline it. Nothing asked declines it too.
    #[tokio::test]
    async fn each_answer_is_one_step_of_the_runtime() {
        use codex_alab_core::wall_kind::Workspace;
        let directory = tempfile::tempdir().unwrap();
        let host = crate::fake::FakeAterm::start(directory.path(), crate::fake::Mode::Current)
            .await
            .unwrap();
        host.set_policy_key("codex_rotation", "1");
        host.set_policy_key("accounts", "wall");
        host.set_policy_key(crate::account_recovery::ROTATION_REFUSAL, "1");
        let client = Client::connect(host.endpoint(), crate::Options::default())
            .await
            .unwrap();
        client.harness_policy().await.unwrap();
        client.claim().await.unwrap();
        let origin = origin();
        let request = format!(
            "@{} codex-exec-handoff prepare thread={} turn={} source_account={} source_user={}",
            host.sid(),
            origin.thread,
            origin.turn,
            origin.identity.account,
            origin.identity.user
        );
        let credits = WallKind::Workspace(Workspace::MemberCreditsDepleted);
        let destination = Identity {
            account: "c".repeat(64),
            user: "d".repeat(64),
        };
        let moves = |keep| Step::Move {
            destination: destination.clone(),
            keep,
        };
        let tell = Step::Tell;
        let off = |word: &str| tell(NoRoute::Off(crate::exec_route::Off::Other(word.into())));
        let again = Step::Again { keep: false };
        let refused = |word: &str| format!("OK codex-exec-handoff state=refused why={word}\n");
        for (reply, ordinary, kept) in [
            (
                "OK codex-exec-handoff state=waiting\n".to_owned(),
                again.clone(),
                Step::Again { keep: true },
            ),
            (selected(&origin), moves(false), moves(true)),
            (
                "OK codex-exec-handoff state=not-needed\n".to_owned(),
                tell(NoRoute::NotNeeded),
                Step::Decline,
            ),
            (
                refused("not-in-tab"),
                tell(NoRoute::NotInTab),
                Step::Decline,
            ),
            (
                refused("unsupported-run"),
                tell(NoRoute::UnsupportedRun),
                Step::Decline,
            ),
            (
                refused("hold"),
                tell(NoRoute::Off(crate::exec_route::Off::Hold)),
                Step::Decline,
            ),
            (refused("agent-off"), off("agent-off"), Step::Decline),
            (
                refused("rotation-off"),
                tell(NoRoute::RotationOff),
                Step::Decline,
            ),
            (refused("a-new-word"), off("a-new-word"), Step::Decline),
            (refused("rotation_off"), off(""), Step::Decline),
            (refused("Not-In-Tab"), off(""), Step::Decline),
            (refused("suspended"), again.clone(), again.clone()),
            (
                "ERR busy retry_ms=100\n".to_owned(),
                again.clone(),
                again.clone(),
            ),
            (
                "ERR codex-exec-handoff capacity\n".to_owned(),
                again.clone(),
                again.clone(),
            ),
            (
                "ERR denied native operation authority\n".to_owned(),
                again.clone(),
                Step::Decline,
            ),
            (
                "OK codex-exec-handoff state=ready\n".to_owned(),
                again.clone(),
                Step::Decline,
            ),
        ] {
            host.reply_once(&request, &reply);
            let step = ask(&client, &origin, &WallKind::Ordinary).await.step(false);
            assert_eq!(step, ordinary, "{reply}");
            assert!(!step.keeps(), "{reply}");
            host.reply_once(&request, &reply);
            let step = ask(&client, &origin, &credits).await.step(true);
            assert_eq!(step, kept, "{reply}");
            assert_eq!(
                step.keeps(),
                reply.contains("state=waiting") || reply.contains("state=selected"),
                "{reply}"
            );
            assert!(
                !matches!(step, Step::Tell(_)),
                "a host-kept wall tells nothing"
            );
        }
        // Neither the model nor the workspace prepare is supported by its corresponding policy below.
        let model = WallKind::Limit("codex_other".into());
        host.reply_once(
            &format!("@{} harness policy", host.sid()),
            "ERR busy retry_ms=100\n",
        );
        assert_eq!(
            ask(&client, &origin, &model).await.step(false),
            Step::Again { keep: false }
        );
        assert_eq!(
            ask(&client, &origin, &model).await.step(false),
            Step::Decline
        );
        host.set_policy_key(crate::account_recovery::ROTATION_REFUSAL, "0");
        client.harness_policy().await.unwrap();
        let before = host.requests().len();
        assert_eq!(
            ask(&client, &origin, &credits).await.step(true),
            Step::Decline
        );
        assert_eq!(host.requests().len(), before, "nothing is sent");
        // A destination is never printed.
        assert!(!format!("{:?}", moves(true)).contains(&destination.account));
    }

    /// THE RUNTIME'S ASK AT EACH KIND OF WALL, against the fake host, and what each answer does to
    /// the run (alab-core's `ExecWalls`, which Core's quota retry reads): a workspace's wall at
    /// today's aterm (no `codex_rotation_refusal`) is asked nothing and declined, so the run
    /// ends at its next refusal; with the word, `waiting` keeps the run refusal after refusal and
    /// `not-needed` ends it; a definite refusal ends it and busy changes nothing; a model's own
    /// limit sends no `prepare`; an ordinary wall is asked as before, word or none.
    #[tokio::test]
    async fn each_kind_of_wall_is_asked_only_where_its_answer_can_move_the_run() {
        use codex_alab_core::exec_wall::ExecWalls;
        use codex_alab_core::exec_wall::UNKNOWN_RESET_WAIT;
        use codex_alab_core::wall_kind::Workspace;
        use std::sync::Arc;
        use std::time::Instant;
        let directory = tempfile::tempdir().unwrap();
        let host = crate::fake::FakeAterm::start(directory.path(), crate::fake::Mode::Current)
            .await
            .unwrap();
        host.set_policy_key("codex_rotation", "1");
        host.set_policy_key("accounts", "wall");
        let client = Client::connect(host.endpoint(), crate::Options::default())
            .await
            .unwrap();
        client.harness_policy().await.unwrap();
        client.claim().await.unwrap();
        let origin = origin();
        let request = format!(
            "@{} codex-exec-handoff prepare thread={} turn={} source_account={} source_user={}",
            host.sid(),
            origin.thread,
            origin.turn,
            origin.identity.account,
            origin.identity.user
        );
        let credits = WallKind::Workspace(Workspace::MemberCreditsDepleted);
        let start = Instant::now();
        // A run's walls at its first refusal: one wait is kept, so that its runtime can ask.
        let walls = || {
            let walls = Arc::new(ExecWalls::default());
            walls.root_turn(&origin.turn);
            assert_eq!(walls.keep(&credits, true, start), Some(UNKNOWN_RESET_WAIT));
            walls.waited(&credits, start);
            walls
        };
        let prepares = || {
            host.requests()
                .iter()
                .filter(|line| line.contains(" codex-exec-handoff prepare "))
                .count()
        };

        // Today's aterm: no word, nothing sent, declined, and the next refusal ends the run.
        let run = walls();
        let asked = ask(&client, &origin, &credits).await;
        assert!(matches!(asked, Asked::NotLeft));
        assert_eq!(
            prepares(),
            0,
            "nothing is asked of a host that leaves no such wall"
        );
        run.decide(&origin.turn, &credits, asked.verdict().expect("a verdict"));
        assert_eq!(run.keep(&credits, true, start + UNKNOWN_RESET_WAIT), None);

        // A host that leaves such a wall: `waiting` keeps the run at each refusal.
        host.set_policy_key(crate::account_recovery::ROTATION_REFUSAL, "1");
        client.harness_policy().await.unwrap();
        let run = walls();
        for retry in 1..=4u32 {
            host.reply_once(&request, "OK codex-exec-handoff state=waiting\n");
            let asked = ask(&client, &origin, &credits).await;
            assert!(matches!(asked, Asked::Answered(Ok(Selection::Waiting))));
            run.decide(&origin.turn, &credits, asked.verdict().expect("a verdict"));
            let now = start + UNKNOWN_RESET_WAIT * retry;
            assert_eq!(
                run.keep(&credits, true, now),
                Some(UNKNOWN_RESET_WAIT),
                "{retry}"
            );
            run.waited(&credits, now);
        }
        assert_eq!(prepares(), 4);
        // `not-needed` ends it at the next refusal.
        host.reply_once(&request, "OK codex-exec-handoff state=not-needed\n");
        let asked = ask(&client, &origin, &credits).await;
        assert!(matches!(asked, Asked::Answered(Ok(Selection::NotNeeded))));
        run.decide(&origin.turn, &credits, asked.verdict().expect("a verdict"));
        assert_eq!(
            run.keep(&credits, true, start + UNKNOWN_RESET_WAIT * 5),
            None
        );

        // `selected` keeps it too: an arm or a reserve that then fails leaves the run kept, at
        // its next refusal, for another ask.
        let run = walls();
        host.reply_once(&request, &selected(&origin));
        let asked = ask(&client, &origin, &credits).await;
        assert!(matches!(
            asked,
            Asked::Answered(Ok(Selection::Selected { .. }))
        ));
        assert_eq!(asked.verdict(), Some(Verdict::Keep));
        run.decide(&origin.turn, &credits, Verdict::Keep);
        assert_eq!(
            run.keep(&credits, true, start + UNKNOWN_RESET_WAIT),
            Some(UNKNOWN_RESET_WAIT)
        );

        // A definite refusal ends it; busy and a refusal in another word change nothing.
        for (reply, ends) in [
            ("ERR denied native operation authority\n", true),
            ("ERR codex-exec-handoff unsupported request\n", true),
            ("ERR unknown verb codex-exec-handoff\n", true),
            ("ERR busy retry_ms=100\n", false),
            ("ERR codex-exec-handoff capacity\n", false),
        ] {
            let run = walls();
            host.reply_once(&request, "OK codex-exec-handoff state=waiting\n");
            let keep = ask(&client, &origin, &credits).await.verdict();
            run.decide(&origin.turn, &credits, keep.expect("waiting"));
            host.reply_once(&request, reply);
            let asked = ask(&client, &origin, &credits).await;
            if let Some(verdict) = asked.verdict() {
                run.decide(&origin.turn, &credits, verdict);
            }
            assert_eq!(
                run.keep(&credits, true, start + UNKNOWN_RESET_WAIT),
                (!ends).then_some(UNKNOWN_RESET_WAIT),
                "{reply}"
            );
        }

        // A host advertising workspace recovery alone cannot accept a named prepare.
        let before = prepares();
        let model = WallKind::Limit("codex_other".into());
        let asked = ask(&client, &origin, &model).await;
        assert!(matches!(asked, Asked::Unasked));
        assert!(asked.verdict().is_none());
        assert_eq!(prepares(), before, "a model's limit sends no prepare");

        // An ordinary wall is asked as before, word or none.
        for word in ["1", "0"] {
            host.set_policy_key(crate::account_recovery::ROTATION_REFUSAL, word);
            client.harness_policy().await.unwrap();
            let before = prepares();
            host.reply_once(&request, "OK codex-exec-handoff state=not-needed\n");
            assert!(matches!(
                ask(&client, &origin, &WallKind::Ordinary).await,
                Asked::Answered(Ok(Selection::NotNeeded))
            ));
            assert_eq!(prepares(), before + 1, "{word}");
            assert_eq!(host.requests().last(), Some(&request));
        }
    }
}
