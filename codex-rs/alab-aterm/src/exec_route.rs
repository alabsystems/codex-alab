// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Why a `codex exec` run that waits at a usage wall will not move to another account, and
//! the one stderr line that says so, once per wall (CONTRACT §1.1, "The line"; gap #2 of
//! the 2026-11-14 audit in `alab/PARALLEL-LOGINS.md`).
//!
//! aterm's exec verbs admit only the tab's proven claim, and only the tab's foreground
//! leader can hold it (CONTRACT §1). So a run that does not lead its controlling
//! terminal's foreground process group sends no claim at all ([`admit`]). aterm would not
//! prove it, and asking every 5 s only set and released the tab's `supervisor`. A leader
//! claims and moves exactly as before. A leader of another terminal (tmux inside aterm,
//! codex's own unified-exec PTY) is not proven ([`NoRoute::Unproven`]), or meets the
//! proven claim of the program that does lead the tab ([`NoRoute::AnotherLeads`]). Either
//! settles the wall, so it claims at most once per wall ([`Latch`]).
//!
//! Only a definite answer is a route. A refusal that does not say what aterm serves (a
//! dialog aterm is showing, a main-thread stall, a busy aterm), and a socket that refuses
//! or is missing for a moment (an aterm seamless update), are no answer yet: silent, and
//! asked again on the 5 s clock as before ([`Miss::Transient`]).
//!
//! The exec process admission that will move the other shapes is planned and not built
//! (CONTRACT §1.1). Its definite refusal, `state=refused why=<word>`, is read already: each
//! word names its route, or no answer yet for a suspension ([`NoRoute::refused`]). No aterm
//! sends one yet.

use std::io::Write;
use std::time::Duration;

use crate::ClaimOutcome;
use crate::Client;
use crate::Endpoint;
use crate::Error;
use crate::Options;
use crate::PolicyAnswer;
use crate::account_recovery;
use crate::exec_grant;
use crate::renewal::Identity;
use codex_alab_core::account_handoff::Purpose;
use codex_alab_core::wall_kind::WallKind;

/// The fixed start of every line.
pub const PREFIX: &str = "codex: account move unavailable: ";

/// How long a hold or an Off waits before it is read again: CONTRACT §0's passive clock.
/// No event this run listens to announces a hold, a suspension or a changed key.
pub const PASSIVE: Duration = Duration::from_secs(30);

/// How long the claim path waits between asks, as before.
pub const RETRY: Duration = Duration::from_secs(5);

/// How long a wall with no recoverable root turn waits before Core is read again, as
/// before. The read is local: nothing is sent to aterm.
pub const ROOT_RETRY: Duration = Duration::from_millis(50);

/// The longest `why=` word a line repeats. A longer or odd one is left out.
const WHY_MAX: usize = 32;

