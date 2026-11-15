// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Moving this session to another already-signed-in Codex account: aterm's
//! `restart account=<label>` or `restart account-auto`, asked of this very process on the
//! accounts connection. aterm stamps the restart with the account, ends the process, and its
//! relaunch resumes the same conversation on that account once the account proves it serves
//! there, else on the one it ran on. Nobody is signed in by this and no credential crosses it:
//! only a roster label travels, and aterm judges every refusal on its own records.
//!
//! The answer is read into a closed [`Outcome`]; no reply text is returned or logged. An aterm
//! before the account words answers `ERR bad restart argument: …` and nothing is restarted
//! ([`Outcome::Unsupported`]).

use crate::Client;
use crate::Error;
use std::time::Duration;

/// How long a move waits for aterm's answer. aterm judges the asked account on the pool's
/// records for up to 5 s (`restart_act::ACCOUNT_JUDGE`) before it takes the act's lock, which
/// another tab's restart may hold, then reads this session's record under that lock for up to
/// 1 s more (`restart_act::ACCOUNT_REST`, [`Why::Busy`]), and only then stamps and signals:
/// the connection's usual [`crate::CALL_TIMEOUT`] (5 s) would give up on a move aterm is still
/// legitimately carrying, and could not read a refusal given at that bound. 15 s holds both
/// bounds and a wait for the lock.
pub const MOVE_TIMEOUT: Duration = Duration::from_secs(15);

/// Where the move asks the relaunch to start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// The roster row with this label (`account=<label>`).
    Row(String),
    /// The row a new launch would get (`account-auto`, never `account=auto`: `auto` is a
    /// label a row may carry).
    Pool,
}

/// What aterm answered a move.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// `OK restarting pid <this pid> cause=<stall|asked>[ discarded=<n>]` naming the asked
    /// account: aterm stamped the restart and is ending this process.
    Restarting,
    /// This process does not lead its tab's foreground process group, so nothing was stamped
    /// or sent and nothing is restarted: `OK restart gone pid <this pid>` (it led it when
    /// stamped and no longer does), `ERR pid <this pid> is not the foreground process group
    /// (<n>)` or `ERR no foreground process group` (answered before the act).
    Gone,
    /// `ERR restart unavailable: <word>`: nothing was sent, and the act left no stamp of its
    /// own (one it wrote just before its signal is taken back before it answers).
    Refused(Why),
    /// `ERR denied native operation authority` or `ERR denied`: the connection may not ask
    /// a restart.
    Denied,
    /// `ERR bad restart argument: …`: an aterm before the account words. Nothing restarts.
    Unsupported,
    /// An answer with no word this build reads that aterm gives only when it signals nothing
    /// and leaves no stamp of the act's own: `ERR restart unavailable: <a word not in
    /// [`Why`]>` (a refused account writes and sends nothing; a stamp written for the act is
    /// taken back before the refusal) or `ERR restart failed: stamp (<why>)` (the stamp could
    /// not be written). Nothing restarts.
    NotRestarted,
    /// Any other answer. Whether a restart follows is not known. Among them is `ERR restart
    /// failed: killpg pid <n>`: the act sent no signal, but where it joined a standing stamp
    /// for the same account (an earlier ask's, already signalled) that stamp is kept, so the
    /// earlier ask's restart may still follow.
    Unknown,
}

