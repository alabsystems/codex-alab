// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The claim (CONTRACT §2-§3): one per process, over the alab-aterm client.
//!
//! - Nothing happens until the TUI registered its primary thread (never during onboarding or
//!   login, which a person answers, nor at the folder gate, which the TUI hook answers first
//!   without a claim) and this is the codex-alab binary inside an aterm session.
//! - The claim is taken only after `harness policy` answered `OK effective=on` (proof of a
//!   new-enough aterm), connection-bound (no `ttl=`), and kept only while aterm shows it
//!   proven. An old aterm (`ERR unknown verb`) is never claimed.
//! - `subscribe @self` runs on the client's second connection; a `meta-change
//!   field=supervisor` not naming codex-alab makes it passive at once, a `harness-policy`
//!   event re-reads the policy.
//! - An off is met by its kind (CONTRACT §2 as decided 2026-10-17, replacing b24e535df's
//!   "passive keeps the claim for any reason"):
//!   - TRANSIENT offs keep the proven claim and act on nothing: a person's hold (as always:
//!     every act waits on `status hold=`), `why=suspended` (aterm suspends its host at every
//!     seamless-handoff commit: a successor adopts the claim, a failed commit resumes it) and
//!     a policy read that failed (`State::standby` for the last two, which also clears
//!     codex-alab's attention, withdraws its description and answers no box). A hold read
//!     after a standby lifts the standby: the hold's own rule (acts wait) is the one left;
//!   - a PERSON'S or the CONFIG'S off gives the claim back (`meta unset supervisor`, as
//!     before b24e535df): `autopilot-off`, `agent-off`, `disabled`, `headless`, `unhosted`,
//!     and any word this build does not know. aterm's own host gives no relaunch, stall or
//!     memory remedy for those reasons anyway, and a kept claim had side effects (status
//!     showed it proven under Autopilot Off, so `aterm drive task` typed mail into a
//!     hands-off session, and other supervisors met `ERR busy`). codex-alab is passive the
//!     moment it decides to give the claim back, before the release is answered.
//!
//!   While passive for any reason codex-alab keeps watching on its own connection and claims
//!   (or acts) again as soon as the policy reads `effective=on`: it reads the policy again at
//!   a `harness-policy` event, at a `meta` or `hold` event that may tell an Autopilot word or
//!   a hold (aterm moves `effective=` for those with no policy event), after lost events (a
//!   `GAP`), and every [`POLICY_RECHECK`] while passive, which is the one clock here: aterm
//!   announces a suspension with no event at all, and a session may start with
//!   `effective=off`. The clock never runs while codex-alab acts, nor while what it waits for
//!   has an event of its own: another holder's claim (`ERR busy`: its release is a `meta`
//!   event), a claim someone cleared, or one aterm did not prove (the policy must move).
//!   Acting resumes a grace later. A claim someone else cleared (a person's `meta unset
//!   supervisor`, aterm's own clear) stays given up: no new claim until the policy moves
//!   (`harness-policy`) or aterm is connected again, whichever of the claim's watch and an
//!   act's checks meets it first. A `value=-` met while unclaimed takes nothing (only one
//!   that ends another holder's claim is a moment to claim). The `value=-` echo of
//!   codex-alab's own release is not a clear: one met after codex-alab claimed again (a fast
//!   off and on) stands when `status` shows the claim proven and its own and no re-confirm
//!   went since that claim (a same-holder set takes back a claim someone just cleared), and a
//!   straddled re-confirm (below) that codex-alab's own release explains gives the claim back
//!   without counting it cleared.
//! - Mail typed into the session (`Inbox: task @<off>`) is read while the proven claim is
//!   held, on standby too (aterm types the line; reading it is not codex-alab's act, and unread
//!   it would reach the model bare). A mail turn's verdict and its owed receipt and report are
//!   recorded at its end whatever codex-alab's state; the receipt and the report are SENT only
//!   while codex-alab acts (the claim held and not on standby): at the turn's end, or deferred
//!   until it acts again. Description writes go only while it acts; `harness_act` records go
//!   while the claim is held (an act that went is recorded though a standby came since).
//! - The exit path gives the claim back once the record is ended ([`Shared::release_claim`],
//!   CONTRACT §4), and nothing is claimed after it.
//! - A request connection that stopped serving (an unanswered call closes it) ends the
//!   session: the claim bound to it is gone, and the client connects again.
//! - Before every act ([`Supervisor::ready`]): the TUI's half (its own input stamp, draft,
//!   queued input, popups, a pending user turn), aterm's half (`status`: `hold`, `hand`,
//!   `input`, `human_ms`, the proven claim), the grace after a claim transition, the claim
//!   re-confirmed in the same tick, and, last and under the lock, that codex-alab still acts
//!   (an off read while the status read or the re-confirm was in flight goes no further).
//! - A box the TUI shows that codex-alab has not answered within the grace raises
//!   `meta set attention owner=codex-alab <kind>: <why>`; it is cleared when the box closes.
//!
//! Every aterm call is bounded by the client's 5 s timeout and runs on a background task;
//! none is awaited inside a lifecycle callback.

use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::Weak;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

use codex_alab_aterm::ClaimOutcome;
use codex_alab_aterm::Client;
use codex_alab_aterm::Endpoint;
use codex_alab_aterm::Error as AtermError;
use codex_alab_aterm::Event;
use codex_alab_aterm::Options as ClientOptions;
use codex_alab_aterm::PolicyAnswer;
use codex_alab_aterm::Signal;
use codex_alab_aterm::claim_signal;
use codex_alab_aterm::status::HumanInput;
use codex_alab_aterm::status::NotReady;
use codex_alab_aterm::status::Status;
use codex_alab_core::mail::Verdict;
use codex_alab_core::session::Autopilot as AutopilotState;
use codex_alab_core::session::BlockedKind;
use codex_alab_core::session::HumanSeen;
use codex_alab_core::session::LaunchWitness;
use codex_alab_core::session::ProgramState;
use codex_alab_core::session::Shared;
use codex_alab_core::session::TerminalContext;
use codex_alab_core::session::now_ms;
use tokio::sync::watch;

use crate::answer;
use crate::answer::BoxDecision;
use crate::decide::Policy;
use crate::story;

/// The `why=` word `harness policy` gives for a person's hold.
const HOLD_WHY: &str = "hold";

/// The `why=` word for a host suspended for a seamless handoff (aterm resumes it, or the
/// handoff's successor takes over).
const SUSPENDED_WHY: &str = "suspended";

/// How often the policy is read again while codex-alab is passive for any reason (released,
/// on standby, held by a hold, or never claimed): aterm announces a suspension with no event,
/// and a session may start with `effective=off`. Never while it acts: then an event moves it.
pub const POLICY_RECHECK: Duration = Duration::from_secs(30);

/// How an `effective=off` is met (CONTRACT §2 as decided 2026-10-17).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Off {
    /// A person's hold: the claim is kept, every act waits on `status hold=`, and attention,
    /// the description and mail stand as before.
    Hold,
    /// A transient off (a suspension): the claim is kept and nothing acts under it
    /// (`State::standby`). A policy read that failed is met the same way.
    Standby,
    /// A person's or the config's off: the claim is given back, and taken again at
    /// `effective=on`. aterm's host gives no relaunch, stall or memory remedy for these.
    Release,
}

/// How the off `why` is met. A word this build does not know is a release: nothing types,
/// and nothing claims to be watching a session aterm may not watch.
pub fn off_kind(why: &str) -> Off {
    match why {
        HOLD_WHY => Off::Hold,
        SUSPENDED_WHY => Off::Standby,
        _ => Off::Release,
    }
}

/// The act a mail turn's receipt and report are recorded as (`harness_act`).
pub const RULE_MAIL_REPORT: &str = "mail-report@v1";

/// How soon a wait with no deadline of its own (a hold, a draft, a popup, an unreadable
/// status) looks again.
pub const RECHECK: Duration = Duration::from_secs(60);

/// How soon a wait on aterm's own short hand looks again: the lease its harness holds while
/// it types a relaunch line ends seconds later, and a carry-on owed behind it should not
/// wait a full [`RECHECK`] (end-to-end run 2026-10-16: the carry-on came a minute late).
pub const HAND_RECHECK: Duration = Duration::from_secs(5);

/// How soon receipts and reports still owed are sent again after a send failed (aterm slow or
/// unreachable): with no mail frame, claim or resume to come, nothing else would, and a
/// manager in `post --wait-ack` would wait on a quiet inbox for good (review of 7e7622ff7,
/// finding 5).
pub const OWED_RETRY: Duration = Duration::from_secs(15);

/// The reconnect ladder after aterm went away (a seamless update, a restart).
const RECONNECT: [Duration; 5] = [
    Duration::from_secs(1),
    Duration::from_secs(5),
    Duration::from_secs(15),
    Duration::from_secs(30),
    Duration::from_secs(60),
];

/// The clock every timing here reads: tokio's (so a test can pause it), as a std `Instant`.
pub fn clock() -> Instant {
    tokio::time::Instant::now().into_std()
}

/// Where this process's aterm session is (`Endpoint::from_env` in the codex binary).
pub type EndpointSource = Arc<dyn Fn() -> Option<Endpoint> + Send + Sync>;

/// The supervisor's options.
#[derive(Clone)]
pub struct SupervisorOptions {
    /// The TUI's shared state (the codex binary's one copy, or a test's own).
    pub shared: &'static Shared,
    pub endpoint: EndpointSource,
    pub client: ClientOptions,
}

impl SupervisorOptions {
    /// The codex binary's: this process's shared state and `ATERM_PARENT_SESSION_ID`.
    pub fn from_env() -> SupervisorOptions {
        SupervisorOptions {
            shared: codex_alab_core::session::shared(),
            endpoint: Arc::new(Endpoint::from_env),
            client: ClientOptions::default(),
        }
    }
}

/// What the before-every-act checks said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ready {
    Go,
    /// Not now: look again at `until`.
    Wait {
        until: Instant,
        why: String,
    },
    /// No proven claim is held (or it was just lost): nothing at all.
    Passive(String),
}