/// Why this run does not move at this wall.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoRoute {
    /// No aterm session is named, or none this run can place (a session id of the wrong
    /// shape, no rendezvous directory): the run is outside any aterm tab.
    Outside,
    /// A session is named, but this run is denied its rendezvous entry or its socket
    /// (`EPERM` or `EACCES`), as a sandbox denies them: law 2, not worked around. A socket
    /// that refuses or is missing, a missing entry and an `ERR auth` are what an aterm
    /// seamless update shows for a moment, so they are [`Miss::Transient`].
    Unreachable,
    /// The host answers that it has no `harness policy`, no exec contract 3 or no exec tree
    /// contract 1 ([`not_served`]).
    NoExecContract,
    /// The host's policy is off for this tab (`effective=off`), with its reason.
    Off(Off),
    /// The policy is on, but its keys move no exec run (`codex_rotation`, `accounts`,
    /// `relaunch`, `resume_limits`; [`account_recovery::policy_allows`]), or none at the run's
    /// kind of wall ([`account_recovery::leaves_exec_wall`]); or aterm refused the run
    /// `rotation-off`, by the same keys. Only an ordinary wall tells it: a workspace's wall that
    /// no host leaves ends its run with no line.
    RotationOff,
    /// The run does not lead its controlling terminal's foreground process group, so it
    /// sends no claim ([`leads_foreground`]). That is checked again locally: a job brought
    /// to the front with `fg` is admitted.
    NotLeader,
    /// The run leads its terminal's foreground group but aterm did not prove its claim: that
    /// terminal is not the tab's.
    Unproven,
    /// The run leads its terminal's foreground group, but another program's proven claim
    /// stands in the tab. aterm keeps a proven claim only while its holder leads the tab's
    /// foreground group, so this run's terminal is not the tab's: a run in codex's
    /// unified-exec PTY under a codex-alab TUI that holds the tab, for one.
    AnotherLeads,
    /// The wall's source is not a ChatGPT login.
    NotChatGpt,
    /// The wall's ChatGPT login names no account or user to hash.
    IdentityUnreadable,
    /// The run has no root turn that another account could recover.
    NoRootTurn,
    /// The host answered `state=not-needed`: no move is needed now, or no account is
    /// there to move to. The wire does not say which, so it is asked again.
    NotNeeded,
    /// aterm refused the run `not-in-tab`: it does not run in the tab it names, by the same
    /// session or by its parents. Also `nested-aterm`: the nearest aterm above it is another
    /// one, whose tab owns it.
    NotInTab,
    /// aterm refused the run `not-alab-image`: its program is not an ALab build of codex.
    NotAlabImage,
    /// aterm refused the run `not-exec`: its command line is not a `codex exec` run.
    NotExec,
    /// aterm refused the run `unsupported-run`: it is an exec run, but one aterm cannot start
    /// again as it was launched (its directory, program or environment).
    UnsupportedRun,
    /// aterm refused the run `no-pool-row`: its login is no row of the account pool. Also
    /// `login-names-another`: its row's login names another account.
    NoPoolRow,
    /// aterm answered a process prepare `ERR denied`: the asker is not one aterm admits (a
    /// proxied or malformed request), which does not change for the wall.
    Denied,
}

/// Why the host's policy is off for this tab (its `why=` word).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Off {
    /// `hold`: a person holds the tab.
    Hold,
    /// `autopilot-off`: the tab's Autopilot is Off.
    AutopilotOff,
    /// Any other off (`agent-off`, `disabled`, `headless`, `unhosted`, or a word this reader
    /// does not know). The word is kept only when it is short and plain; else it is empty.
    Other(String),
}

impl Off {
    /// The off a `why=` word names. `suspended` is not one: [`Off::read`] reads it first.
    fn of(why: &str) -> Self {
        match why {
            "hold" => Off::Hold,
            "autopilot-off" => Off::AutopilotOff,
            other => Off::Other(plain_word(other)),
        }
    }

    /// What an off `why=` word comes to, in the host's policy or in its refusal of this run:
    /// `suspended` is no answer yet, silent and asked again on the 5 s clock (CONTRACT §0 and
    /// §1.1, "How often a run asks"), and any other word is an [`Off`].
    fn read(why: &str) -> Miss {
        match why {
            "suspended" => Miss::Transient,
            why => NoRoute::Off(Off::of(why)).into(),
        }
    }
}

/// `word` if it is short and made of lowercase letters, digits and dashes only. Otherwise
/// it is empty, so a line never repeats what the host sent unchecked.
pub(crate) fn plain_word(word: &str) -> String {
    let plain = !word.is_empty()
        && word.len() <= WHY_MAX
        && word
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if plain {
        word.to_owned()
    } else {
        String::new()
    }
}

