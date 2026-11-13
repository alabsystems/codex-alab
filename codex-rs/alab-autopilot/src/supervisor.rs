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
//!     every act waits on `status hold=`; the `effective=on` read that finds it lifted, at its
//!     `hold` event or on the passive clock, has every thread and a waiting live upgrade look
//!     again at once, as aterm's loop goes at the hold's edge, the wait's [`HOLD_RECHECK`]
//!     only the backstop, aterm's own park step), `why=suspended`
//!     (aterm suspends its host at every seamless-handoff commit: a successor adopts the
//!     claim, a failed commit resumes it) and a policy read that failed (`State::standby` for the last two, which also clears
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
//!   `GAP`), and every [`POLICY_RECHECK`] while passive, the passive clock: aterm announces a
//!   suspension with no event at all, and a session may start with `effective=off`. The
//!   clock never runs while codex-alab acts, nor while what it waits for has an event of its
//!   own: another holder's claim (`ERR busy`: its release is a `meta` event), a claim someone
//!   cleared, or one aterm did not prove (the policy must move). The other clock, the word's
//!   ([`WORD_RECHECK`]), runs only while it acts and `status` last named no Autopilot word:
//!   aterm's admission of the session's codex-alab moves `-` to the word with no event.
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
//!   held, on standby too (a person or a driver types the line, aterm no longer does; reading
//!   it is not codex-alab's act, and unread
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
//! - An automatic wait a person should see (a usage reset, an outage's rung, the short-turn
//!   back-off, the hour's cap) goes up as `meta set attention owner=codex-alab-wait kind=wait
//!   <words>` while the claim is held: aterm's Waiting rows, never a need and never in the
//!   `owner=codex-alab` slot ([`Supervisor::want_wait_for`]).
//! - Ruling F's typing stop names its arm in aterm's `status held=` (`meta set harness_held
//!   <arm>`, `meta unset harness_held`) on the claim's connection while the proven claim is
//!   held: the primary thread's hold, once per change ([`Supervisor::want_held_for`]).
//! - Land's hold goes up as aterm's landing (`meta set harness_land since=<ms> nudges=<n>
//!   [read=1 …]`, `meta unset harness_land`) by the same rule: the primary thread's, under
//!   Land, once per change ([`Supervisor::want_land_for`]).
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
use codex_alab_aterm::Fact;
use codex_alab_aterm::FactAnswer;
use codex_alab_aterm::HeldOutcome;
use codex_alab_aterm::LandOutcome;
use codex_alab_aterm::LandReport;
use codex_alab_aterm::NEEDS_TOLD;
use codex_alab_aterm::NeedsOutcome;
use codex_alab_aterm::Options as ClientOptions;
use codex_alab_aterm::PolicyAnswer;
use codex_alab_aterm::Signal;
use codex_alab_aterm::ToldOutcome;
use codex_alab_aterm::claim_signal;
use codex_alab_aterm::status::HumanInput;
use codex_alab_aterm::status::NotReady;
use codex_alab_aterm::status::Status;
use codex_alab_core::mail::Verdict;
use codex_alab_core::session::Autopilot as AutopilotState;
use codex_alab_core::session::BlockedKind;
use codex_alab_core::session::HarnessWord;
use codex_alab_core::session::HumanSeen;
use codex_alab_core::session::LaunchWitness;
use codex_alab_core::session::ProgramState;
use codex_alab_core::session::Shared;
use codex_alab_core::session::TerminalContext;
use codex_alab_core::session::now_ms;
use tokio::sync::watch;

use crate::answer;
use crate::answer::BoxDecision;
use crate::decide::Mode;
use crate::decide::Policy;
use crate::replies::HeldBy;
use crate::story;

/// The `why=` word `harness policy` gives for a person's hold.
const HOLD_WHY: &str = "hold";

/// The `why=` word for a host suspended for a seamless handoff (aterm resumes it, or the
/// handoff's successor takes over).
const SUSPENDED_WHY: &str = "suspended";

/// The `why=` word for the session's Autopilot set to off. aterm gives it last, once the
/// session is hosted, the harness enabled for its agent and neither held nor suspended: the
/// whole harness is on, and only Autopilot is off (a release all the same).
const AUTOPILOT_OFF_WHY: &str = "autopilot-off";

/// The `why=` word for the harness off in aterm's Settings (`[harness] enabled = false`,
/// aterm's `MasterOff`).
const DISABLED_WHY: &str = "disabled";

/// The `why=` word for aterm's switch for codex-alab sessions off (aterm's
/// `AgentOff(Codex)`, "Watch codex-alab sessions").
const AGENT_OFF_WHY: &str = "agent-off";

/// The `why=` word for an aterm that runs no harness host (aterm's `Unhosted`, "This aterm has
/// no control socket").
const UNHOSTED_WHY: &str = "unhosted";

/// How often the policy is read again while codex-alab is passive for any reason (released,
/// on standby, held by a hold, or never claimed): aterm announces a suspension with no event,
/// and a session may start with `effective=off`. Never while it acts: then an event moves it.
pub const POLICY_RECHECK: Duration = Duration::from_secs(30);

/// How soon `status` is read again for the session's Autopilot word while the claim is held
/// and the last read named no acting mode (`-` before admission, or `off` while a policy
/// still says on): doubling at each read without a mode, up to
/// [`POLICY_RECHECK`]. A word aterm names after the claim (a worker `spawn agent=codex`
/// starts in Land) is met within a second or two, and an aterm that never names one costs a
/// read every [`POLICY_RECHECK`] (review of the act-time word read, finding 1).
pub const WORD_RECHECK: Duration = Duration::from_secs(1);

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

/// The word the TUI's chrome names while codex-alab acts in `mode`.
fn acting_word(mode: Mode) -> HarnessWord {
    match mode {
        Mode::Cruise => HarnessWord::Cruise,
        Mode::Land => HarnessWord::Land,
    }
}

/// The word the TUI's chrome names while passive after an off whose `why=` is `why`: the
/// harness on with Autopilot off for [`AUTOPILOT_OFF_WHY`]; the harness itself off by one of
/// aterm's Settings switches for [`DISABLED_WHY`] and [`AGENT_OFF_WHY`], and with no host for
/// [`UNHOSTED_WHY`], which aterm's chip wears as `⊘ off` (amended 2026-10-29 after the review
/// of gap 12, finding 3: the line wore no face there); no word for the rest (`headless`,
/// whose instance draws no window, and a reason aterm names later).
fn off_word(why: &str) -> HarnessWord {
    match why {
        AUTOPILOT_OFF_WHY => HarnessWord::Off,
        DISABLED_WHY => HarnessWord::HarnessOff,
        AGENT_OFF_WHY => HarnessWord::AgentOff,
        UNHOSTED_WHY => HarnessWord::Unhosted,
        _ => HarnessWord::Unknown,
    }
}

/// The act a mail turn's receipt and report are recorded as (`harness_act`).
pub const RULE_MAIL_REPORT: &str = "mail-report@v1";

/// How soon a wait with no deadline of its own (a draft, a popup, an unreadable status) looks
/// again.
pub const RECHECK: Duration = Duration::from_secs(60);

/// How soon an act that met a person's hold reads `status` again when no lift reached it
/// (aterm's `WAIT_STEP`, the longest step its loop's `park_on_hold` takes between `status`
/// reads). The lift is met at once by the `effective=on` read its `hold` event brings
/// ([`Supervisor::read_on`]); this is the backstop for a lift whose event never came (parity
/// review R2): aterm's park meets a lift between its `status` read and its `await inbox
/// kinds=hold` arm (which latches only on a change from the level armed at) a step later, so
/// codex-alab's worst case is aterm's, not [`RECHECK`]'s minute.
pub const HOLD_RECHECK: Duration = Duration::from_secs(20);

/// How soon a wait on aterm's own short hand looks again: the lease its harness holds while
/// it types a relaunch line ends seconds later, and a carry-on owed behind it should not
/// wait a full [`RECHECK`] (end-to-end run 2026-10-16: the carry-on came a minute late).
pub const HAND_RECHECK: Duration = Duration::from_secs(5);

/// The most a person's message waits on aterm's word of their hand
/// ([`Supervisor::person_input_ago`]): core awaits the read before the turn runs.
pub const HAND_READ_LIMIT: Duration = Duration::from_secs(1);

/// How soon receipts and reports still owed are sent again after a send failed (aterm slow or
/// unreachable): with no mail frame, claim or resume to come, nothing else would, and a
/// manager in `post --wait-ack` would wait on a quiet inbox for good (review of 7e7622ff7,
/// finding 5).
pub const OWED_RETRY: Duration = Duration::from_secs(15);

/// Pace refused metadata publications independently of frames and effective attention
/// changes: freeing a hidden keyed owner does not announce a change of the shown entry.
const ATTENTION_RETRY: Duration = Duration::from_secs(15);

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

/// A fresh Ready result bound to one immutable local authority observation.
/// It grants nothing by itself; recheck synchronously after preparatory waits.
pub(crate) struct AdmissionFence {
    events: Arc<ClaimEvents>,
    event_epoch: u64,
    lost_epoch: u64,
    released: u64,
    claim: u64,
    generation: Option<u64>,
    since: Option<Instant>,
    policy: Option<Policy>,
    tui_generation: u64,
}

#[derive(Default)]
struct State {
    manual_resume_at: Option<u64>,
    activity_sent: Option<(String, Option<u64>)>,
    activity_sequence: u64,
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
    /// The before-every-act checks met `status hold=1` since the last `effective=on` read: the
    /// read that finds the hold gone has every thread look again, as one after a `why=hold`
    /// read does ([`Supervisor::read_on`]).
    hold_met: bool,
    /// The `effective=on` reads so far: checks whose `status` read met a hold while one came
    /// look again at once ([`Supervisor::met_hold`]), as the lift may have come between.
    on_reads: u64,
    /// When the policy was last read (the passive clock counts from it).
    policy_read: Option<Instant>,
    /// No supported `harness policy` reply was ever read on this connection: an old
    /// aterm, never claimed or polled. A later supported event retires this latch.
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
    /// Lost/Gap events admitted by this connection's reader, not yet classified.
    queued_claim_events: Arc<ClaimEvents>,
    /// Reasons sort first by kind, then origin. A root can replace only its own reason;
    /// the session's overlay/mail reasons have no thread and keep the same priority.
    /// Each raise owns a fresh immutable entry, even when its words match an older one.
    attention: BTreeMap<AttentionReason, Arc<str>>,
    shown: Option<String>,
    /// The claim whose own attention entry was last settled. A new proven claim may inherit
    /// a killed image's keyed entry, even when this process has never shown any attention.
    attention_settled: u64,
    attention_retry: Option<Instant>,
    /// A failed startup-screen cleanup before this process's first proven claim. Its
    /// own deadline stays live while policy or claim authority is unavailable.
    startup_attention_retry: Option<Instant>,
    /// THE WORKER'S STANDING NEEDS, raised under `owner=needs kind=ask` (FOREVER CRUISE /
    /// FOREVER LAND: loud, never blocking; [`Supervisor::needs`]): the text wanted, the key of
    /// the set it says, and the text aterm shows. Kept apart from `attention`, so no box close,
    /// person-turn clear, passive spell or the upgrade's `left_to_person` ever reads or takes
    /// it down; only the worker's report, a person's or the config's off and the exit do.
    needs_want: Option<String>,
    needs_key: u64,
    needs_shown: Option<String>,
    /// The set `needs_shown` says, whole, in the order its entry names them (the first of them
    /// is the entry's): what aterm's readers read beside that entry ([`State::needs_told_want`]).
    needs_shown_set: Vec<String>,
    /// The worker's set as its root thread's report last named it (`needs_key`'s set).
    needs_named: Vec<String>,
    /// CODEX-ALAB'S OWN STANDING NEEDS, by reason and the root that raised each, as the
    /// `attention` reasons are kept ([`Supervisor::own_need_for`]: the quota's, B3), put first
    /// in the same set as the worker's, so each is a need of that one entry and none sits
    /// behind codex-alab's one shown `attention` key (an `api:` entry hid the quota's NEEDS
    /// YOU there). Their root's turn that ends past their wall takes them down, as it does
    /// the reason of the same key ([`Supervisor::clear_attention_reasons`]); the root's
    /// stopped runtime, an off and the exit too.
    own_needs: BTreeMap<AttentionReason, String>,
    /// Claims taken by this process, and the one whose `owner=needs` entry a sync last
    /// settled: while they differ ([`State::needs_unknown`]), aterm's entry may hold what this
    /// process never put there (a process before it that crashed or was killed, whose exit
    /// never took its needs down, or another holder of the session). The next sync settles it
    /// (the needs wanted put up in its place, or, none standing, the entry taken down), as
    /// aterm's `NEEDS_LEFT` does at its loop's start (aterm `448726c354`; recheck of
    /// 15dbc93c4, finding 7).
    claims_taken: u64,
    needs_settled: u64,
    needs_retry: Option<Instant>,
    /// CODEX-ALAB'S AUTOMATIC WAITS, by thread (amended 2026-10-27 after the parity sweep,
    /// gap 2: a codex tab waiting out a usage limit or an outage looked idle or stuck): the
    /// words a person should see of each thread's decided wait ([`crate::decide::Decision`]'s
    /// `shown_wait`) and when it ends, raised under `owner=codex-alab-wait kind=wait`
    /// ([`codex_alab_aterm::WAIT_OWNER`]: aterm's Waiting rows, never a need, never `await
    /// task` news) while the claim is held, and synced beside codex-alab's own attention, one
    /// at a time on a permit of its own (`waits_sync`). Kept apart from `attention`, whose one
    /// `owner=codex-alab` slot carries the asks.
    /// A thread's turn start, its decision that waits for nothing, its stopped runtime, and an
    /// Off, a standby, a lost claim and the exit take them down; a not-now re-arm keeps them.
    waits: BTreeMap<String, Waiting>,
    /// The wait text aterm shows, the claim whose entry a sync last settled (a killed image
    /// may have left one: [`State::waits_unknown`]), and the retry after a refused write.
    waits_shown: Option<String>,
    waits_settled: u64,
    waits_retry: Option<Instant>,
    /// RULING F'S HOLD IN ATERM'S `status held=` (amended 2026-10-28 after the parity sweep,
    /// gap 4 stage 3: aterm's `status held=` said `-` on a stopped codex-alab tab, as it reads
    /// only aterm's own loop): the arm each thread's standing hold is told by
    /// ([`crate::replies::Told::arm`]; a limit's names none), set only while codex-alab acts,
    /// and what aterm answered the last `harness_held` sent ([`ClaimSent`]), synced one at a
    /// time on a permit of its own (`held_sync`) on the claim's connection, only while the
    /// proven claim is held. The tab shows one arm ([`State::held_want`]). A thread's decision
    /// that is not held, its turn's end that lifts the hold, its stopped runtime, an Off (its
    /// park lifts every hold) and the exit take a thread's arm out; a lost claim keeps them,
    /// as the threads' holds stand: aterm drops the arm when the claim ends, but keeps it for
    /// this process's re-claim that reaches it first (it keys the arm by the claim's proof), so
    /// the next claim's first sync sends the standing one again, or takes a kept one down.
    holds: BTreeMap<String, HeldBy>,
    held_sent: Option<ClaimSent<HeldBy>>,
    /// LAND'S HOLD IN ATERM'S LANDING (amended 2026-10-28 after the parity sweep, gap 11: a
    /// codex-alab worker under Land showed no landing in aterm, whose `status land_held=`, menu
    /// bar and hold lines read only its own loop): each thread's hold
    /// ([`crate::decide::ThreadState::land_held`]), set only while codex-alab acts, and what
    /// aterm answered the last `harness_land` sent, synced as the arm is (`land_sync`, the
    /// primary thread's under Land: [`State::land_want`]). A thread's hold that ends (its
    /// landing, a person's turn or exchange, leaving Land, its stopped runtime), an Off and the
    /// exit take it out; a lost claim keeps them, and the next claim's first sync sends the
    /// standing one again, or takes a kept one down.
    lands: BTreeMap<String, LandReport>,
    land_sent: Option<ClaimSent<LandReport>>,
    /// THE WORKER'S NEEDS WHOLE IN ATERM (amended 2026-10-28 after the parity sweep, gap 16: a
    /// codex NEEDS YOU showed only its first need and a count, as aterm's menu bar, hover and
    /// `HarnessAsk::others` read every need whole only from the set its own loop tells): what
    /// aterm answered the last `harness_needs` sent, the set the shown `owner=needs` entry says
    /// ([`State::needs_told_want`]), synced as the arm is (`needs_told_sync`), after each sync
    /// of that entry. Where the entry is set or replaced the set it says goes up; where it comes
    /// down (a report's `NEEDS YOU: nothing`, a person's turn that met no wall, the exit) the
    /// set does too, the exit's after its entry and before its release, on the claim it still
    /// holds. AN OFF'S SET GOES WITH ITS RELEASE: aterm drops a self-claim's set with the claim,
    /// and the Off's entry stands until after that release, so an unset before it would change
    /// no reader (aterm's `ToldNeeds::tell` keeps the set outgoing beside the entry it says). A
    /// lost claim, an Off's included, leaves the next claim's first sync to send the standing
    /// set again, or take down one aterm kept for this process.
    needs_told_sent: Option<ClaimSent<NeedsTold>>,
    /// THE CLAIM ON WHICH ATERM HAD NO FACT TO READ (CONTRACT §5 "aterm fact", gap 23): the
    /// [`State::claims_taken`] whose `harness fact` read met an aterm before the verb (its
    /// `harness` usage, naming no `fact` form). While the claim standing is that one, no fact is
    /// read; a new claim, as a newer aterm holds after an update, asks once. No other answer
    /// latches.
    fact_unsupported: Option<u64>,
    /// The session's Autopilot word as `status` said it last ([`Supervisor::autopilot_word`]).
    last_word: Option<Mode>,
    /// The `status` reads in a row that named no acting mode, and when `status` was read
    /// for the word last ([`Supervisor::word_recheck_at`]).
    unsaid_reads: u32,
    word_read: Option<Instant>,
    /// Why the last approval box was left to a person.
    last_defer: Option<String>,
    /// When to look at the open question or plan box again.
    box_look: Option<Instant>,
    /// The answer posted for the open box, until the TUI took or refused it.
    posted: Option<Posted>,
    /// The record's resume-line count (`Shared::resume_lines`) aterm's copy has: read at the
    /// claim, then renewed by a re-confirm whenever the record writer moves it.
    confirmed_line: u64,
    /// The description this process last wrote or adopted from a predecessor: the
    /// exact standing text codex-alab may replace or withdraw, one write per change.
    described: Option<String>,
    /// The latest accepted description, published in registration order rather than
    /// the order its callers' asynchronous control reads happen to finish in.
    description_want: Option<Option<String>>,
    /// Exact runtime tokens for accepted intent and acknowledged remote text.
    /// They can differ while a later intent waits behind an in-flight write.
    description_owner: Option<Arc<()>>,
    described_owner: Option<Arc<()>>,
    /// A fresh identical-text adoption is a new acknowledged publication even
    /// when its runtime owner did not change.
    described_publication: Arc<()>,
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

    /// Count a `status` read's Autopilot word ([`Status::autopilot_said`]): a read that names
    /// no acting mode keeps the recheck going, and one naming a mode is the word read last.
    /// The mode it names, if any (`off` names none, [`Mode::of_status`]).
    fn read_word(&mut self, said: Option<codex_alab_aterm::status::Autopilot>) -> Option<Mode> {
        self.word_read = Some(clock());
        let Some(mode) = said.and_then(Mode::of_status) else {
            self.unsaid_reads = self.unsaid_reads.saturating_add(1);
            return None;
        };
        self.unsaid_reads = 0;
        self.last_word = Some(mode);
        Some(mode)
    }

    /// This claim has not yet reconciled the remote `owner=codex-alab` entry.
    fn attention_unknown(&self) -> bool {
        self.claims_taken != self.attention_settled
    }

    /// A first startup cleanup may finish before any proven claim or publication. Wanted
    /// but unpublished reasons do not own the remote entry and do not stop its cleanup.
    fn may_clear_startup_attention(&self) -> bool {
        self.claims_taken == 0 && self.shown.is_none()
    }