#[derive(Default)]
struct State {
    client: Option<Client>,
    policy: Option<Policy>,
    /// A proven claim is held (acting or not).
    held: bool,
    /// Passive with the claim kept: why (`effective=off why=suspended`, or a policy aterm did
    /// not answer, [`Off::Standby`]). While set, nothing acts, no attention is raised, no
    /// description is written, no box is answered and no mail receipt or report is sent (a
    /// mail turn's end records what it owes); the claim, its record renewals, typed mail's
    /// reading and act records stand.
    standby: Option<String>,
    /// The last policy read said `why=hold` (the claim kept, every act waiting on `status
    /// hold=`): the policy is read again on the passive clock too.
    hold: bool,
    /// When the policy was last read (the passive clock counts from it).
    policy_read: Option<Instant>,
    /// aterm answered `harness policy` with no supported reply on this connection: an old
    /// aterm, never claimed, and not read again until aterm is connected again.
    unsupported: bool,
    /// The last claim transition (taken, lost, or acting stopped or resumed).
    since: Option<Instant>,
    /// The policy generation a claim came back unproven at: not retried until it moves.
    unproven_gen: Option<u64>,
    /// The generation the last supported policy read said (the passive clock stays off for an
    /// unproven claim only while the policy still reads that generation).
    generation: Option<u64>,
    /// Someone else cleared the claim codex-alab held: not claimed again until the policy
    /// moves or aterm is connected again.
    cleared: bool,
    /// Another holder has the session (the last claim met its live claim, `ERR busy`, or the
    /// watch saw the claim go to it): taken again at that claim's own event (its release),
    /// never on the passive clock.
    busy: bool,
    /// Releases codex-alab sent of a claim aterm showed its own (it held it, or aterm had just
    /// said so), whose `value=-` echo this session's loop has not met yet: a `value=-` met
    /// while one is due and while the claim is held again is checked against `status` (and the
    /// re-confirms since the claim) before it counts as a clear. A release that does nothing at
    /// aterm (the claim already gone) is not counted, and one that fails is taken back: a count
    /// no echo consumes would let a later person's clear pass for an echo (review of 7e7622ff7,
    /// finding 2).
    echoes_due: u64,
    attention: BTreeMap<&'static str, String>,
    shown: Option<String>,
    /// Why the last approval box was left to a person.
    last_defer: Option<String>,
    /// When to look at the open question or plan box again.
    box_look: Option<Instant>,
    /// The answer posted for the open box, until the TUI took or refused it.
    posted: Option<Posted>,
    /// The record's resume-line count (`Shared::resume_lines`) aterm's copy has: read at the
    /// claim, then renewed by a re-confirm whenever the record writer moves it.
    confirmed_line: u64,
    /// The description this process last wrote (`meta set description`), one write per
    /// change: the one standing text that is codex-alab's to replace or withdraw.
    described: Option<String>,
    /// A predecessor's description (a crash or a restore carried it) was looked at once.
    carried_checked: bool,
    /// This process's verdict on each mail row it answered (bounded): a row it handled is
    /// never run again, one it deferred may be.
    mail_verdicts: BTreeMap<u64, Verdict>,
    /// Receipts given at a mail turn's end and not yet sent: each waits until codex-alab acts
    /// and no older unhandled task or ask stands below it
    /// (`codex_alab_aterm::mail::receipts_due`).
    receipts_owed: BTreeMap<u64, Verdict>,
    /// Reports owed to mail turns' senders and not yet posted (by row id): each goes once
    /// codex-alab acts.
    reports_owed: BTreeMap<u64, OwedReport>,
    journal: VecDeque<String>,
}

impl State {
    /// Whether codex-alab acts: a proven claim is held and it is not passive under it.
    fn acting(&self) -> bool {
        self.held && self.standby.is_none()
    }
}

/// How many mail verdicts a process keeps (the oldest go first).
const MAIL_VERDICTS_KEPT: usize = 1024;

/// A mail turn's report, owed to its sender until codex-alab acts.
#[derive(Debug, Clone)]
struct OwedReport {
    turn: codex_alab_core::mail::MailTurn,
    verdict: Verdict,
    last_message: Option<String>,
}

/// An answer handed to the TUI's box glue.
#[derive(Debug, Clone)]
struct Posted {
    id: String,
    rule: &'static str,
    words: String,
    at: Instant,
}

/// The process's claim and its watch.
pub struct Supervisor {
    /// Itself, for the tasks it starts (the badge reader).
    me: Weak<Supervisor>,
    opts: SupervisorOptions,
    state: Mutex<State>,
    /// A badge read is running on its own task; another is wanted after it.
    badge_busy: AtomicBool,
    badge_again: AtomicBool,
    started: AtomicBool,
    /// The process is leaving: the claim was given back and is never taken again.
    closing: AtomicBool,
    /// The passive clock's period ([`POLICY_RECHECK`]; tests shorten it), in milliseconds.
    recheck_ms: AtomicU64,
    /// How soon a failed send of owed receipts and reports is tried again ([`OWED_RETRY`];
    /// tests shorten it), in milliseconds.
    owed_retry_ms: AtomicU64,
    /// A retry of the owed sends waits on its timer.
    owed_retry_armed: AtomicBool,
    /// A sign-in attention a signed-out start left standing was looked for (once a process, on
    /// its first session).
    login_looked: AtomicBool,
    /// Attention syncs run one at a time: each compares what it wants with what the last one
    /// showed, so a standby's or a release's clear never compares against a raise still in
    /// flight (it would send nothing, and the raise would then land under the off).
    attention_sync: tokio::sync::Semaphore,
    /// Sends of owed receipts and reports run one at a time (a turn's end, the badge reader):
    /// a send that finds its turn's receipt or report taken by another waits for that one to
    /// finish, so when a turn's end returns, what it owed has gone.
    mail_sync: tokio::sync::Semaphore,
    /// Bumped by the event reader on every `meta-change field=supervisor` not naming
    /// codex-alab (and every gap): an act whose re-confirm straddled one does not go.
    lost_epoch: Arc<AtomicU64>,
    /// Bumped by every release codex-alab sends while aterm showed the claim its own (whose
    /// `value=-` echo then moves `lost_epoch` too): a straddled re-confirm a release of its own
    /// may explain gives the claim back without counting it someone else's clear.
    released: AtomicU64,
    claims: watch::Sender<u64>,
}

enum SessionEnd {
    Retry(Option<Duration>),
    Closing,
}

impl Supervisor {
    pub fn new(opts: SupervisorOptions) -> Arc<Supervisor> {
        let (claims, _) = watch::channel(0);
        Arc::new_cyclic(|me| Supervisor {
            me: me.clone(),
            opts,
            state: Mutex::new(State::default()),
            badge_busy: AtomicBool::new(false),
            badge_again: AtomicBool::new(false),
            started: AtomicBool::new(false),
            closing: AtomicBool::new(false),
            recheck_ms: AtomicU64::new(
                u64::try_from(POLICY_RECHECK.as_millis()).unwrap_or(u64::MAX),
            ),
            owed_retry_ms: AtomicU64::new(
                u64::try_from(OWED_RETRY.as_millis()).unwrap_or(u64::MAX),
            ),
            owed_retry_armed: AtomicBool::new(false),
            login_looked: AtomicBool::new(false),
            attention_sync: tokio::sync::Semaphore::new(1),
            mail_sync: tokio::sync::Semaphore::new(1),
            lost_epoch: Arc::new(AtomicU64::new(0)),
            released: AtomicU64::new(0),
            claims,
        })
    }

    pub fn shared(&self) -> &'static Shared {
        self.opts.shared
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// A line of what the supervisor did (kept for tests and `tracing`).
    pub fn note(&self, line: impl Into<String>) {
        let line = line.into();
        tracing::debug!(target: "codex_alab_autopilot", "{line}");
        let mut st = self.lock();
        if st.journal.len() >= 512 {
            st.journal.pop_front();
        }
        st.journal.push_back(line);
    }

    /// Everything noted so far.
    pub fn journal(&self) -> Vec<String> {
        self.lock().journal.iter().cloned().collect()
    }

    /// Whether a proven claim is held now.
    pub fn held(&self) -> bool {
        let st = self.lock();
        st.held && st.client.as_ref().is_some_and(Client::holds_claim)
    }

    /// Whether codex-alab acts now: a proven claim is held and it is not on standby under it.
    /// Read last before an act's irreversible step, after any await since its checks.
    pub fn acting(&self) -> bool {
        let st = self.lock();
        st.acting() && st.client.as_ref().is_some_and(Client::holds_claim)
    }

    /// Set the passive clock's period (default [`POLICY_RECHECK`]); before [`Supervisor::start`].
    pub fn set_policy_recheck(&self, every: Duration) {
        self.recheck_ms.store(
            u64::try_from(every.as_millis()).unwrap_or(u64::MAX).max(1),
            Ordering::SeqCst,
        );
    }

    /// Set how soon a failed send of owed receipts and reports is tried again (default
    /// [`OWED_RETRY`]).
    pub fn set_owed_retry(&self, after: Duration) {
        self.owed_retry_ms.store(
            u64::try_from(after.as_millis()).unwrap_or(u64::MAX).max(1),
            Ordering::SeqCst,
        );
    }

    /// Why codex-alab is passive under the proven claim it keeps (`None`: it acts, or no
    /// claim is held).
    pub fn standby(&self) -> Option<String> {
        let st = self.lock();
        if st.held { st.standby.clone() } else { None }
    }

    /// The policy, only while a proven claim is held and codex-alab acts under it.
    pub fn policy(&self) -> Option<Policy> {
        let st = self.lock();
        if st.acting() && st.client.as_ref().is_some_and(Client::holds_claim) {
            st.policy.clone()
        } else {
            None
        }
    }

    /// The claim's connection, while connected (tests drive the transitions on it).
    #[cfg(test)]
    pub(crate) fn client(&self) -> Option<Client> {
        self.lock().client.clone()
    }

    /// The event reader's lost-claim epoch, and how many releases codex-alab sent while aterm
    /// showed the claim its own (tests watch the straddle).
    #[cfg(test)]
    pub(crate) fn epochs(&self) -> (u64, u64) {
        (
            self.lost_epoch.load(Ordering::SeqCst),
            self.released.load(Ordering::SeqCst),
        )
    }

    /// The aterm session id (for the notices' text), while connected.
    pub fn sid(&self) -> Option<String> {
        self.lock()
            .client
            .as_ref()
            .map(|c| c.endpoint().sid.clone())
    }

    /// Whether this process runs inside an aterm session (the endpoint is known).
    pub fn in_aterm(&self) -> bool {
        (self.opts.endpoint)().is_some()
    }

    /// A receiver that moves each time a claim is taken (the runtimes look again).
    pub fn claims(&self) -> watch::Receiver<u64> {
        self.claims.subscribe()
    }

    /// Start the claim's task once, on the current tokio runtime.
    pub fn start(self: &Arc<Self>) {
        if tokio::runtime::Handle::try_current().is_err() {
            return;
        }
        if self.started.swap(true, Ordering::SeqCst) {
            return;
        }
        // Outside an aterm session (or outside the codex binary) nothing is ever started.
        if (self.opts.endpoint)().is_none() {
            self.note("no aterm session: the autopilot stays off");
            return;
        }
        let shared = self.opts.shared;
        self.install_release();
        self.install_context();
        self.install_mail();
        // The watch starts from the generation the channel holds: a frame the TUI observes
        // before the watch thread first runs (the primary thread's registration) is a change
        // it sends, not one it silently starts from while the claim's task waits for it.
        let first = shared.generation();
        let (tx, rx) = watch::channel(first);
        let _ = std::thread::Builder::new()
            .name("alab-autopilot-watch".to_string())
            .spawn(move || {
                let mut seen = first;
                loop {
                    seen = shared.wait_change(seen, Duration::from_secs(3600));
                    if tx.send(seen).is_err() {
                        return;
                    }
                }
            });
        tokio::spawn(Arc::clone(self).run(rx));
    }