impl NoRoute {
    /// The one line that says why, ending in `\n`. It names no label, email or path.
    pub fn line(&self) -> String {
        const RESET: &str = "it waits for its usage limit to reset";
        let reason = match self {
            NoRoute::Outside => format!("this run is not in an aterm tab; {RESET}"),
            NoRoute::Unreachable => {
                format!("this run is denied its aterm's socket (a sandbox may forbid it); {RESET}")
            }
            NoRoute::NoExecContract => {
                format!("this aterm does not move a codex exec run; {RESET}")
            }
            NoRoute::Off(Off::Hold) => {
                "this tab is held; the move waits for the hold to lift".to_owned()
            }
            NoRoute::Off(Off::AutopilotOff) => {
                "Autopilot is off in this tab; the move waits for a later read to find it on"
                    .to_owned()
            }
            NoRoute::Off(Off::Other(word)) if word.is_empty() => {
                "aterm's harness is off for this tab; the move waits for a later read to find \
                 it on"
                    .to_owned()
            }
            NoRoute::Off(Off::Other(word)) => format!(
                "aterm's harness is off for this tab ({word}); the move waits for a later read \
                 to find it on"
            ),
            NoRoute::RotationOff => "aterm's settings move no codex exec run to another \
                                     account; the move waits for a later read to allow it"
                .to_owned(),
            NoRoute::NotLeader => "this aterm moves only a run that leads its tab's foreground \
                                   job; the move waits for this run to lead it"
                .to_owned(),
            NoRoute::Unproven => format!(
                "aterm did not prove this run the leader of its tab's foreground job; {RESET}"
            ),
            NoRoute::AnotherLeads => format!(
                "another program leads this tab's foreground job and holds its claim; {RESET}"
            ),
            NoRoute::NotChatGpt => format!("this run is not signed in with ChatGPT; {RESET}"),
            NoRoute::IdentityUnreadable => {
                format!("this run's account cannot be identified; {RESET}")
            }
            NoRoute::NoRootTurn => {
                format!("this run has no turn another account could resume; {RESET}")
            }
            NoRoute::NotNeeded => "aterm sees no move to make now, or no account to move to; \
                                   the move waits for aterm's next read"
                .to_owned(),
            NoRoute::NotInTab => format!("aterm does not find this run in its tab; {RESET}"),
            NoRoute::NotAlabImage => {
                format!("aterm moves only an ALab build of codex, and this run is not one; {RESET}")
            }
            NoRoute::NotExec => {
                format!("aterm does not read this run as a codex exec run; {RESET}")
            }
            NoRoute::UnsupportedRun => {
                format!("aterm cannot start this run again as it was launched; {RESET}")
            }
            NoRoute::NoPoolRow => {
                format!("this run's login is not one of the accounts aterm moves between; {RESET}")
            }
            NoRoute::Denied => format!("aterm denied this run's request to move; {RESET}"),
        };
        format!("{PREFIX}{reason}\n")
    }

    /// What aterm's definite refusal of this run (`state=refused why=<word>`) comes to, by its
    /// word: the words of CONTRACT §1.1, "Wire (planned)", and `unsupported-run`, `no-pool-row`,
    /// `login-names-another` and `rotation-off`, which the amended exec process admission adds
    /// and CONTRACT does not list yet. A refusal that cannot change for the wall stops the
    /// asking: `not-in-tab`, `nested-aterm`, `not-alab-image`, `not-exec`, `unsupported-run`,
    /// `no-pool-row` and `login-names-another`. A Codex-scoped off word is an [`Off`] (`hold`,
    /// `autopilot-off`, `agent-off`, `disabled`, `headless`, `unhosted`), and `rotation-off` is
    /// [`NoRoute::RotationOff`]: each is read again on the passive clock. So is a word this
    /// reader does not know, as [`Off::Other`] (empty when the word is not plain): the asking
    /// never stops on a word it does not know, and never comes back on the 5 s clock for one.
    /// `suspended` is no answer yet ([`Off::read`]), as aterm's policy says it: aterm answers a
    /// suspension `waiting`, and a reply that names one anyway is asked again, silently.
    pub fn refused(why: &str) -> Miss {
        match why {
            "not-in-tab" | "nested-aterm" => NoRoute::NotInTab.into(),
            "not-alab-image" => NoRoute::NotAlabImage.into(),
            "not-exec" => NoRoute::NotExec.into(),
            "unsupported-run" => NoRoute::UnsupportedRun.into(),
            "no-pool-row" | "login-names-another" => NoRoute::NoPoolRow.into(),
            "rotation-off" => NoRoute::RotationOff.into(),
            off => Off::read(off),
        }
    }