/// Why aterm refused a move (`ERR restart unavailable: <word>`), one variant per word it
/// answers: the restart's own fences first, then the account's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    /// `not-the-hosts-agent`: the host holds no snapshot of this process.
    NotTheHostsAgent,
    /// `not-relaunchable`: aterm has no validated relaunch route for it.
    NotRelaunchable,
    /// `not-supervised`: no live claim that is the host's or proven (Autopilot off).
    NotSupervised,
    /// `driver-lease`: another program holds the session's driver lease.
    DriverLease,
    /// `driver-turn`: another program's driven turn is under way.
    DriverTurn,
    /// `halted`: a hold stands on the session.
    Halted,
    /// `start-unread`: the process's kernel start could not be read.
    StartUnread,
    /// `no-state`: aterm's harness state directory is unknown.
    NoState,
    /// `exit-owner-not-ready`: the session's restored task has no harness ready yet to keep it
    /// open through the restart.
    ExitOwnerNotReady,
    /// `account-unmanaged`: the session runs on no stored row of the pool aterm can move.
    Unmanaged,
    /// `account-not-resumable`: no shell under the session would type its relaunch.
    NotResumable,
    /// `account-recovery-pending`: a login renewal or exec handoff of it is in flight.
    RecoveryPending,
    /// `account-off`: `[harness] accounts = off`.
    Off,
    /// `account-unknown`: no roster row has that label.
    UnknownLabel,
    /// `account-no-choice`: the pool has no row a launch would start on.
    NoChoice,
    /// `account-current`: the session already runs on that row (or the pool's choice is it).
    Current,
    /// `account-same`: the row is another row of the account the session runs on.
    Same,
    /// `account-not-enrolled`: the row has no login on this Mac.
    NotEnrolled,
    /// `account-signed-out`: the row is signed out here.
    SignedOut,
    /// `account-held`: held for another Mac, or its claim here is refused.
    Held,
    /// `account-limited`: a usage wall stands on the row.
    Limited,
    /// `account-identity`: a login names another account than its row.
    Identity,
    /// `account-unread`: the records could not be read in time, or of this process.
    Unread,
    /// `account-restart-pending`: a restart of this process already stands for another
    /// destination, or none.
    RestartPending,
    /// `account-busy`, judged last: aterm did not find this process at its own rest when it
    /// came to restart it. Under the act's lock it reads the session record (CONTRACT §4) and
    /// needs its idle point (`status:"idle"`, `draftEmpty:true`, no `exit`), for this pid,
    /// within 1 s; just before its signal it needs no input unread in the pty and no person's
    /// gesture since the ask arrived. A record that is missing, unreadable, late, another
    /// process's or a dead identity's refuses too.
    Busy,
}

/// Every refusal word aterm answers, with its [`Why`].
const WORDS: [(&str, Why); 25] = [
    ("not-the-hosts-agent", Why::NotTheHostsAgent),
    ("not-relaunchable", Why::NotRelaunchable),
    ("not-supervised", Why::NotSupervised),
    ("driver-lease", Why::DriverLease),
    ("driver-turn", Why::DriverTurn),
    ("halted", Why::Halted),
    ("start-unread", Why::StartUnread),
    ("no-state", Why::NoState),
    ("exit-owner-not-ready", Why::ExitOwnerNotReady),
    ("account-unmanaged", Why::Unmanaged),
    ("account-not-resumable", Why::NotResumable),
    ("account-recovery-pending", Why::RecoveryPending),
    ("account-off", Why::Off),
    ("account-unknown", Why::UnknownLabel),
    ("account-no-choice", Why::NoChoice),
    ("account-current", Why::Current),
    ("account-same", Why::Same),
    ("account-not-enrolled", Why::NotEnrolled),
    ("account-signed-out", Why::SignedOut),
    ("account-held", Why::Held),
    ("account-limited", Why::Limited),
    ("account-identity", Why::Identity),
    ("account-unread", Why::Unread),
    ("account-restart-pending", Why::RestartPending),
    ("account-busy", Why::Busy),
];

impl Why {
    /// aterm's word for it.
    pub fn word(self) -> &'static str {
        WORDS
            .iter()
            .find(|(_, why)| *why == self)
            .map_or("", |(word, _)| word)
    }

    fn of(word: &str) -> Option<Self> {
        WORDS
            .iter()
            .find(|(known, _)| *known == word)
            .map(|(_, why)| *why)
    }
}

/// The bare word that asks for the pool's choice.
const ACCOUNT_AUTO: &str = "account-auto";

/// Whether `label` is in aterm's roster grammar: `[a-z0-9][a-z0-9._-]{0,63}`, never `help`.
pub fn admissible_label(label: &str) -> bool {
    let bytes = label.as_bytes();
    label != "help"
        && (1..=64).contains(&bytes.len())
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(b))
}

impl Target {
    /// This target's account word on the restart line.
    fn word(&self) -> String {
        match self {
            Self::Row(label) => format!("account={label}"),
            Self::Pool => ACCOUNT_AUTO.to_owned(),
        }
    }
}