    /// The exit path's release (CONTRACT §4): once the record is ended, the claim is given
    /// back on its own connection while the process still lives, and never taken again. The
    /// call waits for aterm's answer at most `within` (the release runs on this runtime, so
    /// the waiting thread must not be its only worker).
    fn install_release(self: &Arc<Self>) {
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let me = Arc::downgrade(self);
        let _ = self.opts.shared.set_release(Box::new(move |within| {
            let Some(me) = me.upgrade() else {
                return true;
            };
            me.closing.store(true, Ordering::SeqCst);
            // The description is withdrawn here, on the claim's connection, before the claim
            // goes: nothing keeps it true after this process, and a restore would carry it.
            let described = me.lock().described.take();
            let client = me.lock().client.clone().filter(Client::is_open);
            me.lost_locally("the process is leaving");
            let Some(client) = client else {
                return true;
            };
            let (tx, rx) = std::sync::mpsc::channel();
            runtime.spawn(async move {
                if let Some(text) = described {
                    let _ = withdraw_description(&client, &text).await;
                }
                let _ = tx.send(client.release().await);
            });
            match rx.recv_timeout(within) {
                Ok(Ok(())) => {
                    me.note("the claim was given back at the exit");
                    true
                }
                Ok(Err(e)) => {
                    me.note(format!("the claim's release at the exit failed: {e}"));
                    false
                }
                Err(_) => {
                    me.note("the claim's release at the exit was not answered in time");
                    false
                }
            }
        }));
    }

    /// `/ide` inside aterm: the TUI's submit path asks for this session's terminal context
    /// (`codex_alab_aterm::context`). It is read on a connection of its own, so a slow read
    /// never costs the claim its connection, only while the proven claim is held, and waited
    /// for at most the TUI's budget (the read runs on this runtime, so the waiting thread must
    /// not be its only worker).
    fn install_context(self: &Arc<Self>) {
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let me = Arc::downgrade(self);
        let _ = self.opts.shared.set_context_reader(Box::new(move |within| {
            let me = me.upgrade()?;
            if !me.held() {
                return None;
            }
            let endpoint = me.lock().client.as_ref()?.endpoint().clone();
            let options = ClientOptions {
                timeout: within,
                ..me.opts.client.clone()
            };
            let (tx, rx) = std::sync::mpsc::channel();
            runtime.spawn(async move {
                let read = async {
                    let client = Client::connect(endpoint, options).await?;
                    let gathered = codex_alab_aterm::context::gather(&client).await;
                    client.close();
                    gathered
                };
                let _ = tx.send(tokio::time::timeout(within, read).await);
            });
            let gathered = match rx.recv_timeout(within) {
                Ok(Ok(Ok(gathered))) => gathered,
                other => {
                    me.note(format!("terminal context unread: {other:?}"));
                    return None;
                }
            };
            Some(match codex_alab_aterm::context::render(&gathered) {
                Some((prompt, summary)) => TerminalContext { prompt, summary },
                None => TerminalContext {
                    prompt: String::new(),
                    summary: "nothing to attach".to_string(),
                },
            })
        }));
    }

    /// The native inbox (CONTRACT §5 "inbox"): the TUI's submit path asks for the mail a typed
    /// `Inbox: <kind> @<off>` names. Read on a connection of its own (a slow read never costs
    /// the claim its connection), only while the proven claim is held, and waited for at most
    /// the TUI's budget. A standby reads it too: aterm's `drive task` types the line whenever
    /// it shows the claim proven, and a line left unread would reach the model bare, with no
    /// fence and no receipt (review of fea034d20, finding 2); its receipt and report wait
    /// until codex-alab acts again ([`Supervisor::receipt`]).
    fn install_mail(self: &Arc<Self>) {
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let me = Arc::downgrade(self);
        let _ = self
            .opts
            .shared
            .set_mail_reader(Box::new(move |kind, off, within| {
                let me = me.upgrade().ok_or("the supervisor is gone")?;
                if !me.held() {
                    return Err("no proven aterm claim".to_string());
                }
                let endpoint = me
                    .lock()
                    .client
                    .as_ref()
                    .map(|c| c.endpoint().clone())
                    .ok_or("no aterm connection")?;
                let options = ClientOptions {
                    timeout: within,
                    ..me.opts.client.clone()
                };
                // This process's own verdicts: a mail it handled is never run again.
                let verdicts = me.lock().mail_verdicts.clone();
                let given = move |id: u64| verdicts.get(&id).copied();
                let (tx, rx) = std::sync::mpsc::channel();
                runtime.spawn(async move {
                    let read = async {
                        let client = Client::connect(endpoint, options).await?;
                        let mail = codex_alab_aterm::mail::read(&client, kind, off, &given).await;
                        client.close();
                        mail
                    };
                    let _ = tx.send(tokio::time::timeout(within, read).await);
                });
                let read = match rx.recv_timeout(within) {
                    Ok(Ok(Ok(mail))) => Ok(mail),
                    Ok(Ok(Err(e))) => Err(e.to_string()),
                    Ok(Err(_)) | Err(_) => Err("aterm did not answer in time".to_string()),
                };
                match &read {
                    Ok(mail) => me.note(format!(
                        "mail @{off}: {} from {} trust={}",
                        mail.kind, mail.from, mail.trust
                    )),
                    Err(e) => me.note(format!("mail @{off} unread: {e}")),
                }
                read
            }));
    }

    /// A mail turn ended, as the extension sees it (before its receipt task runs): this
    /// process's verdict on the row, and the receipt owed for it, recorded at once, so the
    /// same line typed again is refused even before the receipt is sent. Recorded whatever
    /// codex-alab's state: only the sending waits until it acts (review of fea034d20, finding
    /// 1: a turn that ended on standby or after a release lost its receipt for good, and every
    /// later receipt waited behind its row).
    pub fn hold_receipt(&self, id: u64, verdict: Verdict) {
        let mut st = self.lock();
        st.mail_verdicts.insert(id, verdict);
        while st.mail_verdicts.len() > MAIL_VERDICTS_KEPT {
            st.mail_verdicts.pop_first();
        }
        st.receipts_owed.insert(id, verdict);
        while st.receipts_owed.len() > MAIL_VERDICTS_KEPT {
            st.receipts_owed.pop_first();
        }
    }

    /// A mail turn ended (CONTRACT §5 "inbox"): the receipt `inbox seen <id> <verdict>` (what
    /// the manager's `post --wait-ack` waits for), then the stamped report to the sender with
    /// the turn's last agent message. Both are recorded as owed at once; they are sent only
    /// while codex-alab acts under its proven claim (never merely while it holds one), on a
    /// connection of its own: now, or, when it does not act now (a standby, a release, a
    /// claim someone cleared), once it acts again ([`Supervisor::refresh_badge`] at the resume
    /// and at a new claim). A send that fails is noted and stops there: what it did not send
    /// stays owed and is tried again [`OWED_RETRY`] later (a receipt or report aterm refused
    /// is dropped, noted).
    ///
    /// `inbox seen <id>` marks every row up to `id` handled, so the receipt waits while an
    /// older task or ask nobody has handled stands below it (raising `mail:` attention), and
    /// goes once that one is ([`Supervisor::flush_receipts`]); the report goes at once. A
    /// report goes only to an address aterm's bridge routes ([`codex_alab_aterm::mail::reply_to`]:
    /// a session sender as `@s-…`); the local `post` answers `OK` either way.
    pub async fn receipt(
        &self,
        turn: codex_alab_core::mail::MailTurn,
        verdict: Verdict,
        last_message: Option<String>,
    ) {
        let off = turn.off;
        // The turn's end already held this verdict (the extension holds it first); held again
        // here, a receipt a flush sent meanwhile would be owed, and sent, a second time.
        let held = self.lock().mail_verdicts.get(&turn.id) == Some(&verdict);
        if !held {
            self.hold_receipt(turn.id, verdict);
        }
        {
            let mut st = self.lock();
            st.reports_owed.insert(
                turn.id,
                OwedReport {
                    turn,
                    verdict,
                    last_message,
                },
            );
            while st.reports_owed.len() > MAIL_VERDICTS_KEPT {
                st.reports_owed.pop_first();
            }
        }
        let Some(endpoint) = self.acting_endpoint() else {
            self.note(format!(
                "mail @{off}: codex-alab does not act under a proven claim at the turn's end: \
                 its receipt and report wait until it acts again"
            ));
            return;
        };
        let sent = async {
            let client = Client::connect(endpoint, self.opts.client.clone()).await?;
            let sent = self.send_owed(&client).await;
            client.close();
            sent
        };
        if let Err(e) = sent.await {
            self.note(format!(
                "mail @{off}: receipt or report failed: {e}; still owed, tried again"
            ));
            self.retry_owed();
        }
    }

    /// A send of owed receipts and reports failed: while anything is still owed, look again
    /// [`OWED_RETRY`] later (one timer at a time). The badge reader sends them, only while
    /// codex-alab acts; a resume or a new claim sends them anyway.
    fn retry_owed(&self) {
        let owed = {
            let st = self.lock();
            !st.receipts_owed.is_empty() || !st.reports_owed.is_empty()
        };
        if !owed || self.closing() || self.owed_retry_armed.swap(true, Ordering::SeqCst) {
            return;
        }
        let me = match self.me.upgrade() {
            Some(me) if tokio::runtime::Handle::try_current().is_ok() => me,
            _ => {
                self.owed_retry_armed.store(false, Ordering::SeqCst);
                return;
            }
        };
        let after = Duration::from_millis(self.owed_retry_ms.load(Ordering::SeqCst));
        tokio::spawn(async move {
            tokio::time::sleep(after).await;
            me.owed_retry_armed.store(false, Ordering::SeqCst);
            if !me.closing() {
                me.refresh_badge();
            }
        });
    }

    /// The claim's endpoint while codex-alab acts under its proven claim (`None` otherwise).
    fn acting_endpoint(&self) -> Option<Endpoint> {
        let st = self.lock();
        match &st.client {
            Some(client) if st.acting() && client.holds_claim() => Some(client.endpoint().clone()),
            _ => None,
        }
    }

    /// Send what mail turns' ends left owed, on `client` (a connection of its own, never the
    /// claim's), only while codex-alab acts: the receipts no older unhandled row holds back
    /// ([`Supervisor::flush_receipts`]), then each owed report, recorded as its act. Sends run
    /// one at a time (`mail_sync`), and a report is taken out of the owed set before it is
    /// sent, so it is never posted twice; one whose post failed (not refused) is owed again
    /// and the send stops there (review of 7e7622ff7, finding 5). Whether codex-alab acts is
    /// read under the lock that takes each one out (finding 7). The stamp is read when the
    /// first report goes, so a report popped by any send carries one (finding 6): the screen's
    /// at the send, which for a report owed across a passive spell is not the turn's end.
    async fn send_owed(&self, client: &Client) -> Result<(), AtermError> {
        // One permit, never closed: an error is impossible, and would only mean no wait.
        let _one = self.mail_sync.acquire().await;
        if !self.acting() {
            return Ok(());
        }
        let waits = self.flush_receipts(client).await?;
        let mut stamp: Option<Option<String>> = None;
        loop {
            let popped = {
                let mut st = self.lock();
                let acting = st.acting() && st.client.as_ref().is_some_and(Client::holds_claim);
                if !acting {
                    return Ok(());
                }
                st.reports_owed.pop_first()
            };
            let Some((id, owed)) = popped else {
                return Ok(());
            };
            let OwedReport {
                turn,
                verdict,
                last_message,
            } = &owed;
            let receipt = match &waits {
                Some((held, row)) if *held <= turn.id && self.receipt_owed(turn.id) => format!(
                    "its receipt waits behind the unhandled {} @{}",
                    row.kind, row.off
                ),
                _ if self.receipt_owed(turn.id) => "its receipt is still owed".to_string(),
                _ => "receipt sent".to_string(),
            };
            let words = match codex_alab_aterm::mail::reply_to(&turn.from) {
                None => format!(
                    "{receipt}; no report: aterm's bridge has no route to {}",
                    turn.from
                ),
                Some(to) => {
                    if stamp.is_none() {
                        stamp = Some(codex_alab_aterm::mail::stamp(client).await.ok().flatten());
                    }
                    let body = codex_alab_aterm::mail::report_body(
                        stamp.as_ref().and_then(Option::as_deref),
                        *verdict,
                        last_message.as_deref(),
                    );
                    match codex_alab_aterm::mail::report(client, &to, turn.off, &body).await {
                        Ok(answer) => format!("{receipt}; report posted to {to} ({answer})"),
                        Err(e @ (AtermError::Refused(_) | AtermError::BadRequest(_))) => {
                            self.note(format!(
                                "mail @{}: {receipt}; the report was refused: {e}: dropped",
                                turn.off
                            ));
                            continue;
                        }
                        Err(e) => {
                            self.note(format!(
                                "mail @{}: {receipt}; the report failed: {e}: still owed",
                                turn.off
                            ));
                            self.lock().reports_owed.entry(id).or_insert(owed);
                            return Err(e);
                        }
                    }
                }
            };
            self.record_act(
                RULE_MAIL_REPORT,
                &format!(
                    "{} the {} @{} from {}: {words}",
                    verdict.word(),
                    turn.kind,
                    turn.off,
                    turn.from
                ),
            )
            .await;
        }
    }