    /// When to look again during this wall: `None` when nothing this run can see changes
    /// the outcome before another wall, so it stops asking (CONTRACT §0).
    pub fn again(&self) -> Option<Duration> {
        match self {
            NoRoute::Outside
            | NoRoute::Unreachable
            | NoRoute::NoExecContract
            | NoRoute::Unproven
            | NoRoute::AnotherLeads
            | NoRoute::NotChatGpt
            | NoRoute::IdentityUnreadable
            | NoRoute::NotInTab
            | NoRoute::NotAlabImage
            | NoRoute::NotExec
            | NoRoute::UnsupportedRun
            | NoRoute::NoPoolRow
            | NoRoute::Denied => None,
            NoRoute::Off(_) | NoRoute::RotationOff => Some(PASSIVE),
            // Local: whether `fg` or `bg` moved this run's group is read without aterm.
            NoRoute::NotLeader => Some(RETRY),
            NoRoute::NotNeeded => Some(RETRY),
            NoRoute::NoRootTurn => Some(ROOT_RETRY),
        }
    }
}

/// Why no client came back: a [`NoRoute`] to tell, or no answer yet, which is silent and
/// asked again as before. No answer is a suspension; a policy read or contract probe that
/// fails, or is refused without saying the host serves no such thing ([`not_served`]); a
/// busy, unanswered or closing aterm; a socket, token or rendezvous entry that refuses or
/// is missing, or an `ERR auth` (an aterm seamless update); an unproven claim, or this
/// run's own proven one, standing in the way; and a status read that fails after a standing
/// claim kept this run's out. A refusal of this run reads the same way ([`NoRoute::refused`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Miss {
    /// Ask again on the ordinary clock; tell nothing.
    Transient,
    /// Tell this once for the wall.
    Route(NoRoute),
}

impl From<NoRoute> for Miss {
    fn from(route: NoRoute) -> Self {
        Miss::Route(route)
    }
}

/// The wall's source login, as the line's latch keys it.
#[derive(Clone, PartialEq, Eq)]
pub enum Source {
    /// A ChatGPT login, hashed ([`Identity::from_native`]).
    ChatGpt(Identity),
    /// A ChatGPT login whose account or user is empty.
    Unreadable,
    /// Another kind of login (an API key).
    NotChatGpt,
}

impl Source {
    /// A ChatGPT login's source.
    pub fn login(account: &str, user: &str) -> Self {
        Identity::from_native(account, user).map_or(Source::Unreadable, Source::ChatGpt)
    }
}

/// One wall, as the line is told for it: the source login and the root turn the run would
/// recover. Every quota retry publishes a new observation of the same wall, and a child's
/// wait publishes its own, so an observation's own identity would tell the line again at
/// each retry.
#[derive(Clone, PartialEq, Eq)]
pub struct Wall {
    source: Source,
    root: Option<String>,
}

impl Wall {
    /// The wall of `source` in the root turn `root` (`None`: no recoverable root turn).
    pub fn new(source: Source, root: Option<String>) -> Self {
        Wall { source, root }
    }

