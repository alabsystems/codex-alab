// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Exec preparation through the shared account adapter. A selection has no
//! receipt or capability and cannot authorize suspension or process replacement.

use crate::Client;
use crate::Error;
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
    if !origin.valid() || !client.holds_claim() {
        return Err(Error::BadRequest(
            "exec account selection requires a valid native origin and self-claim".into(),
        ));
    }
    let reply = client
        .on_self(&format!(
            "codex-exec-handoff prepare thread={} turn={} source_account={} source_user={}",
            origin.thread, origin.turn, origin.identity.account, origin.identity.user,
        ))
        .await?;
    parse(&reply.status, origin)
}

/// What an exec run's ask about one wall came to (gap 4 of `alab/PARALLEL-LOGINS.md`, step S3).
pub enum Asked {
    /// Nothing was sent: no host is asked about this kind of wall ([`WallKind::asked`]).
    Unasked,
    /// Nothing was sent: the host's policy leaves no wall of refused ordinary use
    /// ([`crate::account_recovery::leaves_refusals`]), so it would answer `not-needed`.
    NotLeft,
    /// The host's answer to `prepare`.
    Answered(Result<Selection, Error>),
}

/// Ask the host, through `client` (claimed, its policy read), to select a destination for the run
/// standing at a wall of `kind`. A model's own limit is not asked ([`WallKind::asked`]); a wall
/// the run keeps only while a host may move it ([`WallKind::host_kept`]) is asked only of a host
/// whose policy leaves one ([`crate::account_recovery::leaves_exec_wall`]), which a connection
/// claimed at an ordinary wall may not. An ordinary wall is asked as before.
pub async fn ask(client: &Client, origin: &Origin, kind: &WallKind) -> Asked {
    if !kind.asked() {
        return Asked::Unasked;
    }
    if kind.host_kept()
        && !client
            .policy()
            .is_some_and(|policy| crate::account_recovery::leaves_exec_wall(&policy, kind))
    {
        return Asked::NotLeft;
    }
    Asked::Answered(prepare(client, origin).await)
}

impl Asked {
    /// The verdict this ask gives a host-kept wall ([`codex_alab_core::exec_wall`]): a move may
    /// come at `waiting` and `selected`; none will at `not-needed`, at a policy that leaves no
    /// such wall, or at a definite refusal ([`declined`]). `None`: no answer yet, or nothing asked.
    pub fn verdict(&self) -> Option<Verdict> {
        match self {
            Asked::Unasked => None,
            Asked::NotLeft => Some(Verdict::Declined),
            Asked::Answered(Ok(Selection::Waiting | Selection::Selected { .. })) => {
                Some(Verdict::Keep)
            }
            Asked::Answered(Ok(Selection::NotNeeded)) => Some(Verdict::Declined),
            Asked::Answered(Err(error)) => declined(error).then_some(Verdict::Declined),
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

        // A model's own limit: no `prepare`, whatever the host's words.
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