    fn receipt_owed(&self, id: u64) -> bool {
        self.lock().receipts_owed.contains_key(&id)
    }

    /// Send every owed receipt no older unhandled task or ask stands below, in id order, on
    /// `client` (a connection of its own, never the claim's). Answers the first receipt that
    /// still waits and the row it waits behind; `mail:` attention says so while one does. A
    /// receipt aterm refuses (its row is gone from the ring) is dropped and noted.
    async fn flush_receipts(
        &self,
        client: &Client,
    ) -> Result<Option<codex_alab_aterm::mail::Held>, AtermError> {
        let owed = self.lock().receipts_owed.clone();
        if owed.is_empty() {
            // Nothing waits (a concurrent flush may have said otherwise from an older read).
            self.attention("mail", None).await;
            return Ok(None);
        }
        let (seen, rows) = codex_alab_aterm::mail::rows(client).await?;
        let (due, waits) = codex_alab_aterm::mail::receipts_due(seen, &rows, &owed);
        for (id, verdict) in due {
            // Taken out of the owed set before it is sent: a turn's end and the badge reader
            // both flush, and a receipt another flush already took is not sent twice. Under the
            // same lock, codex-alab still acts: a standby or a release that came since the
            // rows were read leaves the rest owed (review of 7e7622ff7, finding 7).
            {
                let mut st = self.lock();
                let acting = st.acting() && st.client.as_ref().is_some_and(Client::holds_claim);
                if !acting {
                    return Ok(None);
                }
                if st.receipts_owed.get(&id) != Some(&verdict) {
                    continue;
                }
                st.receipts_owed.remove(&id);
            }
            match codex_alab_aterm::mail::seen(client, id, verdict).await {
                Ok(()) => {}
                Err(AtermError::Refused(why)) => {
                    self.note(format!("mail row {id}: receipt refused ({why}): dropped"));
                }
                Err(e) => {
                    // Still owed (unless a newer verdict replaced it meanwhile).
                    self.lock().receipts_owed.entry(id).or_insert(verdict);
                    return Err(e);
                }
            }
        }
        let text = waits.as_ref().map(|(held, row)| {
            let off = rows
                .iter()
                .find(|r| r.id == *held)
                .map_or_else(|| format!("row {held}"), |r| format!("@{}", r.off));
            format!(
                "mail: the receipt for {off} waits behind the unhandled {} @{}",
                row.kind, row.off
            )
        });
        if let Some(text) = &text {
            self.note(text.clone());
        }
        self.attention("mail", text).await;
        Ok(waits)
    }

    /// The status line's mail badge (display only: nothing is typed, nothing wakes the
    /// agent): what waits above the handled watermark, read while the claim is held, and
    /// nothing otherwise. The read runs on a task and a connection of its own, never the
    /// claim's (a slow or failed exchange closes the connection it ran on, and the claim
    /// bound to it would go) and never inside the claim's watch, where a burst of mail would
    /// queue reads ahead of a lost claim. Reads asked for while one runs fold into one more.
    /// Receipts and reports still owed are looked at again with it, while codex-alab acts (an
    /// older row may have been handled; acting may have resumed, or a claim been taken).
    fn refresh_badge(&self) {
        self.badge_again.store(true, Ordering::SeqCst);
        if self.badge_busy.swap(true, Ordering::SeqCst) {
            return;
        }
        let Some(me) = self.me.upgrade() else {
            self.badge_busy.store(false, Ordering::SeqCst);
            return;
        };
        if tokio::runtime::Handle::try_current().is_err() {
            self.badge_busy.store(false, Ordering::SeqCst);
            return;
        }
        tokio::spawn(async move {
            loop {
                while me.badge_again.swap(false, Ordering::SeqCst) {
                    me.read_badge().await;
                }
                me.badge_busy.store(false, Ordering::SeqCst);
                // One asked for between the last read and the store above runs now.
                if !me.badge_again.load(Ordering::SeqCst)
                    || me.badge_busy.swap(true, Ordering::SeqCst)
                {
                    return;
                }
            }
        });
    }

    async fn read_badge(&self) {
        let endpoint = {
            let st = self.lock();
            match &st.client {
                Some(client) if st.held && client.holds_claim() => Some(client.endpoint().clone()),
                _ => None,
            }
        };
        let Some(endpoint) = endpoint else {
            self.show_badge(None);
            return;
        };
        let read = async {
            let client = Client::connect(endpoint, self.opts.client.clone()).await?;
            // Receipts and reports go only while codex-alab acts (`send_owed` looks); the badge
            // is display and stands with the claim.
            if let Err(e) = self.send_owed(&client).await {
                self.note(format!(
                    "owed receipts or reports not sent: {e}; still owed, tried again"
                ));
                self.retry_owed();
            }
            let badge = codex_alab_aterm::mail::badge(&client).await;
            client.close();
            badge
        };
        let badge = match read.await {
            Ok(badge) => Some(badge),
            Err(e) => {
                self.note(format!("inbox unread for the badge: {e}"));
                // The connection may be what failed, before anything owed was sent.
                self.retry_owed();
                None
            }
        };
        // The claim may have gone while it was read: then no badge.
        self.show_badge(badge.filter(|_| self.held()));
    }

    fn show_badge(&self, badge: Option<codex_alab_core::mail::MailBadge>) {
        let shared = self.opts.shared;
        if shared.set_mail_badge(badge) {
            // A frame, so the status line shows it; the agent is not woken.
            let _ = shared.wake();
        }
    }

    /// Whether the process is leaving (nothing is claimed any more).
    pub fn closing(&self) -> bool {
        self.closing.load(Ordering::SeqCst)
    }

    async fn run(self: Arc<Self>, mut tui: watch::Receiver<u64>) {
        // The launch probe comes first, before the claim waits for a thread: aterm's relaunch
        // hand stands only until the new process's first idle.
        let mut first = self.probe_launch().await;
        while self.opts.shared.view().primary_thread.is_none() {
            if tui.changed().await.is_err() {
                return;
            }
        }
        let mut tries = 0usize;
        loop {
            if self.closing() {
                return;
            }
            let Some(endpoint) = (self.opts.endpoint)() else {
                self.note("no aterm session: the autopilot stays off");
                return;
            };
            self.opts.shared.set_autopilot(AutopilotState::Passive);
            let end = self
                .session(endpoint, &mut tui, &mut tries, first.take())
                .await;
            self.lost_locally("the aterm connection ended");
            match end {
                SessionEnd::Closing => return,
                SessionEnd::Retry(wait) => {
                    let wait = wait.unwrap_or(RECONNECT[tries.min(RECONNECT.len() - 1)]);
                    tries += 1;
                    tokio::time::sleep(wait).await;
                }
            }
        }
    }

    /// THE LAUNCH PROBE (CONTRACT §4, carry-on): connect, and read `@self status` once, as
    /// this process starts and before codex-alab does anything, into the shared state
    /// ([`Shared::set_launch_witness`]): whether aterm's harness holds its hand on the tab
    /// (aterm's crash relaunch types its line under it) and when a person last typed. The
    /// connection is the one the claim then uses (one connection, CONTRACT §0); none made or
    /// none read is no witness, and no carry-on.
    async fn probe_launch(&self) -> Option<Client> {
        let shared = self.opts.shared;
        let Some(endpoint) = (self.opts.endpoint)() else {
            shared.set_launch_witness(None);
            return None;
        };
        let client = match Client::connect(endpoint, self.opts.client.clone()).await {
            Ok(client) => client,
            Err(e) => {
                self.note(format!("launch probe: aterm unreachable: {e}"));
                shared.set_launch_witness(None);
                return None;
            }
        };
        match client.status().await {
            Ok(status) => {
                let witness = launch_witness(&status, now_ms(), client.server_pid());
                self.note(format!(
                    "launch probe: hand={} hand_pid={:?} human={:?} server_pid={}",
                    witness.hand.as_deref().unwrap_or("-"),
                    witness.hand_pid,
                    witness.human,
                    witness
                        .server_pid
                        .map_or_else(|| "-".to_string(), |pid| pid.to_string())
                ));
                shared.set_launch_witness(Some(witness));
            }
            Err(e) => {
                self.note(format!("launch probe: status unreadable: {e}"));
                shared.set_launch_witness(None);
            }
        }
        client.is_open().then_some(client)
    }