/// `line` as the answer to a move to `target` asked for process `pid`.
fn outcome(line: &str, pid: u32, target: &Target) -> Outcome {
    if line == "ERR denied native operation authority" || line == "ERR denied" {
        return Outcome::Denied;
    }
    if line.starts_with("ERR bad restart argument: ") {
        return Outcome::Unsupported;
    }
    if let Some(word) = line.strip_prefix("ERR restart unavailable: ") {
        return Why::of(word).map_or(Outcome::NotRestarted, Outcome::Refused);
    }
    if line
        .strip_prefix("ERR restart failed: stamp (")
        .is_some_and(|rest| rest.ends_with(')'))
    {
        return Outcome::NotRestarted;
    }
    let pid = pid.to_string();
    if line == format!("OK restart gone pid {pid}") || line == "ERR no foreground process group" {
        return Outcome::Gone;
    }
    if let Some(leader) = line
        .strip_prefix("ERR pid ")
        .and_then(|rest| rest.strip_prefix(pid.as_str()))
        .and_then(|rest| rest.strip_prefix(" is not the foreground process group ("))
        .and_then(|rest| rest.strip_suffix(')'))
        && !leader.is_empty()
        && leader.bytes().all(|b| b.is_ascii_digit())
    {
        return Outcome::Gone;
    }
    let Some(tokens) = line
        .strip_prefix("OK restarting pid ")
        .and_then(|rest| rest.strip_prefix(pid.as_str()))
        .and_then(|rest| rest.strip_prefix(' '))
    else {
        return Outcome::Unknown;
    };
    // The tokens after the pid are read by key, in any order, each at most once: the cause,
    // what the escalation discarded, and the account, which must be the one asked.
    let asked = target.word();
    let (mut cause, mut discarded, mut account) = (false, false, false);
    for token in tokens.split(' ') {
        let seen = if token == "cause=stall" || token == "cause=asked" {
            &mut cause
        } else if token
            .strip_prefix("discarded=")
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        {
            &mut discarded
        } else if token == asked {
            &mut account
        } else {
            return Outcome::Unknown;
        };
        if std::mem::replace(seen, true) {
            return Outcome::Unknown;
        }
    }
    if cause && account {
        Outcome::Restarting
    } else {
        Outcome::Unknown
    }
}