    /// The source identity a move would leave, or why there is none. A wall with no
    /// recoverable root turn is [`NoRoute::NoRootTurn`], whatever its source.
    pub fn identity(&self) -> Result<Identity, NoRoute> {
        match (&self.source, &self.root) {
            (Source::ChatGpt(identity), Some(_)) => Ok(identity.clone()),
            (Source::NotChatGpt, _) => Err(NoRoute::NotChatGpt),
            (Source::Unreadable, _) => Err(NoRoute::IdentityUnreadable),
            (Source::ChatGpt(_), None) => Err(NoRoute::NoRootTurn),
        }
    }
}

/// The told-once latch for the wall in hand. A new wall resets it.
#[derive(Default)]
pub struct Latch {
    wall: Option<Wall>,
    told: bool,
    settled: bool,
}

impl Latch {
    fn at(&mut self, wall: &Wall) {
        if self.wall.as_ref() != Some(wall) {
            *self = Latch {
                wall: Some(wall.clone()),
                told: false,
                settled: false,
            };
        }
    }

    /// Whether an outcome that cannot change before another wall settled `wall`: then
    /// nothing is asked of aterm until another wall comes.
    pub fn settled(&self, wall: &Wall) -> bool {
        self.settled && self.wall.as_ref() == Some(wall)
    }

    /// Note that `wall` met `route`. Returns the line the first time this wall meets any
    /// route, and `None` after that: one line per wall.
    pub fn note(&mut self, wall: &Wall, route: &NoRoute) -> Option<String> {
        self.at(wall);
        self.settled |= route.again().is_none();
        (!std::mem::replace(&mut self.told, true)).then(|| route.line())
    }
}

/// Write `line` whole to stderr: one `write_all` under the stderr lock, never stdout, so
/// `--json` output stays parseable.
pub fn tell(line: &str) {
    tell_to(&mut std::io::stderr().lock(), line);
}

/// [`tell`] to `out`.
pub fn tell_to(out: &mut impl Write, line: &str) {
    // A line that cannot be written changes nothing the run does.
    let _ = out.write_all(line.as_bytes());
    let _ = out.flush();
}

/// Whether this process leads its controlling terminal's foreground process group: it
/// leads its own group (`getpgrp() == getpid()`), and `/dev/tty`'s foreground group is that
/// group. A process with no controlling terminal, or one a sandbox keeps from `/dev/tty`,
/// leads none.
///
/// The terminal is not known to be the tab's. A leader of tmux's or of codex's own PTY
/// leads here, and aterm then does not prove its claim ([`NoRoute::Unproven`]).
pub fn leads_foreground() -> bool {
    use std::os::fd::AsRawFd;
    // SAFETY: getpid and getpgrp take no arguments and cannot fail.
    let (pid, group) = unsafe { (libc::getpid(), libc::getpgrp()) };
    if group != pid {
        return false;
    }
    let Ok(tty) = std::fs::File::open("/dev/tty") else {
        return false;
    };
    // SAFETY: tcgetpgrp reads the foreground group of a descriptor this function owns.
    unsafe { libc::tcgetpgrp(tty.as_raw_fd()) == pid }
}

/// Where this run's tab is served. No session named, or none this run can place, is
/// [`NoRoute::Outside`]. A rendezvous entry the sandbox denies is [`NoRoute::Unreachable`];
/// one that is missing or cannot be read for another reason is no answer yet ([`located`]).
/// Vanilla mode moves nothing and tells nothing.
pub fn locate() -> Result<Endpoint, Miss> {
    let Some(sid) = std::env::var_os(crate::locate::SELF_SID_ENV) else {
        return Err(NoRoute::Outside.into());
    };
    if !codex_alab_core::active() {
        return Err(Miss::Transient);
    }
    match (sid.to_str(), crate::locate::control_socket_dir()) {
        (Some(sid), Some(dir)) if crate::locate::valid_sid(sid) => {
            located(crate::locate::read_endpoint(&dir, sid))
        }
        // aterm names every tab by a canonical tab ID ([`crate::locate::valid_sid`]) and has
        // a rendezvous directory: nothing here names an aterm tab, and the environment does
        // not change.
        _ => Err(NoRoute::Outside.into()),
    }
}