    async fn session(
        &self,
        endpoint: Endpoint,
        tui: &mut watch::Receiver<u64>,
        tries: &mut usize,
        probed: Option<Client>,
    ) -> SessionEnd {
        let connected = match probed.filter(Client::is_open) {
            Some(client) => Ok(client),
            None => Client::connect(endpoint, self.opts.client.clone()).await,
        };
        let client = match connected {
            Ok(client) => client,
            Err(AtermError::Busy { retry_ms, .. }) => {
                return SessionEnd::Retry(retry_ms.map(Duration::from_millis));
            }
            Err(e) => {
                self.note(format!("aterm unreachable: {e}"));
                return SessionEnd::Retry(None);
            }
        };
        let events = match client.subscribe().await {
            Ok(events) => events,
            Err(e) => {
                self.note(format!("subscribe refused: {e}"));
                client.close();
                return SessionEnd::Retry(None);
            }
        };
        *tries = 0;
        // A new connection is a new start: a claim someone cleared on the old one may be
        // taken again once the policy says so, and a newer aterm may answer the policy. No
        // echo of a release on the old stream comes on this one.
        {
            let mut st = self.lock();
            st.cleared = false;
            st.unsupported = false;
            st.busy = false;
            st.echoes_due = 0;
            // A successor aterm starts its policy cell again (at gen 0): an unproven claim at
            // the old one's generation says nothing of it (review of 7e7622ff7, finding 4).
            st.unproven_gen = None;
            st.generation = None;
        }
        self.clear_stale_login(&client).await;
        // The push stream is read on its own task: a line read is not cancel-safe, and the
        // loop below also wakes for the TUI and for box deadlines. The reader marks a lost
        // claim at once, so an act whose re-confirm straddled it does not go.
        let (events_tx, mut events_rx) = tokio::sync::mpsc::unbounded_channel();
        let lost_epoch = Arc::clone(&self.lost_epoch);
        let holder = client.holder().to_string();
        let reader = tokio::spawn(async move {
            let mut events = events;
            while let Some(event) = events.next().await {
                if matches!(claim_signal(&event, &holder), Signal::Lost { .. }) {
                    lost_epoch.fetch_add(1, Ordering::SeqCst);
                }
                if events_tx.send(event).is_err() {
                    return;
                }
            }
        });
        self.lock().client = Some(client.clone());
        self.refresh(&client).await;
        self.check_boxes(&client).await;
        let end = loop {
            let deadline = self.box_deadline();
            let recheck = self.policy_recheck_at();
            tokio::select! {
                event = events_rx.recv() => {
                    let Some(event) = event else {
                        break SessionEnd::Retry(None);
                    };
                    if self.closing() {
                        break SessionEnd::Closing;
                    }
                    if matches!(event, Event::Gap) {
                        self.after_gap(&client).await;
                        self.check_boxes(&client).await;
                        continue;
                    }
                    if matches!(event, Event::Mail { .. } | Event::MailGap | Event::InboxSeen) {
                        self.refresh_badge();
                        continue;
                    }
                    // aterm folds a hold and an Autopilot word into `effective=` without a
                    // `harness-policy` event: a `meta` or `hold` event that may tell one is the
                    // moment to look again, to go passive or to act again. A suspension has no
                    // event at all: the passive clock below sees it.
                    if moves_effective(&event) {
                        self.refresh(&client).await;
                        self.check_boxes(&client).await;
                        continue;
                    }
                    match claim_signal(&event, client.holder()) {
                        Signal::Lost { by } => {
                            // A `value=-` may be the echo of codex-alab's own release; one met
                            // while the claim is held again (a fast off and on: the word's
                            // event came ahead of the echo) is a clear unless `status` still
                            // shows the claim proven and ours (review of fea034d20, finding 3)
                            // and no re-confirm went since the claim: a same-holder set takes
                            // back a claim someone just cleared, and `status` then shows it
                            // ours too (review of 7e7622ff7, finding 2).
                            let (was_held, echo, busy) = {
                                let mut st = self.lock();
                                let echo = by.is_none() && st.echoes_due > 0;
                                if echo {
                                    st.echoes_due -= 1;
                                }
                                (st.held, echo, st.busy)
                            };
                            let echo = echo && was_held && {
                                let reconfirmed = client.reconfirmed_since_claim();
                                if reconfirmed {
                                    self.note(
                                        "a clear met after a re-confirm: whatever it echoes, it \
                                         counts as a clear",
                                    );
                                }
                                !reconfirmed
                            };
                            if echo && self.shown_ours(&client).await {
                                self.note(
                                    "the echo of codex-alab's own earlier release came after \
                                     it claimed again: the claim stands",
                                );
                            } else if was_held {
                                client.note_claim_lost();
                                // Someone else's clear stays given up until the policy moves;
                                // a claim another holder took is taken again at its release.
                                self.lost_locally_marking(
                                    format!(
                                        "the claim went to {}",
                                        by.as_deref().unwrap_or("nobody")
                                    ),
                                    |st| {
                                        if by.is_none() {
                                            st.cleared = true;
                                        } else {
                                            st.busy = true;
                                        }
                                    },
                                );
                                self.clear_attention(&client).await;
                                if by.is_none() {
                                    self.note("the claim was cleared: not taken again until the policy moves");
                                }
                            } else if by.is_some() {
                                // Another holder took the session while codex-alab held no
                                // claim: its release is the moment to claim.
                                self.lock().busy = true;
                            } else if busy {
                                // Another holder's claim went: codex-alab may claim now.
                                self.lock().busy = false;
                                self.refresh(&client).await;
                            } else {
                                // Nothing to take: the echo of codex-alab's own release, or a
                                // clear an act's checks met first (which stays given up). Only
                                // the policy moves it (review of 7e7622ff7, finding 1).
                                self.note("a clear met while unclaimed: nothing to take");
                            }
                        }
                        Signal::Ours => {
                            if !self.lock().held {
                                // A claim codex-alab does not count as held (a re-confirm
                                // that re-took one someone had just cleared): give it back.
                                // aterm just showed it ours, so its release has an echo.
                                let _ = self.give_back(&client, true).await;
                                self.note("gave back a claim taken while passive");
                            }
                        }
                        Signal::PolicyChanged { .. } => {
                            self.lock().cleared = false;
                            self.refresh(&client).await;
                            self.check_boxes(&client).await;
                        }
                        Signal::Ended => break SessionEnd::Closing,
                        Signal::Human | Signal::Nothing => {}
                    }
                }
                () = client.closed() => {
                    self.note("the aterm request connection stopped serving: connecting again");
                    break SessionEnd::Retry(None);
                }
                changed = tui.changed() => {
                    if changed.is_err() {
                        break SessionEnd::Closing;
                    }
                    self.renew_for_record(&client).await;
                    self.check_boxes(&client).await;
                }
                () = sleep_until_opt(deadline), if deadline.is_some() => {
                    self.check_boxes(&client).await;
                }
                () = sleep_until_opt(recheck), if recheck.is_some() => {
                    // The passive clock: no event says a suspension ended, or that a session
                    // that started off may now be claimed.
                    self.refresh(&client).await;
                    self.check_boxes(&client).await;
                }
            }
        };
        reader.abort();
        if self.closing() {
            // The exit's own release may still be on this connection: let it finish (the
            // connection ends with its last clone, or with the process).
            self.lock().client = None;
            return end;
        }
        if self.lock().held && client.is_open() {
            let _ = client.release().await;
        }
        client.close();
        self.lock().client = None;
        end
    }

    /// A sign-in attention a signed-out start left standing (a SIGTERM, a SIGKILL, a panic or
    /// a restore on the sign-in screen, which ends before its clear): this start is past the
    /// sign-in (its primary thread is registered), so it clears codex-alab's own entry while
    /// it reads exactly the sign-in screen's text, and nothing else
    /// (`codex_alab_tui::login::clear_stale`). Looked for once a process, on its first
    /// session, whatever the look met: a later reconnect never clears what this process raised
    /// since (review of fea034d20, finding 4).
    async fn clear_stale_login(&self, client: &Client) {
        if self.login_looked.swap(true, Ordering::SeqCst) {
            return;
        }
        match codex_alab_tui::login::clear_stale(client).await {
            Ok(true) => self.note("cleared the sign-in attention an earlier signed-out start left"),
            Ok(false) => {}
            Err(e) => self.note(format!("a stale sign-in attention was not looked for: {e}")),
        }
    }

    /// The record writer wrote a resume line aterm's copy does not have (the rollout came
    /// to disk after the claim, or a resume's first record came after it): renew the claim,
    /// so aterm re-reads the record now. Without it, a crash before codex-alab's first act
    /// leaves aterm a copy with no line to relaunch (end-to-end run 2026-10-16: "names no
    /// line aterm would relaunch") or none at all ("ended in order").
    async fn renew_for_record(&self, client: &Client) {
        let line = self.opts.shared.resume_lines();
        {
            let st = self.lock();
            if !st.held || st.confirmed_line == line || !client.holds_claim() {
                return;
            }
        }
        match client.reconfirm().await {
            Ok(true) => {
                self.lock().confirmed_line = line;
                self.note("renewed the claim: the record's resume line moved");
            }
            Ok(false) => self.note("the claim did not renew for the record"),
            Err(e) => self.note(format!("the claim did not renew for the record: {e}")),
        }
    }

    /// Events were lost (`Gap`): a claim still shown ours and proven stands, and the policy
    /// is read again (the lost events may have moved it); any other reading is a clear
    /// codex-alab cannot tell from a person's, so it stays given up.
    async fn after_gap(&self, client: &Client) {
        // A lost echo never comes: none is counted due across a gap (one that does come after
        // it counts as a clear, as before the count).
        self.lock().echoes_due = 0;
        if !self.lock().held {
            self.refresh(client).await;
            return;
        }
        match client.status().await {
            Ok(status) if status.proven_holder(client.holder()) => {
                self.note("events were lost; the claim still stands");
                self.refresh(client).await;
            }
            _ => {
                client.note_claim_lost();
                if client.is_open() {
                    let _ = client.release().await;
                }
                self.lost_locally("events were lost and the claim is not shown ours");
                self.lock().cleared = true;
                self.clear_attention(client).await;
            }
        }
    }

    /// Read the policy and take, keep or give back the claim by it (CONTRACT §2): `effective=on`
    /// takes the claim or acts again under it; an off is met by its kind ([`off_kind`]): a
    /// hold keeps the claim and halts the acts, a suspension or an unreadable policy keeps it
    /// on standby, and a person's or the config's off gives it back.
    async fn refresh(&self, client: &Client) {
        if self.closing() {
            return;
        }
        self.lock().policy_read = Some(clock());
        match client.harness_policy().await {
            Ok(PolicyAnswer::Supported(p)) if p.effective => {
                {
                    let mut st = self.lock();
                    st.hold = false;
                    st.generation = Some(p.generation);
                }
                let policy = Policy::from_harness(&p);
                if self.lock().held && client.holds_claim() {
                    match self.resume(policy) {
                        Some(why) => self.note(format!(
                            "policy gen={} effective=on: acting again under the kept claim \
                             (was {why})",
                            p.generation
                        )),
                        None => self.note(format!("policy gen={} re-read", p.generation)),
                    }
                    return;
                }
                if self.lock().unproven_gen == Some(p.generation) {
                    return;
                }
                if self.lock().cleared {
                    self.note("the claim was cleared: waiting for the policy to move");
                    return;
                }
                // aterm reads the record at the claim: a line written after this read is
                // renewed into its copy by `renew_for_record`.
                let line = self.opts.shared.resume_lines();
                match client.claim().await {
                    Ok(ClaimOutcome::Held { .. }) if self.closing() => {
                        // The exit came while it was being taken: give it straight back.
                        let _ = self.give_back(client, false).await;
                    }
                    Ok(ClaimOutcome::Held { .. }) => {
                        self.taken(policy);
                        self.lock().confirmed_line = line;
                        self.note(format!("claimed as {} (proven)", client.holder()));
                        // Owed receipts and reports go now that codex-alab acts.
                        self.refresh_badge();
                    }
                    Ok(ClaimOutcome::Unproven) => {
                        self.lock().unproven_gen = Some(p.generation);
                        self.note("aterm did not prove the claim: given back, passive");
                    }
                    Ok(ClaimOutcome::Busy { holder }) => {
                        // That holder's release is a `meta` event: the claim is taken then,
                        // and the passive clock does not ask again meanwhile (review of
                        // fea034d20, finding 6).
                        self.lock().busy = true;
                        self.note(format!(
                            "no claim: {holder} holds the session; waiting for its release"
                        ));
                    }
                    Ok(other) => self.note(format!("no claim: {other:?}")),
                    Err(e) => self.note(format!("claim failed: {e}")),
                }
            }
            Ok(PolicyAnswer::Supported(p)) => {
                self.lock().generation = Some(p.generation);
                let held = self.lock().held && client.holds_claim();
                let why = format!("effective=off why={}", p.why);
                match off_kind(&p.why) {
                    Off::Hold => {
                        // A person's hold is a halt, not a hand-back: the claim is kept, and
                        // every act waits on `status hold=` in the before-every-act checks.
                        // A hold read after a standby lifts the standby (review of fea034d20,
                        // finding 7): the hold's own rule is the one left, and attention, the
                        // description and mail stand under it as CONTRACT §2 says.
                        self.lock().hold = true;
                        if held {
                            if let Some(was) = self.resume(Policy::from_harness(&p)) {
                                self.note(format!("policy {why}: the standby ({was}) is lifted"));
                            }
                            self.note(format!("policy {why}: the claim is kept, nothing acts"));
                        } else {
                            self.note(format!("policy {why}: passive"));
                        }
                    }
                    Off::Standby => {
                        self.lock().hold = false;
                        if held {
                            self.stand_by(client, why.clone()).await;
                        }
                        self.note(format!("policy {why}: passive"));
                    }
                    Off::Release => {
                        self.lock().hold = false;
                        if held {
                            self.release_for_off(client, why.clone()).await;
                        }
                        self.note(format!("policy {why}: passive, unclaimed"));
                    }
                }
            }
            Ok(PolicyAnswer::Unsupported(line)) if self.lock().held && client.holds_claim() => {
                // The aterm that proved the claim answered the policy before: a reply it
                // cannot give now is a read that failed (transient), not an old aterm.
                self.stand_by(client, format!("policy unreadable: {line}"))
                    .await;
                self.note(format!(
                    "policy unreadable ({line}): passive, the claim kept"
                ));
            }
            Ok(PolicyAnswer::Unsupported(line)) => {
                self.lock().unsupported = true;
                self.note(format!(
                    "aterm has no harness policy ({line}): never claims"
                ));
            }
            Err(e) => {
                // An unanswered read closes the connection, and the claim with it; any other
                // refusal is transient: the claim stands, kept on standby.
                if self.lock().held && client.holds_claim() {
                    self.stand_by(client, format!("policy unreadable: {e}"))
                        .await;
                }
                self.note(format!("policy unreadable: {e}"));
            }
        }
    }