    /// aterm's `owner=needs` entry is unsettled since the last claim ([`State::claims_taken`]).
    fn needs_unknown(&self) -> bool {
        self.claims_taken != self.needs_settled
    }

    /// aterm's `owner=codex-alab-wait` entry is unsettled since the last claim: a killed image
    /// may have left one, which the next sync takes down or replaces.
    fn waits_unknown(&self) -> bool {
        self.claims_taken != self.waits_settled
    }

    /// The wait aterm shows. Every thread shares the one owner slot, so it is the SOONEST
    /// wait's words: when codex-alab next types into this tab. A tie goes to the thread whose
    /// key sorts first (the map iterates by key, and `min_by_key` keeps the first of equals),
    /// so the same waits always show the same words.
    fn wait_want(&self) -> Option<String> {
        self.waits
            .values()
            .min_by_key(|wait| wait.until)
            .map(|wait| wait.text.to_string())
    }

    /// THE ARM THE TAB SHOWS (`holds`): the PRIMARY thread's (`primary`, `View::primary_thread`),
    /// while codex-alab acts; none on standby, where nothing is typed or held back. The tab
    /// has one `held=`, and a lead or a person reads it as the tab's own agent stopped:
    /// the primary thread is the conversation the TUI shows and the session record, its
    /// recovery and its in-flight retry speak for. A Waiting row shows the soonest wait of any
    /// thread, as any thread's typing is typing into the tab, but a hold says codex-alab types
    /// no more, which is true of the tab only where the thread it speaks for holds; nor is it
    /// the description's last writer, which would let another root's stop stand over a primary
    /// thread that works. Another root's hold is told by its own appnotice and warning.
    fn held_want(&self, primary: Option<&str>) -> Option<HeldBy> {
        if !self.acting() {
            return None;
        }
        primary.and_then(|thread| self.holds.get(thread).copied())
    }

    /// LAND'S HOLD THE TAB SHOWS (`lands`): the primary thread's, as the arm is
    /// ([`Self::held_want`]), and only while the session's Autopilot reads Land, as aterm shows
    /// its own Land hold; none on standby.
    fn land_want(&self, primary: Option<&str>) -> Option<LandReport> {
        if !self.acting()
            || self
                .policy
                .as_ref()
                .is_none_or(|p| p.autopilot != Mode::Land)
        {
            return None;
        }
        primary.and_then(|thread| self.lands.get(thread).cloned())
    }

    /// The needs text wanted: codex-alab's own standing needs first, then the worker's named
    /// set, each a need of the one entry ([`needs_text`]). The same need two roots raised (one
    /// account's quota) is one need to a person.
    fn compose_needs(&mut self) {
        self.needs_want = needs_text(&self.needs_standing());
    }

    /// The needs standing, in the order [`Self::compose_needs`] names them.
    fn needs_standing(&self) -> Vec<String> {
        let mut standing: Vec<String> = Vec::new();
        for need in self.own_needs.values().chain(&self.needs_named) {
            if !standing.contains(need) {
                standing.push(need.clone());
            }
        }
        standing
    }

    /// THE NEEDS ATERM'S READERS READ WHOLE (`needs_told_sent`): the set the `owner=needs`
    /// entry aterm shows says, its first [`NEEDS_TOLD`] in order, beside that entry's text;
    /// none where no entry of this process's is shown. aterm reads the set only while the entry
    /// it names stands, so it follows the entry shown, never the one wanted.
    fn needs_told_want(&self) -> Option<NeedsTold> {
        let entry = self.needs_shown.clone()?;
        let needs: Vec<String> = self
            .needs_shown_set
            .iter()
            .take(NEEDS_TOLD)
            .cloned()
            .collect();
        (!needs.is_empty()).then_some(NeedsTold { entry, needs })
    }
}

/// The worker's needs as `harness_needs` tells them: the `owner=needs` entry's text and the set
/// it says, whole, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
struct NeedsTold {
    entry: String,
    needs: Vec<String>,
}

#[derive(Clone)]
struct DescriptionReceipt {
    text: String,
    owner: Option<Arc<()>>,
    publication: Arc<()>,
}

impl DescriptionReceipt {
    fn of(st: &State) -> Option<Self> {
        Some(Self {
            text: st.described.clone()?,
            owner: st.described_owner.clone(),
            publication: Arc::clone(&st.described_publication),
        })
    }

    fn stands(&self, st: &State) -> bool {
        st.described.as_deref() == Some(self.text.as_str())
            && same_description_owner(&st.described_owner, &self.owner)
            && Arc::ptr_eq(&st.described_publication, &self.publication)
    }
}