/// The endpoint a graph entry's read found, or why none. A read the sandbox denies is
/// [`NoRoute::Unreachable`]. Any other failure is no answer yet: an aterm restart or
/// seamless update can rewrite the entry, and a run cannot tell an aterm gone from one
/// coming back. aterm also removes a tab's entry when the tab closes, so a run that
/// outlives its tab (a detached tmux or screen server, an orphaned run) tells nothing and
/// reads the entry again every 5 s for the wall, as a run whose aterm is gone does.
fn located(read: std::io::Result<Endpoint>) -> Result<Endpoint, Miss> {
    read.map_err(|error| {
        if denied(&error) {
            NoRoute::Unreachable.into()
        } else {
            Miss::Transient
        }
    })
}

/// Whether `error` is a denial (`EPERM` or `EACCES`), as a sandbox gives for a socket or
/// a file it forbids. The rendezvous directory and its socket are this user's own, so
/// nothing else denies them.
fn denied(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::PermissionDenied
}

/// Why [`Client::connect`] failed. A denied socket or token is [`NoRoute::Unreachable`].
/// Everything else is no answer yet. During an aterm seamless update, the predecessor
/// exits, then the successor rotates the token and binds (aterm
/// `DESIGN-control-socket-update-handoff.md`). In between, the socket refuses or is
/// missing, and the token beside it can be old, so aterm answers `ERR auth`. codex-alab
/// reconnects after such an update (CONTRACT, "Seamless update of aterm").
fn unconnected(error: Error) -> Miss {
    match error {
        Error::Io(io) if denied(&io) => NoRoute::Unreachable.into(),
        _ => Miss::Transient,
    }
}

/// Whether a reply's status line says this aterm serves no such request. That is any `OK`
/// answer: a probe answered for another contract, or a `harness policy` this reader cannot
/// take. It is also one of these refusals:
/// - the router's `ERR unknown verb …`;
/// - a verb's `ERR <verb> has no handler`;
/// - `ERR usage: …` for a form the verb does not know;
/// - the exec handoff's answers to a request it does not know (`ERR denied native
///   operation authority`, `ERR codex-exec-handoff unsupported request`).
///
/// Any other refusal says nothing about what aterm serves: a dialog aterm is showing, a
/// main-thread stall or dropped reply, an event loop gone, a busy aterm, or a refusal this
/// reader does not know. It is no answer yet.
pub(crate) fn not_served(status: &str) -> bool {
    let status = status.trim();
    status == "OK"
        || status.starts_with("OK ")
        || status.starts_with("ERR unknown verb")
        || status.starts_with("ERR usage:")
        || (status.starts_with("ERR ") && status.ends_with(" has no handler"))
        || matches!(
            status,
            "ERR denied native operation authority" | "ERR codex-exec-handoff unsupported request"
        )
}

/// [`reach`] this run's tab.
pub async fn reach_self() -> Result<Client, Miss> {
    reach(locate()?, Options::default()).await
}

/// [`admit`] this run in its tab, as [`leads_foreground`] finds it, at a wall of `wall`.
pub async fn admit_self(wall: &WallKind) -> Result<Client, Miss> {
    admit(locate()?, leads_foreground(), Options::default(), wall).await
}