    /// A person's or the config's off (CONTRACT §2 as decided 2026-10-17): the claim is given
    /// back on its own connection (`meta unset supervisor holder=…`), codex-alab's attention
    /// cleared and its description withdrawn, as before b24e535df. The connection and its
    /// watch stay: the claim is taken again, by the ordinary proven claim, once the policy
    /// reads `effective=on`.
    ///
    /// codex-alab is passive here first, before the release is answered: an act whose checks
    /// run meanwhile on another task meets no claim held (review of fea034d20, finding 3).
    pub(crate) async fn release_for_off(&self, client: &Client, why: String) {
        self.lost_locally(why.clone());
        if let Err(e) = self.give_back(client, false).await {
            self.note(format!("the claim's release failed: {e}"));
        }
        self.clear_attention(client).await;
        self.note(format!(
            "gave the claim back ({why}): claimed again at effective=on"
        ));
    }

    /// When the passive clock reads the policy next: [`POLICY_RECHECK`] after the last read,
    /// while codex-alab is passive for any reason (unclaimed, on standby, under a hold), never
    /// while it acts, at the exit or against an aterm with no `harness policy`, and never while
    /// what it waits for comes with an event of its own (review of fea034d20, finding 6): an
    /// unheld claim another holder has (its release), someone cleared, or aterm did not prove
    /// at the generation the policy still reads (a policy move: one read since, during a
    /// suspension say, turns the clock back on, review of 7e7622ff7, finding 4).
    fn policy_recheck_at(&self) -> Option<tokio::time::Instant> {
        if self.closing() {
            return None;
        }
        let every = Duration::from_millis(self.recheck_ms.load(Ordering::SeqCst));
        let st = self.lock();
        if st.unsupported || (st.acting() && !st.hold) {
            return None;
        }
        let unproven = st.unproven_gen.is_some() && st.unproven_gen == st.generation;
        if !st.held && (st.busy || st.cleared || unproven) {
            return None;
        }
        let at = st
            .policy_read
            .and_then(|read| read.checked_add(every))
            .unwrap_or_else(clock);
        Some(tokio::time::Instant::from_std(at))
    }

    /// A transient off (CONTRACT §2 as decided 2026-10-17: a suspension, an unreadable
    /// policy): the proven claim is kept and nothing acts under it. codex-alab's attention is
    /// cleared, its description withdrawn and a box answer the TUI has not applied taken back,
    /// as a release does; the record still names the claim, and the claim is still renewed
    /// when the resume line moves. Acting again re-raises what a standing point still owes
    /// (the threads look again), a grace later.
    pub(crate) async fn stand_by(&self, client: &Client, why: String) {
        let described = {
            let mut st = self.lock();
            if st.standby.is_some() {
                // Already passive: only the reason moves.
                st.standby = Some(why);
                return;
            }
            st.standby = Some(why.clone());
            st.policy = None;
            st.box_look = None;
            st.since = Some(clock());
            st.described.take()
        };
        if let Some(text) = described {
            self.withdraw_on_own_connection(text);
        }
        self.withdraw_unapplied_answer();
        self.opts.shared.set_autopilot(AutopilotState::Passive);
        self.clear_attention(client).await;
        self.note(format!("passive, the claim kept: {why}"));
    }

    /// Withdraw `text` from aterm's description on a connection of its own (the claim's may be
    /// the one that ended), while it is still the text this process wrote.
    fn withdraw_on_own_connection(&self, text: String) {
        if let Some(endpoint) = (self.opts.endpoint)()
            && tokio::runtime::Handle::try_current().is_ok()
        {
            let options = self.opts.client.clone();
            tokio::spawn(async move {
                if let Ok(client) = Client::connect(endpoint, options).await {
                    let _ = withdraw_description(&client, &text).await;
                    client.close();
                }
            });
        }
    }

    /// Act again under the kept claim with `policy`: a standby is lifted (its first act waits
    /// the grace, the threads look again, so a point whose attention the standby cleared
    /// raises it again, and owed receipts and reports go). Answers the standby's reason, or
    /// `None` when there was none (only the policy moved).
    fn resume(&self, policy: Policy) -> Option<String> {
        let resumed = {
            let mut st = self.lock();
            st.policy = Some(policy);
            let resumed = st.standby.take();
            if resumed.is_some() {
                // Acting again is a transition: its first act waits the grace.
                st.since = Some(clock());
            }
            resumed
        }?;
        self.opts.shared.set_autopilot(AutopilotState::Active);
        self.claims.send_modify(|n| *n += 1);
        self.refresh_badge();
        Some(resumed)
    }

    /// Give back the claim on `client` (`meta unset supervisor holder=…`). Its `value=-` echo
    /// is counted due ([`State::echoes_due`]) only when aterm showed the claim codex-alab's own:
    /// the client holds it, or aterm just said so (`shown`), and only then does
    /// [`Supervisor::released`] move (an act's checks read it as "an own release may explain
    /// this clear"). A release of a claim already gone does nothing at aterm and has no echo,
    /// and the count is taken back when the release fails (review of 7e7622ff7, finding 2).
    ///
    /// The release runs on a task of its own, its count taken first: a caller cancelled while
    /// it waits (the approval path's budget around the before-act checks) cannot drop it
    /// half-done (finding 3).
    async fn give_back(&self, client: &Client, shown: bool) -> Result<(), AtermError> {
        let counted = shown || client.holds_claim();
        if counted {
            self.released.fetch_add(1, Ordering::SeqCst);
            let mut st = self.lock();
            st.echoes_due = st.echoes_due.saturating_add(1);
        }
        let me = self.me.clone();
        let client = client.clone();
        let release = tokio::spawn(async move {
            let released = client.release().await;
            // A refusal leaves the connection (and its count) standing: no echo comes. A
            // connection that closed ends the session, and its count with it.
            if released.is_err()
                && counted
                && client.is_open()
                && let Some(me) = me.upgrade()
            {
                let mut st = me.lock();
                st.echoes_due = st.echoes_due.saturating_sub(1);
            }
            released
        });
        release.await.unwrap_or(Err(AtermError::Closed))
    }

    /// Whether `status` shows the claim proven and codex-alab's own now.
    async fn shown_ours(&self, client: &Client) -> bool {
        matches!(client.status().await, Ok(status) if status.proven_holder(client.holder()))
    }

    fn taken(&self, policy: Policy) {
        {
            let mut st = self.lock();
            st.held = true;
            st.busy = false;
            st.standby = None;
            st.policy = Some(policy);
            st.since = Some(clock());
            st.unproven_gen = None;
        }
        self.opts
            .shared
            .set_claim(Some(codex_alab_aterm::holder_for(
                self.opts.client.holder_pid,
            )));
        self.opts.shared.set_autopilot(AutopilotState::Active);
        self.claims.send_modify(|n| *n += 1);
    }

    /// Take back a box answer the TUI has not applied. Only then is the posted answer
    /// forgotten: one the glue already took (its box closed, or closing) keeps its record
    /// until its outcome comes, which is recorded once codex-alab acts again (review of
    /// 4cdc96090, finding 7; the race of bcbbea25f).
    fn withdraw_unapplied_answer(&self) {
        if self.opts.shared.withdraw_box_answer() {
            self.lock().posted = None;
            self.note("withdrew a box answer the TUI had not applied");
        }
    }

    /// Passive now, in this process (the claim is gone or being given back).
    fn lost_locally(&self, why: impl Into<String>) {
        self.lost_locally_marking(why, |_| {});
    }

    /// [`Supervisor::lost_locally`], with `mark` applied to the state under the same lock that
    /// makes it passive (what the loss waits for: a policy move, another holder's release), so
    /// no task sees the one without the other.
    fn lost_locally_marking(&self, why: impl Into<String>, mark: impl FnOnce(&mut State)) {
        let (was, described) = {
            let mut st = self.lock();
            mark(&mut st);
            let was = st.held;
            st.held = false;
            st.standby = None;
            st.hold = false;
            st.policy = None;
            st.box_look = None;
            if was {
                st.since = Some(clock());
            }
            (was, st.described.take())
        };
        // Nothing keeps the description true without the claim: it is withdrawn (on a
        // connection of its own; the claim's may be the one that ended), while it is still
        // the text this process wrote.
        if let Some(text) = described {
            self.withdraw_on_own_connection(text);
        }
        if was {
            self.note(format!("passive: {}", why.into()));
        }
        // An answer the TUI has not applied yet is no longer codex-alab's to give.
        self.withdraw_unapplied_answer();
        self.opts.shared.set_claim(None);
        self.opts.shared.set_autopilot(AutopilotState::Passive);
        self.show_badge(None);
    }

    /// The before-every-act checks (CONTRACT §5), the claim re-confirmed last.
    pub async fn ready(&self) -> Ready {
        self.ready_for(Act::Turn).await
    }