fn same_description_owner(left: &Option<Arc<()>>, right: &Option<Arc<()>>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => Arc::ptr_eq(left, right),
        (None, None) => true,
        _ => false,
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

/// Land's hold as the journal says it: its nudges and the legs read, in `status land_held=`'s
/// words (`-` for none read).
fn land_note(land: &LandReport) -> String {
    let legs = land.read.as_ref().map_or_else(
        || "-".to_string(),
        |read| {
            let words: Vec<&str> = read.legs.iter().map(|leg| leg.word()).collect();
            words.join(",")
        },
    );
    format!("nudges={} legs={legs}", land.nudges)
}

/// WHAT ATERM ANSWERED THE LAST SELF-CLAIM FIELD SENT (`harness_held`'s arm, `State::held_sent`;
/// `harness_land`'s hold, `State::land_sent`; `harness_needs`'s set, `State::needs_told_sent`),
/// and under which claim ([`State::claims_taken`]);
/// `None` while nothing is known (this image has had no answer, or the last call failed). aterm
/// keys each by the claim's proof, the pid and its kernel start (`SessionMeta::claim_held`),
/// which this process keeps across a re-claim and an exec: it drops them when the claim ends,
/// but keeps them for a re-claim that reaches it before that end (a reconnect's, taken before
/// it read the old connection's close; the next image's, after an exec that outran the
/// release). So an earlier claim's answer settles a new claim's only where it proved none
/// stands.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ClaimSent<T> {
    claim: u64,
    answer: ClaimAnswer<T>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ClaimAnswer<T> {
    /// `OK`: aterm shows this value (`None`: none).
    Shown(Option<T>),
    /// Any refusal but a denial (an aterm before the verb answers `ERR unknown meta field
    /// (…)`, as for any field it does not know): nothing shows, and the claim does not ask
    /// again until the value moves off this one.
    Unsupported(Option<T>),
    /// `ERR denied`: aterm proves no claim of this connection's; not asked again under it.
    Denied,
}

/// WHETHER `want` IS OWED TO ATERM under the claim `claim`, the last answer `sent`
/// ([`ClaimSent`]): only where it differs from what aterm answered under this claim. A new
/// claim's first sync sends the value wanted or takes down one aterm kept for this process,
/// unless the last answer, under any claim of this image, proved none stands (an unset taken,
/// a refusal, a denial: aterm's standing claim was not this process's, and the value went with
/// it); a value an earlier claim showed, an earlier image's (before an exec) and one a failed
/// call may have set prove nothing. A refusal of a value is not asked again under this claim
/// until the value moves (and a refused aterm shows nothing, so no unset is owed); a denial,
/// not under this claim at all.
fn claim_field_owed<T: PartialEq>(
    sent: &mut Option<ClaimSent<T>>,
    claim: u64,
    want: Option<&T>,
) -> bool {
    let none = ClaimAnswer::Shown(None);
    let settled = match sent.as_ref() {
        Some(sent) if sent.claim == claim => Some(&sent.answer),
        None
        | Some(ClaimSent {
            answer: ClaimAnswer::Shown(Some(_)),
            ..
        }) => None,
        Some(_) => Some(&none),
    };
    match settled {
        Some(ClaimAnswer::Denied) => false,
        Some(ClaimAnswer::Shown(shown) | ClaimAnswer::Unsupported(shown))
            if shown.as_ref() == want =>
        {
            false
        }
        Some(ClaimAnswer::Unsupported(_)) if want.is_none() => {
            // Nothing shows, so nothing comes down; the next value is asked again.
            *sent = Some(ClaimSent {
                claim,
                answer: ClaimAnswer::Unsupported(None),
            });
            false
        }
        _ => {
            // Unsettled until aterm answers: a call in flight or dropped mid-await may have
            // landed at aterm.
            *sent = None;
            true
        }
    }
}

/// One thread's automatic wait as a person should see it (`State::waits`).
#[derive(Debug, Clone)]
struct Waiting {
    /// When the wait ends: the decided act's `not_before`.
    until: Instant,
    /// Its words (`Decision::shown_wait`).
    text: Arc<str>,
}

/// An answer handed to the TUI's box glue.
#[derive(Debug, Clone)]
struct Posted {
    id: String,
    rule: &'static str,
    words: String,
    at: Instant,
    /// The root the answered plan belonged to when it was posted, before a thread switch.
    thread: Option<String>,
    /// Runtime retirement revokes only its reason-clear authority. An applied answer
    /// still keeps its outcome and publishes its original act exactly once.
    plan_current: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct AttentionReason {
    kind: &'static str,
    thread: Option<String>,
}

/// A preparatory read may answer only the exact reason entry it started with.
pub(crate) struct AttentionSnapshot {
    reason: AttentionReason,
    entry: Option<Arc<str>>,
}

impl AttentionReason {
    fn new(kind: &'static str, thread: Option<&str>) -> Self {
        Self {
            kind,
            thread: thread.map(str::to_owned),
        }
    }
}

/// What [`Supervisor::taken`] made of a claim aterm answered held.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Taken {
    /// Taken: codex-alab acts under it.
    Held,
    /// The exit had begun before the take: the caller waits for its cleanup and release,
    /// then gives back this actual claim even if it landed after that release.
    Refused,
    /// The exit began during the take, its locks after the claim was answered: it holds this
    /// claim's connection, where its own release follows its clears. Nothing is given back
    /// ahead of them, which would leave those clears to go with no claim held.
    LeftToExit,
}

/// The process's claim and its watch.
pub struct Supervisor {
    /// Itself, for the tasks it starts (the badge reader).
    me: Weak<Supervisor>,
    opts: SupervisorOptions,
    state: Mutex<State>,
    /// Policy refresh and an unaccepted-claim cleanup share one order. Never acquire
    /// this while holding State; passive cleanup keeps it until its release finishes.
    policy_refresh: tokio::sync::Semaphore,
    /// THE LINE'S TIMER READS GO ONE AT A TIME ([`Supervisor::status_with_timer`]): each
    /// publishes the `land_at=` its own reply carried, so no older reply's timer is stored
    /// after a newer one's. Never acquire this while holding State.
    timer_reads: tokio::sync::Semaphore,
    /// A badge read is running on its own task; another is wanted after it.
    badge_busy: AtomicBool,
    badge_again: AtomicBool,
    started: AtomicBool,
    /// The process is leaving: no new claim is accepted locally.
    closing: AtomicBool,
    /// The one exit cleanup's eventual outcome, retained for claims answered after it.
    exit_result: watch::Sender<Option<bool>>,
    /// The passive clock's period ([`POLICY_RECHECK`]; tests shorten it), in milliseconds.
    recheck_ms: AtomicU64,
    /// How soon a failed send of owed receipts and reports is tried again ([`OWED_RETRY`];
    /// tests shorten it), in milliseconds.
    owed_retry_ms: AtomicU64,
    /// The word's clock's first step ([`WORD_RECHECK`]; tests set it), in milliseconds.
    word_recheck_ms: AtomicU64,
    /// How soon a wait with no deadline of its own looks again ([`RECHECK`]; tests lengthen
    /// it, so what ends a wait sooner is seen to), in milliseconds.
    wait_recheck_ms: AtomicU64,
    /// How soon an act that met a hold reads `status` again ([`HOLD_RECHECK`]; tests lengthen
    /// it with [`Self::wait_recheck_ms`]), in milliseconds.
    hold_recheck_ms: AtomicU64,
    /// A retry of the owed sends waits on its timer.
    owed_retry_armed: AtomicBool,
    /// A startup screen's attention an earlier start left standing (a sign-in, a folder trust)
    /// was looked for (once a process, on its first session).
    login_looked: AtomicBool,
    /// Attention syncs run one at a time: each compares what it wants with what the last one
    /// showed, so a standby's or a release's clear never compares against a raise still in
    /// flight (it would send nothing, and the raise would then land under the off).
    attention_sync: tokio::sync::Semaphore,
    /// The `owner=codex-alab-wait` entry's syncs run one at a time by the same rule, on a
    /// permit of their own: a wait's write never holds up an ask's, nor the root's reason a
    /// native turn's start clears inline (review of f134d2b2cb, finding 4). The exit takes
    /// both.
    waits_sync: tokio::sync::Semaphore,
    /// The `harness_held` syncs run one at a time by the same rule, on a permit of their own.
    held_sync: tokio::sync::Semaphore,
    /// And the `harness_land` syncs, on one of theirs.
    land_sync: tokio::sync::Semaphore,
    /// And the `harness_needs` syncs, on one of theirs; the exit's take-down waits for none
    /// ([`Supervisor::tell_needs`]).
    needs_told_sync: tokio::sync::Semaphore,
    /// An external publisher armed a metadata deadline while the session was waiting.
    attention_wake: tokio::sync::Notify,
    /// Description writes and passive withdrawals share one order. Desired state is
    /// accepted synchronously; a queued publisher always reads its latest value.
    description_sync: tokio::sync::Semaphore,
    /// Every registry shares this process claim and its task face. Acquire
    /// before policy/native/state locks, never while holding one of them.
    task_face_publication: tokio::sync::Mutex<()>,
    /// Sends of owed receipts and reports run one at a time (a turn's end, the badge reader):
    /// a send that finds its turn's receipt or report taken by another waits for that one to
    /// finish, so when a turn's end returns, what it owed has gone.
    mail_sync: tokio::sync::Semaphore,
    /// Bumped by the event reader on every `meta-change field=supervisor` not naming
    /// codex-alab (and every gap): an act whose re-confirm straddled one does not go.
    lost_epoch: Arc<AtomicU64>,
    /// A foreign holder was observed during this process's startup lifetime. The reader
    /// records it before queueing events, so a later release/reconnect cannot revive cleanup.
    startup_foreign: Arc<AtomicBool>,
    /// Bumped by every release codex-alab sends while aterm showed the claim its own (whose
    /// `value=-` echo then moves `lost_epoch` too): a straddled re-confirm a release of its own
    /// may explain gives the claim back without counting it someone else's clear.
    released: AtomicU64,
    /// Bumped by every person's or config's Off codex-alab met under its claim
    /// ([`Supervisor::release_for_off`]): each thread's memory parks for it ([`Supervisor::offs`]).
    offs: AtomicU64,
    claims: watch::Sender<u64>,
    /// Per-instance seam: native race fixtures can hold a read without a latency race.
    #[cfg(test)]
    hand_read_limit: Mutex<Duration>,
    /// Per-instance seam: the answers [`Supervisor::person_input_ago`] gives in turn, before any
    /// connection is read (`Some(None)`: no hand read), so a test plays a read that raced the
    /// supervisor's connection with no aterm at all.
    #[cfg(test)]
    hand_answers: Mutex<std::collections::VecDeque<Option<Duration>>>,
    /// Observe an actual auxiliary client without controlling its shutdown.
    #[cfg(test)]
    description_connection: Mutex<Option<tokio::sync::oneshot::Sender<Client>>>,
    /// Pause the winning exit after publishing closing and before starting async cleanup.
    #[cfg(test)]
    exit_before_cleanup: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    /// Pause an actual received claim event before publishing any admission state.
    #[cfg(test)]
    before_claim_event: Arc<Mutex<Option<ClaimEventHook>>>,
    /// Observe the exact reader-retirement boundary, after requesting its abort.
    #[cfg(test)]
    reader_retiring: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

#[cfg(test)]
type ClaimEventHook = Box<dyn FnOnce() + Send>;

enum SessionEnd {
    Retry(Option<Duration>),
    Closing,
}

/// Queue ownership keeps a clear pending through classification, including awaits.
/// Send failure and a dropped old connection's queue settle their tokens as well.
#[derive(Default)]
struct ClaimEvents {
    pending: AtomicU64,
    drained: tokio::sync::Notify,
    admission_epoch: AtomicU64,
    admission_pending: AtomicU64,
}

/// Mark relevant authority changes in the independent reader before their
/// owning session task can wait on policy transport. Own metadata echoes do
/// not invalidate an admission or keep its Ready check perpetually pending.
struct AdmissionEvent(Arc<ClaimEvents>);

impl AdmissionEvent {
    fn for_event(events: &Arc<ClaimEvents>, event: &Event, holder: &str) -> Option<Self> {
        let changed = moves_effective(event)
            || matches!(
                event,
                Event::HarnessPolicy { .. }
                    | Event::Gap
                    | Event::Human
                    | Event::Agent { .. }
                    | Event::Closing
                    | Event::Exited
            )
            || matches!(claim_signal(event, holder), Signal::Lost { .. });
        if !changed {
            return None;
        }
        events.admission_pending.fetch_add(1, Ordering::SeqCst);
        events.admission_epoch.fetch_add(1, Ordering::SeqCst);
        Some(Self(Arc::clone(events)))
    }
}

impl Drop for AdmissionEvent {
    fn drop(&mut self) {
        self.0.admission_pending.fetch_sub(1, Ordering::SeqCst);
    }
}

struct ClaimEvent(Arc<ClaimEvents>);

impl ClaimEvent {
    fn new(pending: &Arc<ClaimEvents>) -> Self {
        pending.pending.fetch_add(1, Ordering::SeqCst);
        Self(Arc::clone(pending))
    }
}

impl Drop for ClaimEvent {
    fn drop(&mut self) {
        if self.0.pending.fetch_sub(1, Ordering::SeqCst) == 1 {
            self.0.drained.notify_one();
        }
    }
}

#[cfg(test)]
#[path = "admission_fence_tests.rs"]
mod admission_fence_tests;

#[cfg(test)]
#[path = "attention_reconnect_tests.rs"]
mod attention_reconnect_tests;

#[cfg(test)]
#[path = "attention_retry_tests.rs"]
mod attention_retry_tests;

#[cfg(test)]
#[path = "startup_attention_tests.rs"]
mod startup_attention_tests;

#[cfg(test)]
#[path = "description_connection_tests.rs"]
mod description_connection_tests;

#[cfg(test)]
#[path = "exit_release_tests.rs"]
mod exit_release_tests;

#[cfg(test)]
#[path = "claim_publication_tests.rs"]
mod claim_publication_tests;

#[cfg(test)]
#[path = "claim_release_order_tests.rs"]
mod claim_release_order_tests;

impl Supervisor {
    pub fn new(opts: SupervisorOptions) -> Arc<Supervisor> {
        let (claims, _) = watch::channel(0);
        let (exit_result, _) = watch::channel(None);
        Arc::new_cyclic(|me| Supervisor {
            me: me.clone(),
            opts,
            state: Mutex::new(State::default()),
            policy_refresh: tokio::sync::Semaphore::new(1),
            timer_reads: tokio::sync::Semaphore::new(1),
            badge_busy: AtomicBool::new(false),
            badge_again: AtomicBool::new(false),
            started: AtomicBool::new(false),
            closing: AtomicBool::new(false),
            exit_result,
            recheck_ms: AtomicU64::new(
                u64::try_from(POLICY_RECHECK.as_millis()).unwrap_or(u64::MAX),
            ),
            owed_retry_ms: AtomicU64::new(
                u64::try_from(OWED_RETRY.as_millis()).unwrap_or(u64::MAX),
            ),
            word_recheck_ms: AtomicU64::new(
                u64::try_from(WORD_RECHECK.as_millis()).unwrap_or(u64::MAX),
            ),
            wait_recheck_ms: AtomicU64::new(u64::try_from(RECHECK.as_millis()).unwrap_or(u64::MAX)),
            hold_recheck_ms: AtomicU64::new(
                u64::try_from(HOLD_RECHECK.as_millis()).unwrap_or(u64::MAX),
            ),
            owed_retry_armed: AtomicBool::new(false),
            login_looked: AtomicBool::new(false),
            attention_sync: tokio::sync::Semaphore::new(1),
            waits_sync: tokio::sync::Semaphore::new(1),
            held_sync: tokio::sync::Semaphore::new(1),
            land_sync: tokio::sync::Semaphore::new(1),
            needs_told_sync: tokio::sync::Semaphore::new(1),
            attention_wake: tokio::sync::Notify::new(),
            description_sync: tokio::sync::Semaphore::new(1),
            task_face_publication: tokio::sync::Mutex::new(()),
            mail_sync: tokio::sync::Semaphore::new(1),
            lost_epoch: Arc::new(AtomicU64::new(0)),
            startup_foreign: Arc::new(AtomicBool::new(false)),
            released: AtomicU64::new(0),
            offs: AtomicU64::new(0),
            claims,
            #[cfg(test)]
            hand_read_limit: Mutex::new(HAND_READ_LIMIT),
            #[cfg(test)]
            hand_answers: Mutex::new(std::collections::VecDeque::new()),
            #[cfg(test)]
            description_connection: Mutex::new(None),
            #[cfg(test)]
            exit_before_cleanup: Mutex::new(None),
            #[cfg(test)]
            before_claim_event: Arc::new(Mutex::new(None)),
            #[cfg(test)]
            reader_retiring: Mutex::new(None),
        })
    }

    /// How many person's or config's Offs codex-alab met under its claim: a thread whose
    /// memory saw fewer parks it (`decide::ThreadState::park_off`), the needs that Off took
    /// off the person's screen included.
    pub fn offs(&self) -> u64 {
        self.offs.load(Ordering::SeqCst)
    }

    /// Count one Off as [`Supervisor::release_for_off`] does first, and nothing else (tests:
    /// an Off that begins while a probe runs).
    #[cfg(test)]
    pub(crate) fn count_off(&self) {
        self.offs.fetch_add(1, Ordering::SeqCst);
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
        st.acting()
            && st.client.as_ref().is_some_and(Client::holds_claim)
            && self.opts.shared.autopilot() == AutopilotState::Active
    }

    /// Set the passive clock's period (default [`POLICY_RECHECK`]); before [`Supervisor::start`].
    pub fn set_policy_recheck(&self, every: Duration) {
        self.recheck_ms.store(
            u64::try_from(every.as_millis()).unwrap_or(u64::MAX).max(1),
            Ordering::SeqCst,
        );
    }

    /// Set the word's clock's first step (default [`WORD_RECHECK`], doubling up to the passive
    /// clock's period).
    pub fn set_word_recheck(&self, first: Duration) {
        self.word_recheck_ms.store(
            u64::try_from(first.as_millis()).unwrap_or(u64::MAX).max(1),
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

    /// Set how soon a wait with no deadline of its own (a hold, a draft, a popup, an unreadable
    /// status) looks again (default [`RECHECK`], and [`HOLD_RECHECK`] for a hold).
    pub fn set_recheck(&self, every: Duration) {
        let ms = u64::try_from(every.as_millis()).unwrap_or(u64::MAX).max(1);
        self.wait_recheck_ms.store(ms, Ordering::SeqCst);
        self.hold_recheck_ms.store(ms, Ordering::SeqCst);
    }

    /// How soon a wait with no deadline of its own looks again ([`Supervisor::set_recheck`]).
    fn recheck(&self) -> Duration {
        Duration::from_millis(self.wait_recheck_ms.load(Ordering::SeqCst))
    }

    /// How soon an act that met a hold reads `status` again ([`HOLD_RECHECK`]).
    fn hold_recheck(&self) -> Duration {
        Duration::from_millis(self.hold_recheck_ms.load(Ordering::SeqCst))
    }

    /// The before-every-act checks met `status hold=1`, their checks begun at `on_reads`
    /// `effective=on` reads ([`State::on_reads`]): the next such read has every thread look
    /// again. Answers whether one came since they began: then the hold `status` showed may
    /// already be gone, and the checks look again at once. Under one lock with
    /// [`Supervisor::read_on`], so a lift is never met by neither.
    pub(crate) fn met_hold(&self, on_reads: u64) -> bool {
        let mut st = self.lock();
        st.hold_met = true;
        st.on_reads != on_reads
    }

    /// An `effective=on` read at `generation`: counted ([`State::on_reads`]), and whether it
    /// finds a hold lifted (the read before it said `why=hold`, or an act's checks met `status
    /// hold=1` since the last one), so the threads waiting on it look again now.
    pub(crate) fn read_on(&self, generation: u64) -> bool {
        let mut st = self.lock();
        let lifted = std::mem::take(&mut st.hold) | std::mem::take(&mut st.hold_met);
        st.on_reads = st.on_reads.wrapping_add(1);
        st.generation = Some(generation);
        lifted
    }

    /// How many `effective=on` reads there were (tests replay a lift met mid-check).
    #[cfg(test)]
    pub(crate) fn on_reads(&self) -> u64 {
        self.lock().on_reads
    }

    /// Why codex-alab is passive under the proven claim it keeps (`None`: it acts, or no
    /// claim is held).
    pub fn standby(&self) -> Option<String> {
        let st = self.lock();
        if st.held { st.standby.clone() } else { None }
    }

    /// WHETHER A NETWORK WALL'S WAIT IS CODEX-ALAB'S TO MEASURE NOW: it acts ([`Self::acting`])
    /// and its policy retries API errors and lets the API be probed (aterm's `probe_api`, as
    /// its host's `reach` asks its probe only with that switch on: gap 30), as aterm's loop asks
    /// for a route's measure only while it acts at the wall. A passive spell (Off, a standby, a
    /// claim not proven), retries off or the probe off lets the route's lease lapse (parity-3
    /// review, F6, F7), and the probe off holds back a probe that lease would still run
    /// ([`Self::probe_off`]).
    pub fn measures_walls(&self) -> bool {
        let st = self.lock();
        st.acting()
            && st.client.as_ref().is_some_and(Client::holds_claim)
            && st
                .policy
                .as_ref()
                .is_some_and(|p| p.retry_api_errors && p.probe_api)
            && self.opts.shared.autopilot() == AutopilotState::Active
    }

    /// WHETHER aterm'S `probe_api` IS OFF in the policy codex-alab holds (gap 30): no probe
    /// runs, however a lease taken before it went off still stands
    /// ([`crate::netwatch::MayProbe`]). With none held (none read yet, a standby, no claim)
    /// nothing says it is off, and a lease lapses as a passive spell's does.
    pub fn probe_off(&self) -> bool {
        self.lock().policy.as_ref().is_some_and(|p| !p.probe_api)
    }

    /// The policy, only while a proven claim is held and codex-alab acts under it.
    pub fn policy(&self) -> Option<Policy> {
        if codex_alab_core::is_alab_binary() && !codex_alab_core::active() {
            return None;
        }
        let st = self.lock();
        if st.acting() && st.client.as_ref().is_some_and(Client::holds_claim) {
            st.policy.clone()
        } else {
            None
        }
    }

    /// The upgrade's final synchronous commit. Hold the policy/claim lock through the
    /// native-state commit: a policy disabling upgrades cannot race its leave request.
    pub(crate) fn commit_upgrade(&self, checked: &Policy, native_generation: u64) -> bool {
        let Ok(_publication) = self.task_face_publication.try_lock() else {
            return false;
        };
        let st = self.lock();
        let grace_ms = u64::try_from(checked.human_grace.as_millis()).unwrap_or(u64::MAX);
        let quiet = self
            .opts
            .shared
            .last_human_input_ms()
            .is_none_or(|at| now_ms().saturating_sub(at) >= grace_ms)
            && st.since.is_none_or(|since| {
                clock().saturating_duration_since(since) >= checked.human_grace
            });
        st.acting()
            && !st.hold
            && !st.hold_met
            && quiet
            && st.policy.as_ref() == Some(checked)
            && checked.upgrade
            && st.client.as_ref().is_some_and(Client::holds_claim)
            && self.opts.shared.commit_upgrade(native_generation)
    }

    /// A pending task-face write settles before any registry starts native work.
    pub(crate) async fn task_face_publication(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.task_face_publication.lock().await
    }

    /// The claim's connection, while connected (tests drive the transitions on it).
    pub(crate) fn renewal_client(&self) -> Option<Client> {
        let state = self.lock();
        if !state.acting() {
            return None;
        }
        state.client.clone().filter(Client::holds_claim)
    }

    /// The claim's connection, while connected (tests drive the transitions on it).
    #[cfg(test)]
    pub(crate) fn client(&self) -> Option<Client> {
        self.lock().client.clone()
    }

    /// Receipt acknowledgment uses only an existing native claim, independently
    /// of acting policy. It never acquires or reconfirms execution authority.
    pub(crate) fn renewal_ack_client(&self) -> Option<Client> {
        self.lock().client.clone().filter(Client::holds_claim)
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
        // Outside an aterm session (or outside the codex binary) nothing is ever started.
        if (self.opts.endpoint)().is_none() {
            self.note("no aterm session: the autopilot stays off");
            return;
        }
        if self.started.swap(true, Ordering::SeqCst) {
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
            // Register before publishing closing: a refused take can then wait for this
            // same operation, even when the exit's caller exhausts its own wait budget.
            let finished = me.exit_result.subscribe();
            let captured = {
                let mut st = me.lock();
                if me.closing.swap(true, Ordering::SeqCst) {
                    None
                } else {
                    // Publish closing and capture its connection/debt in one State operation.
                    // Session retirement and a take's closing-induced loss use this same lock,
                    // so neither can discard the client or accepted claim before this snapshot.
                    let client = st.client.clone().filter(Client::is_open);
                    let described = st.described.take();
                    st.needs_want = None;
                    st.needs_key = 0;
                    st.needs_named.clear();
                    st.own_needs.clear();
                    let needs_unsettled = st.needs_unknown() && st.held;
                    st.needs_settled = st.claims_taken;
                    st.attention.clear();
                    let own_unsettled = st.attention_unknown() && st.held;
                    st.attention_settled = st.claims_taken;
                    st.waits.clear();
                    let waits_unsettled = st.waits_unknown() && st.held;
                    st.waits_settled = st.claims_taken;
                    // Nobody is left to hold a thread; the release drops aterm's arm with the
                    // claim, so none is sent (an exec that outruns the release leaves it to the
                    // next image's first sync).
                    st.holds.clear();
                    st.lands.clear();
                    Some((
                        client,
                        described,
                        needs_unsettled,
                        own_unsettled,
                        waits_unsettled,
                    ))
                }
            };
            if let Some((client, described, needs_unsettled, own_unsettled, waits_unsettled)) =
                captured
            {
                #[cfg(test)]
                me.before_exit_cleanup();
                me.lost_locally("the process is leaving");
                if let Some(client) = client {
                    let leaving = Arc::clone(&me);
                    runtime.spawn(async move {
                        {
                            let _one = leaving.description_sync.acquire().await;
                            if let Some(text) = described {
                                let _ = withdraw_description(&client, &text).await;
                            }
                        }
                        {
                            // After the sync in flight, if any: aterm applied its set before it
                            // answered, and an exit that read what was shown first left the entry up
                            // with nobody to carry on beside it.
                            let _one = leaving.attention_sync.acquire().await;
                            let _waits = leaving.waits_sync.acquire().await;
                            let (needs, own, waits) = {
                                let mut st = leaving.lock();
                                st.needs_shown_set.clear();
                                (
                                    st.needs_shown.take().is_some(),
                                    st.shown.take().is_some(),
                                    st.waits_shown.take().is_some(),
                                )
                            };
                            if needs || needs_unsettled {
                                let _ = client
                                    .clear_attention_owned(codex_alab_aterm::NEEDS_OWNER)
                                    .await;
                            }
                            if own || own_unsettled {
                                let _ = client.clear_attention().await;
                            }
                            // Nobody is left to type when the wait ends.
                            if waits || waits_unsettled {
                                let _ = client
                                    .clear_attention_owned(codex_alab_aterm::WAIT_OWNER)
                                    .await;
                            }
                        }
                        // The set the needs entry said goes with it, on the claim still held.
                        leaving.tell_needs(&client, true).await;
                        let released = match client.release().await {
                            Ok(()) => {
                                leaving.note("the claim was given back at the exit");
                                true
                            }
                            Err(e) => {
                                leaving
                                    .note(format!("the claim's release at the exit failed: {e}"));
                                false
                            }
                        };
                        leaving.exit_result.send_replace(Some(released));
                    });
                } else {
                    me.exit_result.send_replace(Some(true));
                }
            }
            let completed = *finished.borrow();
            if let Some(completed) = completed {
                return completed;
            }
            let (tx, rx) = std::sync::mpsc::channel();
            runtime.spawn(async move {
                let _ = tx.send(Self::exit_finished(finished).await);
            });
            match rx.recv_timeout(within) {
                Ok(released) => released,
                Err(_) => {
                    me.note("the claim's release at the exit was not answered in time");
                    false
                }
            }
        }));
    }

    #[cfg(test)]
    fn before_exit_cleanup(&self) {
        let hook = self
            .exit_before_cleanup
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(hook) = hook {
            hook();
        }
    }

    /// Wait for the one exit operation, without borrowing its watch value across a yield.
    async fn exit_finished(mut finished: watch::Receiver<Option<bool>>) -> bool {
        loop {
            let result = *finished.borrow_and_update();
            if let Some(result) = result {
                return result;
            }
            if finished.changed().await.is_err() {
                return false;
            }
        }
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

    /// Whether the process is leaving (no new claim is accepted locally).
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

    /// Full-runtime supervision waits for native settlement, then asks the host
    /// to adopt its independent socket under the still-live fresh constructor.
    async fn connect_control(&self, endpoint: Endpoint) -> Result<Client, AtermError> {
        if codex_alab_tui::upgrade_state::pending().is_some() {
            return codex_alab_aterm::source_member::connect_settled_runtime_client(
                endpoint,
                self.opts.client.clone(),
            )
            .await;
        }
        Client::connect(endpoint, self.opts.client.clone()).await
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
        let client = match self.connect_control(endpoint).await {
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
            None => self.connect_control(endpoint).await,
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
                client.closed().await;
                return SessionEnd::Retry(None);
            }
        };
        *tries = 0;
        // A new connection is a new start: a claim someone cleared on the old one may be
        // taken again once the policy says so, and a newer aterm may answer the policy. No
        // echo of a release on the old stream comes on this one.
        let queued_claim_events = Arc::new(ClaimEvents::default());
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
        // The push stream is read on its own task: a line read is not cancel-safe, and the
        // loop below also wakes for the TUI and for box deadlines. The reader marks a lost
        // claim at once, so an act whose re-confirm straddled it does not go.
        let (events_tx, mut events_rx) = tokio::sync::mpsc::unbounded_channel();
        let lost_epoch = Arc::clone(&self.lost_epoch);
        let startup_foreign = Arc::clone(&self.startup_foreign);
        let holder = client.holder().to_string();
        let reader_claim_events = Arc::clone(&queued_claim_events);
        #[cfg(test)]
        let before_claim_event = Arc::clone(&self.before_claim_event);
        let reader = tokio::spawn(async move {
            let mut events = events;
            while let Some(event) = events.next().await {
                let admission = AdmissionEvent::for_event(&reader_claim_events, &event, &holder);
                let pending = if let Signal::Lost { by } = claim_signal(&event, &holder) {
                    #[cfg(test)]
                    {
                        let hook = before_claim_event
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .take();
                        if let Some(hook) = hook {
                            hook();
                        }
                    }
                    let pending = ClaimEvent::new(&reader_claim_events);
                    if by.is_some() {
                        startup_foreign.store(true, Ordering::SeqCst);
                    }
                    lost_epoch.fetch_add(1, Ordering::SeqCst);
                    Some(pending)
                } else {
                    None
                };
                if events_tx.send((event, pending, admission)).is_err() {
                    return;
                }
            }
        });
        self.clear_stale_screen(&client).await;
        {
            let mut st = self.lock();
            st.client = Some(client.clone());
            st.queued_claim_events = Arc::clone(&queued_claim_events);
        }
        self.refresh(&client).await;
        self.check_boxes(&client).await;
        let mut was_vanilla = codex_alab_core::is_alab_binary() && !codex_alab_core::active();
        let end = loop {
            let deadline = self.box_deadline();
            let recheck = self.policy_recheck_at();
            let word = self.word_recheck_at();
            let attention = self.attention_retry_at();
            let startup_attention = self.startup_attention_retry_at();
            tokio::select! {
                event = events_rx.recv() => {
                    let Some((event, pending, _admission)) = event else {
                        break SessionEnd::Retry(None);
                    };
                    if self.closing() {
                        break SessionEnd::Closing;
                    }
                    if matches!(event, Event::Gap) {
                        self.after_gap(&client).await;
                        drop(pending);
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
                                let mut st = self.lock();
                                st.busy = true;
                                st.startup_attention_retry = None;
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
                            self.give_back_if_unheld(&client).await;
                        }
                        Signal::PolicyChanged { .. } => {
                            self.lock().cleared = false;
                            self.refresh(&client).await;
                            self.check_boxes(&client).await;
                        }
                        Signal::Ended => break SessionEnd::Closing,
                        Signal::Human | Signal::Nothing => {}
                    }
                    drop(pending);
                }
                () = queued_claim_events.drained.notified() => {
                    if !self.closing()
                        && queued_claim_events.pending.load(Ordering::SeqCst) == 0 {
                        self.resume_after_claim_events(&client).await;
                        self.check_boxes(&client).await;
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
                    let vanilla = codex_alab_core::is_alab_binary() && !codex_alab_core::active();
                    if vanilla || was_vanilla != vanilla {
                        self.refresh(&client).await;
                    }
                    was_vanilla = vanilla;
                    if vanilla {
                        continue;
                    }
                    self.renew_for_record(&client).await;
                    // The primary thread may have moved: the tab shows its arm and its hold.
                    self.sync_held(&client).await;
                    self.sync_land(&client).await;
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
                () = sleep_until_opt(word), if word.is_some() => {
                    self.recheck_word(&client).await;
                }
                () = self.attention_wake.notified() => {},
                () = sleep_until_opt(attention), if attention.is_some() => {
                    self.retry_attention(&client).await;
                }
                () = sleep_until_opt(startup_attention), if startup_attention.is_some() => {
                    self.reconcile_startup_attention(&client).await;
                }
            }
        };
        reader.abort();
        #[cfg(test)]
        {
            let hook = self
                .reader_retiring
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            if let Some(hook) = hook {
                hook();
            }
        }
        // Abort cannot interrupt a running poll. Finish its global publications
        // before any closing return or replacement connection can proceed.
        let _ = reader.await;
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
        // close() only notifies the request task. Retire its actual socket before
        // a replacement connection can claim and publish for this session.
        client.closed().await;
        self.lock().client = None;
        end
    }

    /// A startup screen's attention an earlier start left standing (a SIGTERM, a SIGKILL, a
    /// panic or a restore on a sign-in, folder-trust, model-migration or hooks-review screen,
    /// which ends before its clear): this start is past them (its primary thread is
    /// registered), so it clears codex-alab's own entry while it reads as one of those
    /// screens' texts, and nothing else (`codex_alab_tui::waiting::stale`). Looked for
    /// once a process, on its first session. A transient failure keeps that cleanup pending
    /// on its own deadline until it answers or a claim/publication takes its custody away.
    /// A reconnect after that retirement never scans again (review of fea034d20, finding 4).
    async fn clear_stale_screen(&self, client: &Client) {
        if self.login_looked.swap(true, Ordering::SeqCst) {
            return;
        }
        self.reconcile_startup_attention(client).await;
    }

    /// Reconcile only the first startup cleanup, serialized with the current process's
    /// publications. Each attempt reads the current text; no remembered text grants a
    /// later clear, and a proven claim permanently retires this startup authority.
    async fn reconcile_startup_attention(&self, client: &Client) {
        let _one = self.attention_sync.acquire().await;
        {
            let mut st = self.lock();
            st.startup_attention_retry = None;
            if self.closing()
                || self.startup_foreign.load(Ordering::SeqCst)
                || !st.may_clear_startup_attention()
            {
                return;
            }
        }
        let read = client.meta().await;
        {
            let st = self.lock();
            if self.closing()
                || !st.may_clear_startup_attention()
                || self.startup_foreign.load(Ordering::SeqCst)
            {
                return;
            }
        }
        let result = match read {
            Ok(fields) => {
                // An omitted holder is unknown; only the explicit `-` says it is empty.
                if !fields.contains_key("supervisor") {
                    self.defer_startup_attention();
                    return;
                }
                let holder = codex_alab_aterm::wire::field(&fields, "supervisor");
                if holder
                    .as_deref()
                    .is_some_and(|holder| !holder.trim().is_empty() && holder != client.holder())
                {
                    self.startup_foreign.store(true, Ordering::SeqCst);
                    return;
                }
                let owner = codex_alab_aterm::wire::field(&fields, "attention_owner");
                let attention = codex_alab_aterm::wire::field(&fields, "attention");
                if fields.get("attention").is_some_and(|value| value == "-") && owner.is_none() {
                    return;
                }
                if holder
                    .as_deref()
                    .is_some_and(|holder| holder.trim().is_empty())
                    || attention
                        .as_deref()
                        .is_none_or(|attention| attention.trim().is_empty())
                    || owner.as_deref() != Some(codex_alab_aterm::ATTENTION_OWNER)
                {
                    // Hidden entries and incomplete/empty tokens prove no replacement or
                    // foreign holder. Leave the key alone and retry on the independent clock.
                    self.defer_startup_attention();
                    return;
                }
                if !codex_alab_tui::waiting::stale(&fields) {
                    return;
                }
                client.clear_attention().await
            }
            Err(error) => Err(error),
        };
        match result {
            Ok(()) => self.note("cleared a startup screen's attention an earlier start left"),
            Err(e) => {
                self.defer_startup_attention();
                self.note(format!(
                    "a stale startup screen's attention was not looked for: {e}"
                ));
            }
        }
    }

    /// Keep the initial debt across an unanswered read, refused clear or hidden key. A
    /// closed socket does not cancel it; the next connection can finish the same cleanup.
    fn defer_startup_attention(&self) {
        let mut st = self.lock();
        if !self.closing()
            && st.may_clear_startup_attention()
            && !self.startup_foreign.load(Ordering::SeqCst)
        {
            st.startup_attention_retry = Some(clock() + ATTENTION_RETRY);
        }
        drop(st);
        self.attention_wake.notify_one();
    }

    /// Startup cleanup has no acting authority and does not borrow the policy clock or
    /// the claimed attention-retry clock: initial Off/unproven/older-policy starts can owe it.
    fn startup_attention_retry_at(&self) -> Option<tokio::time::Instant> {
        let mut st = self.lock();
        if self.closing()
            || self.startup_foreign.load(Ordering::SeqCst)
            || !st.may_clear_startup_attention()
        {
            st.startup_attention_retry = None;
            return None;
        }
        st.startup_attention_retry
            .map(tokio::time::Instant::from_std)
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
            if !st.held
                || st.confirmed_line == line
                || !client.holds_claim()
                || st.queued_claim_events.pending.load(Ordering::SeqCst) > 0
            {
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

    /// Once queued clears are classified, a coalesced TUI change still owes its renewal
    /// and ordinary decisions must look again without waiting for another frame.
    async fn resume_after_claim_events(&self, client: &Client) {
        {
            let st = self.lock();
            if !st.held || st.queued_claim_events.pending.load(Ordering::SeqCst) > 0 {
                return;
            }
        }
        self.renew_for_record(client).await;
        self.claims.send_modify(|n| *n += 1);
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
        let _one = self.policy_refresh.acquire().await;
        if self.closing() {
            return;
        }
        if codex_alab_core::is_alab_binary() && !codex_alab_core::active() {
            if client.holds_claim() {
                self.release_for_off(client, "vanilla mode".into(), HarnessWord::Unknown)
                    .await;
            }
            return;
        }
        self.lock().policy_read = Some(clock());
        let answer = client.harness_policy().await;
        // Publish support and classify an unknown reply under one lock. An
        // overlapping older refusal cannot re-latch unsupported after support
        // was established, even before the supported branch takes a claim.
        let known_support = {
            let mut st = self.lock();
            match &answer {
                Ok(PolicyAnswer::Supported(policy)) => {
                    st.generation = Some(policy.generation);
                    st.unsupported = false;
                    true
                }
                Ok(PolicyAnswer::Unsupported(_)) => {
                    let known = st.generation.is_some();
                    if !known {
                        st.unsupported = true;
                    }
                    known
                }
                Err(_) => false,
            }
        };
        match answer {
            Ok(PolicyAnswer::Supported(p)) if p.effective => {
                let lifted = self.read_on(p.generation);
                let mut policy = Policy::from_harness(&p);
                policy.autopilot = self.autopilot_word(client).await;
                let word = policy.autopilot.word();
                let chrome = acting_word(policy.autopilot);
                if let Some((resumed, moved)) =
                    self.resume_held(client, policy.clone(), chrome, false)
                {
                    match resumed {
                        Some(why) => self.note(format!(
                            "policy gen={} effective=on: acting again under the kept claim \
                             (was {why})",
                            p.generation
                        )),
                        None if moved => {
                            // Every changed setting can change the decision at an idle
                            // point, including an already armed retry. Re-reading an
                            // identical policy leaves its original timer alone.
                            self.note(format!(
                                "policy gen={} autopilot={word} settings changed: the threads look again",
                                p.generation
                            ));
                            self.claims.send_modify(|n| *n += 1);
                        }
                        None if lifted => {
                            // A person's hold lifted: every act waiting on it goes now, as
                            // aterm's own loop goes at the hold's edge (`park_on_hold`), not
                            // at the wait's recheck.
                            self.note(format!(
                                "policy gen={} re-read: the hold lifted, the threads look again",
                                p.generation
                            ));
                            self.claims.send_modify(|n| *n += 1);
                        }
                        None => self.note(format!("policy gen={} re-read", p.generation)),
                    }
                    self.sync_attention(client).await;
                    self.sync_needs(client, NEEDS_REPORTED).await;
                    self.sync_held(client).await;
                    // The Autopilot word may have left Land: the hold comes down.
                    self.sync_land(client).await;
                    return;
                }
                if self.lock().unproven_gen == Some(p.generation) {
                    self.passive(HarnessWord::Unknown);
                    return;
                }
                if self.lock().cleared {
                    self.passive(HarnessWord::Unknown);
                    self.note("the claim was cleared: waiting for the policy to move");
                    return;
                }
                // THE RECORD AT THE CLAIM IS WRITTEN THROUGH (CX-B B1): aterm reads the record
                // at the claim and relaunches a crash only on the line its copy names. The launch
                // probe answered before any claim (it opens the connection the claim uses), so
                // the record says now what the probe settled, the line and the act its proof
                // lets the record carry, never left to the coalescing second (parity-3's land8:
                // a SIGKILL 2 ms after the claim beat the write that carried the owed act).
                crate::extension::write_through(self.opts.shared).await;
                // aterm reads the record at the claim: a line written after this read is
                // renewed into its copy by `renew_for_record`.
                let line = self.opts.shared.resume_lines();
                match client.claim().await {
                    Ok(ClaimOutcome::Held { .. }) => {
                        match self.taken(policy) {
                            Taken::Held => {}
                            Taken::Refused => {
                                // The exit may still owe clears on this connection, or its
                                // release may predate this claim. Finish that cleanup first,
                                // then give back this actual take in either case.
                                let _ = Self::exit_finished(self.exit_result.subscribe()).await;
                                let _ = self.give_back(client, false).await;
                                return;
                            }
                            // The exit's own release follows its clears on this connection.
                            Taken::LeftToExit => return,
                        }
                        self.lock().confirmed_line = line;
                        self.note(format!("claimed as {} (proven)", client.holder()));
                        // Owed receipts and reports go now that codex-alab acts, and the
                        // needs standing are shown again.
                        self.refresh_badge();
                        self.sync_attention(client).await;
                        self.sync_needs(client, NEEDS_REPORTED).await;
                        // A new claim may find the arm or the hold aterm kept for this
                        // process: the tab's is sent, or a kept one comes down.
                        self.sync_held(client).await;
                        self.sync_land(client).await;
                    }
                    Ok(ClaimOutcome::Unproven) => {
                        self.lock().unproven_gen = Some(p.generation);
                        self.passive(HarnessWord::Unknown);
                        self.note("aterm did not prove the claim: given back, passive");
                    }
                    Ok(ClaimOutcome::Busy { holder }) => {
                        // That holder's release is a `meta` event: the claim is taken then,
                        // and the passive clock does not ask again meanwhile (review of
                        // fea034d20, finding 6).
                        self.startup_foreign.store(true, Ordering::SeqCst);
                        let mut st = self.lock();
                        st.busy = true;
                        st.startup_attention_retry = None;
                        drop(st);
                        self.passive(HarnessWord::Unknown);
                        self.note(format!(
                            "no claim: {holder} holds the session; waiting for its release"
                        ));
                    }
                    Ok(other) => {
                        self.passive(HarnessWord::Unknown);
                        self.note(format!("no claim: {other:?}"));
                    }
                    Err(e) => {
                        self.passive(HarnessWord::Unknown);
                        self.note(format!("claim failed: {e}"));
                    }
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
                        let said_off = if held {
                            // The session's Autopilot word holds under the hold too: a policy
                            // stamped Cruise here would end a Land episode or a landing at
                            // the first look during it (review of 2a9241bbf, findings 3, 13
                            // and 17).
                            let mut policy = Policy::from_harness(&p);
                            let (word, said_off) = self.autopilot_word_said(client).await;
                            policy.autopilot = word;
                            let word = policy.autopilot.word();
                            // The policy keeps its mode under the hold (a landing survives
                            // it); the chrome names the Off the person chose, as aterm's
                            // glass does.
                            let chrome = if said_off {
                                HarnessWord::Off
                            } else {
                                acting_word(policy.autopilot)
                            };
                            // Settings that moved under the hold are acted on as the acting
                            // path acts on it: every thread looks again under it now (its
                            // act waits on `status hold=`). Stored silently, the effective
                            // read after the hold would find the same policy and never
                            // reconsider its point (including a disabled retry).
                            if let Some((resumed, moved)) =
                                self.resume_held(client, policy, chrome, true)
                            {
                                match resumed {
                                    Some(was) => {
                                        self.note(format!(
                                            "policy {why}: the standby ({was}) is lifted"
                                        ));
                                    }
                                    None if moved => {
                                        self.note(format!(
                                            "policy {why} autopilot={word} settings changed: the threads look again"
                                        ));
                                        self.claims.send_modify(|n| *n += 1);
                                    }
                                    None => {}
                                }
                                // A standby lifted shows the standing attention, needs, arm
                                // and hold again.
                                self.sync_attention(client).await;
                                self.sync_needs(client, NEEDS_REPORTED).await;
                                self.sync_held(client).await;
                                self.sync_land(client).await;
                                self.note(format!("policy {why}: the claim is kept, nothing acts"));
                                return;
                            }
                            said_off
                        } else {
                            // aterm names a hold before an Off: a hold over Autopilot off
                            // leaves the harness on with Autopilot off.
                            self.status_with_timer(client)
                                .await
                                .ok()
                                .and_then(|status| status.autopilot_said())
                                == Some(codex_alab_aterm::status::Autopilot::Off)
                        };
                        let word = if said_off {
                            HarnessWord::Off
                        } else {
                            HarnessWord::Unknown
                        };
                        // A claim another task's refresh took while status was read
                        // stands: this speaks for an unheld session only, under the lock
                        // that taking the claim holds.
                        let unheld = {
                            let mut st = self.lock();
                            if !st.held {
                                st.hold = true;
                                self.passive(word);
                            }
                            !st.held
                        };
                        if unheld {
                            self.note(format!("policy {why}: passive"));
                        }
                    }
                    Off::Standby => {
                        self.lock().hold = false;
                        if held {
                            self.stand_by(client, why.clone()).await;
                        } else {
                            self.passive(HarnessWord::Unknown);
                        }
                        self.note(format!("policy {why}: passive"));
                    }
                    Off::Release => {
                        self.lock().hold = false;
                        let word = off_word(&p.why);
                        if held {
                            self.release_for_off(client, why.clone(), word).await;
                        } else {
                            self.passive(word);
                        }
                        self.note(format!("policy {why}: passive, unclaimed"));
                    }
                }
            }
            Ok(PolicyAnswer::Unsupported(line)) if known_support => {
                // Any supported reply on this connection proves the protocol, even
                // before its first claim or after Off gave that claim back. A later
                // refusal is transient; an unheld session keeps its passive clock.
                if self.lock().held && client.holds_claim() {
                    self.stand_by(client, format!("policy unreadable: {line}"))
                        .await;
                    self.note(format!(
                        "policy unreadable ({line}): passive, the claim kept"
                    ));
                } else {
                    self.passive(HarnessWord::Unknown);
                    self.note(format!("policy unreadable ({line}): passive, unclaimed"));
                }
            }
            Ok(PolicyAnswer::Unsupported(line)) => {
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
                } else {
                    self.passive(HarnessWord::Unknown);
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
    /// `word` is what the TUI's chrome names after it: [`HarnessWord::Off`] for Autopilot off
    /// with the harness still on, [`HarnessWord::HarnessOff`] or [`HarnessWord::AgentOff`] for
    /// the harness itself off by a Settings switch, [`HarnessWord::Unhosted`] for no harness
    /// host, [`HarnessWord::Unknown`] for the rest ([`off_word`]).
    pub(crate) async fn release_for_off(&self, client: &Client, why: String, word: HarnessWord) {
        // Retire the Off's local state before any transport can wait. A native
        // report completing meanwhile parks at this epoch and may name fresh needs;
        // the delayed release/cleanup must reconcile, never dismiss that new set.
        self.lose_locally(why.clone(), word, |st| {
            self.offs.fetch_add(1, Ordering::SeqCst);
            st.attention.clear();
            // OFF'S PARK LIFTS EVERY THREAD'S HOLD (`ThreadState::park_off`): no arm stands for
            // the next claim to send, whenever each thread's memory parks, and it ends every
            // Land episode, its hold with it.
            st.holds.clear();
            st.lands.clear();
            // Every thread's wait goes with it, as with any loss ([`Self::lose_locally`]):
            // each thread's memory parks at this Off (`park_if_off_from` owns no entry).
            st.needs_want = None;
            // ALab's own needs and the worker's last set go with it (parity-5, N; review R3):
            // a set a fresh report names after this is that report's alone.
            st.needs_named.clear();
            st.own_needs.clear();
            let off = std::mem::take(&mut st.needs_key);
            if off != 0 {
                // History has no Off; carry the exact set it dismissed to the next image.
                self.opts.shared.set_needs_off(off);
            }
        });
        // An explicit Off ends provider recovery before a remote release can wait. A
        // transient Passive keeps it; lost_locally already prevents new publication.
        self.opts.shared.set_recovery(None);
        if !self.opts.shared.flush_record() {
            self.note("the explicit off could not persist its cleared provider recovery");
        }
        // The release takes the set the needs entry says down with the claim (`needs_told_sent`);
        // the entry comes down after it.
        if let Err(e) = self.give_back(client, false).await {
            self.note(format!("the claim's release failed: {e}"));
        }
        self.sync_attention(client).await;
        self.sync_needs_after_off(client, "a person's or the config's off")
            .await;
        self.note(format!(
            "gave the claim back ({why}): claimed again at effective=on"
        ));
    }

    /// When the word's clock reads `status` next: while codex-alab acts and the last read named
    /// no acting mode, [`WORD_RECHECK`] after that read, doubling at each such read up to
    /// the passive clock's period. Nothing else reads the word while it acts on an event-quiet
    /// session (the passive clock stops), and aterm moves `-` to the session's word with no
    /// event.
    fn word_recheck_at(&self) -> Option<tokio::time::Instant> {
        if self.closing() {
            return None;
        }
        let every = Duration::from_millis(self.recheck_ms.load(Ordering::SeqCst));
        let first = Duration::from_millis(self.word_recheck_ms.load(Ordering::SeqCst));
        let st = self.lock();
        if !st.acting() || st.unsaid_reads == 0 {
            return None;
        }
        let doublings = (st.unsaid_reads - 1).min(16);
        let wait = first.saturating_mul(1 << doublings).min(every);
        let at = st
            .word_read
            .and_then(|read| read.checked_add(wait))
            .unwrap_or_else(clock);
        Some(tokio::time::Instant::from_std(at))
    }

    /// The word's clock ([`Supervisor::word_recheck_at`]): a word `status` names now that the
    /// policy in force does not is taken as an act's checks take it, every thread looking
    /// again under it. Off reconciles the full policy; a hold keeps its claim and landing.
    /// A read that fails counts as one that named none.
    async fn recheck_word(&self, client: &Client) {
        let status = self.status_with_timer(client).await.ok();
        let said = status.as_ref().and_then(Status::autopilot_said);
        let word = self.lock().read_word(said);
        if said == Some(codex_alab_aterm::status::Autopilot::Off) {
            if status.as_ref().is_some_and(|status| status.hold) {
                self.refresh(client).await;
            } else {
                self.reconcile_status_off(client).await;
            }
            return;
        }
        if let Some(word) = word {
            self.adopt_word(word);
        }
    }

    /// Revoke before transport can wait. A contradictory on reply cannot lend a fresh
    /// claim to an old decision or immediately re-arm it; the passive clock retries.
    async fn reconcile_status_off(&self, client: &Client) {
        self.release_for_off(client, "status autopilot=off".to_string(), HarnessWord::Off)
            .await;
        self.refresh(client).await;
        if self.acting() {
            self.release_for_off(
                client,
                "status autopilot=off contradicts effective=on".to_string(),
                HarnessWord::Off,
            )
            .await;
        }
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
            if !st.held {
                self.passive(HarnessWord::Unknown);
                return;
            }
            if st.standby.is_some() {
                // Already passive: only the reason moves.
                st.standby = Some(why);
                return;
            }
            st.standby = Some(why.clone());
            st.policy = None;
            st.box_look = None;
            st.since = Some(clock());
            st.description_want = None;
            st.description_owner = None;
            // A later resume publishes under this same fence; cleanup below cannot
            // put that resumed claim back into passive mode.
            self.passive(HarnessWord::Unknown);
            DescriptionReceipt::of(&st)
        };
        if let Some(receipt) = described {
            self.withdraw_on_own_connection(receipt);
        }
        self.withdraw_unapplied_answer();
        self.clear_attention(client).await;
        // The kept claim keeps aterm's arm and Land's hold: nothing is held back on standby, so
        // both come down ([`State::held_want`], [`State::land_want`]), and acting again sends
        // the standing ones.
        self.sync_held(client).await;
        self.sync_land(client).await;
        self.note(format!("passive, the claim kept: {why}"));
    }

    /// Withdraw `text` from aterm's description on a connection of its own (the claim's may be
    /// the one that ended), while it is still the text this process wrote.
    fn withdraw_on_own_connection(&self, receipt: DescriptionReceipt) {
        if let Some(endpoint) = (self.opts.endpoint)()
            && tokio::runtime::Handle::try_current().is_ok()
        {
            let options = self.opts.client.clone();
            let me = self.me.clone();
            tokio::spawn(async move {
                let Some(me) = me.upgrade() else {
                    return;
                };
                me.withdraw_description_at(endpoint, options, receipt).await;
            });
        }
    }

    async fn withdraw_description_at(
        &self,
        endpoint: Endpoint,
        options: ClientOptions,
        receipt: DescriptionReceipt,
    ) {
        let _one = self.description_sync.acquire().await;
        {
            let st = self.lock();
            if !receipt.stands(&st) {
                return;
            }
        }
        if let Ok(client) = Client::connect(endpoint, options).await {
            #[cfg(test)]
            self.observe_description_connection(&client);
            let removed = self.withdraw_described_on(&client, &receipt).await;
            client.close();
            // Keep publication custody until the request worker drops its socket,
            // including when an unanswered withdrawal may still be at the server.
            client.closed().await;
            if matches!(removed, Ok(true)) {
                // An adoption may have arrived while the acknowledged unset
                // was already in flight. Its latest desire still must stand.
                self.reconcile_description().await;
            }
        }
    }

    #[cfg(test)]
    fn observe_description_connection(&self, client: &Client) {
        let observer = self
            .description_connection
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(observer) = observer {
            let _ = observer.send(client.clone());
        }
    }

    /// A successful read/withdraw settles this acknowledged baseline. A newer
    /// publication may already own different text; never clear its local custody.
    async fn withdraw_described_on(
        &self,
        client: &Client,
        receipt: &DescriptionReceipt,
    ) -> Result<bool, AtermError> {
        if !receipt.stands(&self.lock()) {
            return Ok(false);
        }
        let fields = client.meta().await?;
        if !receipt.stands(&self.lock()) {
            return Ok(false);
        }
        let standing = codex_alab_aterm::wire::field(&fields, "description");
        if standing.as_deref() == Some(receipt.text.as_str()) {
            client.unset_description().await?;
            let mut st = self.lock();
            if st.described.as_deref() == Some(receipt.text.as_str()) {
                // This acknowledged unset removed these actual bytes, including
                // a fresh same-text adoption that arrived during the write.
                st.described = None;
                st.described_owner = None;
            }
            Ok(true)
        } else {
            let mut st = self.lock();
            if receipt.stands(&st) {
                st.described = None;
                st.described_owner = None;
            }
            Ok(false)
        }
    }

    /// Act again under the kept claim with `policy`: a standby is lifted (its first act waits
    /// the grace, the threads look again, so a point whose attention the standby cleared
    /// raises it again, and owed receipts and reports go). Answers the standby's reason, or
    /// `None` when there was none (only the policy moved).
    ///
    /// `word` is the chrome's under the kept claim, standby or not (a read that moved the
    /// mode from cruise to land says so at once), published under the lock that stores the
    /// policy, so it is ordered with [`Supervisor::adopt_word`]'s.
    #[cfg(test)]
    fn resume(&self, policy: Policy, word: HarnessWord) -> Option<String> {
        let resumed = {
            let mut st = self.lock();
            if !st.held {
                return None;
            }
            st.policy = Some(policy);
            let resumed = st.standby.take();
            if resumed.is_some() {
                // Acting again is a transition: its first act waits the grace.
                st.since = Some(clock());
                self.opts
                    .shared
                    .set_autopilot_with_word(AutopilotState::Active, word);
            } else {
                self.opts.shared.set_harness_word(word);
            }
            resumed
        };
        let resumed = resumed?;
        self.claims.send_modify(|n| *n += 1);
        self.refresh_badge();
        Some(resumed)
    }

    fn resume_held(
        &self,
        client: &Client,
        policy: Policy,
        word: HarnessWord,
        hold: bool,
    ) -> Option<(Option<String>, bool)> {
        let (resumed, moved) = {
            let mut st = self.lock();
            if !st.held || !client.holds_claim() {
                return None;
            }
            st.hold = hold;
            let moved = st.policy.as_ref().is_some_and(|old| old != &policy);
            st.policy = Some(policy);
            let resumed = st.standby.take();
            if resumed.is_some() {
                // Acting again is a transition: its first act waits the grace.
                st.since = Some(clock());
                self.opts
                    .shared
                    .set_autopilot_with_word(AutopilotState::Active, word);
            } else {
                self.opts.shared.set_harness_word(word);
            }
            (resumed, moved)
        };
        if resumed.is_some() {
            self.claims.send_modify(|n| *n += 1);
            self.refresh_badge();
        }
        Some((resumed, moved))
    }

    /// An Ours event can precede the refresh's proof and local take. Recheck after
    /// that refresh finishes; an old unheld observation cannot release its take.
    async fn give_back_if_unheld(&self, client: &Client) {
        let Some(me) = self.me.upgrade() else {
            return;
        };
        let client = client.clone();
        // Cancellation of the waiting event loop must not drop the refresh fence
        // while give_back's own release task still has a request to finish.
        let release = tokio::spawn(async move {
            me.give_back_if_unheld_on(client).await;
        });
        let _ = release.await;
    }

    async fn give_back_if_unheld_on(&self, client: Client) {
        let _one = self.policy_refresh.acquire().await;
        if !self.lock().held {
            // A claim codex-alab does not count as held (a re-confirm that re-took
            // one someone had just cleared) is given back, with its observed echo.
            let _ = self.give_back(&client, true).await;
            self.note("gave back a claim taken while passive");
        }
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

    /// Take the claim aterm answered held, unless the exit has begun. The exit publishes
    /// `closing` and captures connection/debt under this same State lock: a later take sees
    /// it ([`Taken::Refused`]), and an earlier take is included in that snapshot. The second
    /// look keeps shared state published after the exit's loss passive ([`Taken::LeftToExit`]).
    fn taken(&self, policy: Policy) -> Taken {
        let word = acting_word(policy.autopilot);
        {
            let mut st = self.lock();
            if self.closing() {
                return Taken::Refused;
            }
            st.held = true;
            st.busy = false;
            st.standby = None;
            st.policy = Some(policy);
            st.since = Some(clock());
            st.unproven_gen = None;
            st.claims_taken = st.claims_taken.wrapping_add(1);
            st.activity_sent = None;
            st.manual_resume_at = None;
            st.startup_attention_retry = None;
            // The claim, then the Autopilot with its word in one store, under the lock that
            // stored the policy: a mode [`Supervisor::adopt_word`] moves after it is never
            // overwritten by this one.
            self.opts
                .shared
                .set_claim(Some(codex_alab_aterm::holder_for(
                    self.opts.client.holder_pid,
                )));
            self.opts
                .shared
                .set_autopilot_with_word(AutopilotState::Active, word);
        }
        if self.closing() {
            self.lost_locally("the process is leaving");
            return Taken::LeftToExit;
        }
        self.claims.send_modify(|n| *n += 1);
        Taken::Held
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
        self.lose_locally(why, HarnessWord::Unknown, mark);
    }

    /// [`Supervisor::lost_locally_marking`], passive under `word` ([`Supervisor::passive`]).
    fn lose_locally(
        &self,
        why: impl Into<String>,
        word: HarnessWord,
        mark: impl FnOnce(&mut State),
    ) {
        let (was, described, waited) = {
            let mut st = self.lock();
            mark(&mut st);
            // EVERY THREAD'S WAIT GOES WITH THE CLAIM, however it went (an act's checks that
            // found it gone, the connection's end, the claim's watch, an Off): nothing types
            // when one ends. Taken here, under the lock that makes codex-alab passive, so an
            // unheld sync takes the entry down and a later claim's first sync settles it
            // rather than raising the old words again (review of f134d2b2cb, finding 1: only
            // the watch's loss and a gap's took them down).
            st.waits.clear();
            let waited = st.waits_shown.is_some();
            // A loss under a person's hold over Off keeps the Off named, as the hold arm names
            // it for an unheld session: the harness is on and Autopilot off all the same.
            let word = if word == HarnessWord::Unknown
                && st.hold
                && self.opts.shared.harness_word() == HarnessWord::Off
            {
                HarnessWord::Off
            } else {
                word
            };
            let was = st.held;
            st.held = false;
            st.standby = None;
            st.hold = false;
            st.policy = None;
            st.box_look = None;
            st.description_want = None;
            st.description_owner = None;
            if was {
                st.since = Some(clock());
            }
            // Revoke before releasing the fence used by taken and resume. Deferred
            // cleanup may be delayed after a fresh claim is already active.
            self.opts.shared.set_claim(None);
            self.passive(word);
            (was, DescriptionReceipt::of(&st), waited)
        };
        // Nothing keeps the description true without the claim: it is withdrawn (on a
        // connection of its own; the claim's may be the one that ended), while it is still
        // the text this process wrote.
        if let Some(receipt) = described {
            self.withdraw_on_own_connection(receipt);
        }
        // The shown wait comes down on the claim's connection where it is still open; one that
        // ended leaves it to the next claim's first sync. The exit takes its own down.
        if waited && !self.closing() {
            self.show_waits_later();
        }
        if was {
            self.note(format!("passive: {}", why.into()));
        }
        // An answer the TUI has not applied yet is no longer codex-alab's to give.
        self.withdraw_unapplied_answer();
        self.show_badge(None);
    }

    /// Passive in the record, with the word the TUI's chrome names: [`HarnessWord::Off`]
    /// where the harness is on and only Autopilot is off, [`HarnessWord::HarnessOff`] or
    /// [`HarnessWord::AgentOff`] where a Settings switch holds the harness off and
    /// [`HarnessWord::Unhosted`] where aterm runs no harness host ([`off_word`]),
    /// [`HarnessWord::Unknown`] for every other reason (the claim not proven, busy, cleared or
    /// failed, the harness headless, held or suspended, the policy unreadable). One store, so the chrome never reads a stale word beside the new
    /// state.
    fn passive(&self, word: HarnessWord) {
        self.opts
            .shared
            .set_autopilot_with_word(AutopilotState::Passive, word);
    }

    /// How long ago a person last gave the window input, as aterm's `@self status` says now
    /// (`human_ms=`, stamped by a window keystroke, click or scroll and never by a control
    /// write): what says a person's hand opened the turn their message starts
    /// ([`crate::decide::ThreadState::observe_person_hand`], aterm's `hand`). `None` with no
    /// connection, a read that failed or took past [`HAND_READ_LIMIT`], or none named.
    pub async fn person_input_ago(&self) -> Option<Duration> {
        #[cfg(test)]
        if let Some(answer) = self
            .hand_answers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pop_front()
        {
            return answer;
        }
        let client = self.lock().client.clone()?;
        #[cfg(not(test))]
        let limit = HAND_READ_LIMIT;
        #[cfg(test)]
        let limit = *self
            .hand_read_limit
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match tokio::time::timeout(limit, client.status()).await {
            Ok(Ok(status)) => match status.human {
                HumanInput::Ago(ms) => Some(Duration::from_millis(ms)),
                HumanInput::Never | HumanInput::Unknown => None,
            },
            Ok(Err(_)) | Err(_) => None,
        }
    }

    /// How long ago the person last typed in the window, as aterm's `@self status` says now
    /// (`human_typed_ms=`, its `human_ms=` from an aterm that prints no typing stamp: never a
    /// control write), or zero where another driver's hand is on the tab
    /// ([`codex_alab_aterm::status::Status::outside_hand`]): the key that holds the owner's
    /// standing go-ahead for the hour after it ([`crate::decide::ThreadState::person_keyed`]).
    /// `None` with no connection, a read that failed or took past [`HAND_READ_LIMIT`], or none
    /// named.
    pub async fn person_typed_ago(&self) -> Option<Duration> {
        let client = self.lock().client.clone()?;
        #[cfg(not(test))]
        let limit = HAND_READ_LIMIT;
        #[cfg(test)]
        let limit = *self
            .hand_read_limit
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let status = match tokio::time::timeout(limit, client.status()).await {
            Ok(Ok(status)) => status,
            Ok(Err(_)) | Err(_) => return None,
        };
        if status.outside_hand(client.holder()) {
            return Some(Duration::ZERO);
        }
        match status.typed() {
            HumanInput::Ago(ms) => Some(Duration::from_millis(ms)),
            HumanInput::Never | HumanInput::Unknown => None,
        }
    }

    /// The answers the next reads of the person's hand give, in turn ([`Self::hand_answers`]).
    #[cfg(test)]
    pub(crate) fn answer_hand_reads(&self, answers: impl IntoIterator<Item = Option<Duration>>) {
        self.hand_answers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .extend(answers);
    }

    #[cfg(test)]
    pub(crate) fn set_hand_read_limit(&self, limit: Duration) {
        *self
            .hand_read_limit
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = limit;
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

    /// Obtain a Ready result that cannot borrow authority from a change queued
    /// before or during its checks. Call the synchronous predicate again after
    /// the final database admission lease; never await network under that lease.
    pub(crate) async fn ready_for_admission(&self) -> Result<AdmissionFence, Ready> {
        let tui_generation = self.opts.shared.upgrade_generation();
        let fence = {
            let st = self.lock();
            AdmissionFence {
                events: Arc::clone(&st.queued_claim_events),
                event_epoch: st
                    .queued_claim_events
                    .admission_epoch
                    .load(Ordering::SeqCst),
                lost_epoch: self.lost_epoch.load(Ordering::SeqCst),
                released: self.released.load(Ordering::SeqCst),
                claim: st.claims_taken,
                generation: st.generation,
                since: st.since,
                policy: st.policy.clone(),
                tui_generation,
            }
        };
        match self.ready().await {
            Ready::Go if self.admission_fence_current(&fence) => Ok(fence),
            Ready::Go => Err(Ready::Wait {
                until: clock() + self.recheck(),
                why: "admission authority changed during readiness checks".into(),
            }),
            other => Err(other),
        }
    }

    /// Recheck only already observed authority, without transport, a new claim
    /// or a refreshed generation. A loss followed by reacquisition cannot make
    /// an old fence current again, nor can clearing a later TUI draft.
    pub(crate) fn admission_fence_current(&self, fence: &AdmissionFence) -> bool {
        if self.closing()
            || !codex_alab_core::active()
            || self.opts.shared.autopilot() != AutopilotState::Active
            || self.opts.shared.upgrade_generation() != fence.tui_generation
        {
            return false;
        }
        let grace = {
            let st = self.lock();
            if !st.acting()
                || st.hold
                || st.hold_met
                || st.claims_taken != fence.claim
                || st.generation != fence.generation
                || st.since != fence.since
                || st.policy != fence.policy
                || !Arc::ptr_eq(&st.queued_claim_events, &fence.events)
                || st
                    .client
                    .as_ref()
                    .is_none_or(|client| !client.holds_claim() || !client.reconfirmed_since_claim())
            {
                return false;
            }
            let Some(policy) = st.policy.as_ref() else {
                return false;
            };
            policy.human_grace
        };
        self.lost_epoch.load(Ordering::SeqCst) == fence.lost_epoch
            && self.released.load(Ordering::SeqCst) == fence.released
            && fence.events.pending.load(Ordering::SeqCst) == 0
            && fence.events.admission_pending.load(Ordering::SeqCst) == 0
            && fence.events.admission_epoch.load(Ordering::SeqCst) == fence.event_epoch
            && self.tui_wait_for(Act::Turn, grace, clock()).is_none()
            && self.opts.shared.upgrade_generation() == fence.tui_generation
    }

    /// The current TUI half of turn readiness, without remote I/O or a claim grant.
    /// Re-read after preparatory awaits, immediately before issuing native work.
    pub(crate) fn tui_wait(&self, grace: Duration) -> Option<(Instant, &'static str)> {
        self.tui_wait_for(Act::Turn, grace, clock())
    }

    fn tui_wait_for(
        &self,
        act: Act,
        grace: Duration,
        now: Instant,
    ) -> Option<(Instant, &'static str)> {
        let shared = self.opts.shared;
        let resume_at = self.lock().manual_resume_at;
        let why = if shared.screen_taken() {
            Some("an overlay or the external editor has the screen")
        } else if !shared.record_draft_empty() {
            Some("a draft or an unrendered key stands in the composer")
        } else if shared.queued_input() {
            Some("the TUI holds a person's queued input")
        } else if act == Act::Turn && shared.popup_open() {
            Some("a popup or modal is open")
        } else if shared.pending_user_turn() {
            Some("a person's turn is starting")
        } else {
            None
        };
        if let Some(why) = why {
            return Some((now + self.recheck(), why));
        }
        let grace_ms = u64::try_from(grace.as_millis()).unwrap_or(u64::MAX);
        if let Some(at) = shared.last_human_input_ms() {
            let age = now_ms().saturating_sub(at);
            if age < grace_ms && !resume_at.is_some_and(|resumed| at <= resumed) {
                return Some((
                    now + Duration::from_millis(grace_ms - age),
                    "a person typed in the TUI",
                ));
            }
        }
        None
    }

    async fn ready_for(&self, act: Act) -> Ready {
        let answer = self.ready_for_inner(act).await;
        self.report_continuation(&answer);
        answer
    }

    fn report_continuation(&self, answer: &Ready) {
        let (reason, wait_ms) = match answer {
            Ready::Go => ("ready", None),
            Ready::Passive(_) => return,
            Ready::Wait { until, why } => {
                let reason = if why.contains("draft") || why.contains("queued input") {
                    "composing"
                } else if why.contains("popup") || why.contains("overlay") || why.contains("editor")
                {
                    "dialog"
                } else if why.contains("Hold") {
                    "paused"
                } else if why.contains("person") {
                    "person"
                } else {
                    "waiting"
                };
                let wait = (reason == "person").then(|| {
                    u64::try_from(until.saturating_duration_since(clock()).as_millis())
                        .unwrap_or(u64::MAX)
                });
                (reason, wait)
            }
        };
        let stamp = (
            reason.to_owned(),
            wait_ms.map(|ms| now_ms().saturating_add(ms) / 1000),
        );
        let (client, sequence) = {
            let mut state = self.lock();
            if state.activity_sent.as_ref() == Some(&stamp) {
                return;
            }
            state.activity_sent = Some(stamp);
            state.activity_sequence = state.activity_sequence.saturating_add(1);
            (
                state.client.clone().filter(Client::holds_claim),
                state.activity_sequence,
            )
        };
        if let Some(client) = client {
            tokio::spawn(async move {
                let _ = client.harness_activity(reason, wait_ms, sequence).await;
            });
        }
    }

    async fn ready_for_inner(&self, act: Act) -> Ready {
        if codex_alab_core::is_alab_binary() && !codex_alab_core::active() {
            return Ready::Passive("vanilla mode".into());
        }
        let (client, policy, since, on_reads, claim_events) = {
            let st = self.lock();
            if let Some(why) = st.standby.as_ref().filter(|_| st.held) {
                return Ready::Passive(format!("passive under the kept claim: {why}"));
            }
            match (&st.client, &st.policy) {
                (Some(client), Some(policy))
                    if st.acting()
                        && client.holds_claim()
                        && self.opts.shared.autopilot() == AutopilotState::Active =>
                {
                    (
                        client.clone(),
                        policy.clone(),
                        st.since,
                        st.on_reads,
                        Arc::clone(&st.queued_claim_events),
                    )
                }
                _ => return Ready::Passive("no proven claim".to_string()),
            }
        };
        let now = clock();
        let grace = policy.human_grace;
        let grace_ms = u64::try_from(grace.as_millis()).unwrap_or(u64::MAX);
        let epoch = self.lost_epoch.load(Ordering::SeqCst);
        let released = self.released.load(Ordering::SeqCst);
        let status = match client.status().await {
            Ok(status) => status,
            Err(e) => {
                return Ready::Wait {
                    until: now + self.recheck(),
                    why: format!("status unreadable: {e}"),
                };
            }
        };
        let resume_at = status
            .fields
            .get("resume_seq")
            .filter(|seq| seq.as_str() != "-" && status.fields.get("human_seq") == Some(*seq))
            .and_then(|_| status.fields.get("resume_at"))
            .and_then(|at| at.parse::<u64>().ok());
        self.lock().manual_resume_at = resume_at;
        if let Some((until, why)) = self.tui_wait_for(act, grace, now) {
            return Ready::Wait {
                until,
                why: why.to_string(),
            };
        }
        let native_grace = if resume_at.is_some() && act == Act::Turn {
            Duration::ZERO
        } else {
            grace
        };
        match status.ready_for_act(client.holder(), native_grace) {
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
            Err(NotReady::Hold) => {
                // A person's hold: the policy read that finds it lifted has every thread look
                // again ([`Supervisor::read_on`]), so this recheck is only the backstop
                // ([`HOLD_RECHECK`], aterm's park step); one that came while this `status` read
                // was in flight is met by looking again now.
                let lifted = self.met_hold(on_reads);
                return Ready::Wait {
                    until: if lifted {
                        now
                    } else {
                        now + self.hold_recheck()
                    },
                    why: format!("aterm says not now: {:?}", NotReady::Hold),
                };
            }
            Err(other) => {
                let after = match recheck_after(&other) {
                    RECHECK => self.recheck(),
                    after => after,
                };
                return Ready::Wait {
                    until: now + after,
                    why: format!("aterm says not now: {other:?}"),
                };
            }
        }
        // AN ACT GOES UNDER THE SESSION'S WORD AS ATERM SAYS IT NOW. A claim taken at launch
        // can read no word (`autopilot=-` until aterm's status sweep admits this codex-alab)
        // and so decide under Cruise in a Land session, as a worker `spawn agent=codex` starts;
        // the admission then moves the printed word with no event. Met here, the policy takes
        // the word and every thread looks again under it: the decision this act came from is
        // no longer current, so nothing goes under the old word. Its `land_at=` is not
        // published: this read runs on the thread's own task beside the session loop's reads,
        // and a reply it took before theirs could store a timer theirs had seen go
        // ([`Supervisor::status_with_timer`]; review of gap 12, finding 6).
        let observed = status.autopilot_said();
        let said = self.lock().read_word(observed);
        if observed == Some(codex_alab_aterm::status::Autopilot::Off) {
            // Hold was checked first. Preserve the remembered mode, but retire this
            // decision's Off epoch before any release or policy refresh can await.
            self.reconcile_status_off(&client).await;
            return Ready::Wait {
                until: clock() + self.recheck(),
                why: "the session's Autopilot is off".to_string(),
            };
        }
        if let Some(word) = said
            && word != policy.autopilot
        {
            self.adopt_word(word);
            return Ready::Wait {
                until: now,
                why: format!("the session's Autopilot word is {} now", word.word()),
            };
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
        if claim_events.pending.load(Ordering::SeqCst) > 0 {
            return Ready::Wait {
                until: clock() + self.recheck(),
                why: "a queued claim change is still being settled".to_string(),
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
        if let Some((until, why)) = self.tui_wait_for(act, grace, clock()) {
            return Ready::Wait {
                until,
                why: why.to_string(),
            };
        }
        Ready::Go
    }

    /// Record an act on the session (`harness_act`), tell aterm a question box's answer
    /// ([`story`]; no other act), and record it in aterm's message log.
    ///
    /// While the proven claim is held, a standby included: every act passed its checks while
    /// codex-alab acted, so one a standby came after still went, and is recorded (review of
    /// fea034d20, finding 5; aterm takes the record under the kept claim). Without the claim
    /// nothing is recorded as codex-alab's.
    pub async fn record_act(&self, rule: &str, words: &str) {
        let _ = self.record_act_acknowledged(rule, words, "").await;
    }

    /// Whether aterm acknowledged the act record. A local deduplication mark may
    /// settle only after this succeeds; a failed write still needs publication.
    ///
    /// The act's words are `said` then `kept`: where its message-log record would pass aterm's
    /// 512 bytes, the end of `said` gives way to `…` and `kept` (a typed act's fact, why and
    /// turn) stays whole ([`Self::told`]).
    pub(crate) async fn record_act_acknowledged(&self, rule: &str, said: &str, kept: &str) -> bool {
        let words = format!("{said}{kept}");
        let Some(client) = self.claimed_client(rule, &words) else {
            return false;
        };
        match client.harness_act(rule).await {
            Ok(codex_alab_aterm::ActOutcome::Recorded) => {}
            outcome => {
                self.note(format!("act {rule} not recorded: {outcome:?} ({words})"));
                return false;
            }
        }
        self.told(&client, rule, said, kept).await;
        true
    }

    /// THE SESSION'S UNTOLD ATERM FACT for an act under `rule` typed into the thread `reader`, at
    /// its wall track's class word `wall` (`-` for none) (CONTRACT §5 "aterm fact", gap 23): `@self
    /// harness fact` on the claim's connection, only while the proven claim is held and
    /// codex-alab acts, and this claim has not met an aterm without the verb
    /// ([`State::fact_unsupported`]). A `None`, `Denied`, `Failed` or `Unusable` answer, and an
    /// error (which has already closed the claim's connection, and with it the claim, so the act's
    /// own last look stops it), are no fact: each is journaled and nothing is acknowledged. Never
    /// awaited inside a lifecycle callback, a render or an input hook.
    pub(crate) async fn fact_for(&self, rule: &str, reader: &str, wall: &str) -> Option<Fact> {
        let (client, claim) = {
            let st = self.lock();
            let client = st
                .client
                .clone()
                .filter(|client| st.acting() && client.holds_claim())?;
            if st.fact_unsupported == Some(st.claims_taken) {
                return None;
            }
            (client, st.claims_taken)
        };
        let answer = match client.harness_fact(rule, reader, wall).await {
            Ok(answer) => answer,
            Err(e) => {
                self.note(format!("fact: {e} ({rule})"));
                return None;
            }
        };
        match answer {
            FactAnswer::Fact(fact) => Some(fact),
            FactAnswer::Unsupported(status) => {
                {
                    let mut st = self.lock();
                    if st.claims_taken == claim {
                        st.fact_unsupported = Some(claim);
                    }
                }
                self.note(format!(
                    "fact: aterm {} has no harness fact ({status}); not asked again under this claim",
                    release_named(&client)
                ));
                None
            }
            FactAnswer::None(why) => {
                self.note(format!("fact: {why} ({rule})"));
                None
            }
            FactAnswer::Denied(status)
            | FactAnswer::Failed(status)
            | FactAnswer::Unusable(status) => {
                self.note(format!("fact: {status} ({rule})"));
                None
            }
            FactAnswer::NotHeld => None,
        }
    }

    /// THE ACT THAT CARRIED `fact` STARTED AND WAS RECORDED (CONTRACT §5 "aterm fact"): `@self
    /// harness fact told episode=<016x>` on the claim's connection, while the proven claim is held
    /// (a standby included, as the act's own record is). A refusal or a failure is journaled, and
    /// the episode stays untold in aterm, which offers it again.
    pub(crate) async fn fact_told(&self, fact: &Fact) {
        let client = {
            let st = self.lock();
            st.client
                .clone()
                .filter(|client| st.held && client.holds_claim())
        };
        let outcome = match client {
            Some(client) => client.harness_fact_told(fact.episode).await,
            None => Ok(ToldOutcome::NotHeld),
        };
        match outcome {
            Ok(ToldOutcome::Told) => {
                self.note(format!("fact {} {:016x} told", fact.kind, fact.episode));
            }
            Ok(other) => self.note(format!(
                "fact {} {:016x} not told: {other:?}",
                fact.kind, fact.episode
            )),
            Err(e) => self.note(format!(
                "fact {} {:016x} not told: {e}",
                fact.kind, fact.episode
            )),
        }
    }

    /// A task-face publication whose failed transport exchange must settle the server's
    /// lease before the native-acceptance gate can be released (its caller holds that gate
    /// throughout).
    ///
    /// AN ATERM WITHOUT THE LEASE (CX-C C1): one before 0.110.0 answers `meta prepare
    /// harness_act` with its plain usage. From `unbounded_from` on (the first release that reads
    /// `rule` rightly from its unbounded spelling) the act goes as `meta set harness_act
    /// <rule>`, still inside the gate, and only while no turn runs, none is submitted and no
    /// input is queued (that aterm's own at-rest check is missing): an answered write took its
    /// turn-count mark before any turn core starts. What it cannot fence (a submission counted
    /// between that look and the write, a write left unanswered) is CONTRACT §5's. An older
    /// aterm, or one whose release cannot be read, would read the act wrong: nothing is sent
    /// ([`BeforeTurn::TooOld`]).
    pub(crate) async fn record_act_before_turn(
        &self,
        rule: &str,
        words: &str,
        unbounded_from: (u64, u64),
    ) -> BeforeTurn {
        let Some(client) = self.claimed_client(rule, words) else {
            return BeforeTurn::Retry;
        };
        let outcome = match client.harness_act_before_turn(rule).await {
            Ok(codex_alab_aterm::ActOutcome::Unsupported(status)) => {
                let aterm = release_named(&client);
                if !client
                    .server_release()
                    .is_some_and(|release| release >= unbounded_from)
                {
                    self.note(format!(
                        "act {rule} not recorded: aterm {aterm} has no act lease ({status}) and \
                         reads no {rule} before {}.{}.0 ({words})",
                        unbounded_from.0, unbounded_from.1
                    ));
                    return BeforeTurn::TooOld { aterm };
                }
                let shared = self.opts.shared;
                if shared.turn_running() || shared.pending_user_turn() || shared.queued_input() {
                    self.note(format!(
                        "act {rule} not recorded: a turn runs or is on its way, and aterm \
                         {aterm} cannot refuse a point past it ({words})"
                    ));
                    return BeforeTurn::Retry;
                }
                self.note(format!(
                    "act {rule}: aterm {aterm} has no act lease ({status}): its unbounded \
                     spelling, inside the gate"
                ));
                client.harness_act(rule).await
            }
            outcome => outcome,
        };
        match outcome {
            Ok(codex_alab_aterm::ActOutcome::Recorded) => {}
            Ok(codex_alab_aterm::ActOutcome::Unsupported(status)) => {
                let aterm = release_named(&client);
                self.note(format!(
                    "act {rule} not recorded: aterm {aterm} takes no harness_act ({status}) \
                     ({words})"
                ));
                return BeforeTurn::TooOld { aterm };
            }
            outcome => {
                self.note(format!("act {rule} not recorded: {outcome:?} ({words})"));
                return BeforeTurn::Retry;
            }
        }
        self.told(&client, rule, words, "").await;
        BeforeTurn::Recorded
    }

    /// The claim's connection, while the proven claim is held there; noted when it is not.
    fn claimed_client(&self, rule: &str, words: &str) -> Option<Client> {
        let client = {
            let st = self.lock();
            st.client
                .clone()
                .filter(|client| st.held && client.holds_claim())
        };
        if client.is_none() {
            self.note(format!(
                "act {rule} not recorded: no proven claim is held ({words})"
            ));
        }
        client
    }

    /// An act aterm recorded, told in aterm's message log (and, a question box's answer only,
    /// as a `story chose`): `codex-alab @<sid>: <said><kept>`, within aterm's 512 bytes, which
    /// refuses a longer notice whole: the end of `said` gives way to `…` and `kept` stays whole
    /// ([`Client::appnotice_harness`]). The journal keeps the words whole.
    async fn told(&self, client: &Client, rule: &str, said: &str, kept: &str) {
        if let Some((verb, text)) = story::for_act(rule) {
            let _ = client.story(verb, text).await;
        }
        let _ = client
            .appnotice_harness(
                &format!("codex-alab @{}: {said}", client.endpoint().sid),
                kept,
            )
            .await;
        self.note(format!("act {rule}: {said}{kept}"));
    }

    /// WHETHER A LEAD WAITS ON THIS SESSION (aterm's `TurnEndState::managed`, set from its
    /// `task` read: a task record stands that is not `exited`): `@self task` on the claim's
    /// connection. A record none stands for, an aterm that refuses the verb, and a read that
    /// fails (a timeout, a closed connection) are no lead, as aterm's `task_seed` leaves
    /// `managed` unset at a lost read (a read fails where the claim's connection does, and the
    /// claim after it decides the point again); so is no connection, where nothing types and no
    /// ask can be raised. What it decides ([`crate::decide::ThreadState::managed`]): a lead's
    /// worker's first short done claims and questions wait no back-off, and its hold is told
    /// quietly (its lead reads it); reading no lead keeps the ladder and the loud hold.
    pub(crate) async fn managed(&self) -> bool {
        let Some(client) = self.lock().client.clone() else {
            return false;
        };
        match client.task_state().await {
            Ok(state) => state.is_some_and(|state| state != "exited"),
            Err(_) => false,
        }
    }

    /// Tell aterm's message log something a person should know, never a row
    /// (`appnotice harness`), on the claim's connection while one is open; noted either way.
    pub(crate) async fn appnotice(&self, words: &str) {
        let client = self.lock().client.clone();
        if let Some(client) = client {
            let _ = client
                .appnotice_harness(
                    &format!("codex-alab @{}: {words}", client.endpoint().sid),
                    "",
                )
                .await;
        }
        self.note(words.to_string());
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
    #[cfg(any(test, debug_assertions))]
    pub async fn describe(&self, text: Option<String>) {
        if self.want_description(text) {
            self.publish_description().await;
        }
    }

    /// Accept while the caller owns its exact runtime registration. The remote
    /// reconciliation begins only after that synchronous guard is released.
    #[cfg(any(test, debug_assertions))]
    pub(crate) fn want_description(&self, text: Option<String>) -> bool {
        self.want_description_owned(None, text)
    }

    pub(crate) fn want_description_for(&self, owner: &Arc<()>, text: Option<String>) -> bool {
        self.want_description_owned(Some(Arc::clone(owner)), text)
    }

    fn want_description_owned(&self, owner: Option<Arc<()>>, text: Option<String>) -> bool {
        let mut st = self.lock();
        if self.closing.load(Ordering::SeqCst)
            || !st.acting()
            || !st.client.as_ref().is_some_and(Client::holds_claim)
        {
            return false;
        }
        if text.is_some() && st.described == text {
            // Identical acknowledged words now speak for the latest owner too.
            st.described_owner = owner.clone();
            st.described_publication = Arc::new(());
        }
        st.description_want = Some(text);
        st.description_owner = owner;
        true
    }

    /// Called while registration is still held, before teardown can yield.
    /// Another runtime's desired/acknowledged text keeps its own custody.
    pub(crate) fn retire_description(&self, owner: &Arc<()>) {
        let described = {
            let mut st = self.lock();
            if st
                .description_owner
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, owner))
            {
                st.description_want = None;
                st.description_owner = None;
            }
            st.described_owner
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, owner))
                .then(|| DescriptionReceipt::of(&st))
                .flatten()
        };
        if let Some(receipt) = described {
            self.withdraw_on_own_connection(receipt);
        }
    }

    #[cfg(test)]
    pub(crate) fn wants_description(&self, text: &str) -> bool {
        self.lock()
            .description_want
            .as_ref()
            .is_some_and(|want| want.as_deref() == Some(text))
    }

    #[cfg(test)]
    pub(crate) async fn descriptions_settled(&self) {
        let _one = self.description_sync.acquire().await;
    }

    pub(crate) async fn publish_description(&self) {
        let _one = self.description_sync.acquire().await;
        self.reconcile_description().await;
    }

    /// The caller owns description_sync, including cleanup after an acknowledged unset.
    async fn reconcile_description(&self) {
        loop {
            let (endpoint, text, described, carried, claim, owner) = {
                let mut st = self.lock();
                let Some(text) = st.description_want.clone() else {
                    return;
                };
                if self.closing.load(Ordering::SeqCst)
                    || !st.acting()
                    || st.described == text && (text.is_some() || st.carried_checked)
                {
                    return;
                }
                let endpoint = match &st.client {
                    Some(client) if client.holds_claim() => client.endpoint().clone(),
                    _ => return,
                };
                let carried = st.described.is_none() && !st.carried_checked && self.carried();
                if text.is_none() && st.described.is_none() && !carried {
                    st.carried_checked = true;
                    return;
                }
                (
                    endpoint,
                    text,
                    DescriptionReceipt::of(&st),
                    carried,
                    st.claims_taken,
                    st.description_owner.clone(),
                )
            };
            let done = match Client::connect(endpoint, self.opts.client.clone()).await {
                Ok(client) => {
                    #[cfg(test)]
                    self.observe_description_connection(&client);
                    let done = self
                        .describe_on(&client, &text, described, carried, claim, &owner)
                        .await;
                    client.close();
                    client.closed().await;
                    done
                }
                Err(e) => Err(e),
            };
            let retry = match done {
                Ok(retry) => retry,
                Err(e) => {
                    self.note(format!("description not written: {e}"));
                    false
                }
            };
            let current = {
                let st = self.lock();
                st.description_want.as_ref() == Some(&text)
                    && st.claims_taken == claim
                    && same_description_owner(&st.description_owner, &owner)
            };
            if current && !retry {
                return;
            }
            // A newer registration/step arrived during I/O. Keep its desired
            // state authoritative even when its publisher is still queued.
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
        text: &Option<String>,
        described: Option<DescriptionReceipt>,
        carried: bool,
        claim: u64,
        owner: &Option<Arc<()>>,
    ) -> Result<bool, AtermError> {
        let fields = client.meta().await?;
        let standing = codex_alab_aterm::wire::field(&fields, "description");
        let (current, passive) = {
            let mut st = self.lock();
            let passive = self.closing.load(Ordering::SeqCst) || !st.acting();
            let current = !passive
                && st.claims_taken == claim
                && st.description_want.as_ref() == Some(text)
                && same_description_owner(&st.description_owner, owner);
            if current {
                // Only a successful, current read spends predecessor custody.
                // Keep its exact text owned if the following write fails.
                st.carried_checked = true;
                if carried && st.described.is_none() {
                    st.described = standing
                        .as_ref()
                        .filter(|text| crate::describe::is_ours(text))
                        .cloned();
                    if st.described.is_some() {
                        st.described_owner = owner.clone();
                        st.described_publication = Arc::new(());
                    }
                }
            }
            (current, passive)
        };
        if !current {
            return if passive && let Some(receipt) = described {
                self.withdraw_described_on(client, &receipt).await
            } else {
                Ok(false)
            };
        }
        let ours = |s: &str| {
            described.as_ref().is_some_and(|receipt| receipt.text == s)
                || (described.is_none() && carried && crate::describe::is_ours(s))
        };
        let Some(text) = text.as_ref() else {
            let mut removed = false;
            if let Some(standing) = standing.filter(|s| ours(s.as_str())) {
                client.unset_description().await?;
                removed = true;
                self.note(format!("description withdrawn ({standing})"));
            }
            let mut st = self.lock();
            st.described = None;
            st.described_owner = None;
            return Ok(removed);
        };
        match standing {
            Some(standing) if standing == text.as_str() => {
                // Already says it (a predecessor's, carried): taken as this process's own.
                self.note(format!("description stands as written: {text}"));
            }
            Some(standing) if !ours(standing.as_str()) => {
                self.note(format!(
                    "a person's description stands ({standing}): not replaced"
                ));
                return Ok(false);
            }
            _ => {
                client.set_description(text).await?;
                self.note(format!("description: {text}"));
            }
        }
        // A standby or a release that came while this was written found nothing of this text
        // to withdraw: it is withdrawn here instead, so no description stands under an off.
        let (kept, receipt) = {
            let mut st = self.lock();
            let acting = !self.closing.load(Ordering::SeqCst)
                && st.acting()
                && st.description_want.is_some()
                && st.client.as_ref().is_some_and(Client::holds_claim);
            st.described = Some(text.clone());
            st.described_publication = Arc::new(());
            st.described_owner = if st
                .description_want
                .as_ref()
                .is_some_and(|want| want.as_deref() == Some(text.as_str()))
            {
                st.description_owner.clone()
            } else {
                owner.clone()
            };
            (
                acting,
                DescriptionReceipt {
                    text: text.clone(),
                    owner: st.described_owner.clone(),
                    publication: Arc::clone(&st.described_publication),
                },
            )
        };
        let removed = !kept && self.withdraw_described_on(client, &receipt).await?;
        if removed {
            self.note(format!(
                "description withdrawn: passive since it was written ({text})"
            ));
        }
        Ok(removed)
    }

    /// Record a wait in aterm's message log (a record, never a row).
    pub async fn record_wait(&self, words: &str) {
        let Some(client) = self.lock().client.clone() else {
            return;
        };
        let _ = client
            .appnotice_harness(
                &format!("codex-alab @{}: {words}", client.endpoint().sid),
                "",
            )
            .await;
        self.note(format!("wait: {words}"));
    }

    /// Raise (`Some`) or clear (`None`) one of codex-alab's attention reasons; aterm shows
    /// the first standing one under `owner=codex-alab`. Only while the claim is held. A need
    /// a person must answer while the work goes on is never one of these: it goes up with the
    /// worker's own ([`Self::own_need_for`]), where no other reason can hide it.
    pub async fn attention(&self, key: &'static str, text: Option<String>) {
        self.attention_owned(None, key, text).await;
    }

    async fn attention_owned(&self, thread: Option<&str>, key: &'static str, text: Option<String>) {
        self.publish_attention(self.want_attention_owned(thread, key, text))
            .await;
    }

    fn want_attention_owned(
        &self,
        thread: Option<&str>,
        key: &'static str,
        text: Option<String>,
    ) -> Option<Client> {
        let key = AttentionReason::new(key, thread);
        {
            let mut st = self.lock();
            match text {
                Some(text) if st.acting() => {
                    st.attention.insert(key, text.into());
                }
                Some(_) => return None,
                None => {
                    st.attention.remove(&key);
                }
            }
            st.client.clone()
        }
    }

    /// Mutate locally while the caller holds its runtime registration fence; reconciliation
    /// reads the latest desired map only after that fence has been released.
    pub(crate) fn want_attention_for(
        &self,
        thread: &str,
        key: &'static str,
        text: Option<String>,
    ) -> Option<Client> {
        self.want_attention_owned(Some(thread), key, text)
    }

    /// Whether thread `thread`'s attention reason `key` stands as this process wants it.
    pub(crate) fn attention_stands(&self, thread: &str, key: &'static str) -> bool {
        self.lock()
            .attention
            .contains_key(&AttentionReason::new(key, Some(thread)))
    }

    pub(crate) fn attention_snapshot_for(
        &self,
        thread: &str,
        key: &'static str,
    ) -> AttentionSnapshot {
        let reason = AttentionReason::new(key, Some(thread));
        let entry = self.lock().attention.get(&reason).cloned();
        AttentionSnapshot { reason, entry }
    }

    /// Compare and retire together: a later raise, including identical words, keeps its
    /// custody across the caller's asynchronous read. Keeping the old Arc prevents ABA.
    pub(crate) fn want_clear_attention(&self, snapshot: &AttentionSnapshot) -> Option<Client> {
        let mut st = self.lock();
        let current = match (st.attention.get(&snapshot.reason), &snapshot.entry) {
            (Some(current), Some(earlier)) => Arc::ptr_eq(current, earlier),
            (None, None) => true,
            _ => false,
        };
        if !current {
            return None;
        }
        st.attention.remove(&snapshot.reason);
        st.client.clone()
    }

    /// Bring the `owner=codex-alab` entry to the reasons wanted: the asks' slot only. A wait's
    /// own changes owe [`Self::show_waits`], so a reason's write never carries a wait's with it
    /// (a native turn's start clears the root's `wall` reason inline, and its wait's write goes
    /// on a task of its own: review of f134d2b2cb, finding 4).
    pub(crate) async fn publish_attention(&self, client: Option<Client>) {
        if let Some(client) = client {
            self.sync_own_attention_held(&client, false).await;
        }
    }

    /// A reason belonging to one registered root. Another root's progress cannot retire it.
    pub async fn attention_for(&self, thread: &str, key: &'static str, text: Option<String>) {
        self.attention_owned(Some(thread), key, text).await;
    }

    /// A stopped or superseded runtime leaves no scoped reason behind. Session reasons
    /// and other roots remain; the existing posted-answer/outcome custody stays intact.
    pub(crate) fn clear_thread_attention(self: &Arc<Self>, thread: &str) {
        let (client, needs, unheld, unlanded) = {
            let mut st = self.lock();
            st.attention
                .retain(|reason, _| reason.thread.as_deref() != Some(thread));
            // Its wait too: nothing types into a stopped thread when it ends.
            st.waits.remove(thread);
            // And its hold and Land's: nothing holds a stopped thread.
            let unheld = st.holds.remove(thread).is_some();
            let unlanded = st.lands.remove(thread).is_some();
            let had = st.own_needs.len();
            st.own_needs
                .retain(|reason, _| reason.thread.as_deref() != Some(thread));
            let needs = st.own_needs.len() != had;
            if needs {
                st.compose_needs();
            }
            if let Some(posted) = st
                .posted
                .as_mut()
                .filter(|posted| posted.thread.as_deref() == Some(thread))
            {
                posted.plan_current = false;
            }
            (st.client.clone(), needs, unheld, unlanded)
        };
        // And its face on the line: a stopped thread wears none.
        self.shared()
            .set_task_face(thread, codex_alab_core::session::TaskFace::Open);
        self.reconcile_later(client, needs);
        if unheld {
            self.show_held_later();
        }
        if unlanded {
            self.show_land_later();
        }
    }

    /// Retire completed local reasons before yielding. The deferred write reads the
    /// current desired map, so a subsequent raise with the same key and words survives.
    pub(crate) fn clear_attention_reasons(
        self: &Arc<Self>,
        thread: &str,
        reasons: impl IntoIterator<Item = &'static str>,
    ) {
        let (client, needs) = {
            let mut st = self.lock();
            let mut needs = false;
            for key in reasons {
                let reason = AttentionReason::new(key, Some(thread));
                st.attention.remove(&reason);
                // Codex-alab's own need of the same key: the wall it named is behind the root.
                needs |= st.own_needs.remove(&reason).is_some();
            }
            if needs {
                st.compose_needs();
            }
            (st.client.clone(), needs)
        };
        self.reconcile_later(client, needs);
    }

    /// The deferred writes after a local retire: the reasons, and the needs entry where one
    /// of codex-alab's own needs went.
    fn reconcile_later(self: &Arc<Self>, client: Option<Client>, needs: bool) {
        if let Some(client) = client {
            let sup = Arc::clone(self);
            tokio::spawn(async move {
                sup.sync_attention(&client).await;
                if needs {
                    sup.sync_needs(&client, NEEDS_OWN_PAST).await;
                }
            });
        }
    }

    /// Clear codex-alab's own attention and every thread's wait (a standby, a lost claim).
    async fn clear_attention(&self, client: &Client) {
        {
            let mut st = self.lock();
            st.attention.clear();
            st.waits.clear();
        }
        self.sync_attention(client).await;
    }

    /// Bring codex-alab's own `owner=codex-alab` entry, then its `owner=codex-alab-wait`
    /// entry, to what is wanted.
    async fn sync_attention(&self, client: &Client) {
        self.sync_attention_held(client, false).await;
    }

    async fn sync_attention_held(&self, client: &Client, held_only: bool) {
        self.sync_own_attention_held(client, held_only).await;
        self.sync_waits_held(client, held_only).await;
    }

    async fn sync_own_attention_held(&self, client: &Client, held_only: bool) {
        // One at a time (`attention_sync`, one permit, never closed); what is wanted is read
        // once the one before is done.
        let _one = self.attention_sync.acquire().await;
        let (want, shown, held, unknown, claim) = {
            let st = self.lock();
            (
                st.attention.values().next().map(ToString::to_string),
                st.shown.clone(),
                st.held,
                st.attention_unknown(),
                st.claims_taken,
            )
        };
        if held_only && (!held || !client.holds_claim()) {
            return;
        }
        // The first sync under a new proven claim settles whatever an earlier image left,
        // including a key hidden by another owner's newer entry. While unheld, only a
        // known entry's existing cleanup can run; no unknown key is this process's to take.
        if want == shown && !unknown {
            self.lock().attention_retry = None;
            return;
        }
        if (want.is_some() || unknown) && !held {
            return;
        }
        // From the exit on, nobody is left to carry an entry put up: it only comes down.
        if want.is_some() && self.closing() {
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
            self.note(match &want {
                Some(text) => format!("attention: {text}"),
                None => "attention cleared".to_string(),
            });
            let mut st = self.lock();
            st.shown = want;
            // A claim taken during the write is still unsettled; only this sampled claim
            // was reconciled, and a refused/unanswered write settles none.
            st.attention_settled = claim;
            st.attention_retry = None;
        } else {
            self.lock().attention_retry = client.is_open().then(|| clock() + ATTENTION_RETRY);
            self.attention_wake.notify_one();
        }
    }

    /// THREAD `thread`'S AUTOMATIC WAIT (amended 2026-10-27 after the parity sweep, gap 2):
    /// `Some((until, words))` raises it, `None` takes it down; `true` when what aterm should
    /// show may have moved, and [`Self::show_waits`] is then owed. A wait is raised only while
    /// codex-alab acts. Synchronous, so a caller can hold its runtime gate over it (the gate a
    /// native turn's start takes before it takes the wait down), as over
    /// [`Self::want_attention_for`].
    pub(crate) fn want_wait_for(&self, thread: &str, wait: Option<(Instant, String)>) -> bool {
        let mut st = self.lock();
        match wait {
            Some((until, text)) if st.acting() => {
                let moved = st
                    .waits
                    .get(thread)
                    .is_none_or(|old| old.until != until || *old.text != *text);
                st.waits.insert(
                    thread.to_owned(),
                    Waiting {
                        until,
                        text: text.into(),
                    },
                );
                moved
            }
            Some(_) => false,
            None => st.waits.remove(thread).is_some(),
        }
    }

    /// Bring aterm's `owner=codex-alab-wait` entry to the waits wanted now.
    pub(crate) async fn show_waits(&self) {
        let client = self.lock().client.clone();
        if let Some(client) = client {
            self.sync_waits_held(&client, false).await;
        }
    }

    /// [`Self::show_waits`] on a task of its own, for a caller that must not wait on aterm: a
    /// native turn's start, which core awaits before the turn runs, and a loss of the claim
    /// (review of f134d2b2cb, findings 1 and 4). The sync reads what is wanted under the
    /// permit when it runs, so a late one never shows a wait taken down since.
    pub(crate) fn show_waits_later(&self) {
        if tokio::runtime::Handle::try_current().is_err() {
            return;
        }
        let me = self.me.clone();
        tokio::spawn(async move {
            if let Some(me) = me.upgrade() {
                me.show_waits().await;
            }
        });
    }

    /// The `owner=codex-alab-wait` entry, always `kind=wait`: an entry with no kind is an ask
    /// (NEEDS YOU, a border), and the `owner=codex-alab` slot is the asks'. Its rules are the
    /// own entry's ([`Self::sync_own_attention_held`]), one at a time under `waits_sync`:
    /// shown only while the claim is held, the first sync under a new claim settles what an
    /// earlier image left, and from the exit on it only comes down.
    async fn sync_waits_held(&self, client: &Client, held_only: bool) {
        let _one = self.waits_sync.acquire().await;
        let (want, shown, held, unknown, claim) = {
            let st = self.lock();
            (
                st.wait_want(),
                st.waits_shown.clone(),
                st.held,
                st.waits_unknown(),
                st.claims_taken,
            )
        };
        if held_only && (!held || !client.holds_claim()) {
            return;
        }
        if want == shown && !unknown {
            self.lock().waits_retry = None;
            return;
        }
        if (want.is_some() || unknown) && !held {
            return;
        }
        if want.is_some() && self.closing() {
            return;
        }
        let ok = match &want {
            Some(text) => client
                .set_attention_owned(codex_alab_aterm::WAIT_OWNER, Some("wait"), text)
                .await
                .is_ok(),
            None => client
                .clear_attention_owned(codex_alab_aterm::WAIT_OWNER)
                .await
                .is_ok(),
        };
        if ok {
            self.note(match &want {
                Some(text) => format!("waiting shown: {text}"),
                None => "waiting cleared".to_string(),
            });
            let mut st = self.lock();
            st.waits_shown = want;
            st.waits_settled = claim;
            st.waits_retry = None;
        } else {
            self.lock().waits_retry = client.is_open().then(|| clock() + ATTENTION_RETRY);
            self.attention_wake.notify_one();
        }
    }

    /// THREAD `thread`'S HOLD IN ATERM'S `status held=` (amended 2026-10-28 after the parity
    /// sweep, gap 4 stage 3): `Some(arm)` is the arm its standing hold is told by, `None` takes
    /// it out; `true` when it moved, and [`Self::show_held`] is then owed. An arm is set only
    /// while codex-alab acts. Synchronous, so a caller can hold its runtime gate over it, as
    /// over [`Self::want_wait_for`].
    pub(crate) fn want_held_for(&self, thread: &str, arm: Option<HeldBy>) -> bool {
        let mut st = self.lock();
        match arm {
            Some(arm) if st.acting() => st.holds.insert(thread.to_owned(), arm) != Some(arm),
            Some(_) => false,
            None => st.holds.remove(thread).is_some(),
        }
    }

    /// Drop thread `thread`'s arm in [`State::holds`] unless it is `keep` (`None` takes any arm
    /// out; `Some(arm)` takes out the other arm, and never sets one not already held).
    pub(crate) fn keep_held_for(&self, thread: &str, keep: Option<HeldBy>) -> bool {
        let mut st = self.lock();
        if st.holds.get(thread).copied() != keep {
            st.holds.remove(thread).is_some()
        } else {
            false
        }
    }

    /// Bring aterm's `status held=` to the arm the tab shows now.
    pub(crate) async fn show_held(&self) {
        let client = self.lock().client.clone();
        if let Some(client) = client {
            self.sync_held(&client).await;
        }
    }

    /// [`Self::show_held`] on a task of its own, for a caller that must not wait on aterm: a
    /// turn's end and an Off's park under their runtime gate, a stopped runtime. The sync reads
    /// the arm wanted under the permit when it runs, so a late one never shows an arm taken out
    /// since.
    pub(crate) fn show_held_later(&self) {
        if tokio::runtime::Handle::try_current().is_err() {
            return;
        }
        let me = self.me.clone();
        tokio::spawn(async move {
            if let Some(me) = me.upgrade() {
                me.show_held().await;
            }
        });
    }

    /// THE TAB'S ARM ([`State::held_want`]) on the claim's connection, one at a time under
    /// `held_sync`: sent only while the proven claim is held and before the exit (whose release
    /// drops it), and only where it differs from what aterm answered under this claim. A new
    /// claim's first sync sends the tab's arm or takes down one aterm kept for this process
    /// ([`ClaimSent`], [`claim_field_owed`]), unless the last answer, under any claim of this image, proved none
    /// stands (an unset taken, a refusal, a denial: aterm's standing claim was not this
    /// process's, and the arm went with it); an arm an earlier claim showed, an earlier image's
    /// (before an exec) and one a failed call may have set prove nothing. An aterm that refused
    /// the arm (one before the verb answers `ERR unknown meta field (…)`) is not asked again
    /// under this claim until the arm moves; one that denied it, not under this claim at all. A
    /// call that fails unsettles what was: aterm may have taken the line it never answered, and
    /// keep that arm for the reconnect's claim.
    async fn sync_held(&self, client: &Client) {
        let _one = self.held_sync.acquire().await;
        let primary = self.opts.shared.view().primary_thread;
        let (want, claim) = {
            let mut st = self.lock();
            if self.closing() || !st.held || !client.holds_claim() {
                return;
            }
            let want = st.held_want(primary.as_deref());
            let claim = st.claims_taken;
            if !claim_field_owed(&mut st.held_sent, claim, want.as_ref()) {
                return;
            }
            (want, claim)
        };
        let answer = match client.harness_held(want.map(HeldBy::word)).await {
            Ok(HeldOutcome::Shown) => {
                self.note(match want {
                    Some(arm) => format!("held shown: {}", arm.word()),
                    None => "held cleared".to_string(),
                });
                ClaimAnswer::Shown(want)
            }
            Ok(HeldOutcome::Unsupported(status)) => {
                self.note(format!(
                    "held not shown: aterm takes no harness_held ({status})"
                ));
                ClaimAnswer::Unsupported(want)
            }
            Ok(HeldOutcome::Denied(status)) => {
                self.note(format!("held not shown: {status}"));
                ClaimAnswer::Denied
            }
            Ok(HeldOutcome::NotHeld) => return,
            Err(e) => {
                self.note(format!("held not shown: {e}"));
                self.lock().held_sent = None;
                return;
            }
        };
        let mut st = self.lock();
        if self.closing() || !st.held || st.claims_taken != claim {
            return;
        }
        st.held_sent = Some(ClaimSent { claim, answer });
    }

    /// THREAD `thread`'S LAND HOLD IN ATERM'S LANDING (amended 2026-10-28 after the parity sweep,
    /// gap 11): `Some(hold)` is its hold now ([`crate::decide::ThreadState::land_held`]), `None`
    /// takes it out; `true` when it moved, and [`Self::show_land`] is then owed. A hold is set
    /// only while codex-alab acts; while passive, a turn's start or end that updates the yield
    /// reading on the standing hold updates its `read` in place for the next sync. Synchronous,
    /// so a caller can hold its runtime gate and the thread's state over it, as over
    /// [`Self::want_held_for`].
    pub(crate) fn want_land_for(&self, thread: &str, land: Option<LandReport>) -> bool {
        let mut st = self.lock();
        match land {
            Some(land) if st.acting() => {
                st.lands.get(thread) != Some(&land) && {
                    st.lands.insert(thread.to_owned(), land);
                    true
                }
            }
            Some(land) => {
                if let Some(held) = st
                    .lands
                    .get_mut(thread)
                    .filter(|held| held.since_ms == land.since_ms && held.nudges == land.nudges)
                {
                    held.read = land.read;
                }
                false
            }
            None => st.lands.remove(thread).is_some(),
        }
    }

    /// Bring aterm's landing to the hold the tab shows now.
    pub(crate) async fn show_land(&self) {
        let client = self.lock().client.clone();
        if let Some(client) = client {
            self.sync_land(&client).await;
        }
    }

    /// [`Self::show_land`] on a task of its own, for a caller that must not wait on aterm (a
    /// lifecycle callback, a runtime gate held): the sync reads the hold wanted under its
    /// permit when it runs, so a late one never shows a hold moved since.
    pub(crate) fn show_land_later(&self) {
        if tokio::runtime::Handle::try_current().is_err() {
            return;
        }
        let me = self.me.clone();
        tokio::spawn(async move {
            if let Some(me) = me.upgrade() {
                me.show_land().await;
            }
        });
    }

    /// LAND'S HOLD THE TAB SHOWS ([`State::land_want`]) on the claim's connection, one at a time
    /// under `land_sync`, by the arm's rule ([`claim_field_owed`]): sent only while the proven
    /// claim is held and before the exit (whose release drops it), and only where it differs
    /// from what aterm answered under this claim; a claim's first sync sends the standing hold
    /// or takes down one aterm kept for this process. An aterm before the verb is asked again
    /// only once the hold moves; one that denied it, not under this claim at all. A call that
    /// fails unsettles what was.
    async fn sync_land(&self, client: &Client) {
        let _one = self.land_sync.acquire().await;
        let primary = self.opts.shared.view().primary_thread;
        let (want, claim) = {
            let mut st = self.lock();
            if self.closing() || !st.held || !client.holds_claim() {
                return;
            }
            let want = st.land_want(primary.as_deref());
            let claim = st.claims_taken;
            if !claim_field_owed(&mut st.land_sent, claim, want.as_ref()) {
                return;
            }
            (want, claim)
        };
        let answer = match client.harness_land(want.as_ref()).await {
            Ok(LandOutcome::Shown) => {
                self.note(match &want {
                    Some(land) => format!("landing shown: {}", land_note(land)),
                    None => "landing cleared".to_string(),
                });
                ClaimAnswer::Shown(want)
            }
            Ok(LandOutcome::Unsupported(status)) => {
                self.note(format!(
                    "landing not shown: aterm takes no harness_land ({status})"
                ));
                ClaimAnswer::Unsupported(want)
            }
            Ok(LandOutcome::Denied(status)) => {
                self.note(format!("landing not shown: {status}"));
                ClaimAnswer::Denied
            }
            Ok(LandOutcome::NotHeld) => return,
            Err(e) => {
                self.note(format!("landing not shown: {e}"));
                self.lock().land_sent = None;
                return;
            }
        };
        let mut st = self.lock();
        if self.closing() || !st.held || st.claims_taken != claim {
            return;
        }
        st.land_sent = Some(ClaimSent { claim, answer });
    }

    /// Metadata retries have no unheld authority. Off retains its immediate cleanup;
    /// pending deadlines park until the next proven claim reconciles current desired state.
    fn attention_retry_at(&self) -> Option<tokio::time::Instant> {
        let st = self.lock();
        if self.closing() || !st.held || !st.client.as_ref().is_some_and(Client::holds_claim) {
            return None;
        }
        [st.attention_retry, st.waits_retry, st.needs_retry]
            .into_iter()
            .flatten()
            .min()
            .map(tokio::time::Instant::from_std)
    }

    async fn retry_attention(&self, client: &Client) {
        let (attention, waits, needs) = {
            let st = self.lock();
            let now = clock();
            (
                st.attention_retry.is_some_and(|at| at <= now),
                st.waits_retry.is_some_and(|at| at <= now),
                st.needs_retry.is_some_and(|at| at <= now),
            )
        };
        if attention {
            self.sync_own_attention_held(client, true).await;
        }
        if waits {
            self.sync_waits_held(client, true).await;
        }
        if needs {
            self.sync_needs_projection(client, NEEDS_REPORTED, false, true)
                .await;
        }
    }

    /// THE WORKER'S STANDING NEEDS (FOREVER CRUISE / FOREVER LAND: "if you need something
    /// from the user, be LOUDER ABOUT IT ... but NEVER let that block you"): raised under
    /// `owner=needs kind=ask` as `Codex needs you: <first need>[ (+N more)] — carrying on`
    /// ([`needs_text`]), at the point itself (never behind a back-off or the grace), or
    /// cleared where `standing` is empty. `key` is the set's ([`crate::needs::needs_key`]).
    /// Shown while a proven claim is held; kept across a passive spell; taken down only by
    /// the worker's report, a person's turn, a person's or the config's off and the exit.
    pub async fn needs(&self, standing: &[String], key: u64) {
        if self.want_needs(standing, key) {
            self.show_needs().await;
        }
    }

    /// The needs set to show ([`Supervisor::needs`], without the aterm call): `true` when it
    /// moved, and [`Supervisor::show_needs`] is then owed. Synchronous, so the callers' order
    /// is the order the sets are wanted in, whatever order their shows run in.
    pub fn want_needs(&self, standing: &[String], key: u64) -> bool {
        let mut st = self.lock();
        self.want_needs_from(&mut st, standing, key)
    }

    /// A thread's memory must have seen the latest Off before its needs can stand.
    /// Check while owning desired state: an Off advances and dismisses the old set
    /// under this lock, so old memory cannot publish after that dismissal.
    pub(crate) fn want_needs_at_off(&self, standing: &[String], key: u64, offs: u64) -> bool {
        let mut st = self.lock();
        if self.offs() != offs {
            return false;
        }
        self.want_needs_from(&mut st, standing, key)
    }

    fn want_needs_from(&self, st: &mut State, standing: &[String], key: u64) -> bool {
        if st.needs_key == key {
            return false;
        }
        st.needs_key = key;
        st.needs_named = standing.to_vec();
        st.compose_needs();
        if key != 0 {
            // A set stands again: whatever an Off took down is not what stands now.
            self.opts.shared.set_needs_off(0);
        }
        true
    }

    /// ONE OF CODEX-ALAB'S OWN NEEDS (`Some`: the quota's, B3, which only a person lifts) said
    /// loud, or taken down (`None`): a need of the same `owner=needs kind=ask` entry as the
    /// worker's, put first ([`needs_text`]: `Codex needs you: <first>[ (+N more)] — carrying
    /// on`, aterm's frame, whose count aterm reads as `needs_n=`), so it is never one of the
    /// `attention` reasons only the first of which aterm shows (an `api:` entry hid the
    /// quota's NEEDS YOU there), and a manager's `await task` wakes on it as on any worker's
    /// need. Raised only while codex-alab acts; kept across a passive spell, as the worker's
    /// needs are; taken down at any time. Kept with the root that raised it, as its
    /// `attention` reasons are: that root's turn past the wall, or its stopped runtime, takes
    /// it down ([`Self::clear_attention_reasons`], [`Self::clear_thread_attention`]).
    pub async fn own_need_for(&self, thread: &str, key: &'static str, need: Option<String>) {
        if self.want_own_need_for(thread, key, need) {
            let client = self.lock().client.clone();
            if let Some(client) = client {
                self.sync_needs(&client, NEEDS_OWN_PAST).await;
            }
        }
    }

    /// The own need wanted ([`Self::own_need_for`], without the aterm call): `true` when the
    /// set moved, and [`Self::show_needs`] is then owed. Synchronous, so a caller can hold its
    /// runtime registration fence over it, as over [`Self::want_attention_for`].
    pub(crate) fn want_own_need_for(
        &self,
        thread: &str,
        key: &'static str,
        need: Option<String>,
    ) -> bool {
        let mut st = self.lock();
        Self::want_own_need_from(&mut st, thread, key, need)
    }

    /// A decision's need can outlive preparation awaits. Its caller holds the
    /// native registration fence; policy and Off must still match while this
    /// same guard inserts the need, so an Off cannot clear and then be undone.
    pub(crate) fn want_own_need_for_at_policy(
        &self,
        thread: &str,
        key: &'static str,
        need: Option<String>,
        policy: &crate::decide::Policy,
        offs: u64,
    ) -> bool {
        let mut st = self.lock();
        if st.policy.as_ref() != Some(policy) || self.offs() != offs {
            return false;
        }
        Self::want_own_need_from(&mut st, thread, key, need)
    }

    fn want_own_need_from(
        st: &mut State,
        thread: &str,
        key: &'static str,
        need: Option<String>,
    ) -> bool {
        let reason = AttentionReason::new(key, Some(thread));
        let moved = match need {
            Some(need) if st.acting() => {
                st.own_needs.insert(reason, need.clone()).as_ref() != Some(&need)
            }
            Some(_) => false,
            None => st.own_needs.remove(&reason).is_some(),
        };
        if moved {
            st.compose_needs();
        }
        moved
    }

    /// Bring aterm's `owner=needs` attention to the set wanted last.
    pub async fn show_needs(&self) {
        let client = self.lock().client.clone();
        if let Some(client) = client {
            self.sync_needs(&client, NEEDS_REPORTED).await;
        }
    }

    /// The key of the needs set last handed to [`Supervisor::needs`] (`0`: none).
    pub fn needs_key(&self) -> u64 {
        self.lock().needs_key
    }

    /// The needs text aterm shows now, if any (tests read it).
    pub fn needs_shown(&self) -> Option<String> {
        self.lock().needs_shown.clone()
    }

    async fn sync_needs_after_off(&self, client: &Client, why: &str) {
        self.sync_needs_with_off(client, why, true).await;
    }

    /// Bring aterm's `owner=needs` entry to the set wanted; `why_clear` says why where it goes.
    async fn sync_needs(&self, client: &Client, why_clear: &str) {
        self.sync_needs_with_off(client, why_clear, false).await;
    }

    async fn sync_needs_with_off(&self, client: &Client, why_clear: &str, after_off: bool) {
        self.sync_needs_projection(client, why_clear, after_off, false)
            .await;
    }

    /// The `owner=needs` entry brought to the set wanted, then the set it says told whole
    /// ([`Self::sync_needs_told`]): wherever the entry is set, replaced or taken down, the set
    /// follows it (after an Off, its release took the set down already).
    async fn sync_needs_projection(
        &self,
        client: &Client,
        why_clear: &str,
        after_off: bool,
        held_only: bool,
    ) {
        self.sync_needs_entry(client, why_clear, after_off, held_only)
            .await;
        self.sync_needs_told(client).await;
    }

    async fn sync_needs_entry(
        &self,
        client: &Client,
        why_clear: &str,
        after_off: bool,
        held_only: bool,
    ) {
        // One at a time, with codex-alab's own attention (one permit, never closed).
        let _one = self.attention_sync.acquire().await;
        let (want, standing, shown, held, unknown, claim) = {
            let st = self.lock();
            (
                st.needs_want.clone(),
                st.needs_standing(),
                st.needs_shown.clone(),
                st.held,
                st.needs_unknown(),
                st.claims_taken,
            )
        };
        if held_only && (!held || !client.holds_claim()) {
            return;
        }
        // Off takes its old entry down even if a fresh report arrived during the
        // release. Keep that report wanted for the next claim; only its wire target
        // is empty while released. A reclaim instead reconciles the latest set.
        let want = if after_off && !held { None } else { want };
        // An entry this process did not put up may stand ([`State::needs_unknown`]): with none
        // wanted it is taken down all the same; with a set wanted, putting it up replaces it.
        let left = want.is_none() && unknown;
        if want.is_some() && (!held || self.closing()) {
            return;
        }
        if want == shown && !unknown {
            let mut st = self.lock();
            st.needs_retry = None;
            // The entry shown says the set wanted now: a set whose first need and count are the
            // ones shown changes no word of the entry, but its others are what aterm reads.
            if want.is_some() {
                st.needs_shown_set = standing;
            }
            return;
        }
        let why_clear = if left && shown.is_none() {
            NEEDS_LEFT_DOWN
        } else {
            why_clear
        };
        let ok = match &want {
            Some(text) => client
                .set_attention_owned(codex_alab_aterm::NEEDS_OWNER, Some("ask"), text)
                .await
                .is_ok(),
            None => client
                .clear_attention_owned(codex_alab_aterm::NEEDS_OWNER)
                .await
                .is_ok(),
        };
        if ok {
            self.note(match &want {
                Some(text) => format!("NEEDS {text}"),
                None => format!("CLEARED needs ({why_clear})"),
            });
            let mut st = self.lock();
            st.needs_shown_set = if want.is_some() { standing } else { Vec::new() };
            st.needs_shown = want;
            // Settled for the claim it was read under: a claim taken since is settled by the
            // sync it owes.
            st.needs_settled = claim;
            st.needs_retry = None;
        } else {
            self.lock().needs_retry = client.is_open().then(|| clock() + ATTENTION_RETRY);
            self.attention_wake.notify_one();
        }
    }

    /// THE SET THE SHOWN ENTRY SAYS, WHOLE ([`State::needs_told_want`]), on the claim's
    /// connection, one at a time under `needs_told_sync`, by the arm's rule
    /// ([`claim_field_owed`]): sent only while the proven claim is held and before the exit, and
    /// only where it differs from what aterm answered under this claim; a claim's first sync
    /// sends the standing set or takes down one aterm kept for this process, unless the last
    /// answer proved none stands. An aterm before the verb is asked again only once the set
    /// moves; one that denied it, not under this claim at all. A call that fails unsettles what
    /// was.
    async fn sync_needs_told(&self, client: &Client) {
        let _one = self.needs_told_sync.acquire().await;
        self.tell_needs(client, false).await;
    }

    /// [`Self::sync_needs_told`]'s body; `down` takes the set down whatever is shown, the exit's
    /// after its entry, on the claim it still holds though closing. The exit calls it without
    /// the permit: the release that follows on this connection drops whatever set stands, so it
    /// never waits for a sync in flight (which a session task stopped mid-call would hold), and
    /// a set such a sync lands after it lives only until that release.
    async fn tell_needs(&self, client: &Client, down: bool) {
        let (want, claim) = {
            let mut st = self.lock();
            if (!down && (self.closing() || !st.held)) || !client.holds_claim() {
                return;
            }
            let want = if down { None } else { st.needs_told_want() };
            let claim = st.claims_taken;
            if !claim_field_owed(&mut st.needs_told_sent, claim, want.as_ref()) {
                return;
            }
            (want, claim)
        };
        let told = want
            .as_ref()
            .map(|told| (told.entry.as_str(), told.needs.as_slice()));
        let answer = match client.harness_needs(told).await {
            Ok(NeedsOutcome::Shown) => {
                self.note(match &want {
                    Some(told) => format!("needs told whole: {}", told.needs.len()),
                    None => "needs told cleared".to_string(),
                });
                ClaimAnswer::Shown(want)
            }
            Ok(NeedsOutcome::Unsupported(status)) => {
                self.note(format!(
                    "needs not told whole: aterm takes no harness_needs ({status})"
                ));
                ClaimAnswer::Unsupported(want)
            }
            Ok(NeedsOutcome::Denied(status)) => {
                self.note(format!("needs not told whole: {status}"));
                ClaimAnswer::Denied
            }
            Ok(NeedsOutcome::NotHeld) => return,
            Err(e) => {
                self.note(format!("needs not told whole: {e}"));
                self.lock().needs_told_sent = None;
                return;
            }
        };
        let mut st = self.lock();
        if (!down && (self.closing() || !st.held)) || st.claims_taken != claim {
            return;
        }
        st.needs_told_sent = Some(ClaimSent { claim, answer });
    }

    /// Store the session's Autopilot `word` a `status` read named, where the policy in force
    /// says another: every thread looks again under it, as a policy read that moves the word
    /// has them do.
    fn adopt_word(&self, word: Mode) {
        let moved = {
            let mut st = self.lock();
            match st.policy.as_mut() {
                Some(policy) if policy.autopilot != word => {
                    policy.autopilot = word;
                    // Under the lock that moved the policy, so ordered with resume's and
                    // taken's words; acting only, so a passive session keeps its own.
                    self.opts.shared.set_acting_word(acting_word(word));
                    true
                }
                _ => false,
            }
        };
        if moved {
            self.note(format!(
                "status autopilot={}: the threads look again",
                word.word()
            ));
            self.show_land_later();
            self.claims.send_modify(|n| *n += 1);
        }
    }

    /// `@self status` for a read of the session's word, publishing THE LINE'S TIMER (gap 12 of
    /// the parity sweep, 2026-10-29: a codex tab showed no timer): aterm's `status land_at=`
    /// as this reply said it, before the word it read is adopted (CONTRACT §3). A read that
    /// fails publishes nothing, so the timer last read stands, as the word does. These reads
    /// (the policy's, the word's recheck, a hold's while unheld) go one at a time
    /// ([`Supervisor::timer_reads`]), the read and its publication together, so an older reply
    /// never stores its timer after a newer one's; an act's own read ([`Supervisor::ready`])
    /// publishes none, as it runs beside them on the thread's task (amended 2026-10-29 after
    /// the review of gap 12, finding 6: its reply, taken before a refresh's, could store a
    /// timer the refresh had seen cancelled).
    async fn status_with_timer(&self, client: &Client) -> Result<Status, AtermError> {
        let _one = self.timer_reads.acquire().await;
        let status = client.status().await?;
        self.shared().set_land_at(status.land_at());
        Ok(status)
    }

    /// The session's Autopilot word (`@self status autopilot=`); the word in force where aterm
    /// says none (`-`, or no `autopilot=` from an older aterm), says `off`, or `status` cannot
    /// be read now. `-` is no word: aterm prints it until its status sweep admits the
    /// codex-alab in the session, so a read at launch or while aterm names another program
    /// says nothing of the session's word ([`Status::autopilot_said`]). `off` is no mode:
    /// aterm names a person's hold before an Off (`why=hold` while `status autopilot=off`),
    /// and an Off taken for Cruise there made every thread look again and ended a landing
    /// (the ninth review of the strict reading, finding 22); the release when the hold lifts
    /// parks the threads, keeping the landing. The word in force is the stored policy's, else
    /// the word read last (kept apart from the policy, which a standby drops), else Cruise.
    async fn autopilot_word(&self, client: &Client) -> Mode {
        self.autopilot_word_said(client).await.0
    }

    /// [`Supervisor::autopilot_word`], and whether `status` said `off`: under a person's hold
    /// the mode in force stays, and the chrome names the Off.
    async fn autopilot_word_said(&self, client: &Client) -> (Mode, bool) {
        let said = self
            .status_with_timer(client)
            .await
            .ok()
            .and_then(|status| status.autopilot_said());
        let said_off = said == Some(codex_alab_aterm::status::Autopilot::Off);
        let mut st = self.lock();
        let word = match st.read_word(said) {
            Some(word) => word,
            None => st
                .policy
                .as_ref()
                .map(|p| p.autopilot)
                .or(st.last_word)
                .unwrap_or(Mode::Cruise),
        };
        (word, said_off)
    }

    /// The first of codex-alab's attention reasons that stands, if any (a point it left to a
    /// person).
    pub fn standing_attention(&self) -> Option<String> {
        self.lock()
            .attention
            .values()
            .next()
            .map(ToString::to_string)
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
            (
                st.box_look,
                st.attention
                    .contains_key(&AttentionReason::new("box", None)),
            )
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
            let (posted, reconcile) = {
                let mut st = self.lock();
                let posted = match &st.posted {
                    Some(p) if p.id == outcome.id => st.posted.take(),
                    _ => None,
                };
                // Consume the outcome and its local reason-clear authority together.
                // Runtime retirement uses this same lock: it either revokes the still
                // pending Posted, or follows this clear before any replacement can raise.
                let clear_plan = outcome.applied.is_ok()
                    && posted.as_ref().is_some_and(|posted| {
                        posted.rule == answer::RULE_PLAN && posted.plan_current
                    });
                let reconcile = if clear_plan {
                    if let Some(thread) =
                        posted.as_ref().and_then(|posted| posted.thread.as_deref())
                    {
                        st.attention
                            .remove(&AttentionReason::new("plan", Some(thread)));
                    }
                    st.client.clone()
                } else {
                    None
                };
                (posted, reconcile)
            };
            self.publish_attention(reconcile).await;
            match (outcome.applied, posted) {
                (Ok(()), Some(posted)) if answer::records_act(posted.rule) => {
                    self.record_act(posted.rule, &posted.words).await;
                }
                (Ok(()), Some(posted)) => {
                    // No act of the task (the goal box's `Leave paused`): told, never recorded.
                    if let Some(client) = self.claimed_client(posted.rule, &posted.words) {
                        self.told(&client, posted.rule, &posted.words, "").await;
                    }
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
                self.lock()
                    .attention
                    .insert(AttentionReason::new("box", None), text.into());
            }
            Some(_) => return,
            None => {
                self.lock()
                    .attention
                    .remove(&AttentionReason::new("box", None));
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
            st.attention
                .insert(AttentionReason::new("box", None), text.into());
        };
        // Only a root thread of this TUI's own: never a sub-agent's box it shows.
        let shared = self.opts.shared;
        let thread = match &open.kind {
            codex_alab_core::boxes::BoxKind::Question { thread, .. }
            | codex_alab_core::boxes::BoxKind::Approval { thread, .. } => Some(thread.clone()),
            codex_alab_core::boxes::BoxKind::Plan { .. }
            | codex_alab_core::boxes::BoxKind::Goal { .. } => shared.view().displayed_thread,
        };
        let root = thread
            .as_deref()
            .is_some_and(|thread| shared.is_root_thread(thread));
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
                                        thread: thread.clone(),
                                        plan_current: true,
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

/// How a task-face act went before another native turn
/// ([`Supervisor::record_act_before_turn`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BeforeTurn {
    /// aterm acknowledged it.
    Recorded,
    /// Not recorded now, and another look may publish it: no claim, a refusal, a failed
    /// exchange, or a turn running or on its way under an aterm that cannot refuse past it.
    Retry,
    /// This aterm cannot take the act rightly: nothing was sent, and no look is owed for it.
    TooOld {
        /// The aterm release, as its `version` answer names it (`unknown` where it names none).
        aterm: String,
    },
}

/// The release a client's aterm names in its `version` answer, or `unknown`.
fn release_named(client: &Client) -> String {
    let fields = codex_alab_aterm::wire::parse_fields(client.server_version());
    codex_alab_aterm::wire::field(&fields, "version").unwrap_or_else(|| "unknown".to_string())
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

/// Why a needs entry goes when the worker's own reports moved it.
const NEEDS_REPORTED: &str = "the worker's report names none, or a person's turn answered it";

/// Why a needs entry goes when codex-alab's own need went ([`Supervisor::own_need_for`]).
pub(crate) const NEEDS_OWN_PAST: &str =
    "codex-alab's own need: its root got past the wall, or stopped";

/// Why a needs entry this process never put up came down ([`State::claims_taken`]), aterm's
/// `NEEDS_LEFT_DOWN` in codex-alab's words.
pub const NEEDS_LEFT_DOWN: &str = "an entry left before this claim; no need stands at it";

/// The most display cells the first need takes in its attention text ([`needs_text`]).
pub const NEED_CELLS: usize = 64;

/// The most bytes a needs attention text takes (aterm's attention cap).
const NEEDS_MAX_BYTES: usize = 200;

/// The attention text of a standing needs set: `Codex needs you: <first need, at most
/// [`NEED_CELLS`] cells>[ (+N more)] — carrying on`, within aterm's 200 bytes; `None` for no
/// need.
pub fn needs_text(standing: &[String]) -> Option<String> {
    let first = standing.first()?;
    let more = match standing.len() {
        0 | 1 => String::new(),
        n => format!(" (+{} more)", n - 1),
    };
    let head = "Codex needs you: ";
    let tail = format!("{more} \u{2014} carrying on");
    let room = NEEDS_MAX_BYTES.saturating_sub(head.len() + tail.len());
    let fits =
        unicode_width::UnicodeWidthStr::width(first.as_str()) <= NEED_CELLS && first.len() <= room;
    let cut = if fits {
        first.clone()
    } else {
        // Clipped with `…`, the mark inside the bounds too.
        let mut cells = 0;
        let mut cut = String::new();
        for c in first.chars() {
            let width = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
            if cells + width + 1 > NEED_CELLS || cut.len() + c.len_utf8() + '…'.len_utf8() > room
            {
                break;
            }
            cells += width;
            cut.push(c);
        }
        cut.truncate(cut.trim_end().len());
        cut.push('…');
        cut
    };
    Some(format!("{head}{cut}{tail}"))
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