/// Connect to `endpoint` and check that its host moves exec runs here. That needs `harness
/// policy`, exec contract 3, exec tree contract 1, an `effective=on` policy and the
/// rotation keys. No claim is sent. This is the check every reconnect of an operation
/// makes.
pub async fn reach(endpoint: Endpoint, options: Options) -> Result<Client, Miss> {
    let client = Client::connect(endpoint, options)
        .await
        .map_err(unconnected)?;
    let policy = match client.harness_policy().await {
        Err(_) => return Err(Miss::Transient),
        Ok(PolicyAnswer::Unsupported(line)) if not_served(&line) => {
            return Err(NoRoute::NoExecContract.into());
        }
        // A policy read that failed: aterm answers it through its main thread, which
        // refuses while a dialog shows, while it stalls, or when its reply is dropped.
        Ok(PolicyAnswer::Unsupported(_)) => return Err(Miss::Transient),
        Ok(PolicyAnswer::Supported(policy)) => policy,
    };
    for served in [
        exec_grant::supported(&client).await,
        exec_grant::tree::supported(&client).await,
    ] {
        match served {
            Ok(true) => {}
            Ok(false) => return Err(NoRoute::NoExecContract.into()),
            Err(_) => return Err(Miss::Transient),
        }
    }
    if !policy.effective {
        return Err(Off::read(&policy.why));
    }
    if !account_recovery::policy_allows(&policy, Purpose::WeeklyQuota) {
        return Err(NoRoute::RotationOff.into());
    }
    Ok(client)
}

/// Admit this run for a move from a wall of `wall` under the claim admission, aterm's only one
/// today. A run that does not lead its foreground group sends nothing at all, not even a
/// connection. A leader is [`reach`]ed. A host whose policy does not move an exec run at such a
/// wall ([`account_recovery::leaves_exec_wall`]) is sent no claim and is
/// [`NoRoute::RotationOff`]: an ordinary wall's policy is the one [`reach`] checked, and a
/// workspace's wall needs `codex_rotation_refusal=1` too, which no aterm prints yet (gap 4, step
/// S3). Otherwise the leader claims as before. An unproven claim is given back
/// ([`Client::claim`]) and is [`NoRoute::Unproven`]. A claim refused for another holder's
/// is judged by that holder's claim ([`claimed_by_another`]).
pub async fn admit(
    endpoint: Endpoint,
    leads: bool,
    options: Options,
    wall: &WallKind,
) -> Result<Client, Miss> {
    if !leads {
        return Err(NoRoute::NotLeader.into());
    }
    let client = reach(endpoint, options).await?;
    if !client
        .policy()
        .is_some_and(|policy| account_recovery::leaves_exec_wall(&policy, wall))
    {
        client.close();
        return Err(NoRoute::RotationOff.into());
    }
    match client.claim().await {
        Ok(ClaimOutcome::Held { .. }) if client.holds_claim() => Ok(client),
        Ok(ClaimOutcome::Unproven) => Err(NoRoute::Unproven.into()),
        Ok(ClaimOutcome::Busy { .. }) => Err(claimed_by_another(&client).await),
        _ => Err(Miss::Transient),
    }
}

/// Why a standing claim (`ERR busy supervisor=`) kept this run's claim out, read off
/// `status`. aterm keeps a proven claim only while its holder leads the tab's foreground
/// group: its status sweep releases one whose holder left it. So another holder's proven
/// claim says that this run leads some other terminal, and that does not change before
/// another wall ([`NoRoute::AnotherLeads`]). Everything else is no answer yet:
/// - this run's own proven claim, which an unproven claim of this run's never replaces (its
///   ask before, whose connection's close aterm has not read, met by a run that left the
///   front since): the sweep releases it, or the run leads again;
/// - an unproven claim, which stands until it lapses or its holder gives it back. A driver's
///   `ttl=` claim lapses at its ttl; one set without `ttl=` (an older client's) is bound to
///   its connection with no expiry, so a leader asks again every 5 s, silently, for as long
///   as that connection stands;
/// - a status that cannot be read.
async fn claimed_by_another(client: &Client) -> Miss {
    match client.status().await {
        Ok(status)
            if status.supervisor_proven == Some(true)
                && status
                    .supervisor
                    .as_deref()
                    .is_some_and(|holder| holder != client.holder()) =>
        {
            NoRoute::AnotherLeads.into()
        }
        _ => Miss::Transient,
    }
}

#[cfg(test)]
#[path = "exec_route_tests.rs"]
mod tests;