    /// [`Supervisor::ready`] for an act on a box the TUI shows: that box is the open "modal"
    /// (it is the TUI's active view, or its glue would not have reported it), so only the
    /// popup check is the box's own; every other check holds.
    pub async fn ready_for_box(&self) -> Ready {
        self.ready_for(Act::Box).await
    }

    async fn ready_for(&self, act: Act) -> Ready {
        let (client, policy, since) = {
            let st = self.lock();
            if let Some(why) = st.standby.as_ref().filter(|_| st.held) {
                return Ready::Passive(format!("passive under the kept claim: {why}"));
            }
            match (&st.client, &st.policy) {
                (Some(client), Some(policy)) if st.acting() && client.holds_claim() => {
                    (client.clone(), policy.clone(), st.since)
                }
                _ => return Ready::Passive("no proven claim".to_string()),
            }
        };
        let now = clock();
        let grace = policy.human_grace;
        let shared = self.opts.shared;
        let grace_ms = u64::try_from(grace.as_millis()).unwrap_or(u64::MAX);
        if let Some(at) = shared.last_human_input_ms() {
            let age = now_ms().saturating_sub(at);
            if age < grace_ms {
                return Ready::Wait {
                    until: now + Duration::from_millis(grace_ms - age),
                    why: "a person typed in the TUI".to_string(),
                };
            }
        }
        let tui_hand = if shared.screen_taken() {
            Some("an overlay or the external editor has the screen")
        } else if !shared.draft_empty() {
            Some("a draft stands in the composer")
        } else if shared.queued_input() {
            Some("the TUI holds a person's queued input")
        } else if act == Act::Turn && shared.popup_open() {
            Some("a popup or modal is open")
        } else if shared.pending_user_turn() {
            Some("a person's turn is starting")
        } else {
            None
        };
        if let Some(why) = tui_hand {
            return Ready::Wait {
                until: now + RECHECK,
                why: why.to_string(),
            };
        }
        let epoch = self.lost_epoch.load(Ordering::SeqCst);
        let released = self.released.load(Ordering::SeqCst);
        let status = match client.status().await {
            Ok(status) => status,
            Err(e) => {
                return Ready::Wait {
                    until: now + RECHECK,
                    why: format!("status unreadable: {e}"),
                };
            }
        };
        match status.ready_for_act(client.holder(), grace) {
            Ok(()) => {}
            Err(NotReady::Human(HumanInput::Ago(ms))) => {
                return Ready::Wait {
                    until: now + Duration::from_millis(grace_ms.saturating_sub(ms).max(1)),
                    why: "a person at the terminal".to_string(),
                };
            }
            Err(not_ours @ (NotReady::NotOurClaim(_) | NotReady::Unproven)) => {
                // aterm no longer shows a proven claim of ours: whatever it still keeps under
                // our name proves nothing, so it is given back. Passive first, before any
                // await: a caller cancelled meanwhile leaves no claim counted held that the
                // client no longer holds (review of 7e7622ff7, finding 3). A claim nobody holds
                // now was cleared by someone (a person's `meta unset supervisor`), unless
                // codex-alab gave it back itself since the read began: that clear stays given up
                // until the policy moves, whether the claim's watch or these checks meet it
                // first (finding 1). Another holder's is taken again at its release, which the
                // watch meets after the claim's move to it.
                client.note_claim_lost();
                let nobody = matches!(not_ours, NotReady::NotOurClaim(None));
                self.lost_locally_marking("status shows no proven claim of ours", |st| {
                    if nobody && self.released.load(Ordering::SeqCst) == released {
                        st.cleared = true;
                    }
                });
                if client.is_open() {
                    let client = client.clone();
                    let _ = tokio::spawn(async move { client.release().await }).await;
                }
                return Ready::Passive("status shows no proven claim of ours".to_string());
            }
            Err(other) => {
                return Ready::Wait {
                    until: now + recheck_after(&other),
                    why: format!("aterm says not now: {other:?}"),
                };
            }
        }
        // After any claim transition the first act waits the grace (checked last, so a
        // person's hand is what a wait names when there is one).
        if let Some(since) = since
            && let Some(until) = since.checked_add(grace)
            && now < until
        {
            return Ready::Wait {
                until,
                why: "the claim just moved: waiting the grace".to_string(),
            };
        }
        match client.reconfirm().await {
            Ok(true) => {}
            _ => {
                self.lost_locally("the claim did not re-confirm");
                return Ready::Passive("the claim did not re-confirm".to_string());
            }
        }
        // A clear (or another holder) seen since the status read: the same-holder set may
        // have taken back a claim someone had just cleared. Give it back; no act. It counts as
        // someone else's clear (not taken again until the policy moves) unless codex-alab gave
        // a claim back itself meanwhile, whose own echo may be what moved the epoch (review of
        // fea034d20, finding 3: an Autopilot Off's release then left the session unclaimed
        // with Autopilot back on).
        if self.lost_epoch.load(Ordering::SeqCst) != epoch {
            let own = self.released.load(Ordering::SeqCst) != released;
            // Passive first, before the release is awaited (review of 7e7622ff7, finding 3).
            // Its echo is counted while the client still holds the claim (the re-confirm's
            // `OK` showed it ours), not after codex-alab's own release already gave it back.
            self.lost_locally_marking("the claim moved while it was re-confirmed", |st| {
                if !own {
                    st.cleared = true;
                }
            });
            if client.is_open() {
                let _ = self.give_back(&client, false).await;
            }
            client.note_claim_lost();
            return Ready::Passive("the claim moved while it was re-confirmed".to_string());
        }
        // Last, under the lock: an off read while the status read or the re-confirm was in
        // flight (a standby, a release) sends nothing and moves no epoch, so it is looked for
        // here (review of 4cdc96090, finding 1), the claim's connection included.
        let passive = {
            let st = self.lock();
            let holds = st.client.as_ref().is_some_and(Client::holds_claim);
            (!(st.acting() && holds)).then(|| st.standby.clone())
        };
        if let Some(standby) = passive {
            return Ready::Passive(match standby {
                Some(why) => format!("passive under the kept claim: {why}"),
                None => "the claim went while the checks ran".to_string(),
            });
        }
        Ready::Go
    }

    /// Record an act on the session (`harness_act`), tell the presence band ([`story`]), and
    /// record it in aterm's message log.
    ///
    /// While the proven claim is held, a standby included: every act passed its checks while
    /// codex-alab acted, so one a standby came after still went, and is recorded (review of
    /// fea034d20, finding 5; aterm takes the record under the kept claim). Without the claim
    /// nothing is recorded as codex-alab's.
    pub async fn record_act(&self, rule: &str, words: &str) {
        let client = {
            let st = self.lock();
            st.client
                .clone()
                .filter(|client| st.held && client.holds_claim())
        };
        let Some(client) = client else {
            self.note(format!(
                "act {rule} not recorded: no proven claim is held ({words})"
            ));
            return;
        };
        let _ = client.harness_act(rule).await;
        if let Some((verb, text)) = story::for_act(rule) {
            let _ = client.story(verb, text).await;
        }
        let _ = client
            .appnotice_harness(&format!("codex-alab @{}: {words}", client.endpoint().sid))
            .await;
        self.note(format!("act {rule}: {words}"));
    }

    /// Say what the session is doing (`meta set description`, [`crate::describe`]): only while
    /// the proven claim is held, once per change, on a connection of its own (never the
    /// claim's), and never over a description a person wrote. `None` (no goal, no step) withdraws
    /// the one this process wrote.
    ///
    /// A standing text is codex-alab's only when it is exactly the one this process wrote, or,
    /// before this process wrote any, when a dead predecessor of this session left a record
    /// (a crash or an aterm restore carried its text) and the text has codex-alab's shape
    /// ([`crate::describe::is_ours`]). Any other text is a person's, whatever it starts
    /// with. aterm has no compare-and-set: the read and the write are back to back, and a
    /// person's text set between them is the one race left.
    pub async fn describe(&self, text: Option<String>) {
        let (endpoint, described, carried) = {
            let mut st = self.lock();
            if !st.acting() || st.described == text && (text.is_some() || st.carried_checked) {
                return;
            }
            let endpoint = match &st.client {
                Some(client) if client.holds_claim() => client.endpoint().clone(),
                _ => return,
            };
            let carried = st.described.is_none() && !st.carried_checked && self.carried();
            st.carried_checked = true;
            if text.is_none() && st.described.is_none() && !carried {
                // Nothing of codex-alab's can stand: nothing to withdraw.
                return;
            }
            (endpoint, st.described.clone(), carried)
        };
        let client = match Client::connect(endpoint, self.opts.client.clone()).await {
            Ok(client) => client,
            Err(e) => {
                self.note(format!("description unread, not written: {e}"));
                return;
            }
        };
        let done = self.describe_on(&client, text, described, carried).await;
        client.close();
        if let Err(e) = done {
            self.note(format!("description not written: {e}"));
        }
    }

    /// Whether a dead predecessor of this process's primary thread left a record (a crash, an
    /// aterm restore), whose description may still stand.
    fn carried(&self) -> bool {
        let shared = self.opts.shared;
        shared
            .view()
            .primary_thread
            .is_some_and(|thread| shared.predecessor(&thread).is_some())
    }

    async fn describe_on(
        &self,
        client: &Client,
        text: Option<String>,
        described: Option<String>,
        carried: bool,
    ) -> Result<(), AtermError> {
        let fields = client.meta().await?;
        let standing = codex_alab_aterm::wire::field(&fields, "description");
        let ours = |s: &str| {
            described.as_deref() == Some(s)
                || (described.is_none() && carried && crate::describe::is_ours(s))
        };
        let Some(text) = text else {
            if let Some(standing) = standing.filter(|s| ours(s.as_str())) {
                client.unset_description().await?;
                self.note(format!("description withdrawn ({standing})"));
            }
            self.lock().described = None;
            return Ok(());
        };
        match standing {
            Some(standing) if standing == text => {
                // Already says it (a predecessor's, carried): taken as this process's own.
                self.note(format!("description stands as written: {text}"));
            }
            Some(standing) if !ours(standing.as_str()) => {
                self.note(format!(
                    "a person's description stands ({standing}): not replaced"
                ));
                return Ok(());
            }
            _ => {
                client.set_description(&text).await?;
                self.note(format!("description: {text}"));
            }
        }
        // A standby or a release that came while this was written found nothing of this text
        // to withdraw: it is withdrawn here instead, so no description stands under an off.
        let kept = {
            let mut st = self.lock();
            let acting = st.acting() && st.client.as_ref().is_some_and(Client::holds_claim);
            if acting {
                st.described = Some(text.clone());
            }
            acting
        };
        if !kept && withdraw_description(client, &text).await? {
            self.note(format!(
                "description withdrawn: passive since it was written ({text})"
            ));
        }
        Ok(())
    }

    /// Record a wait in aterm's message log (a record, never a row), and tell the band.
    pub async fn record_wait(&self, words: &str) {
        let Some(client) = self.lock().client.clone() else {
            return;
        };
        let (verb, text) = story::for_wait(words);
        let _ = client.story(verb, &text).await;
        let _ = client
            .appnotice_harness(&format!("codex-alab @{}: {words}", client.endpoint().sid))
            .await;
        self.note(format!("wait: {words}"));
    }