impl Client {
    /// Ask aterm to restart this process on `target` (`restart pid=<this pid>
    /// account=<label>` or `... account-auto`), on this session. A label outside aterm's
    /// roster grammar is [`Error::BadRequest`] and nothing is sent. An answer is read into
    /// [`Outcome`]; an error is the connection's (no answer was read).
    ///
    /// An [`Outcome::Restarting`] answer means aterm is ending this process: whatever must
    /// survive the restart (the session record) is written before this is called. The answer
    /// is waited for at most [`MOVE_TIMEOUT`].
    pub async fn account_move(&self, target: &Target) -> Result<Outcome, Error> {
        if let Target::Row(label) = target
            && !admissible_label(label)
        {
            return Err(Error::BadRequest("not an account label".into()));
        }
        let pid = std::process::id();
        let reply = self
            .on_self_within(
                &format!("restart pid={pid} {}", target.word()),
                MOVE_TIMEOUT,
            )
            .await?;
        Ok(outcome(&reply.status, pid, target))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PID: u32 = 4242;

    fn row(label: &str) -> Target {
        Target::Row(label.into())
    }

    #[test]
    fn account_move_reads_the_reply_tokens_by_key() {
        let work = row("work");
        for line in [
            "OK restarting pid 4242 cause=asked account=work",
            "OK restarting pid 4242 cause=stall account=work",
            "OK restarting pid 4242 cause=asked discarded=17 account=work",
            "OK restarting pid 4242 account=work discarded=0 cause=stall",
        ] {
            assert_eq!(outcome(line, PID, &work), Outcome::Restarting, "{line}");
        }
        assert_eq!(
            outcome(
                "OK restarting pid 4242 cause=asked account-auto",
                PID,
                &Target::Pool
            ),
            Outcome::Restarting
        );
        // A row labelled `auto` is asked and answered `account=auto`, never the bare word.
        assert_eq!(
            outcome(
                "OK restarting pid 4242 cause=asked account=auto",
                PID,
                &row("auto")
            ),
            Outcome::Restarting
        );
        let pool = "OK restarting pid 4242 cause=asked account-auto";
        let auto = "OK restarting pid 4242 cause=asked account=auto";
        assert_eq!(outcome(auto, PID, &Target::Pool), Outcome::Unknown);
        assert_eq!(outcome(pool, PID, &row("auto")), Outcome::Unknown);
        for line in [
            "OK restarting pid 4242 cause=asked account=main",
            "OK restarting pid 4243 cause=asked account=work",
            "OK restarting pid 42 cause=asked account=work",
            "OK restarting pid 42420 cause=asked account=work",
            // A plain restart's reply: the account was not asked of it.
            "OK restarting pid 4242 cause=asked",
            "OK restarting pid 4242 account=work",
            "OK restarting pid 4242 cause=later account=work",
            "OK restarting pid 4242 cause=asked cause=asked account=work",
            "OK restarting pid 4242 cause=asked account=work account=work",
            "OK restarting pid 4242 cause=asked discarded= account=work",
            "OK restarting pid 4242 cause=asked discarded=x account=work",
            "OK restarting pid 4242 cause=asked account=work extra",
            "OK restarting pid 4242  cause=asked account=work",
            "OK restarting pid 4242 cause=asked account=work ",
            "OK restart gone pid 4243",
            "OK",
            "",
            // No signal was sent, but a joined stamp for the same account is kept.
            "ERR restart failed: killpg pid 4242",
            "ERR restart failed: stamp",
            "ERR restart failed: stamp (EACCES",
            // Answers to options this client never sends (`if-stalled`, `quiet=`).
            "ERR not-stalled",
            "ERR busy person human_ms=12",
            // Another process's answer, or one this build cannot read.
            "ERR pid 4243 is not the foreground process group (77)",
            "ERR pid 4242 is not the foreground process group ()",
            "ERR pid 4242 is not the foreground process group (7x)",
            "ERR pid 4242 is not the foreground process group",
            "ERR unknown verb (try: help)",
            "ERR denied by policy",
        ] {
            assert_eq!(outcome(line, PID, &work), Outcome::Unknown, "{line}");
        }
        for line in [
            "OK restart gone pid 4242",
            "ERR pid 4242 is not the foreground process group (77)",
            "ERR no foreground process group",
        ] {
            assert_eq!(outcome(line, PID, &work), Outcome::Gone, "{line}");
        }
        // Answered only before anything is stamped or sent.
        for line in [
            "ERR restart unavailable: account-something",
            "ERR restart unavailable: ",
            "ERR restart failed: stamp (Permission denied (os error 13))",
        ] {
            assert_eq!(outcome(line, PID, &work), Outcome::NotRestarted, "{line}");
        }
        for line in ["ERR denied native operation authority", "ERR denied"] {
            assert_eq!(outcome(line, PID, &work), Outcome::Denied, "{line}");
        }
        for line in [
            "ERR bad restart argument: account=work",
            "ERR bad restart argument: account-auto",
        ] {
            assert_eq!(outcome(line, PID, &work), Outcome::Unsupported, "{line}");
        }
    }

    #[test]
    fn account_move_names_every_refusal_word_aterm_answers() {
        // aterm's catalogue entry for `restart` (control_verbs.rs), in its order.
        let words = "not-the-hosts-agent|not-relaunchable|not-supervised|driver-lease|\
                     driver-turn|halted|start-unread|no-state|exit-owner-not-ready|\
                     account-unmanaged|account-not-resumable|account-recovery-pending|\
                     account-off|account-unknown|account-no-choice|account-current|\
                     account-same|account-not-enrolled|account-signed-out|account-held|\
                     account-limited|account-identity|account-unread|account-restart-pending|\
                     account-busy";
        let mut seen = std::collections::BTreeSet::new();
        for word in words.split('|') {
            let Outcome::Refused(why) = outcome(
                &format!("ERR restart unavailable: {word}"),
                PID,
                &row("work"),
            ) else {
                panic!("{word} is no refusal");
            };
            assert_eq!(why.word(), word);
            assert!(seen.insert(word));
        }
        assert_eq!(seen.len(), WORDS.len());
    }

    #[test]
    fn account_move_labels_follow_the_roster_grammar() {
        for label in ["main", "work", "auto", "a", "0", "a.b_c-d", &"a".repeat(64)] {
            assert!(admissible_label(label), "{label}");
        }
        for label in [
            "",
            "help",
            "Work",
            "-a",
            ".a",
            "_a",
            "a b",
            "a=b",
            "a\n",
            "é",
            &"a".repeat(65),
        ] {
            assert!(!admissible_label(label), "{label:?}");
        }
    }

    async fn host() -> (tempfile::TempDir, crate::fake::FakeAterm, Client) {
        let directory = tempfile::tempdir().unwrap();
        let host = crate::fake::FakeAterm::start(directory.path(), crate::fake::Mode::Current)
            .await
            .unwrap();
        let client = Client::connect(host.endpoint(), crate::Options::default())
            .await
            .unwrap();
        (directory, host, client)
    }

    /// The restart lines the fake host was sent.
    fn restarts(host: &crate::fake::FakeAterm) -> Vec<String> {
        host.requests()
            .into_iter()
            .filter(|line| line.split(' ').nth(1) == Some("restart"))
            .collect()
    }

    #[tokio::test]
    async fn account_move_sends_one_exact_line_and_reads_each_answer() {
        let (_directory, host, client) = host().await;
        let pid = std::process::id();
        let to_work = format!("@{} restart pid={pid} account=work", host.sid());
        let to_pool = format!("@{} restart pid={pid} account-auto", host.sid());

        // An aterm before the account words refuses the word and restarts nothing.
        assert_eq!(
            client.account_move(&row("work")).await.unwrap(),
            Outcome::Unsupported
        );
        assert_eq!(
            client.account_move(&Target::Pool).await.unwrap(),
            Outcome::Unsupported
        );
        assert_eq!(restarts(&host), [to_work.clone(), to_pool.clone()]);

        // One that has them.
        host.answer_account_moves(true);
        assert_eq!(
            client.account_move(&row("work")).await.unwrap(),
            Outcome::Restarting
        );
        assert_eq!(
            client.account_move(&Target::Pool).await.unwrap(),
            Outcome::Restarting
        );
        assert_eq!(restarts(&host)[2..], [to_work.clone(), to_pool.clone()]);

        for (reply, expected) in [
            (
                format!("OK restarting pid {pid} cause=stall account=work"),
                Outcome::Restarting,
            ),
            (
                format!("OK restarting pid {pid} cause=asked discarded=512 account=work"),
                Outcome::Restarting,
            ),
            (format!("OK restart gone pid {pid}"), Outcome::Gone),
            (
                "ERR restart unavailable: account-not-resumable".to_owned(),
                Outcome::Refused(Why::NotResumable),
            ),
            (
                "ERR restart unavailable: account-unread".to_owned(),
                Outcome::Refused(Why::Unread),
            ),
            (
                "ERR restart unavailable: halted".to_owned(),
                Outcome::Refused(Why::Halted),
            ),
            // aterm's last fence: the session was not at its own rest when it came to signal.
            (
                "ERR restart unavailable: account-busy".to_owned(),
                Outcome::Refused(Why::Busy),
            ),
            (
                "ERR restart unavailable: exit-owner-not-ready".to_owned(),
                Outcome::Refused(Why::ExitOwnerNotReady),
            ),
            (
                "ERR denied native operation authority".to_owned(),
                Outcome::Denied,
            ),
            ("ERR denied".to_owned(), Outcome::Denied),
            (
                format!("OK restarting pid {} cause=asked account=work", pid + 1),
                Outcome::Unknown,
            ),
            (
                format!("OK restarting pid {pid} cause=asked account=main"),
                Outcome::Unknown,
            ),
        ] {
            host.reply_once(&to_work, &format!("{reply}\n"));
            assert_eq!(
                client.account_move(&row("work")).await.unwrap(),
                expected,
                "{reply}"
            );
        }

        // A malformed answer is Unknown, and its text goes nowhere.
        host.reply_once(&to_work, "OK restarting pid secret-token-text\n");
        let answer = client.account_move(&row("work")).await;
        assert!(matches!(answer, Ok(Outcome::Unknown)));
        assert!(!format!("{answer:?}").contains("secret-token-text"));

        // A label outside the grammar is refused before anything is sent.
        let before = restarts(&host).len();
        for label in ["Work", "help", "a b", ""] {
            assert!(matches!(
                client.account_move(&row(label)).await,
                Err(Error::BadRequest(_))
            ));
        }
        assert_eq!(restarts(&host).len(), before);
    }

    #[tokio::test]
    async fn account_move_waits_out_aterms_account_judgement() {
        // aterm may judge the account for 5 s before it acts: a move still unanswered past the
        // connection's usual deadline is still being carried, and its answer is read.
        let (_directory, host, client) = host().await;
        host.answer_account_moves(true);
        host.gate("restart");
        let work = row("work");
        let mut asked = Box::pin(client.account_move(&work));
        tokio::select! {
            answer = &mut asked => panic!("the gated move was answered: {answer:?}"),
            seen = host.wait_for(|host| !host.gated_requests().is_empty()) => assert!(seen),
        }
        tokio::time::pause();
        tokio::time::advance(crate::CALL_TIMEOUT + Duration::from_secs(5)).await;
        let waiting = std::future::poll_fn(|cx| {
            std::task::Poll::Ready(std::future::Future::poll(asked.as_mut(), cx))
        })
        .await;
        assert!(
            waiting.is_pending(),
            "a move past the usual deadline is still waited for: {waiting:?}"
        );
        tokio::time::resume();
        host.open_gate("restart");
        assert_eq!(asked.await.unwrap(), Outcome::Restarting);

        // MOVE_TIMEOUT still bounds it.
        host.gate("restart");
        let mut asked = Box::pin(client.account_move(&work));
        tokio::select! {
            answer = &mut asked => panic!("the gated move was answered: {answer:?}"),
            seen = host.wait_for(|host| host.gated_requests().len() == 2) => assert!(seen),
        }
        tokio::time::pause();
        tokio::time::advance(MOVE_TIMEOUT + Duration::from_millis(1)).await;
        assert!(matches!(asked.await, Err(Error::Unanswered)));
        tokio::time::resume();
    }
}