    /// Raise (`Some`) or clear (`None`) one of codex-alab's attention reasons; aterm shows
    /// the first standing one under `owner=codex-alab`. Only while the claim is held.
    pub async fn attention(&self, key: &'static str, text: Option<String>) {
        let client = {
            let mut st = self.lock();
            match text {
                Some(text) if st.acting() => {
                    st.attention.insert(key, text);
                }
                Some(_) => return,
                None => {
                    st.attention.remove(key);
                }
            }
            st.client.clone()
        };
        if let Some(client) = client {
            self.sync_attention(&client).await;
        }
    }

    async fn clear_attention(&self, client: &Client) {
        self.lock().attention.clear();
        self.sync_attention(client).await;
    }

    async fn sync_attention(&self, client: &Client) {
        // One at a time (`attention_sync`, one permit, never closed); what is wanted is read
        // once the one before is done.
        let _one = self.attention_sync.acquire().await;
        let (want, shown) = {
            let st = self.lock();
            (st.attention.values().next().cloned(), st.shown.clone())
        };
        if want == shown {
            return;
        }
        let ok = match &want {
            Some(text) => {
                let (kind, why) = text.split_once(": ").unwrap_or(("codex-alab", text));
                client.set_attention(kind, why).await.is_ok()
            }
            None => client.clear_attention().await.is_ok(),
        };
        if ok {
            if let Some(text) = &want {
                let (verb, story) = story::for_attention(text);
                let _ = client.story(verb, &story).await;
            }
            self.note(match &want {
                Some(text) => format!("attention: {text}"),
                None => "attention cleared".to_string(),
            });
            self.lock().shown = want;
        }
    }

    /// The first of codex-alab's attention reasons that stands, if any (a point it left to a
    /// person).
    pub fn standing_attention(&self) -> Option<String> {
        self.lock().attention.values().next().cloned()
    }

    /// The approval contributor left a box to a person, for this reason.
    pub fn deferred(&self, why: String) {
        self.lock().last_defer = Some(why);
    }

    fn box_age(&self) -> Option<(BlockedKind, Duration, Duration)> {
        let grace = self.lock().policy.as_ref()?.human_grace;
        let view = self.opts.shared.view();
        let ProgramState::Blocked(kind) = view.state else {
            return None;
        };
        let age = Duration::from_millis(now_ms().saturating_sub(view.state_since_ms));
        Some((kind, age, grace))
    }

    fn box_deadline(&self) -> Option<tokio::time::Instant> {
        let (look, attention) = {
            let st = self.lock();
            if !st.acting() {
                return None;
            }
            (st.box_look, st.attention.contains_key("box"))
        };
        // A question or plan box is looked at only when its own deadline says (never by the
        // blocked state's age, which would spin once the grace has passed).
        let blocked = if attention || self.opts.shared.open_box().is_some() {
            None
        } else {
            self.box_age()
                .map(|(_, age, grace)| tokio::time::Instant::now() + grace.saturating_sub(age))
        };
        [blocked, look.map(tokio::time::Instant::from_std)]
            .into_iter()
            .flatten()
            .min()
    }

    /// The boxes the TUI shows: a question or plan box is answered (or left with attention);
    /// any other box left unanswered past the grace raises attention; a closed box clears it.
    async fn check_boxes(&self, client: &Client) {
        if !self.lock().acting() {
            return;
        }
        let shared = self.opts.shared;
        if let Some(outcome) = shared.take_box_outcome() {
            let posted = {
                let mut st = self.lock();
                match &st.posted {
                    Some(p) if p.id == outcome.id => st.posted.take(),
                    _ => None,
                }
            };
            match (outcome.applied, posted) {
                (Ok(()), Some(posted)) => {
                    if posted.rule == answer::RULE_PLAN {
                        // codex-alab's own yes to the plan box went in: the Plan-mode turn end
                        // it left is answered, and the plan is being carried out (review of
                        // the inbox, finding 8).
                        self.attention("plan", None).await;
                    }
                    self.record_act(posted.rule, &posted.words).await;
                }
                (Ok(()), None) => self.note(format!("box {} took an answer", outcome.id)),
                (Err(why), posted) => {
                    self.note(format!("the TUI refused the answer: {why}"));
                    // The posted answer is gone (taken out above): the box is looked at
                    // again a grace later, where a person's hand on it waits that out.
                    if posted.is_some() {
                        let mut st = self.lock();
                        st.box_look = st.policy.as_ref().map(|p| clock() + p.human_grace);
                    }
                }
            }
        }
        if let Some(open) = shared.open_box() {
            self.consider_box(client, open).await;
            return;
        }
        // The box closed. An answer posted for it stays until its outcome comes: the TUI's glue
        // closes the box and THEN reports the outcome, and a look between the two must not
        // forget the answer, or the act goes unrecorded. It names its box, so it is never
        // taken for another one.
        self.lock().box_look = None;
        match self.box_age() {
            Some((kind, age, grace)) if age >= grace => {
                let last_defer = self.lock().last_defer.clone();
                let text = left_box_text(kind, last_defer.as_deref());
                self.lock().attention.insert("box", text);
            }
            Some(_) => return,
            None => {
                self.lock().attention.remove("box");
            }
        }
        self.sync_attention(client).await;
    }

    /// A question or plan box (CONTRACT §5): after the grace since it opened and the
    /// before-every-act checks, its answer is posted for the TUI's glue to apply; a box that
    /// is a person's, or one still not answered a grace later, raises attention.
    async fn consider_box(&self, client: &Client, open: codex_alab_core::boxes::OpenBox) {
        let Some(policy) = self.lock().policy.clone() else {
            return;
        };
        let grace = policy.human_grace;
        let now = clock();
        let age = Duration::from_millis(now_ms().saturating_sub(open.opened_ms));
        let word = open.kind.word();
        if age < grace {
            self.lock().box_look = Some(now + (grace - age));
            return;
        }
        let raise = |text: String| {
            let mut st = self.lock();
            st.box_look = None;
            st.attention.insert("box", text);
        };
        // Only a root thread of this TUI's own: never a sub-agent's box it shows.
        let shared = self.opts.shared;
        let root = match &open.kind {
            codex_alab_core::boxes::BoxKind::Question { thread, .. } => {
                shared.is_root_thread(thread)
            }
            codex_alab_core::boxes::BoxKind::Approval { thread, .. } => {
                shared.is_root_thread(thread)
            }
            codex_alab_core::boxes::BoxKind::Plan { .. } => shared
                .view()
                .displayed_thread
                .as_deref()
                .is_some_and(|thread| shared.is_root_thread(thread)),
        };
        if !root {
            raise(format!(
                "{word}: left to a person (not a root thread of this TUI)"
            ));
            self.sync_attention(client).await;
            return;
        }
        match answer::decide_box(&open, &policy) {
            BoxDecision::Leave(why) => {
                raise(format!("{word}: left to a person ({why})"));
            }
            BoxDecision::Answer {
                answer,
                rule,
                words,
            } => {
                let posted = self.lock().posted.clone();
                if let Some(posted) = posted.filter(|p| p.id == open.id) {
                    match posted.at.checked_add(grace) {
                        Some(until) if now < until => self.lock().box_look = Some(until),
                        _ => raise(format!("{word}: codex-alab's answer was not taken")),
                    }
                } else {
                    match self.ready_for_box().await {
                        Ready::Go => {
                            if self.opts.shared.post_box_answer(answer) {
                                {
                                    let mut st = self.lock();
                                    st.posted = Some(Posted {
                                        id: open.id.clone(),
                                        rule,
                                        words,
                                        at: now,
                                    });
                                    st.box_look = Some(now + grace);
                                }
                                self.note(format!("{word} {}: answer posted", open.id));
                                if !self.opts.shared.wake() {
                                    self.note("no frame request from the TUI: the answer waits for its next frame");
                                }
                            }
                        }
                        Ready::Wait { until, why } => {
                            self.note(format!("{word} {}: not now ({why})", open.id));
                            if age >= grace.saturating_mul(2) {
                                raise(format!("{word}: not answered yet ({why})"));
                            }
                            self.lock().box_look = Some(until);
                        }
                        Ready::Passive(_) => return,
                    }
                }
            }
        }
        self.sync_attention(client).await;
    }
}

/// The attention a box the TUI shows raises once codex-alab has left it past the grace: an
/// approval names why it was left (the approval contributor's [`crate::approval::Defer`]
/// words, or the wait it was left in), a question box and a sign-in say what they are.
pub fn left_box_text(kind: BlockedKind, last_defer: Option<&str>) -> String {
    match kind {
        BlockedKind::Permission => format!(
            "approval: left to a person ({})",
            last_defer.unwrap_or("codex-alab did not answer it")
        ),
        BlockedKind::Question => {
            "question: a question box codex-alab leaves to a person".to_string()
        }
        BlockedKind::Auth => "login: Codex asks for a sign-in".to_string(),
    }
}

/// Which act the before-every-act checks are for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Act {
    /// A turn, a compaction, an approval, an upgrade.
    Turn,
    /// An answer to the box the TUI shows.
    Box,
}

/// What one `status` read at `read_at_ms` says for the launch witness: its `hand=`, when a
/// person last gave input, and the pid the kernel names as the socket's server.
pub fn launch_witness(status: &Status, read_at_ms: u64, server_pid: Option<u32>) -> LaunchWitness {
    LaunchWitness {
        server_pid,
        hand: status.hand.clone(),
        hand_pid: status.hand_pid,
        human: match status.human {
            HumanInput::Never => HumanSeen::Never,
            HumanInput::Ago(ms) => HumanSeen::At(read_at_ms.saturating_sub(ms)),
            HumanInput::Unknown => HumanSeen::Unknown,
        },
        read_at_ms,
    }
}

/// When to look again after aterm said "not now": soon behind its own harness's lease
/// ([`HAND_RECHECK`]), else [`RECHECK`].
pub fn recheck_after(not_ready: &NotReady) -> Duration {
    match not_ready {
        NotReady::Hand(hand) if hand.starts_with("lease:aterm-harness@") => HAND_RECHECK,
        _ => RECHECK,
    }
}

/// Whether `event` may have moved `harness policy`'s `effective=` without a `harness-policy`
/// event: a `meta` change of any field but the claim (the claim watch reads it) and the
/// session's words (attention, description, title, icon, role: codex-alab writes the first two
/// itself, and none moves the policy), or a `hold` (`EVENT <local> hold <0|1> reason=…
/// origin=…`).
pub fn moves_effective(event: &Event) -> bool {
    match event {
        Event::Meta { field, .. } => !matches!(
            field.as_str(),
            "supervisor" | "attention" | "description" | "title" | "icon" | "role"
        ),
        Event::Other(line) => line.split_ascii_whitespace().nth(2) == Some("hold"),
        _ => false,
    }
}

async fn sleep_until_opt(deadline: Option<tokio::time::Instant>) {
    match deadline {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending().await,
    }
}

/// Withdraw `text` as the session's description (`meta unset description`), only while it is
/// still the standing one: a text a person set since is theirs.
async fn withdraw_description(client: &Client, text: &str) -> Result<bool, AtermError> {
    let fields = client.meta().await?;
    if codex_alab_aterm::wire::field(&fields, "description").as_deref() != Some(text) {
        return Ok(false);
    }
    client.unset_description().await?;
    Ok(true)
}
