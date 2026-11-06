// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The app-server extension: lifecycle contributors that feed the decider, the per-thread
//! timers, the acts, and the approval contributor.
//!
//! Rules every callback keeps: it acts only for a root thread this process's TUI
//! registered, only when [`codex_alab_core::active`] and a proven claim is held; it never
//! awaits aterm I/O (it spawns); a timer holds the thread's runtime state and a `ThreadId`,
//! never an `Arc<CodexThread>` across its sleep, and is cancelled (and joined) when a turn
//! starts and when the thread stops.
//!
//! FOREVER CRUISE / FOREVER LAND (aterm owner direction 2026-10-19): the session's Autopilot
//! word is folded before each decision ([`ThreadState::observe_mode`]); the repository probe
//! runs where the decider asks for it ([`Act::Probe`], [`crate::repo::probe`]); Land's landing
//! is recorded, never typed ([`Act::Landed`]); the needs a report names go on the person's
//! screen at once ([`Supervisor::want_needs`]); and the live upgrade carries a paced act owed
//! across its re-exec instead of waiting on it ([`crate::upgrade`]).

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::Weak;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

use codex_alab_core::session::LastTurn;
use codex_alab_core::session::now_ms;
use codex_core::CodexThread;
use codex_core::StartIfIdleSubmission;
use codex_core::ThreadManager;
use codex_core::TurnInput;
use codex_core::TurnInputRequest;
use codex_core::TurnStartOptions;
use codex_core::config::Config;
use codex_extension_api::ApprovalDecision;
use codex_extension_api::ApprovalDecisionInput;
use codex_extension_api::ApprovalReviewContributor;
use codex_extension_api::CommandStartInput;
use codex_extension_api::ExtensionData;
use codex_extension_api::ExtensionFuture;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::ThreadIdleInput;
use codex_extension_api::ThreadLifecycleContributor;
use codex_extension_api::ThreadReadyInput;
use codex_extension_api::ThreadResumeInput;
use codex_extension_api::ThreadStartInput;
use codex_extension_api::ThreadStopInput;
use codex_extension_api::ToolLifecycleContributor;
use codex_extension_api::ToolLifecycleFuture;
use codex_extension_api::ToolPayload;
use codex_extension_api::ToolStartInput;
use codex_extension_api::TurnAbortInput;
use codex_extension_api::TurnErrorInput;
use codex_extension_api::TurnLifecycleContributor;
use codex_extension_api::TurnStartInput;
use codex_extension_api::TurnStopInput;
use codex_goal_extension::GoalObjectiveUpdate;
use codex_goal_extension::GoalService;
use codex_goal_extension::GoalServiceError;
use codex_goal_extension::GoalSetRequest;
use codex_goal_extension::GoalTokenBudgetUpdate;
use codex_protocol::ThreadId;
use codex_protocol::config_types::ModeKind;
use codex_protocol::error::CodexErrorDetails;
use codex_protocol::items::TurnItem;
use codex_protocol::models::MessagePhase;
use codex_protocol::protocol::AgentStatus;
use codex_protocol::protocol::CodexErrorInfo;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::ReviewDecision;
use codex_protocol::protocol::SessionSource;
use codex_protocol::turn_input::CyberAccessProgram;
use codex_protocol::user_input::UserInput;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use codex_alab_aterm::account_recovery as account_broker;
#[path = "renewal_follow.rs"]
mod renewal_follow;

use crate::approval;
use crate::decide;
use crate::decide::Act;
use crate::decide::LoginRead;
use crate::decide::Owed;
use crate::decide::ThreadState;
use crate::decide::Timing;
use crate::decide::TurnEnd;
use crate::decide::Wall;
use crate::login;
use crate::netwatch;
use crate::netwatch::Reach;
use crate::repo;
use crate::supervisor::BeforeTurn;
use crate::supervisor::RECHECK;
use crate::supervisor::Ready;
use crate::supervisor::Supervisor;
use crate::supervisor::clock;
use crate::upgrade;
use crate::upgrade::UpgradeCheck;
use crate::upgrade_ask;
use crate::upgrade_ask::AskStep;
use crate::words;

/// How the autopilot reads the host's sleep ([`codex_alab_core::wake::read`], or a test's).
pub(crate) type WakeRead = Arc<dyn Fn() -> Option<codex_alab_core::wake::Reading> + Send + Sync>;

/// Every submission's `client_id` starts so (`alab-autopilot-<n>`): a `UserMessage` with
/// another `client_id` is a person's turn.
pub const CLIENT_PREFIX: &str = "alab-autopilot-";

/// The `turn_trigger` codex-alab's turns carry (persisted in `TurnStarted`).
pub const TURN_TRIGGER: &str = "alab_autopilot";

/// The approval act's rule (`harness_act`).
pub const RULE_APPROVE: &str = "approve-all@v1";

/// The plan tool (`core/src/tools/handlers/plan_spec.rs`), whose step in progress the
/// session's description says.
const UPDATE_PLAN: &str = "update_plan";

/// THE TOOLS THAT ARE NO WORK (ruling F's [`TurnEnd::ran_tools`]): the plan's and a question's,
/// which aterm's readers draw no call row for (Claude Code's todo list and its question box);
/// every other tool core starts is a tool run, as aterm's call rows are.
const NOT_WORK_TOOLS: [&str; 3] = [
    UPDATE_PLAN,
    "request_user_input",
    "request_user_input_async",
];

/// The most the approval contributor waits for aterm (the status read and the re-confirm)
/// before it leaves the box to a person: a box is never held up by a wedged aterm.
pub const APPROVAL_ATERM_BUDGET: Duration = Duration::from_secs(1);

/// One bounded background wait for the startup sweep. Expiry rechecks native authority;
/// it never turns still-pending predecessor evidence into a negative verdict.
const SWEPT_WAIT: Duration = Duration::from_secs(5);

/// How long a resumed thread waits for the launch probe (a connect and a `status`, each
/// bounded by the client's 5 s call deadline).
const LAUNCH_PROBE_WAIT: Duration = Duration::from_secs(12);

/// The longest a crash witness may have stood unwritten and still be read as the session
/// aterm just relaunched: a record a day old is a person's later resume. The start-up sweep
/// leaves a younger record for the process it names, and removes an older one.
pub const RELAUNCH_WINDOW: Duration = codex_alab_core::session::RELAUNCH_HORIZON;

/// The most an act or a turn's start waits for the session record to say `busy` (CONTRACT
/// §4: aterm may end the process at the record's rest, so the record leaves rest first).
pub const WRITE_THROUGH_BUDGET: Duration = Duration::from_secs(1);

/// How soon an act whose record could not be written looks again.
const WRITE_THROUGH_RETRY: Duration = Duration::from_secs(5);

/// The most a landing waits for the TUI to take its land turn's end ([`Autopilot::tui_caught_up`]).
const TUI_CATCH_UP: Duration = Duration::from_secs(5);

/// How soon that wait looks again.
const TUI_CATCH_UP_EVERY: Duration = Duration::from_millis(10);

/// The most the decision at a person's Esc waits for core to publish the abort
/// ([`Autopilot::abort_published`]).
const ABORT_PUBLISHED: Duration = Duration::from_secs(5);

/// How soon that wait looks again.
const ABORT_PUBLISHED_EVERY: Duration = Duration::from_millis(10);

/// How long an act that only queues its turn (a compaction, a goal resumed) keeps the record
/// `busy` for that turn to start (`Shared::await_turn`).
pub const ACT_TURN_WAIT: Duration = Duration::from_secs(30);

/// How long a network wall's route may take to read (codex's own proxy resolution, which may
/// run a PAC lookup): past it the route is custom, and its wall goes on the time ladder.
pub const ROUTE_READ_BUDGET: Duration = Duration::from_secs(5);

/// How soon the live upgrade looks again when a thread was deciding (its evaluation permit
/// held: a timer that fired, a repository probe).
pub(crate) const UPGRADE_RELOOK: Duration = Duration::from_secs(5);

/// Write the session record now (not coalesced), off the async runtime, within
/// [`WRITE_THROUGH_BUDGET`]; `true` once it says what the shared state says (or no record is
/// kept).
pub async fn write_through(shared: &'static codex_alab_core::session::Shared) -> bool {
    write_through_with_budget(shared, WRITE_THROUGH_BUDGET).await
}

async fn write_through_with_budget(
    shared: &'static codex_alab_core::session::Shared,
    budget: Duration,
) -> bool {
    matches!(
        tokio::time::timeout(
            budget,
            tokio::task::spawn_blocking(move || shared.flush_record()),
        )
        .await,
        Ok(Ok(true))
    )
}

/// Which threads are the autopilot's by their source: the TUI's root threads (`Cli`).
pub type SourceGate = fn(&SessionSource) -> bool;

/// The codex binary's gate: the TUI's own threads, never exec, MCP, sub-agents or internal.
pub fn tui_threads(source: &SessionSource) -> bool {
    matches!(source, SessionSource::Cli)
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Native account ownership captured before a request starts.
#[derive(Clone, Debug, PartialEq, Eq)]
struct QuotaIdentity {
    account: String,
    user: Option<String>,
    owner: u64,
}

#[derive(Clone, Debug)]
struct QuotaWall {
    wall: codex_alab_core::quota::Wall,
    identity: QuotaIdentity,
}

/// Run a test hook that may block until the test releases it. On a multi-thread runtime the
/// worker first hands its queued tasks to another thread: the task in its LIFO slot cannot be
/// stolen, and it may be the one the test waits on before it releases the hook.
#[cfg(test)]
fn run_hook(hook: Box<dyn FnOnce() + Send>) {
    let multi = tokio::runtime::Handle::try_current()
        .is_ok_and(|h| h.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread);
    if multi {
        tokio::task::block_in_place(hook);
    } else {
        hook();
    }
}

/// One root thread's runtime state, kept in its thread store.
pub struct ThreadRt {
    quota_wall: Mutex<Option<QuotaWall>>,
    /// A live successful human turn can recover only the limited goal it began with.
    /// Kept in memory: a restored transcript or an inherited plain continuation is
    /// never evidence that provider capacity recovered in this process.
    human_goal_recovery: Mutex<Option<(Instant, codex_state::ThreadGoal)>>,
    /// THE GOAL A TURN ERROR BLOCKED AT THE WALL THE POINT STANDS AT ([`wall_goal_after`]):
    /// active when the walled turn began, which called no goal tool and saw no person's stop of
    /// the goal, so upstream's `turn-error` stop is the wall's, never a person's or the model's.
    /// The wall's try resumes it ([`GoalStop::Wall`]). The record carries it
    /// ([`codex_alab_core::session::Pace::wall_goal`]): the image after a re-exec and aterm's
    /// relaunch bind it again ([`Autopilot::seed_pace`]), as the stop they would otherwise read
    /// as a person's is the wall's this process saw (the forever-persist lane, item a).
    wall_goal: Mutex<Option<codex_alab_core::session::WallGoal>>,
    quota_request: Mutex<Option<(QuotaIdentity, u64)>>,
    auth_wall_scope: Mutex<Option<Arc<codex_alab_core::renewal::AuthRequestScope>>>,
    auth_observation: Mutex<Option<Arc<AuthWallObservation>>>,
    renewal_ack_retry: Mutex<Option<Arc<renewal_follow::RenewalAckRetry>>>,
    /// A non-weekly wall keeps ordinary quota recovery until the next native start.
    quota_rotation_declined: AtomicU64,
    /// Known original association, including a positively standalone turn.
    auth_wall_goal_origin: Mutex<Option<Option<String>>>,
    id: ThreadId,
    key: String,
    state: Mutex<ThreadState>,
    /// Core can release a finishing reservation before its lifecycle notification. Keep
    /// native admission and completion ordered here, with the latest locally accepted ID.
    native_turn: tokio::sync::Mutex<Option<String>>,
    /// The selected native turn's program, retained only when continuing its safe scope.
    cyber_access_program: Mutex<Option<CyberAccessProgram>>,
    config: Option<Arc<Config>>,
    token_budget: bool,
    jitter: Duration,
    timer: Mutex<Option<Timer>>,
    timer_seq: AtomicU64,
    /// Native turns supersede a suspended act even when their point timestamps alias.
    accepted_turns: AtomicU64,
    /// The person's exchange point already published to aterm's task face. Native
    /// acceptance distinguishes different turns even if their point timestamps alias.
    left_recorded: Mutex<Option<(u64, Instant)>>,
    /// One decision at a time per thread (a permit, never a guard held across I/O).
    eval: Arc<tokio::sync::Semaphore>,
    stopped: AtomicBool,
    resumed: AtomicBool,
    scanned: AtomicBool,
    /// THE RESUME SCAN READ THIS THREAD'S BOOKS before any turn here (gap 32, review finding
    /// 1): its history, and the pace the record handed on, said whether the session worked
    /// before this process ([`ThreadState::seed_worked`]). A resumed thread whose scan did not
    /// (a turn here beat it, or its history was unread) is past its opening at its first turn's
    /// end ([`Autopilot::finish`]): failing toward carrying, as the ruling does.
    books_read: AtomicBool,
    announced: Mutex<Option<String>>,
    /// HOW RULING F'S HOLD WAS LAST TOLD for this thread ([`Autopilot::tell_held`]): `None`
    /// once a decision is not held, so the next hold is told again.
    held_told: Mutex<Option<crate::replies::Told>>,
    evaluations: AtomicU64,
    /// The `update_plan` step in progress, as the last plan said (the description's).
    plan_step: Mutex<Option<String>>,
    /// Opaque authority for this exact registration, independent of its reused key.
    description_owner: Arc<()>,
    /// A newer refresh invalidates an older unread result without retiring the
    /// acknowledged description when the newer goal read itself fails.
    description_request: Mutex<Arc<()>>,
    /// The paced act a timer that fired is about to decide: out of the timer's slot (a turn
    /// it starts must not cancel the very task starting it), and here until an evaluation
    /// holds the thread's permit. The live upgrade, which holds every permit, carries it as
    /// it would the pending timer's (review of 2a9241bbf, findings 5 and 9).
    fired: Mutex<Option<TrackedOwed>>,
    /// A validated inherited automatic intent's conservative awake settling origin.
    inherited_settle: Mutex<Option<InheritedSettle>>,
    /// The supervisor's count of Offs this thread's memory has been parked for
    /// ([`Supervisor::offs`], [`ThreadState::park_off`]).
    off_seen: AtomicU64,
    /// A VANILLA SPELL COVERED THIS THREAD (edges-1, edges-2): a hook stood aside for one of its
    /// turns, or for its resume, while the vanilla experience stood ([`Autopilot::skipped`]).
    /// The next look ([`Autopilot::park_with`]: a decision's, a turn's start or its end) takes
    /// the memory's books as unwatched since ([`ThreadState::unwatched`]); a decision's reads
    /// the thread's history first and stands at its last turn end ([`rewatch_bound`]).
    unwatched: AtomicBool,
    /// The folder the worker's last command ran in (`on_command_start`): the first the
    /// repository probe reads, as aterm reads the transcript's last cwd (review of 2a9241bbf,
    /// finding 6). A remote executor's folder is no path of this host's: in no repository here,
    /// the probe reads nothing of it.
    workdir: Mutex<Option<std::path::PathBuf>>,
    /// WHAT THE REPOSITORY PROBE READS (review of the forever probe, land-probe-last-workdir and
    /// land-probe-head-only): every folder the worker touched (each command's, each folder a
    /// command `cd`ed into or pointed git at, each edit's), compacted to their repositories at
    /// each probe and past [`codex_alab_core::session::PROBE_SCOPE_COMPACT_AT`] folders between
    /// probes ([`Autopilot::compact_soon`]), the thread's first turn and the branches its
    /// commands created or checked out with the folder each was named in; carried in the record
    /// (`probeScope`).
    scope: Mutex<codex_alab_core::session::ProbeScope>,
    /// A compaction of [`Self::scope`] between probes runs ([`Autopilot::compact_soon`]).
    compacting: AtomicBool,
    /// How many folders [`Self::scope`] held after its last compaction: the next starts once it
    /// holds twice as many (and at least
    /// [`codex_alab_core::session::PROBE_SCOPE_COMPACT_AT`]), so a scope of that many
    /// repositories is not compacted again at every command.
    compacted_to: AtomicUsize,
    /// The point the last evaluation decided ([`ThreadState::point_at`]; `Some(None)`: the
    /// carry-on's), `None` before any, and again once a claim transition or a timer asked for
    /// another look: the live upgrade waits for a point no evaluation has decided yet, as for
    /// one in flight (review of the forever autopilot, 2026-10-20, finding 4).
    decided: Mutex<Option<Option<Instant>>>,
    /// THE ROUTE OF THE NETWORK WALL THE POINT STANDS AT ([`netwatch::route_at_wall`]), read as
    /// the turn met it; `None` at any other point, and from the start of the next turn. A
    /// direct route is measured while the point stands ([`netwatch::Waiter`]).
    net: Mutex<Option<netwatch::Route>>,
    /// THE LOST LOGIN THE POINT STANDS AT ([`LoginWatch`], [`crate::login`]): its baseline, read
    /// as the turn met the wall (or at the first decision of a wall a relaunch restored), and the
    /// watch that looks at its file while codex-alab acts and no turn of the thread runs; `None`
    /// from the next turn's end that meets no login wall.
    login: Mutex<Option<LoginWatch>>,
    transient_auth_goal: Mutex<Option<Arc<renewal_follow::TransientGoalRecovery>>>,
    /// The supervisor that says whether codex-alab waits on walls now
    /// ([`Supervisor::measures_walls`]): set once the thread is kept
    /// ([`Autopilot::keep_thread`], its start); unset, the thread stands at no wall's route.
    sup: std::sync::OnceLock<Weak<Supervisor>>,
    /// Run once after the next repository probe of this thread, before its finding is used
    /// (tests: an Off that comes while a probe runs).
    #[cfg(test)]
    after_probe: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    #[cfg(test)]
    before_resume_bookkeeping: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    #[cfg(test)]
    before_park: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    /// Pause after an act's preparatory reads, before native core or the goal service.
    #[cfg(test)]
    before_submit: Mutex<
        Option<(
            tokio::sync::oneshot::Sender<()>,
            tokio::sync::oneshot::Receiver<()>,
        )>,
    >,
    /// Pause after an act passed every check and was booked, just before native core starts its
    /// turn.
    #[cfg(test)]
    before_start: Mutex<
        Option<(
            tokio::sync::oneshot::Sender<()>,
            tokio::sync::oneshot::Receiver<()>,
        )>,
    >,
    #[cfg(test)]
    before_ready: Mutex<
        Option<(
            tokio::sync::oneshot::Sender<()>,
            tokio::sync::oneshot::Receiver<()>,
        )>,
    >,
    /// Model a goal read that returns after its runtime was retired.
    #[cfg(test)]
    after_description_read: Mutex<
        Option<(
            tokio::sync::oneshot::Sender<()>,
            tokio::sync::oneshot::Receiver<()>,
        )>,
    >,
    /// Model task preemption after the real goal read, before attention publication.
    #[cfg(test)]
    after_goal_read: Mutex<Option<GoalReadGate>>,
    /// Model the vanilla experience for this thread alone (`/alab off`), without the
    /// process-wide switch other tests read ([`ThreadRt::vanilla`]).
    #[cfg(test)]
    vanilla_seam: AtomicBool,
}

#[cfg(test)]
struct GoalReadGate {
    read: tokio::sync::oneshot::Sender<Option<codex_state::ThreadGoalStatus>>,
    resume: tokio::sync::oneshot::Receiver<()>,
    finished: tokio::sync::oneshot::Sender<()>,
}

/// Local observation of one actual failed request, separate from execution
/// authority. Its original baseline survives a temporarily empty auth cache.
struct AuthWallObservation {
    scope: Arc<codex_alab_core::renewal::AuthRequestScope>,
    cause: Arc<codex_alab_core::renewal::AuthObservation>,
    manager: Arc<codex_login::AuthManager>,
    baseline: login::Baseline,
}

/// A LOGIN WALL'S POINT AND ITS BASELINE ([`login::Baseline`]), the login file's stat as its
/// watches last read it, and the token of the watch that looks at the file
/// ([`Autopilot::watch_login`]).
struct LoginWatch {
    point: Instant,
    baseline: login::Baseline,
    /// The file's stat as the last look read it (the baseline's at first): a watch armed after
    /// a passive spell compares with it first, so a file that moved once is reloaded once, not
    /// once more at each watch armed ([`ThreadRt::saw_login`]).
    seen: Option<login::Stat>,
    /// A completed same-identity reload reconciles the initial file across passive rearming.
    initial_reconciled: bool,
    /// The token of the watch that runs; `None` while none runs (none armed yet, or the last one
    /// ended at a passive spell or a turn's start), so the next decision at the point arms one.
    watching: Option<u64>,
}

impl LoginWatch {
    /// The wall at `point` with `baseline`, no watch armed yet.
    fn at(point: Instant, baseline: login::Baseline) -> LoginWatch {
        LoginWatch {
            point,
            seen: baseline.stat(),
            initial_reconciled: false,
            baseline,
            watching: None,
        }
    }
}

/// The token of the next login watch armed ([`LoginWatch::watching`]).
static LOGIN_WATCHES: AtomicU64 = AtomicU64::new(0);

/// A pending look-again: its generation, its task, and the paced act it waits for (the old
/// image's decision, [`Owed`]), which the live upgrade carries across its re-exec instead of
/// waiting on it.
struct Timer {
    generation: u64,
    handle: JoinHandle<()>,
    owes: Option<TrackedOwed>,
}

/// The accepted sleep origin of one immutable intent, independent of later
/// decisions that may already have credited their own state deadlines.
#[derive(Clone)]
struct TrackedOwed {
    act: Owed,
    selected: Option<codex_alab_core::wake::Reading>,
}

/// A new image cannot date its first sleep-count sample. Keep that uncertainty
/// out of provider deadlines and settle only the inherited automatic intent.
struct InheritedSettle {
    since: Instant,
    asleep: Option<Duration>,
}

impl InheritedSettle {
    fn new(
        now: Instant,
        reading: Option<codex_alab_core::wake::Reading>,
        supported: bool,
    ) -> Option<Self> {
        (supported || reading.is_some()).then(|| Self {
            since: reading
                .and_then(|read| read.last)
                .map_or(now, |wake| wake.at),
            asleep: reading.map(|read| read.asleep),
        })
    }

    fn observe(
        &mut self,
        now: Instant,
        reading: Option<codex_alab_core::wake::Reading>,
    ) -> Instant {
        let Some(reading) = reading else {
            return self.since;
        };
        match self.asleep {
            None => {
                self.asleep = Some(reading.asleep);
                self.since = self.since.max(now);
            }
            Some(before)
                if reading
                    .asleep
                    .checked_sub(before)
                    .is_some_and(|slept| slept >= codex_alab_core::wake::MIN_SLEEP) =>
            {
                self.asleep = Some(reading.asleep);
                let known = reading.last.filter(|wake| wake.at > self.since);
                self.since = self.since.max(known.map_or(now, |wake| wake.at));
            }
            _ => {}
        }
        self.since
    }
}

impl TrackedOwed {
    fn new(act: Owed, selected: Option<codex_alab_core::wake::Reading>) -> Self {
        Self { act, selected }
    }

    fn from_state(act: Owed, asleep: Option<Duration>) -> Self {
        Self::new(
            act,
            asleep.map(|asleep| codex_alab_core::wake::Reading { asleep, last: None }),
        )
    }

    fn establish(&mut self, reading: Option<codex_alab_core::wake::Reading>) {
        if self.selected.is_none() {
            self.selected = reading;
        }
    }

    fn at_reading(&self, reading: Option<codex_alab_core::wake::Reading>, now: Instant) -> Owed {
        let mut act = self.act.clone();
        act.not_before =
            codex_alab_core::wake::Deadline::with_reading(act.not_before, self.selected)
                .at_reading(reading, now);
        act
    }
}

/// A tentative handoff owns its suspended paced act until the TUI commits leave.
/// Cancellation/rejection restores it only if no new turn superseded its point.
struct UpgradeHandoff {
    autopilot: Arc<Autopilot>,
    owed: Option<HandoffAct>,
}

/// One selected act and the native point and timer that surrendered it.
struct HandoffAct {
    rt: Arc<ThreadRt>,
    owed: codex_alab_core::session::OwedAct,
    point: Option<Instant>,
    accepted_turns: u64,
    timer_generation: u64,
}

/// A continuation just decided under its thread's evaluation permit. It has no
/// timer yet, but must cross an upgrade before starting the next native turn.
struct DecidedUpgradeAct {
    rt: Arc<ThreadRt>,
    owed: TrackedOwed,
    point: Option<Instant>,
    accepted_turns: u64,
}

/// The policy and native point that authorized one decision. Preparatory awaits
/// may outlive either, even when a newer native turn has already returned idle.
struct DecisionAuthority {
    quota: Option<codex_alab_core::quota::Wall>,
    quota_identity: Option<QuotaIdentity>,
    policy: decide::Policy,
    offs: u64,
    point: Option<Instant>,
    accepted_turns: u64,
    native_origin: Option<codex_alab_core::session::NativeOrigin>,
}

impl DecisionAuthority {
    fn same_point(&self, rt: &ThreadRt) -> bool {
        rt.accepted_turns.load(Ordering::SeqCst) == self.accepted_turns
            && lock(&rt.state).point_at() == self.point
    }

    fn point_current(&self, autopilot: &Autopilot, rt: &ThreadRt) -> bool {
        !rt.stopped.load(Ordering::SeqCst)
            && !autopilot.upgrade_requested.load(Ordering::SeqCst)
            && autopilot.sup.policy().as_ref() == Some(&self.policy)
            && autopilot.sup.offs() == self.offs
            && self.same_point(rt)
            && self
                .quota
                .as_ref()
                .is_none_or(|wall| autopilot.quota_restored(rt, wall, clock()))
            && (self.quota.is_none() || self.quota_identity == autopilot.quota_identity())
            && !autopilot.sup.shared().turn_running_on(&rt.key)
    }

    fn current(&self, autopilot: &Autopilot, rt: &ThreadRt) -> bool {
        autopilot.sup.acting() && self.point_current(autopilot, rt)
    }

    /// THE GO-AHEAD DECIDED AT THIS POINT IS NOT SUBMITTED HERE (review of the port, finding
    /// 1; aterm's §15.6: `answering` empties where the act was not submitted): a goal that
    /// stands owns the pace, Plan mode leaves the turn's end to a person, the goal's admission
    /// could not be guarded, or the point is no longer this decision's. The lines held off the
    /// menu bar for it at the turn's end or at the decision ([`ThreadState::go_ahead_foreseen`],
    /// [`ThreadState::go_ahead_judge`]) are back on it now, where the point still stands: a later
    /// point's own lines are its own to judge.
    fn unwritten(&self, autopilot: &Autopilot, rt: &ThreadRt) {
        let back = {
            let mut state = lock(&rt.state);
            state.point_at() == self.point && state.go_ahead_unwritten()
        };
        if back {
            autopilot.surface_needs(rt);
        }
    }

    /// The act did not start: the state before it stands again. Whether lines the go-ahead
    /// decided were held off the menu bar for it, which are back now
    /// ([`ThreadState::go_ahead_unwritten`]; the caller shows them).
    fn restore_if_unanswered(&self, rt: &ThreadRt, before: ThreadState) -> bool {
        let mut state = lock(&rt.state);
        if rt.accepted_turns.load(Ordering::SeqCst) == self.accepted_turns
            && state.point_at().is_none()
        {
            *state = before;
            return state.go_ahead_unwritten();
        }
        false
    }
}

impl DecidedUpgradeAct {
    fn for_thread(&self, rt: &ThreadRt) -> Option<Owed> {
        std::ptr::eq(self.rt.as_ref(), rt).then(|| self.owed.act.clone())
    }

    fn current(&self, shared: &codex_alab_core::session::Shared) -> bool {
        !self.rt.stopped.load(Ordering::SeqCst)
            && self.rt.accepted_turns.load(Ordering::SeqCst) == self.accepted_turns
            && lock(&self.rt.state).point_at() == self.point
            && !shared.turn_running_on(&self.rt.key)
    }
}

impl Drop for DecidedUpgradeAct {
    fn drop(&mut self) {
        // Cancellation before the handoff guard exists must not leave an empty,
        // "decided" point that a later upgrade could silently pass over. A carried
        // timer still owns its act; an ordinary fallback still holds this permit.
        self.rt.ask_again();
    }
}

/// Own consideration from the first await, including when the caller is cancelled
/// before there is an act to suspend. Only this guard releases the consideration slot.
struct UpgradeAttempt<'a>(&'a Autopilot);

impl Drop for UpgradeAttempt<'_> {
    fn drop(&mut self) {
        if !self.0.sup.shared().exec_pending() {
            self.0.upgrading.store(false, Ordering::SeqCst);
        }
    }
}

#[cfg(test)]
#[path = "upgrade_handoff_tests.rs"]
mod upgrade_handoff_tests;

#[cfg(test)]
#[path = "wake_settle_handoff_tests.rs"]
mod wake_settle_handoff_tests;

#[cfg(test)]
#[path = "inherited_settle_tests.rs"]
mod inherited_settle_tests;

#[cfg(test)]
#[path = "goal_upgrade_read_tests.rs"]
mod goal_upgrade_read_tests;

#[cfg(test)]
#[path = "goal_stop_admission_tests.rs"]
mod goal_stop_admission_tests;

#[cfg(test)]
#[path = "submit_authority_tests.rs"]
mod submit_authority_tests;

#[cfg(test)]
#[path = "quota_tests.rs"]
mod quota_tests;

#[cfg(test)]
#[path = "submit_tui_hand_tests.rs"]
mod submit_tui_hand_tests;

#[cfg(test)]
#[path = "land_act_native_tests.rs"]
mod land_act_native_tests;

#[cfg(test)]
#[path = "fact_rider_tests.rs"]
mod fact_rider_tests;

#[cfg(test)]
#[path = "goal_resume_tests.rs"]
mod goal_resume_tests;

#[cfg(test)]
#[path = "sub_agent_scope_tests.rs"]
mod sub_agent_scope_tests;

#[cfg(test)]
#[path = "human_goal_recovery_tests.rs"]
mod human_goal_recovery_tests;

#[cfg(test)]
#[path = "go_ahead_unwritten_tests.rs"]
mod go_ahead_unwritten_tests;

#[cfg(test)]
#[path = "cross_root_attention_tests.rs"]
mod cross_root_attention_tests;

#[cfg(test)]
#[path = "description_custody_tests.rs"]
mod description_custody_tests;

#[cfg(test)]
#[path = "description_goal_read_tests.rs"]
mod description_goal_read_tests;

#[cfg(test)]
#[path = "description_request_custody_tests.rs"]
mod description_request_custody_tests;

#[cfg(test)]
#[path = "carried_description_read_tests.rs"]
mod carried_description_read_tests;

#[cfg(test)]
#[path = "carry_on_custody_tests.rs"]
mod carry_on_custody_tests;

#[cfg(test)]
#[path = "item_callback_custody_tests.rs"]
mod item_callback_custody_tests;

#[cfg(test)]
#[path = "resume_race_tests.rs"]
mod resume_race_tests;

#[cfg(test)]
#[path = "opening_tests.rs"]
mod opening_tests;

#[cfg(test)]
#[path = "resume_needs_custody_tests.rs"]
mod resume_needs_custody_tests;

#[cfg(test)]
#[path = "off_needs_custody_tests.rs"]
mod off_needs_custody_tests;

#[cfg(test)]
#[path = "goal_attention_custody_tests.rs"]
mod goal_attention_custody_tests;

impl Drop for UpgradeHandoff {
    fn drop(&mut self) {
        let shared = self.autopilot.sup.shared();
        let kept = lock(&self.autopilot.threads);
        shared.cancel_upgrade(|| {
            #[cfg(test)]
            if let Some(hook) = lock(&self.autopilot.before_upgrade_restore).take() {
                hook();
            }
            self.autopilot
                .upgrade_requested
                .store(false, Ordering::SeqCst);
            shared.set_owed_act(None);
            let current = self.owed.as_ref().filter(|act| {
                Autopilot::current_runtime(&kept, &act.rt)
                    && !shared.turn_running_on(&act.rt.key)
                    && act.rt.accepted_turns.load(Ordering::SeqCst) == act.accepted_turns
                    && lock(&act.rt.state).point_at() == act.point
                    && act.rt.timer_seq.load(Ordering::SeqCst) == act.timer_generation
            });
            // A relaunchable signal hands the suspended act to the record, even if
            // teardown already stopped its thread. No timer runs while exiting.
            if shared.signal().is_some() {
                if shared.leaving_for_relaunch()
                    && let Some(act) = current
                {
                    shared.set_owed_act(Some(act.owed.clone()));
                }
                return;
            }
            if self
                .owed
                .as_ref()
                .is_some_and(|act| act.rt.stopped.load(Ordering::SeqCst))
            {
                return;
            }
            if let Some(act) = current {
                self.autopilot
                    .put_back_from(&act.rt, &act.owed, &kept, Some(act.timer_generation));
            }
            self.autopilot.sync_record_owed_from(&kept);
        });
    }
}

impl ThreadRt {
    /// A root thread's runtime: `config` is its start-time config (approvals read its layers
    /// and requirements; `None` defers every box).
    pub fn new(
        id: ThreadId,
        config: Option<Arc<Config>>,
        token_budget: bool,
        jitter: Duration,
    ) -> Self {
        ThreadRt {
            quota_wall: Mutex::new(None),
            human_goal_recovery: Mutex::new(None),
            wall_goal: Mutex::new(None),
            quota_request: Mutex::new(None),
            auth_wall_scope: Mutex::new(None),
            auth_observation: Mutex::new(None),
            renewal_ack_retry: Mutex::new(None),
            quota_rotation_declined: AtomicU64::new(u64::MAX),
            auth_wall_goal_origin: Mutex::new(None),
            id,
            key: id.to_string(),
            state: Mutex::new(ThreadState::default()),
            native_turn: tokio::sync::Mutex::new(None),
            cyber_access_program: Mutex::new(None),
            config,
            token_budget,
            jitter,
            timer: Mutex::new(None),
            timer_seq: AtomicU64::new(0),
            accepted_turns: AtomicU64::new(0),
            left_recorded: Mutex::new(None),
            eval: Arc::new(tokio::sync::Semaphore::new(1)),
            stopped: AtomicBool::new(false),
            resumed: AtomicBool::new(false),
            scanned: AtomicBool::new(false),
            books_read: AtomicBool::new(false),
            announced: Mutex::new(None),
            held_told: Mutex::new(None),
            evaluations: AtomicU64::new(0),
            plan_step: Mutex::new(None),
            description_owner: Arc::new(()),
            description_request: Mutex::new(Arc::new(())),
            fired: Mutex::new(None),
            inherited_settle: Mutex::new(None),
            off_seen: AtomicU64::new(0),
            unwatched: AtomicBool::new(false),
            workdir: Mutex::new(None),
            scope: Mutex::new(codex_alab_core::session::ProbeScope::default()),
            compacting: AtomicBool::new(false),
            compacted_to: AtomicUsize::new(0),
            decided: Mutex::new(None),
            net: Mutex::new(None),
            login: Mutex::new(None),
            transient_auth_goal: Mutex::new(None),
            sup: std::sync::OnceLock::new(),
            #[cfg(test)]
            after_probe: Mutex::new(None),
            #[cfg(test)]
            before_resume_bookkeeping: Mutex::new(None),
            #[cfg(test)]
            before_park: Mutex::new(None),
            #[cfg(test)]
            before_submit: Mutex::new(None),
            #[cfg(test)]
            before_start: Mutex::new(None),
            #[cfg(test)]
            before_ready: Mutex::new(None),
            #[cfg(test)]
            after_description_read: Mutex::new(None),
            #[cfg(test)]
            after_goal_read: Mutex::new(None),
            #[cfg(test)]
            vanilla_seam: AtomicBool::new(false),
        }
    }

    /// WHETHER CODEX-ALAB STANDS ASIDE FOR THIS THREAD NOW: the vanilla experience
    /// (`/alab off`, or a launch with `[tui] alab_interface = false`), under which every hook
    /// skips it ([`Autopilot::rt`]).
    fn vanilla(&self) -> bool {
        #[cfg(test)]
        if self.vanilla_seam.load(Ordering::SeqCst) {
            return true;
        }
        !codex_alab_core::active()
    }

    /// Put this thread alone in (or out of) the vanilla experience (tests).
    #[cfg(test)]
    pub(crate) fn set_vanilla(&self, vanilla: bool) {
        self.vanilla_seam.store(vanilla, Ordering::SeqCst);
    }

    /// The thread's waits are measured as `sup` acts ([`netwatch::Waiter::waits_for`]).
    fn measured_by(&self, sup: &Arc<Supervisor>) {
        let _ = self.sup.set(Arc::downgrade(sup));
    }

    /// Set the route of the network wall the point stands at, as the turn's end reads it
    /// (tests).
    #[cfg(test)]
    pub(crate) fn set_route(&self, route: Option<netwatch::Route>) {
        *lock(&self.net) = route;
    }

    /// Read the route retained by the real turn lifecycle (tests).
    #[cfg(test)]
    pub(crate) fn route_for_test(&self) -> Option<netwatch::Route> {
        lock(&self.net).clone()
    }

    /// THE POINT NO EVALUATION HAS DECIDED YET (review of the forever autopilot, 2026-10-20,
    /// finding 4): a turn end, a carry-on owed, or a look a claim transition or a timer asked
    /// for, whose evaluation is queued for the thread's permit; or a resume whose history is
    /// not read yet. The live upgrade, which takes every free permit, would otherwise exec
    /// over it, and the new image never decides it (a re-exec resumes with no carry-on). A
    /// paced act owed is carried instead, a landing is carried, and a stopped thread decides
    /// nothing. A LANDING WHOSE JUDGEMENT IS OWED ([`ThreadState::judging`]: an Off and back,
    /// the task's own wake) is undecided until the probe settled it, whatever was decided at
    /// its point before: carried on the record's word, the next image would hold it over
    /// work the judgement finds (a false landing).
    pub(crate) fn undecided(&self) -> bool {
        if self.stopped.load(Ordering::SeqCst) || self.owed_timer().is_some() {
            return false;
        }
        if self.resumed.load(Ordering::SeqCst) && !self.scanned.load(Ordering::SeqCst) {
            return true;
        }
        let (stands, judging, at) = {
            let state = lock(&self.state);
            let stands = !state.landed && (state.point.is_some() || (state.carry_on && state.task));
            (stands, state.judging(), state.point_at())
        };
        judging || (stands && *lock(&self.decided) != Some(at))
    }

    /// Another look is asked for (a claim transition, a timer that fired): the point stands
    /// undecided until an evaluation decides it.
    fn ask_again(&self) {
        *lock(&self.decided) = None;
    }

    /// Whether the login watch `token` is still this thread's ([`Autopilot::watch_login`]): the
    /// thread runs, its point is still the login wall at `point`, and the watch is the one its
    /// slot names.
    fn watches_login(&self, point: Instant, token: u64) -> bool {
        if self.stopped.load(Ordering::SeqCst) {
            return false;
        }
        let stands = lock(&self.state)
            .point
            .as_ref()
            .is_some_and(|end| end.at == point && matches!(end.wall, Some(Wall::Login)));
        stands
            && lock(&self.login)
                .as_ref()
                .is_some_and(|watch| watch.point == point && watch.watching == Some(token))
    }

    /// The login watch `token` ended at a passive spell or a turn's start: the point's next
    /// decision arms another.
    fn unwatch_login(&self, point: Instant, token: u64) {
        if let Some(watch) = lock(&self.login)
            .as_mut()
            .filter(|watch| watch.point == point && watch.watching == Some(token))
        {
            watch.watching = None;
        }
    }

    /// A TURN BEGINS: the lost login's watch ends, whichever turn it is (a person's, a goal's,
    /// codex-alab's own line), and reloads nothing more inside it; the turn's end reads the
    /// login again. Whether a watch was running.
    fn end_login_watch(&self) -> bool {
        lock(&self.login)
            .as_mut()
            .and_then(|watch| watch.watching.take())
            .is_some()
    }

    /// The login watch `token` read the file's stat `stat` at the wall at `point`: the next
    /// watch armed there compares with it first ([`LoginWatch::seen`]).
    #[cfg(test)]
    pub(crate) fn set_login_scope_for_test(
        &self,
        scope: Arc<codex_alab_core::renewal::AuthRequestScope>,
    ) {
        *lock(&self.auth_wall_scope) = Some(scope);
    }

    #[cfg(test)]
    pub(crate) fn login_watch_for_test(&self) -> Option<(Option<login::Stat>, bool)> {
        lock(&self.login)
            .as_ref()
            .map(|watch| (watch.seen, watch.initial_reconciled))
    }

    fn saw_login(&self, point: Instant, token: u64, stat: Option<login::Stat>, taken: bool) {
        if let Some(watch) = lock(&self.login)
            .as_mut()
            .filter(|watch| watch.point == point && watch.watching == Some(token))
        {
            watch.seen = stat;
            watch.initial_reconciled |= taken;
        }
    }

    /// What `manager` says of the login wall at `point` now ([`login::Baseline::read`]); `None`
    /// once the thread stands at it no more.
    fn read_login(
        &self,
        manager: &codex_login::AuthManager,
        point: Instant,
    ) -> Option<login::Read> {
        lock(&self.login)
            .as_ref()
            .filter(|watch| watch.point == point)
            .map(|watch| watch.baseline.read(manager))
    }

    /// How many times a decision was looked at (tests count timer firings with it).
    pub fn evaluations(&self) -> u64 {
        self.evaluations.load(Ordering::SeqCst)
    }

    /// A copy of the decider's memory (tests read it).
    pub fn state(&self) -> ThreadState {
        lock(&self.state).clone()
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    /// Change the decider's memory in place (tests set the point a resume or an act left).
    #[cfg(test)]
    pub(crate) fn with_state(&self, change: impl FnOnce(&mut ThreadState)) {
        change(&mut lock(&self.state));
    }

    /// Set the plan step in progress, as an `update_plan` call does (tests).
    #[cfg(test)]
    pub(crate) fn set_plan_step(&self, step: Option<&str>) {
        *lock(&self.plan_step) = step.map(str::to_string);
    }

    /// The plan step in progress the description says (tests).
    #[cfg(test)]
    pub(crate) fn plan_step(&self) -> Option<String> {
        lock(&self.plan_step).clone()
    }

    /// Custody-only acknowledgment attempts while a retry remains pending.
    pub fn renewal_ack_attempts(&self) -> Option<u64> {
        lock(&self.renewal_ack_retry)
            .as_ref()
            .map(|retry| retry.attempts.load(Ordering::SeqCst))
    }

    /// Whether a timer is pending.
    pub fn timer_pending(&self) -> bool {
        lock(&self.timer).is_some()
    }

    /// The paced act the pending timer waits for, or the one a timer that fired is about to
    /// decide, if any.
    pub fn owed_timer(&self) -> Option<Owed> {
        self.tracked_owed_timer().map(|owed| owed.act)
    }

    fn tracked_owed_timer(&self) -> Option<TrackedOwed> {
        self.tracked_owed_timer_at(None)
    }

    fn tracked_owed_timer_at(
        &self,
        reading: Option<codex_alab_core::wake::Reading>,
    ) -> Option<TrackedOwed> {
        let mut slot = lock(&self.timer);
        let mut fired = lock(&self.fired);
        let owed = slot
            .as_mut()
            .and_then(|timer| timer.owes.as_mut())
            .or(fired.as_mut())?;
        owed.establish(reading);
        Some(owed.clone())
    }

    /// Hold this thread's evaluation permit, as an evaluation in flight does (tests).
    #[cfg(test)]
    pub(crate) fn hold_eval(&self) -> tokio::sync::SemaphorePermit<'_> {
        self.eval
            .try_acquire()
            .unwrap_or_else(|_| panic!("the evaluation permit is free"))
    }

    /// Run `hook` once after this thread's next repository probe (tests).
    #[cfg(test)]
    pub(crate) fn after_probe(&self, hook: impl FnOnce() + Send + 'static) {
        *lock(&self.after_probe) = Some(Box::new(hook));
    }

    #[cfg(test)]
    pub(crate) fn sync_off_seen(&self, offs: u64) {
        self.off_seen.store(offs, Ordering::SeqCst);
    }

    #[cfg(test)]
    pub(crate) fn note_accepted_turn(&self) {
        self.accepted_turns.fetch_add(1, Ordering::SeqCst);
    }

    /// Hold this thread's evaluation permit once it is free (tests).
    #[cfg(test)]
    pub(crate) async fn hold_eval_when_free(&self) -> tokio::sync::SemaphorePermit<'_> {
        self.eval
            .acquire()
            .await
            .unwrap_or_else(|_| panic!("the evaluation permit is never closed"))
    }

    /// The folder the worker's last command ran in, if one ran (tests).
    pub fn workdir(&self) -> Option<std::path::PathBuf> {
        lock(&self.workdir).clone()
    }

    /// What the repository probe reads for this thread ([`Self::scope`]).
    pub fn probe_scope(&self) -> codex_alab_core::session::ProbeScope {
        lock(&self.scope).clone()
    }
}

/// A thread waits for its route's measure while its point stands at a network wall whose
/// route is direct, and it has not stopped.
/// A THREAD STANDS AT ITS NETWORK WALL'S ROUTE only while codex-alab waits on that wall (parity-3
/// review, F6, F7): it acts with `retry_api_errors` and `probe_api` on
/// ([`Supervisor::measures_walls`]: no Off, standby or unproven claim), and the thread waits on
/// its point (a look again pending, one that fired, or its evaluation in hand). aterm renews a
/// route's lease only while its loop acts and asks, so its probes stop a lease after it goes
/// passive; the measure's task here renews the lease of every thread standing, so a thread that
/// stood on through a passive spell kept the probes going for as long as it lasted.
impl netwatch::Waiter for ThreadRt {
    fn waits_for(&self) -> Option<netwatch::Dest> {
        if self.stopped.load(Ordering::SeqCst) {
            return None;
        }
        let measures = self
            .sup
            .get()
            .and_then(Weak::upgrade)
            .is_some_and(|sup| sup.measures_walls());
        let waits = self.timer_pending()
            || lock(&self.fired).is_some()
            || self.eval.available_permits() == 0;
        if !measures || !waits {
            return None;
        }
        let Some(netwatch::Route::Direct(dest)) = lock(&self.net).clone() else {
            return None;
        };
        let state = lock(&self.state);
        let stands = match state.point.as_ref().and_then(|point| point.wall.as_ref()) {
            Some(Wall::Network { .. }) => true,
            // A usage wall stands at its route once its due resume asked for the reach
            // ([`ThreadState::net_asked`]): never through the hours before its reset.
            Some(Wall::Usage { .. } | Wall::Entitlement) => state.net_asked.is_some(),
            _ => false,
        };
        stands.then_some(dest)
    }

    fn decided(&self) -> Reach {
        lock(&self.state).reach
    }
}

#[derive(Default)]
struct TrackData {
    limited_goal_at_start: Option<codex_state::ThreadGoal>,
    /// The goal active when the turn began ([`wall_goal_after`]).
    active_goal_at_start: Option<codex_state::ThreadGoal>,
    /// A person's stops of the thread's goal as the turn began
    /// ([`codex_alab_core::session::Shared::person_goal_stops`], read before the goal): one since
    /// makes the goal's stop no wall's ([`wall_goal_after`]).
    person_stops_at_start: u64,
    /// THE TURN CALLED A GOAL TOOL (`update_goal`, `create_goal`): the model may have stopped
    /// or replaced the goal itself, so a stop read after it is no wall's ([`wall_goal_after`]).
    goal_tool: bool,
    started: Option<Instant>,
    native_epoch: Option<u64>,
    by_autopilot: bool,
    by_person: bool,
    saw_regular_input: bool,
    text: Option<String>,
    final_text: Option<String>,
    /// Every agent message of the turn, in order, and the index of the one marked its final
    /// answer (the last such): the turn's earlier words ([`turn_said`]).
    said: Vec<String>,
    final_at: Option<usize>,
    questions: bool,
    /// THE TURN RAN A TOOL ([`TurnEnd::ran_tools`], `on_tool_start`): ruling F's real work.
    ran_tools: bool,
    wall: Option<Wall>,
    /// The route of the network wall the turn met ([`netwatch::route_at_wall`]).
    route: Option<netwatch::Route>,
    /// The login as codex held it when the turn met a login wall ([`login::Baseline`]).
    login: Option<login::Baseline>,
    auth_scope: Option<Arc<codex_alab_core::renewal::AuthRequestScope>>,
    auth_observation: Option<Arc<AuthWallObservation>>,
    /// The sleep total when this wall's absolute deadline was materialized, before
    /// its route read or a delayed finish can yield across another host sleep.
    wall_asleep: Option<Duration>,
    /// The mail this turn was submitted for (the TUI's native inbox): answered at its end.
    mail: Option<codex_alab_core::mail::MailTurn>,
    /// THE PERSON'S HAND WAS NOT READ as their message arrived (no connection yet, or the read
    /// past its bound): read again at the turn's end ([`Autopilot::finish`]; edges-6).
    hand_unread: bool,
}

/// THE TURN'S EARLIER WORDS (aterm's `turn_said`, [`TurnEnd::earlier`]): every agent message
/// of the turn but the one its end reads as the reply (`last`, the final answer's index;
/// `None`, the last message), blank-line joined, EACH READ ON ITS OWN: a code fence a message
/// leaves open is closed at its end ([`crate::needs::fence_closed`]), as the TUI renders each
/// message as its own Markdown item; `None` where the turn said no more.
pub(crate) fn turn_said(said: &[String], last: Option<usize>) -> Option<String> {
    let last = last.or_else(|| said.len().checked_sub(1))?;
    let earlier: Vec<std::borrow::Cow<'_, str>> = said
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != last)
        .map(|(_, t)| crate::needs::fence_closed(t))
        .collect();
    (!earlier.is_empty()).then(|| earlier.join("\n\n"))
}

/// One turn's facts, kept in its turn store.
#[derive(Default)]
struct TurnTrack(Mutex<TrackData>);

/// The books a decision's look read for a thread a vanilla spell covered
/// ([`Autopilot::unwatched_books`]): its history (or why it was unread), and the native turns
/// accepted as the read began.
struct Books {
    accepted: u64,
    items: Result<Vec<codex_history::RolloutItem>, String>,
}

/// Whether the turn `turn` called a goal tool so far ([`TrackData::goal_tool`], read before its
/// end takes the turn's facts; tests).
#[cfg(test)]
pub(crate) fn turn_called_goal_tool(turn: &ExtensionData) -> bool {
    turn.get::<TurnTrack>()
        .is_some_and(|track| lock(&track.0).goal_tool)
}

/// The autopilot extension (one per registry; the claim is the process's).
pub struct Autopilot {
    #[cfg(test)]
    quota_test_identity: Mutex<Option<QuotaIdentity>>,
    me: Weak<Autopilot>,
    sup: Arc<Supervisor>,
    thread_manager: Mutex<Option<Weak<ThreadManager>>>,
    goals: Option<Arc<GoalService>>,
    timing: Timing,
    gate: SourceGate,
    threads: Mutex<Vec<Weak<ThreadRt>>>,
    /// THE SUB-AGENTS THIS PROCESS RUNS, each by its thread's key, with the key of the root
    /// thread whose session spawned it ([`SubAgentOf`]): a sub-agent a sub-agent spawned names
    /// only its parent, so its root is read here. One entry per live sub-agent: its stop takes
    /// it out.
    sub_agents: Mutex<HashMap<String, String>>,
    seq: AtomicU64,
    watching: AtomicBool,
    watching_launch: AtomicBool,
    /// An upgrade is being considered, or was requested (then for good).
    upgrading: AtomicBool,
    #[cfg(test)]
    upgrade_begin: Mutex<
        Option<(
            tokio::sync::oneshot::Sender<()>,
            tokio::sync::oneshot::Receiver<()>,
        )>,
    >,
    /// Pause after upgrade checks, before selecting the act whose timer is taken.
    #[cfg(test)]
    before_upgrade_take: Mutex<
        Option<(
            tokio::sync::oneshot::Sender<()>,
            tokio::sync::oneshot::Receiver<()>,
        )>,
    >,
    /// A re-look at the upgrade is scheduled (a person's hand was on it).
    upgrade_relook: AtomicBool,
    /// The TUI was asked to leave for the upgrade's exec: nothing more is typed here.
    upgrade_requested: AtomicBool,
    /// THE LIVE UPGRADE'S ROUND OF NOTICES under aterm's `upgrade_ask` ([`upgrade_ask`];
    /// amended 2026-10-28 after the parity sweep, gap 27): the process's, as the upgrade is.
    /// Locked last: a turn's end hears under the thread list, and nothing else is locked while
    /// it is held (the thread list, a thread's state and the host's sleep are read before it).
    ask: Mutex<upgrade_ask::Round>,
    /// The act the previous image owed at its re-exec was handed to its thread (once).
    owed_spent: AtomicBool,
    /// A person was told aterm is too old to show their own exchange as theirs (once).
    newer_aterm_told: AtomicBool,
    /// THE API'S REACH, measured per destination while a thread waits at a network wall
    /// ([`netwatch`]); started on the first ask, on codex's own trust and aterm's schedule.
    net: std::sync::OnceLock<Arc<netwatch::NetWatch>>,
    /// The route's proxy decision is the test's own ([`Self::set_net_probe`]).
    net_direct_proxy: AtomicBool,
    /// How many looks every lost login's watch took ([`Self::watch_login`]; tests wait on it).
    login_looks: AtomicU64,
    /// How many changes in a login file's folder every lost login's watch took
    /// ([`Self::watch_login`]; tests wait on it).
    login_wakes: AtomicU64,
    /// The login a test's codex holds, with no thread manager behind it
    /// ([`Self::set_auth_manager`]).
    #[cfg(test)]
    auth_seam: Mutex<Option<Arc<codex_login::AuthManager>>>,
    /// THE HOST'S SLEEP as this process reads it ([`codex_alab_core::wake::read`]; a test's own,
    /// [`Self::set_wake_read`]): each decision reads it ([`ThreadState::slept_to`],
    /// [`ThreadState::woke`]), and each pending timer counts the time slept toward its deadline.
    wake: Mutex<WakeRead>,
    /// The upgrade's facts as a test sets them ([`Self::set_upgrade_facts`]): a test reaches
    /// the before-every-act checks with no thread manager or captured command line.
    #[cfg(test)]
    facts_seam: Mutex<Option<upgrade::UpgradeFacts>>,
    /// A resumed thread's history as a test sets it ([`Self::set_history`]): a test reaches
    /// the resume scan's restore with no thread manager.
    #[cfg(test)]
    history_seam: Mutex<Option<Vec<codex_history::RolloutItem>>>,
    #[cfg(test)]
    before_upgrade_restore: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    #[cfg(test)]
    before_rearm_route: Mutex<
        Option<(
            tokio::sync::oneshot::Sender<()>,
            tokio::sync::oneshot::Receiver<()>,
        )>,
    >,
    #[cfg(test)]
    before_wall_route: Mutex<
        Option<(
            tokio::sync::oneshot::Sender<()>,
            tokio::sync::oneshot::Receiver<()>,
        )>,
    >,
    #[cfg(test)]
    recovery_write_budget: Mutex<Duration>,
    /// A goal store that fails, as a test sets it ([`Self::refuse_goal_set`]): the resume's
    /// `GoalService::set_thread_goal` returns this error, the goal unchanged.
    #[cfg(any(test, debug_assertions))]
    goal_set_refusal: Mutex<Option<String>>,
}

impl Autopilot {
    pub fn new(
        sup: Arc<Supervisor>,
        thread_manager: Option<Weak<ThreadManager>>,
        goals: Option<Arc<GoalService>>,
        timing: Timing,
        gate: SourceGate,
    ) -> Arc<Autopilot> {
        Arc::new_cyclic(|me| Autopilot {
            me: me.clone(),
            sup,
            thread_manager: Mutex::new(thread_manager),
            goals,
            timing,
            gate,
            #[cfg(test)]
            quota_test_identity: Mutex::new(None),
            threads: Mutex::new(Vec::new()),
            sub_agents: Mutex::new(HashMap::new()),
            seq: AtomicU64::new(0),
            watching: AtomicBool::new(false),
            watching_launch: AtomicBool::new(false),
            upgrading: AtomicBool::new(false),
            #[cfg(test)]
            upgrade_begin: Mutex::new(None),
            #[cfg(test)]
            before_upgrade_take: Mutex::new(None),
            upgrade_relook: AtomicBool::new(false),
            upgrade_requested: AtomicBool::new(false),
            ask: Mutex::new(upgrade_ask::Round::default()),
            owed_spent: AtomicBool::new(false),
            newer_aterm_told: AtomicBool::new(false),
            net: std::sync::OnceLock::new(),
            net_direct_proxy: AtomicBool::new(false),
            login_looks: AtomicU64::new(0),
            login_wakes: AtomicU64::new(0),
            #[cfg(test)]
            auth_seam: Mutex::new(None),
            wake: Mutex::new(Arc::new(codex_alab_core::wake::read)),
            #[cfg(test)]
            facts_seam: Mutex::new(None),
            #[cfg(test)]
            history_seam: Mutex::new(None),
            #[cfg(test)]
            before_upgrade_restore: Mutex::new(None),
            #[cfg(test)]
            before_rearm_route: Mutex::new(None),
            #[cfg(test)]
            before_wall_route: Mutex::new(None),
            #[cfg(test)]
            recovery_write_budget: Mutex::new(WRITE_THROUGH_BUDGET),
            #[cfg(any(test, debug_assertions))]
            goal_set_refusal: Mutex::new(None),
        })
    }

    /// Have every goal resume's `GoalService::set_thread_goal` fail with `why` (`None`: as
    /// the goal store answers). The narrowest seam there is to a goal store that fails: the
    /// instruction record upstream runs under the goal runtime's mutation permit, after its
    /// checks and before any goal state moves, whose error `set_thread_goal` returns with the
    /// goal unchanged ([`Self::goal_set_fault`], once the resume booked its try, as a goal
    /// write that fails after the record leaves it). Tests only (the end-to-end tests' seam,
    /// so debug builds only).
    #[cfg(any(test, debug_assertions))]
    pub fn refuse_goal_set(&self, why: Option<String>) {
        *lock(&self.goal_set_refusal) = why;
    }

    /// Have the host's sleep read through `read` (tests: a scripted wake on tokio's clock).
    #[cfg(test)]
    pub(crate) fn set_wake_read(&self, read: WakeRead) {
        *lock(&self.wake) = read;
    }

    /// The host's sleep, read now.
    fn wake_reading(&self) -> Option<codex_alab_core::wake::Reading> {
        let read = lock(&self.wake).clone();
        read()
    }

    /// Hold the actual wall route read after classification (tests only).
    #[cfg(test)]
    pub(crate) fn hold_wall_route(
        &self,
        entered: tokio::sync::oneshot::Sender<()>,
        release: tokio::sync::oneshot::Receiver<()>,
    ) {
        *lock(&self.before_wall_route) = Some((entered, release));
    }

    /// The end of the instruction record a goal resume hands `GoalService::set_thread_goal`,
    /// past its recheck and the booked try: `Ok` (the goal write goes on), unless a test has
    /// the goal store fail ([`Self::refuse_goal_set`]).
    fn goal_set_fault(&self) -> Result<(), GoalServiceError> {
        #[cfg(any(test, debug_assertions))]
        if let Some(why) = lock(&self.goal_set_refusal).clone() {
            return Err(GoalServiceError::Internal(why));
        }
        Ok(())
    }

    /// Have the API's reach measured through `trust` on `timing`, every route's proxy decision
    /// read as direct (tests: a local TLS listener whose certificate only `trust` holds, and a
    /// schedule a test can wait for; the end-to-end tests' seam, so debug builds only). `false`
    /// where a measure was already set up.
    #[cfg(any(test, debug_assertions))]
    pub fn set_net_probe(&self, trust: netwatch::Trust, timing: netwatch::NetTiming) -> bool {
        self.net_direct_proxy.store(true, Ordering::SeqCst);
        let watch = netwatch::NetWatch::new(
            Arc::new(netwatch::SystemResolve),
            trust,
            timing,
            self.on_reach_edge(),
            self.may_probe(),
        );
        self.net.set(watch).is_ok()
    }

    /// The API's reach measure (started with codex's own trust on aterm's schedule).
    pub fn net(&self) -> &Arc<netwatch::NetWatch> {
        self.net.get_or_init(|| {
            netwatch::NetWatch::new(
                Arc::new(netwatch::SystemResolve),
                netwatch::codex_trust(),
                netwatch::NetTiming::default(),
                self.on_reach_edge(),
                self.may_probe(),
            )
        })
    }

    /// A DUE PROBE RUNS ONLY WHILE aterm'S `probe_api` IS NOT OFF ([`Supervisor::probe_off`],
    /// gap 30): a lease taken before the switch went off runs no probe of its own.
    fn may_probe(&self) -> netwatch::MayProbe {
        let sup = Arc::downgrade(&self.sup);
        Arc::new(move || sup.upgrade().is_some_and(|sup| !sup.probe_off()))
    }

    /// A MEASURE THE THREAD DID NOT DECIDE WITH: it looks again (aterm's loop reads the reach
    /// at every wait step; a codex-alab thread waits on a timer, so the measure's edge wakes it).
    fn on_reach_edge(&self) -> netwatch::OnEdge {
        let me = self.me.clone();
        Arc::new(move |key: &str| {
            let Some(me) = me.upgrade() else {
                return;
            };
            let Some(rt) = me.thread_rt(key) else {
                return;
            };
            me.sup.note(format!(
                "{key}: the API's measured reach moved; looking again"
            ));
            rt.ask_again();
            tokio::spawn(me.evaluate(rt));
        })
    }

    /// The reach the thread's point reads: its route's measure while it stands at a network
    /// wall whose route is direct (the ask renews its lease), else unknown.
    pub(crate) fn reach_for(&self, rt: &Arc<ThreadRt>) -> Reach {
        rt.measured_by(&self.sup);
        let Some(dest) = netwatch::Waiter::waits_for(&**rt) else {
            return Reach::Unknown;
        };
        let weak: Weak<ThreadRt> = Arc::downgrade(rt);
        let waiter: Weak<dyn netwatch::Waiter> = weak;
        self.net().ask(&rt.key, waiter, &dest)
    }

    /// The thread manager's `AuthManager`, codex's one store of this process's login, if it
    /// can be reached.
    fn auth_manager(&self) -> Option<Arc<codex_login::AuthManager>> {
        #[cfg(test)]
        if let Some(manager) = lock(&self.auth_seam).clone() {
            return Some(manager);
        }
        let manager = lock(&self.thread_manager).clone()?.upgrade()?;
        Some(manager.auth_manager())
    }

    /// Have this autopilot read `manager` as codex's login, with no thread manager (tests).
    #[cfg(test)]
    pub(crate) fn set_auth_manager(&self, manager: Arc<codex_login::AuthManager>) {
        *lock(&self.auth_seam) = Some(manager);
    }

    /// The sign-in the thread manager's auth holds now, if it can be read.
    fn auth_mode(&self) -> Option<codex_protocol::auth::AuthMode> {
        self.auth_manager()?.auth_mode()
    }

    /// How many looks the lost logins' watches took so far ([`Self::watch_login`]): a test
    /// waits for looks to have passed before it says nothing came of them (the end-to-end
    /// tests' seam, so debug builds only).
    #[cfg(any(test, debug_assertions))]
    pub fn login_looks(&self) -> u64 {
        self.login_looks.load(Ordering::SeqCst)
    }

    /// How many changes in a login file's folder the lost logins' watches took so far
    /// ([`Self::watch_login`]): a test waits for one to have come before it says nothing came
    /// of it (the end-to-end tests' seam, so debug builds only).
    #[cfg(any(test, debug_assertions))]
    pub fn login_wakes(&self) -> u64 {
        self.login_wakes.load(Ordering::SeqCst)
    }

    /// THE LOST LOGIN AS THIS DECISION READS IT ([`LoginRead`], gap 13 stage 1): where the point
    /// stands at a login wall, its baseline (read now for a wall a relaunch restored, which the
    /// login as it stands now is the baseline of: its process loaded the login fresh), the watch
    /// on its file armed while codex-alab acts and no turn of the thread runs
    /// ([`Self::watch_login`]), and whether the login came back ([`login::Baseline::read`],
    /// which reads codex's cache, never a store), with the point it was read at. The default at
    /// any other point.
    fn login_for(self: &Arc<Self>, rt: &Arc<ThreadRt>) -> (Option<Instant>, LoginRead) {
        let observing = self.observe_auth_wall(rt);
        if !self.eligible_login_scope(rt) && !observing {
            return (None, LoginRead::default());
        }
        let Some(manager) = self.auth_manager() else {
            return (None, LoginRead::default());
        };
        let acting = self.sup.acting();
        // A turn of the thread runs: its end reads the login again, and no watch runs inside it.
        let running = self.sup.shared().turn_running_on(&rt.key);
        let (read, restored, arm) = {
            let state = lock(&rt.state);
            let Some(point) = state
                .point
                .as_ref()
                .filter(|point| matches!(point.wall, Some(Wall::Login)))
                .map(|point| point.at)
            else {
                return (None, LoginRead::default());
            };
            let mut slot = lock(&rt.login);
            let restored = slot.as_ref().is_none_or(|watch| watch.point != point);
            if restored {
                *slot = Some(LoginWatch::at(point, login::Baseline::of(&manager)));
            }
            let Some(watch) = slot.as_mut() else {
                return (None, LoginRead::default());
            };
            let arm = (watch.watching.is_none() && acting && !running).then(|| {
                let token = LOGIN_WATCHES.fetch_add(1, Ordering::SeqCst);
                watch.watching = Some(token);
                (point, token, watch.baseline.clone(), watch.seen)
            });
            ((point, watch.baseline.read(&manager)), restored, arm)
        };
        if restored {
            self.sup.note(format!(
                "{}: a login wall with no baseline (a relaunch restored it): the login as it \
                 stands now is its baseline",
                rt.key
            ));
        }
        if let Some((point, token, baseline, seen)) = arm {
            self.watch_login(rt, manager, point, token, baseline, seen);
        }
        let (point, read) = read;
        (
            Some(point),
            LoginRead {
                back: read == login::Read::Back && self.eligible_login_scope(rt),
            },
        )
    }

    /// THE LOST LOGIN'S WATCH (gap 13 stage 1): while the thread's point stands at the login
    /// wall at `point`, codex-alab acts and no turn of the thread runs, it wakes on codex's own
    /// change of the login in this process (`auth_change_state_receiver`), on a change in the
    /// login file's folder ([`login::FolderWatch`], registered before this returns) and every
    /// [`Timing::login_look`] (aterm's `LOGIN_SHARED_LOOK`). A look only stats the file
    /// ([`login::stat`]; `last` is the stat it compares with first, [`LoginWatch::seen`]).
    /// A folder's change only asks for that stat: codex reloads the login only where the stat
    /// moved, never on a bare timer or on an event alone (a change the OS reports late, or one to
    /// the file's attributes, moves no stat and reloads nothing; a keyring or auto store's reload
    /// can put a keychain dialog on the person's screen), and takes only the walled identity's
    /// ([`login::Baseline::reload`]). Once the
    /// login reads back the thread looks again as a reach measure's edge has it
    /// (`on_reach_edge`), and the watch says so once that look is over. It ends with the point,
    /// the thread, this autopilot, at a passive spell (an Off, a standby, an unproven claim) and
    /// at a turn's start; the next decision at the point arms the next.
    fn watch_login(
        self: &Arc<Self>,
        rt: &Arc<ThreadRt>,
        manager: Arc<codex_login::AuthManager>,
        point: Instant,
        token: u64,
        baseline: login::Baseline,
        mut last: Option<login::Stat>,
    ) {
        let file = baseline.file().to_path_buf();
        let mut folder = login::FolderWatch::of(&file);
        self.sup.note(format!(
            "{}: the lost login's file is watched while the wall stands",
            rt.key
        ));
        let me = Arc::downgrade(self);
        let weak = Arc::downgrade(rt);
        let look = self.timing.login_look;
        let initial_pending = lock(&rt.login).as_ref().is_some_and(|watch| {
            watch.point == point && watch.watching == Some(token) && !watch.initial_reconciled
        });
        tokio::spawn(async move {
            enum Wake {
                Initial,
                Codex,
                Folder(Option<bool>),
                Look,
            }
            let mut changes = manager.auth_change_state_receiver();
            changes.borrow_and_update();
            let mut next_look = tokio::time::Instant::now() + look;
            let mut was_back = false;
            let mut was_known_profile = false;
            // Reconcile a file already present when the watch starts. Re-reading
            // identical attempted credentials cannot advance the manager's
            // credential generation. Only a selected file store qualifies;
            // leftover files never provoke an Auto/Keyring lookup.
            let mut initial_file =
                initial_pending && manager.alab_uses_file_auth() && login::stat(&file).is_some();
            let mut reconcile_initial_file = initial_file;
            loop {
                let wake = if initial_file {
                    initial_file = false;
                    Wake::Initial
                } else {
                    tokio::select! {
                        changed = changes.changed() => match changed {
                            Ok(()) => Wake::Codex,
                            Err(_) => return,
                        },
                        event = async {
                            match folder.as_mut() {
                                Some(folder) => folder.next().await,
                                None => std::future::pending().await,
                            }
                        } => Wake::Folder(event),
                        () = tokio::time::sleep_until(next_look) => Wake::Look,
                    }
                };
                let (Some(me), Some(rt)) = (me.upgrade(), weak.upgrade()) else {
                    return;
                };
                if !rt.watches_login(point, token) {
                    return;
                }
                if me.sup.shared().turn_running_on(&rt.key) {
                    rt.unwatch_login(point, token);
                    me.sup.note(format!(
                        "{}: a turn runs; the lost login's watch ends",
                        rt.key
                    ));
                    return;
                }
                if !me.sup.acting() {
                    rt.unwatch_login(point, token);
                    me.sup
                        .note(format!("{}: passive; the lost login's watch ends", rt.key));
                    return;
                }
                if !me.eligible_login_scope(&rt) && !me.observe_auth_wall(&rt) {
                    rt.unwatch_login(point, token);
                    return;
                }
                let stats = match wake {
                    Wake::Initial => true,
                    Wake::Codex => false,
                    Wake::Folder(None) => {
                        folder = None;
                        continue;
                    }
                    Wake::Folder(Some(_)) => {
                        me.login_wakes.fetch_add(1, Ordering::SeqCst);
                        true
                    }
                    Wake::Look => {
                        next_look = tokio::time::Instant::now() + look;
                        me.login_looks.fetch_add(1, Ordering::SeqCst);
                        true
                    }
                };
                if stats {
                    let now = login::stat(&file);
                    let moved = now != last || reconcile_initial_file;
                    reconcile_initial_file = false;
                    last = now;
                    if !moved {
                        continue;
                    }
                    let taken = baseline.reload(&manager).await;
                    rt.saw_login(point, token, now, taken);
                    if taken {
                        let read = rt.read_login(&manager, point);
                        me.sup.note(format!(
                            "{}: the lost login's file changed; codex reloaded the login: {}",
                            rt.key,
                            read.map_or("the wall left", login::Read::words)
                        ));
                    } else {
                        me.sup.note(format!(
                            "{}: the lost login's file changed; it holds another identity's \
                             login, or none, which codex does not take: nothing is typed",
                            rt.key
                        ));
                    }
                }
                let back = rt.read_login(&manager, point) == Some(login::Read::Back);
                let observing = { lock(&rt.auth_observation).is_some() };
                let known_profile = observing && me.eligible_renewal_scope(&rt);
                if (back && !was_back) || (known_profile && !was_known_profile) {
                    me.sup.note(if back {
                        format!("{}: the lost login is back; looking again", rt.key)
                    } else {
                        format!(
                            "{}: the original login profile is known; checking native renewal",
                            rt.key
                        )
                    });
                    rt.ask_again();
                    let _ = tokio::spawn(Arc::clone(&me).evaluate(Arc::clone(&rt))).await;
                    me.sup.note(if back {
                        format!("{}: looked again at the lost login's return", rt.key)
                    } else {
                        format!("{}: checked the original profile's native renewal", rt.key)
                    });
                }
                was_back = back;
                was_known_profile = known_profile;
            }
        });
    }

    fn quota_identity(&self) -> Option<QuotaIdentity> {
        #[cfg(test)]
        if let Some(identity) = lock(&self.quota_test_identity).clone() {
            return Some(identity);
        }
        let manager = lock(&self.thread_manager).clone()?.upgrade()?;
        let auth_manager = manager.auth_manager();
        let auth = auth_manager.auth_cached()?;
        let generation = auth_manager.auth_change_state_for(&auth)?.owner_generation;
        Some(QuotaIdentity {
            account: auth.get_account_id()?,
            user: auth.get_chatgpt_user_id(),
            owner: generation,
        })
    }

    fn bind_quota_wall(&self, rt: &ThreadRt, bucket: String, now: Instant) -> Option<QuotaWall> {
        let (identity, generation) = lock(&rt.quota_request).clone()?;
        if self.quota_identity().as_ref() != Some(&identity) {
            return None;
        }
        self.sup
            .shared()
            .quota()
            .wall_in(identity.account.clone(), generation, bucket, now)
            .map(|wall| QuotaWall { wall, identity })
    }

    fn quota_restored(
        &self,
        rt: &ThreadRt,
        wall: &codex_alab_core::quota::Wall,
        now: Instant,
    ) -> bool {
        let entitlement = matches!(
            lock(&rt.state)
                .point
                .as_ref()
                .and_then(|point| point.wall.as_ref()),
            Some(Wall::Entitlement)
        );
        if entitlement {
            self.sup.shared().quota().ordinary_restored(wall, now)
        } else {
            self.sup.shared().quota().restored(wall, now)
        }
    }

    fn restore_quota_scope(&self, rt: &ThreadRt, scope: Option<&codex_alab_core::quota::Scope>) {
        *lock(&rt.quota_wall) = None;
        *lock(&rt.quota_request) = None;
        let Some(scope) = scope.filter(|scope| scope.valid()) else {
            return;
        };
        let Some(identity) = self.quota_identity().filter(|identity| {
            identity.account == scope.account
                && identity.user.is_some()
                && identity.user == scope.user
        }) else {
            return;
        };
        let generation = self.sup.shared().quota().select_owner(
            &identity.account,
            identity.user.as_deref(),
            identity.owner,
        );
        let wall = self
            .sup
            .shared()
            .quota()
            .wall_in(
                identity.account.clone(),
                generation,
                scope.bucket.clone(),
                clock(),
            )
            .map(|wall| wall.with_ordinary_retry(scope.ordinary_retry, scope.ordinary_denied));
        *lock(&rt.quota_request) = Some((identity.clone(), generation));
        *lock(&rt.quota_wall) = wall.map(|wall| QuotaWall { wall, identity });
    }

    fn quota_scope(&self, rt: &ThreadRt) -> Option<codex_alab_core::quota::Scope> {
        let binding = lock(&rt.quota_wall).clone()?;
        if self.quota_identity().as_ref() != Some(&binding.identity) {
            return None;
        }
        Some(
            self.sup
                .shared()
                .quota()
                .scope(&binding.wall, binding.identity.user),
        )
    }

    /// THE ROUTE OF A NETWORK WALL this thread's turn met ([`netwatch::route_at_wall`]), read
    /// within [`ROUTE_READ_BUDGET`].
    async fn route_at_wall(&self, rt: &ThreadRt, details: &CodexErrorDetails) -> netwatch::Route {
        #[cfg(test)]
        {
            let gate = lock(&self.before_wall_route).take();
            if let Some((entered, release)) = gate {
                entered.send(()).expect("wall-route observer remains");
                release.await.expect("wall-route release");
            }
        }
        let Some(config) = rt.config.clone() else {
            return netwatch::Route::Custom("the thread's config is unknown".to_string());
        };
        let auth = self.auth_mode();
        let direct = self.net_direct_proxy.load(Ordering::SeqCst);
        tokio::time::timeout(
            ROUTE_READ_BUDGET,
            netwatch::route_at_wall(details, &config, auth, direct),
        )
        .await
        .unwrap_or_else(|_| {
            netwatch::Route::Custom("the route could not be read in time".to_string())
        })
    }

    /// Have the resume scan read `items` as every resumed thread's history (tests).
    #[cfg(test)]
    pub(crate) fn set_history(&self, items: Vec<codex_history::RolloutItem>) {
        *lock(&self.history_seam) = Some(items);
    }

    /// THE CARRIED WALL'S ROUTE (B2): a network wall a recovery restored, whose route the
    /// process before measured ([`crate::recovery::rearms`]), has its route read again from
    /// the thread's config and sign-in, so its wait measures it ([`netwatch::Waiter`]). The
    /// process before judged the failure's own details (no TLS or proxy refusal); the route
    /// alone is read again.
    pub(crate) async fn rearm_route(&self, rt: &ThreadRt) -> netwatch::Route {
        #[cfg(test)]
        {
            let gate = lock(&self.before_rearm_route).take();
            if let Some((entered, release)) = gate {
                entered.send(()).expect("route-read observer remains");
                release.await.expect("route-read release");
            }
        }
        self.route_at_wall(
            rt,
            &CodexErrorDetails::Stream("the carried network wall".to_string()),
        )
        .await
    }

    /// Have the upgrade read `facts` instead of the process's (tests).
    #[cfg(test)]
    pub(crate) fn set_upgrade_facts(&self, facts: Option<upgrade::UpgradeFacts>) {
        *lock(&self.facts_seam) = facts;
    }

    /// Set this instance's recovery-write hang detector (tests).
    #[cfg(test)]
    pub(crate) fn set_recovery_write_budget(&self, budget: Duration) {
        *lock(&self.recovery_write_budget) = budget;
    }

    /// Mark the upgrade's exec under way, as a request that went does (tests; the end-to-end
    /// tests' seam, so debug builds only).
    #[cfg(any(test, debug_assertions))]
    pub fn set_upgrade_requested(&self, on: bool) {
        self.upgrade_requested.store(on, Ordering::SeqCst);
    }

    /// Register every contributor on `builder`.
    pub fn register(self: &Arc<Self>, builder: &mut ExtensionRegistryBuilder<Config>) {
        builder.thread_lifecycle_contributor(Arc::clone(self) as _);
        builder.turn_lifecycle_contributor(Arc::clone(self) as _);
        builder.approval_review_contributor(Arc::clone(self) as _);
        builder.tool_lifecycle_contributor(Arc::clone(self) as _);
    }

    /// Bind the thread manager (a registry built before its manager, as tests build one).
    pub fn bind_thread_manager(&self, thread_manager: Weak<ThreadManager>) {
        *lock(&self.thread_manager) = Some(thread_manager);
    }

    pub fn supervisor(&self) -> &Arc<Supervisor> {
        &self.sup
    }

    async fn attention_for(&self, rt: &ThreadRt, key: &'static str, text: Option<String>) {
        let client = self
            .with_current_runtime(rt, || self.sup.want_attention_for(&rt.key, key, text))
            .flatten();
        self.sup.publish_attention(client).await;
    }

    /// Publish a decision's need only while its native point and policy still own
    /// it. Registration orders the check with native acceptance/completion; the
    /// supervisor checks policy and Off under the lock that inserts the need.
    async fn need_for(
        &self,
        rt: &ThreadRt,
        authority: &DecisionAuthority,
        key: &'static str,
        need: Option<String>,
    ) {
        if self.with_current_runtime(rt, || {
            if self.upgrade_requested.load(Ordering::SeqCst)
                || !authority.same_point(rt)
                || self.sup.shared().turn_running_on(&rt.key)
            {
                return false;
            }
            self.sup.want_own_need_for_at_policy(
                &rt.key,
                key,
                need,
                &authority.policy,
                authority.offs,
            )
        }) == Some(true)
        {
            self.sup.show_needs().await;
        }
    }

    /// Keep synchronous publications with the instance that owns this registration.
    /// A stopped or replaced evaluator may still owe its actual act's remote receipt.
    fn with_current_runtime<T>(&self, rt: &ThreadRt, publish: impl FnOnce() -> T) -> Option<T> {
        let kept = lock(&self.threads);
        if rt.stopped.load(Ordering::SeqCst) || !Self::current_runtime(&kept, rt) {
            return None;
        }
        Some(publish())
    }

    fn current_runtime(kept: &[Weak<ThreadRt>], rt: &ThreadRt) -> bool {
        kept.iter()
            .rev()
            .filter_map(Weak::upgrade)
            .find(|candidate| candidate.key == rt.key)
            .is_some_and(|candidate| std::ptr::eq(candidate.as_ref(), rt))
    }

    /// The runtime of a thread the autopilot keeps (tests read it).
    pub fn thread_rt(&self, key: &str) -> Option<Arc<ThreadRt>> {
        lock(&self.threads)
            .iter()
            .rev()
            .filter_map(Weak::upgrade)
            .find(|rt| rt.key == key)
    }

    async fn thread(&self, id: ThreadId) -> Option<Arc<CodexThread>> {
        let manager = lock(&self.thread_manager).clone()?.upgrade()?;
        manager.get_thread(id).await.ok()
    }

    /// Keep `rt` as one of this autopilot's threads, as `on_thread_start` does (tests).
    #[cfg(test)]
    pub(crate) fn keep_thread(&self, rt: &Arc<ThreadRt>) {
        rt.measured_by(&self.sup);
        lock(&self.threads).push(Arc::downgrade(rt));
    }

    /// THE SESSION IS PAST ITS OPENING on `rt`, where no books this process read say otherwise
    /// (gap 32, review findings 4 and 6: a fork, a late completion's end;
    /// [`ThreadState::past_opening`]). A thread this process starts later is not, unless it
    /// carries on another's work (`Shared::continue_work`, `on_thread_start`): A NEW CHAT
    /// (`/new`, `/clear`) OPENS A SESSION OF ITS OWN, as aterm's `opened_fresh` opens the next
    /// exchange's after a fresh program's banner or a later taskless screen (edges-3, which
    /// replaces gap 32's review finding 4: there a thread started once this process worked was
    /// past its opening, as aterm's loop, one per tab, judged a session's opening once); a fork
    /// is past it from its own source (`on_thread_ready`) and a resumed thread from its books
    /// (the resume scan, or [`ThreadRt::books_read`]'s fallback).
    fn seed_worked(&self, rt: &ThreadRt) {
        lock(&rt.state).seed_worked();
    }

    /// Every claim transition (taken, acting again) has each live thread look again, and say
    /// again what it is doing: a standby or a release withdrew its description, and a long turn
    /// has no start or plan step to write it back (review of 7e7622ff7, finding 8).
    pub(crate) fn watch_claims(self: &Arc<Self>) {
        if self.watching.swap(true, Ordering::SeqCst) {
            return;
        }
        let weak = Arc::downgrade(self);
        self.sup.shared().quota().set_waker(move || {
            let Some(me) = weak.upgrade() else { return };
            let live: Vec<_> = lock(&me.threads).iter().filter_map(Weak::upgrade).collect();
            for rt in live {
                if lock(&rt.quota_wall).is_some() {
                    rt.ask_again();
                    tokio::spawn(Arc::clone(&me).evaluate(rt));
                }
            }
        });
        let me = Arc::downgrade(self);
        let mut claims = self.sup.claims();
        tokio::spawn(async move {
            while claims.changed().await.is_ok() {
                let Some(me) = me.upgrade() else {
                    return;
                };
                let live: Vec<Arc<ThreadRt>> =
                    lock(&me.threads).iter().filter_map(Weak::upgrade).collect();
                for rt in live {
                    rt.ask_again();
                    tokio::spawn(Arc::clone(&me).evaluate(Arc::clone(&rt)));
                    let me = Arc::clone(&me);
                    tokio::spawn(async move { me.refresh_description(&rt).await });
                }
            }
        });
    }

    /// Watch the launch path (atpkg's twin) and reconcile every minute. Notifications are
    /// optional hints: a failed watch or a change behind the watched directory must not
    /// strand a quiet session on an older harness. Only inside an aterm session.
    fn watch_launch(self: &Arc<Self>) {
        if !self.sup.in_aterm() {
            codex_alab_tui::upgrade::update_state(
                "Automatic session updates run inside aterm; this session is not hosted there",
            );
            return;
        }
        if self.watching_launch.swap(true, Ordering::SeqCst) {
            return;
        }
        let Some(argv0) = self
            .sup
            .shared()
            .launch()
            .and_then(|launch| launch.upgrade_path().map(std::path::Path::to_path_buf))
        else {
            codex_alab_tui::upgrade::update_state(
                "Updates cannot follow this session: its launch path is unknown",
            );
            return;
        };
        // The twin's directory, not the twin: a twin that is a symlink is re-pointed by a
        // rename in that directory, and a watch on the link itself follows its target.
        let mut changes = None;
        let mut watch = None;
        match codex_file_watcher::FileWatcher::new() {
            Ok(watcher) => {
                let watcher = Arc::new(watcher);
                let (subscriber, receiver) = watcher.add_subscriber();
                if let Some(dir) = argv0.parent() {
                    let registration =
                        subscriber.register_paths(vec![codex_file_watcher::WatchPath {
                            path: dir.to_path_buf(),
                            recursive: false,
                        }]);
                    if !registration.registration_succeeded() {
                        codex_alab_tui::upgrade::update_watch_error(
                            "the launch directory could not be watched",
                        );
                    }
                    changes = Some(receiver);
                    watch = Some((watcher, subscriber, registration));
                }
            }
            Err(e) => {
                self.sup.note(format!(
                    "the launch path cannot be watched: {e}; minute checks remain active"
                ));
                codex_alab_tui::upgrade::update_watch_error(e.to_string());
            }
        }
        let me = Arc::downgrade(self);
        tokio::spawn(async move {
            let _watch = watch;
            let mut monitor = crate::upgrade_monitor::Monitor::new();
            loop {
                let wake = monitor
                    .wait(async {
                        match changes.as_mut() {
                            Some(receiver) => receiver.recv().await.is_some(),
                            None => std::future::pending().await,
                        }
                    })
                    .await;
                if wake == crate::upgrade_monitor::Wake::WatchClosed {
                    changes = None;
                    codex_alab_tui::upgrade::update_watch_error("the launch watcher stopped");
                    continue;
                }
                let Some(me) = me.upgrade() else {
                    return;
                };
                me.consider_upgrade().await;
            }
        });
    }

    /// The live upgrade (CONTRACT §5): when the launch path names another build and every
    /// precondition and the before-every-act checks hold, record the act and leave like
    /// `/exit`; the exit path execs (`codex_alab_tui::upgrade`).
    pub async fn consider_upgrade(self: Arc<Self>) {
        self.consider_upgrade_carrying(None).await;
    }

    async fn consider_upgrade_carrying(
        self: Arc<Self>,
        decided: Option<DecidedUpgradeAct>,
    ) -> bool {
        if !codex_alab_core::active() || self.upgrade_requested.load(Ordering::SeqCst) {
            return false;
        }
        let Some(policy) = self.sup.policy() else {
            codex_alab_tui::upgrade::update_state(
                "Waiting for aterm to provide the harness update policy",
            );
            return false;
        };
        let shared = self.sup.shared();
        let Some(launch) = shared.launch() else {
            codex_alab_tui::upgrade::update_state(
                "Updates cannot follow this session: its launch path is unknown",
            );
            return false;
        };
        if !policy.upgrade {
            codex_alab_tui::upgrade::update_state(
                "Automatic session updates are disabled by aterm's harness policy",
            );
            return false;
        }
        if !codex_alab_tui::upgrade::launch_names_other_build(launch) {
            let target = launch
                .upgrade_path()
                .and_then(codex_alab_tui::upgrade::named_build);
            let same = target
                .as_deref()
                .and_then(codex_alab_tui::upgrade::build_id)
                .zip(launch.exe_id)
                .is_some_and(|(target, (dev, ino))| target.dev == dev && target.ino == ino);
            codex_alab_tui::upgrade::update_state(if same {
                "The launch target still names the running build"
            } else {
                "An update target is not usable yet; the current session keeps running"
            });
            return false;
        }
        // UNDER `upgrade_ask` THE UPGRADE ASKS FIRST (gap 27): a notice that comes due has the
        // root thread on screen look again, whose decision types it at a clean point.
        self.wake_due_notice(&policy);
        if self.upgrading.swap(true, Ordering::SeqCst) {
            return false;
        }
        let _attempt = UpgradeAttempt(&self);
        #[cfg(test)]
        {
            let barrier = { lock(&self.upgrade_begin).take() };
            if let Some((entered, release)) = barrier {
                let _ = entered.send(());
                let _ = release.await;
            }
        }
        // Core can finish before the displayed widget takes its terminal notification.
        // Give that exact end the existing bounded catch-up wait before the same decided
        // act starts another native turn, then try its handoff once more. Other refusals
        // retain the ordinary single-continuation fallback.
        let tui_end_pending = decided.as_ref().and_then(|act| {
            let origin = act.owed.act.native_origin.as_ref()?;
            (origin.phase == codex_alab_core::session::NativePhase::Complete
                && shared.has_waker()
                && shared.displays(&act.rt.key)
                && shared.latest_turn(&act.rt.key).as_deref() == Some(origin.turn_id.as_str())
                && !shared.tui_handled_end(&act.rt.key, &origin.turn_id))
            .then(|| {
                (
                    origin.turn_id.clone(),
                    act.rt.off_seen.load(Ordering::SeqCst),
                )
            })
        });
        let left = match decided.as_ref() {
            Some(decided) => self.try_upgrade_carrying(Some(decided)).await,
            None => self.try_upgrade().await,
        };
        if !left && let (Some(act), Some((turn, offs))) = (decided.as_ref(), tui_end_pending) {
            let current = || {
                act.current(shared)
                    && self.sup.acting()
                    && self.sup.policy().as_ref() == Some(&policy)
                    && self.sup.offs() == offs
                    && shared.is_registered(&act.rt.key)
                    && {
                        let kept = lock(&self.threads);
                        Self::current_runtime(&kept, &act.rt)
                    }
                    && shared.displays(&act.rt.key)
                    && shared.latest_turn(&act.rt.key).as_deref() == Some(turn.as_str())
            };
            if current() {
                self.tui_caught_up(&act.rt).await;
                if current() && shared.tui_handled_end(&act.rt.key, &turn) {
                    self.sup.note(format!(
                        "{}: the TUI caught up to turn {turn}; retrying the same handoff once",
                        act.rt.key
                    ));
                    return self.try_upgrade_carrying(Some(act)).await;
                }
            }
        }
        left
    }

    pub(crate) async fn upgrade_facts(&self) -> upgrade::UpgradeFacts {
        self.upgrade_facts_carrying(None).await
    }

    async fn upgrade_facts_carrying(
        &self,
        decided: Option<&DecidedUpgradeAct>,
    ) -> upgrade::UpgradeFacts {
        #[cfg(test)]
        if let Some(facts) = lock(&self.facts_seam).clone() {
            return facts;
        }
        let shared = self.sup.shared();
        let mut threads = Vec::new();
        let manager = lock(&self.thread_manager)
            .clone()
            .and_then(|weak| weak.upgrade());
        let mut unresumable = match &manager {
            Some(_) => codex_alab_tui::upgrade::exec_plan(shared).err(),
            None => Some("no thread manager".to_string()),
        };
        if let Some(manager) = manager {
            for id in manager.list_thread_ids().await {
                let Ok(thread) = manager.get_thread(id).await else {
                    continue;
                };
                let goal_active = match goal_status_read(&thread, id).await {
                    Ok(status) => status == Some(codex_state::ThreadGoalStatus::Active),
                    Err(error) => {
                        // Unknown goal state proves neither absence nor work safe
                        // to interrupt. The independent monitor retries this scan.
                        unresumable.get_or_insert_with(|| {
                            format!("could not read thread {id}'s goal: {error}")
                        });
                        false
                    }
                };
                threads.push(upgrade::LoadedThread {
                    id: id.to_string(),
                    idle: at_rest(&thread.agent_status().await),
                    background_terminals: thread.list_background_terminals().await.len(),
                    goal_active,
                });
            }
        }
        // A paced act's wait crosses the re-exec in the record (FOREVER CRUISE: one is almost
        // always pending); any other wait would be lost by it. A WALL'S TRY'S WAIT holds it
        // (parity-3 review, F3, F10, F11): the record carries the try for aterm's relaunch
        // after a crash at rest, but a measured outage's hold is this image's measure (the
        // next one never measures the route before its try, and would wait blind for the
        // whole hold), and an image older than the record's wall tries (a rollback) reads
        // none and would type nothing; one whose timer fired holds it until its evaluation
        // decides it.
        let mut waits_pending = false;
        let mut carried = 0;
        let mut undecided = None;
        for rt in self.live_threads() {
            let owes = decided
                .and_then(|act| act.for_thread(&rt))
                .or_else(|| rt.owed_timer());
            let wall = owes
                .as_ref()
                .is_some_and(|owed| decide::is_wall_rule(owed.rule));
            waits_pending |= (rt.timer_pending() && owes.is_none()) || wall;
            if (owes.is_some() && !wall) || lock(&rt.state).landed {
                carried += 1;
            }
            // A landing an Off stood over since its memory last looked owes its judgement too
            // ([`ThreadState::park_off`]), though no evaluation has parked it yet.
            let off_unseen =
                lock(&rt.state).landed && rt.off_seen.load(Ordering::SeqCst) != self.sup.offs();
            if undecided.is_none()
                && shared.is_registered(&rt.key)
                && (rt.undecided() || off_unseen)
            {
                undecided = Some(rt.key.clone());
            }
        }
        upgrade::UpgradeFacts {
            enabled: true,
            other_build: true,
            unconfirmed: self.upgrade_unconfirmed(),
            unresumable,
            threads,
            waits_pending,
            carried,
            undecided,
            left_to_person: self.sup.standing_attention(),
        }
    }

    /// THE BUILD THE LIVE UPGRADE WOULD MOVE TO, where it asks first (aterm's `upgrade_ask`;
    /// amended 2026-10-28 after the parity sweep, gap 27): the round's key (the build's path and
    /// identity, so a build installed over the same path is another) and the path the notice
    /// names. `None` where the policy does not ask, the upgrade is off, or the launch path names
    /// no other build.
    fn ask_target(&self, policy: &decide::Policy) -> Option<(String, String)> {
        if !policy.upgrade || !policy.upgrade_ask {
            return None;
        }
        let launch = self.sup.shared().launch()?;
        if !codex_alab_tui::upgrade::launch_names_other_build(launch) {
            return None;
        }
        let path = launch.upgrade_path()?;
        let build =
            codex_alab_tui::upgrade::named_build(path).unwrap_or_else(|| path.to_path_buf());
        let named = build.display().to_string();
        let key = match codex_alab_tui::upgrade::build_id(&build) {
            Some(id) => format!("{named} ({}:{})", id.dev, id.ino),
            None => named.clone(),
        };
        Some((key, named))
    }

    /// THE ROUND'S CLOCK IS HELD (aterm's `clock_held`, at a usage limit or a login): while a
    /// root thread's point stands at a wall, or the supervisor holds the policy before its
    /// session reads acting, no re-ask comes due and no round gives up
    /// ([`upgrade_ask::Round::step`]). An Off or a standby holds nothing, as aterm's Off does
    /// not: the policy is gone then (`Supervisor::policy`), nothing looks at the round, and the
    /// first look after it counts the time that passed. Read before the round is locked.
    fn ask_held(&self) -> bool {
        let shared = self.sup.shared();
        !self.sup.acting()
            || self.live_threads().iter().any(|rt| {
                shared.is_root_thread(&rt.key)
                    && lock(&rt.state)
                        .point
                        .as_ref()
                        .is_some_and(|end| end.wall.is_some())
            })
    }

    /// THE ROUND AT THIS LOOK (aterm's `upgrade::next_step`; gap 27), aimed at the build the
    /// launch path names, or `None` where the upgrade does not ask ([`Self::ask_target`]).
    fn ask_round(
        &self,
        policy: &decide::Policy,
    ) -> Option<(MutexGuard<'_, upgrade_ask::Round>, AskStep, String)> {
        let (key, named) = self.ask_target(policy)?;
        let (mut round, now, held) = self.round_at(&key);
        let step = round.step(now, held);
        Some((round, step, named))
    }

    /// THE ROUND AIMED AT `key` NOW, the host's sleep since its last look counted
    /// ([`upgrade_ask::Round::slept_to`]; aterm's drive reads unix seconds), with the time it was
    /// read at and whether its clock is held ([`Self::ask_held`]), both read before it is locked.
    fn round_at(&self, key: &str) -> (MutexGuard<'_, upgrade_ask::Round>, Instant, bool) {
        let held = self.ask_held();
        let asleep = self.wake_reading().map(|reading| reading.asleep);
        let mut round = lock(&self.ask);
        round.aim(key);
        let now = clock();
        round.slept_to(asleep, now);
        (round, now, held)
    }

    /// THE NOTICE `rt`'S DECISION TYPES FIRST, where one is due (`upgrade_ask`; gap 27): the
    /// round's next notice, its reference offered, for the root thread the TUI shows (aterm types
    /// its notice into the tab, whose conversation is the root thread on screen). Set before each
    /// decision ([`ThreadState::upgrade_notice`]); the decider types it only at a clean point.
    fn upgrade_notice_for(&self, rt: &ThreadRt, policy: &decide::Policy) -> Option<String> {
        let shared = self.sup.shared();
        if self.upgrade_requested.load(Ordering::SeqCst)
            || !shared.is_root_thread(&rt.key)
            || !shared.displays(&rt.key)
        {
            return None;
        }
        let (mut round, step, named) = self.ask_round(policy)?;
        if step != AskStep::Notice {
            return None;
        }
        let marker = round.offer(&rt.key);
        Some(words::upgrade_notice(
            &named,
            codex_alab_tui::session::version(),
            &marker,
        ))
    }

    /// UNDER `upgrade_ask`, WHY THE RE-EXEC STILL WAITS FOR THE AGENT (gap 27): the round's step
    /// in a person's words while a thread holds work in hand and the agent's latest reply did
    /// not say ready; `None` without the key, once it said so, and where no thread holds work in
    /// hand, so the session moves with no notice as aterm's cooperative mode moves a quiet one
    /// (its `Facts::quiet`): no task (aterm's `Step::Fresh`), a landing (it left nothing to stop,
    /// and Land types nothing into it), or a point left to a person's own exchange (aterm's
    /// `left_to_person`: no notice is typed there, `decide::ThreadState::upgrade_notice`). IT
    /// FAILS CLOSED (review of gap 27, finding 7): with no policy in hand, or the round not to be
    /// read (the launch path, read again, names no other build), it says so, and the re-exec,
    /// whose facts say another build, waits for the next look.
    fn upgrade_unconfirmed(&self) -> Option<String> {
        let Some(policy) = self.sup.policy() else {
            return Some("aterm's harness policy is not in hand".to_string());
        };
        if !policy.upgrade_ask {
            return None;
        }
        if !self.live_threads().iter().any(|rt| {
            let state = lock(&rt.state);
            !rt.stopped.load(Ordering::SeqCst) && state.task && !state.landed && !state.left
        }) {
            return None;
        }
        let Some((round, step, _)) = self.ask_round(&policy) else {
            return Some("the build to ask about is not in hand".to_string());
        };
        match step {
            AskStep::Ready => None,
            AskStep::Notice => Some(format!(
                "the readiness notice ({} of {}) goes in at the agent's next stopping point",
                round.asks() + 1,
                upgrade_ask::MAX_ASKS
            )),
            AskStep::Wait { why, .. } => Some(why),
        }
    }

    /// A NOTICE THAT CAME DUE has the root thread on screen look again, once for each
    /// ([`upgrade_ask::Round::wake_due`]): a thread at rest has no turn end to come that would
    /// decide it (aterm's host types its notice into an idle tab).
    fn wake_due_notice(self: &Arc<Self>, policy: &decide::Policy) {
        let due = self.ask_target(policy).is_some_and(|(key, _)| {
            let (mut round, now, held) = self.round_at(&key);
            round.wake_due(now, held)
        });
        if !due {
            return;
        }
        let shared = self.sup.shared();
        for rt in self.live_threads() {
            if shared.is_root_thread(&rt.key)
                && shared.displays(&rt.key)
                && !shared.turn_running_on(&rt.key)
            {
                self.sup.note(format!(
                    "{}: the upgrade's readiness notice is due; looking again",
                    rt.key
                ));
                rt.ask_again();
                tokio::spawn(Arc::clone(self).evaluate(rt));
            }
        }
    }

    fn live_threads(&self) -> Vec<Arc<ThreadRt>> {
        lock(&self.threads)
            .iter()
            .filter_map(Weak::upgrade)
            .collect()
    }

    /// The act a thread owes (a paced act's wait, or the one a timer that fired is about to
    /// decide) or its landing, as the record carries it across the re-exec (the old image's
    /// decision, its words included), with that thread; its timer is cancelled (the act is the
    /// next image's now). `None` when no thread has anything to carry. A wall's try is never
    /// taken: its wait holds the upgrade ([`Autopilot::upgrade_facts`]). The upgrade fences
    /// its initial live threads' evaluations and rechecks fresh native facts before leave;
    /// each selected point owns its own cancellation and rollback.
    #[cfg(test)]
    pub(crate) async fn take_owed_act(
        &self,
    ) -> Option<(Arc<ThreadRt>, codex_alab_core::session::OwedAct)> {
        self.take_owed_act_carrying(None, None).await
    }

    async fn take_owed_act_carrying(
        &self,
        decided: Option<&DecidedUpgradeAct>,
        mut handoff: Option<&mut UpgradeHandoff>,
    ) -> Option<(Arc<ThreadRt>, codex_alab_core::session::OwedAct)> {
        for rt in self.live_threads() {
            let reading = self.wake_reading();
            let (pending, owed) = {
                let kept = lock(&self.threads);
                if rt.stopped.load(Ordering::SeqCst)
                    || !Self::current_runtime(&kept, &rt)
                    || self.sup.shared().turn_running_on(&rt.key)
                {
                    continue;
                }
                let state = lock(&rt.state);
                let point = state.point_at();
                let accepted_turns = rt.accepted_turns.load(Ordering::SeqCst);
                let candidate = decided.filter(|act| Arc::ptr_eq(&act.rt, &rt));
                if candidate
                    .is_some_and(|act| act.accepted_turns != accepted_turns || act.point != point)
                {
                    continue;
                }
                let mut timer = lock(&rt.timer);
                let mut fired = lock(&rt.fired);
                if let Some(owed) = timer.as_mut().and_then(|timer| timer.owes.as_mut()) {
                    owed.establish(reading);
                }
                if let Some(owed) = fired.as_mut() {
                    owed.establish(reading);
                }
                let mut owes = candidate
                    .map(|act| act.owed.clone())
                    .or_else(|| timer.as_ref().and_then(|timer| timer.owes.clone()))
                    .or_else(|| fired.clone())
                    .filter(|owed| !decide::is_wall_rule(owed.act.rule));
                if let Some(owed) = owes.as_mut() {
                    owed.establish(reading);
                }
                if owes.is_none() && !state.landed {
                    continue;
                }
                let owed = owed_act_of(
                    &rt.key,
                    owes.as_ref(),
                    state.landed,
                    state.native_origin.as_ref(),
                    reading,
                );
                if let Some(handoff) = handoff.as_mut() {
                    // The same point owns the exact timer before cancellation can await.
                    handoff.owed = Some(HandoffAct {
                        rt: Arc::clone(&rt),
                        owed: owed.clone(),
                        point,
                        accepted_turns,
                        timer_generation: rt.timer_seq.load(Ordering::SeqCst),
                    });
                }
                fired.take();
                (timer.take(), owed)
            };
            if let Some(timer) = pending {
                timer.handle.abort();
                let _ = timer.handle.await;
            }
            return Some((rt, owed));
        }
        None
    }

    /// THE RECORD CARRIES THE PACED ACT OWED NOW (FOREVER CRUISE / FOREVER LAND: the
    /// back-off's wait is rest, so the record says `idle` there): the first live thread's
    /// act waiting on its timer, one a timer that fired left, or one the image before handed
    /// over not yet waited on, as decided; none once it went or was dropped. A crash at rest hands it to aterm's relaunch ([`Relaunch::Owed`]), as
    /// the live upgrade's record hands its act to the next image. Left alone while the
    /// upgrade's exec is under way: its record carries what it took.
    pub(crate) fn sync_record_owed(&self) {
        let kept = lock(&self.threads);
        self.sync_record_owed_from(&kept);
    }

    fn sync_record_owed_from(&self, kept: &[Weak<ThreadRt>]) {
        if self.upgrade_requested.load(Ordering::SeqCst) {
            return;
        }
        let reading = self.wake_reading();
        let owed = kept.iter().filter_map(Weak::upgrade).find_map(|rt| {
            rt.tracked_owed_timer_at(reading)
                .or_else(|| {
                    let mut state = lock(&rt.state);
                    if state.owed.is_some() && state.asleep_mark.is_none() {
                        state.asleep_mark = reading.map(|reading| reading.asleep);
                    }
                    state
                        .owed
                        .clone()
                        .map(|owed| TrackedOwed::from_state(owed, state.asleep_mark))
                })
                .map(|owes| owed_act_of(&rt.key, Some(&owes), false, None, reading))
        });
        self.sup.shared().set_owed_act(owed);
    }

    /// The act `carried` names (its rule a paced act's or a wall's try, [`decide::carried_rule`],
    /// its words and mode read), as this image's [`Owed`]: its time turned into this clock.
    pub(crate) fn owed_of(carried: &codex_alab_core::session::OwedAct) -> Option<Owed> {
        let epoch = now_ms();
        let now = clock();
        Self::owed_of_at(carried, now, epoch)
    }

    fn owed_of_at(
        carried: &codex_alab_core::session::OwedAct,
        now: Instant,
        epoch: u64,
    ) -> Option<Owed> {
        let rule = carried.rule.as_deref().and_then(decide::carried_rule)?;
        let text = carried.text.clone()?;
        let mode = carried.mode.as_deref().and_then(decide::Mode::of_word)?;
        let wait = carried.not_before_ms.saturating_sub(epoch);
        // A record from an image that carried no open work owed the defend prompt's suffix
        // all the same (it read the repository at every owed defend prompt): its open words
        // stay as decided.
        let repo_owed = carried
            .repo_owed
            .clone()
            .or_else(|| (rule == crate::words::RULE_DONE_CHECK).then(Vec::new));
        Some(Owed {
            native_origin: carried.native_origin.clone(),
            rule,
            text,
            mode,
            not_before: now + Duration::from_millis(wait),
            repo_owed,
            resume_goal: carried.resume_goal && rule == crate::words::RULE_LIMIT_RESUME,
            landing: carried.landing && decide::is_wall_rule(rule),
        })
    }

    /// The upgrade did not go after all: the act taken for the record waits on `rt` again, at
    /// its time, as it did before.
    #[cfg(test)]
    pub(crate) fn put_back(
        self: &Arc<Self>,
        rt: &Arc<ThreadRt>,
        carried: &codex_alab_core::session::OwedAct,
    ) {
        let kept = lock(&self.threads);
        self.put_back_from(rt, carried, &kept, None);
    }

    fn put_back_from(
        self: &Arc<Self>,
        rt: &Arc<ThreadRt>,
        carried: &codex_alab_core::session::OwedAct,
        kept: &[Weak<ThreadRt>],
        timer_generation: Option<u64>,
    ) {
        if let Some(owed) = Self::owed_of(carried) {
            self.arm_timer_if_unchanged(rt, owed.not_before, Some(owed), timer_generation);
        }
        self.sync_record_owed_from(kept);
    }

    /// `true` once the TUI was asked to leave for the exec. Every thread's evaluation permit
    /// is held from the facts to the request: an evaluation in flight (a timer that fired, a
    /// repository probe of up to `repo::PROBE_LIMIT`) is waited out by a later look, never
    /// raced, so the act it was about to type is neither typed into the exec nor left out of
    /// the record (review of 2a9241bbf, findings 5 and 9).
    pub(crate) async fn try_upgrade(self: &Arc<Self>) -> bool {
        self.try_upgrade_carrying(None).await
    }

    async fn try_upgrade_carrying(self: &Arc<Self>, decided: Option<&DecidedUpgradeAct>) -> bool {
        if decided.is_some_and(|act| !act.current(self.sup.shared())) {
            return false;
        }
        let live = self.live_threads();
        let mut permits = Vec::with_capacity(live.len());
        for rt in &live {
            // The caller already holds this permit from decision through handoff.
            if decided.is_some_and(|act| Arc::ptr_eq(&act.rt, rt)) {
                continue;
            }
            match rt.eval.try_acquire() {
                Ok(permit) => permits.push(permit),
                Err(_) => {
                    codex_alab_tui::upgrade::update_pending(format!(
                        "Update pending: thread {} is deciding",
                        rt.key
                    ));
                    self.sup
                        .note(format!("upgrade waits: {} is deciding", rt.key));
                    self.relook_upgrade(clock() + UPGRADE_RELOOK, self.sup.claims());
                    return false;
                }
            }
        }
        let facts = match decided {
            Some(_) => self.upgrade_facts_carrying(decided).await,
            None => self.upgrade_facts().await,
        };
        if decided.is_some_and(|act| !act.current(self.sup.shared())) {
            return false;
        }
        match upgrade::check(&facts) {
            UpgradeCheck::NotDue(why) => {
                codex_alab_tui::upgrade::update_state(format!("Update not due: {why}"));
                self.sup.note(format!("upgrade: {why}"));
                return false;
            }
            UpgradeCheck::Wait(why) => {
                codex_alab_tui::upgrade::update_pending(format!("Update pending: {why}"));
                self.sup.note(format!("upgrade waits: {why}"));
                if facts.undecided.is_some() {
                    // Its evaluation is queued behind the permits held here: look again once
                    // it ran, as for one in flight.
                    self.relook_upgrade(clock() + UPGRADE_RELOOK, self.sup.claims());
                }
                return false;
            }
            UpgradeCheck::Go => {}
        }
        // Watched from before the checks: a hold's lift the claims watch carries after them
        // (or during them, after their `status` read met the hold) wakes the look again.
        let claims = self.sup.claims();
        let checked_policy = self.sup.policy();
        match self.sup.ready().await {
            Ready::Go => {}
            Ready::Wait { until, why } => {
                codex_alab_tui::upgrade::update_pending(format!("Update pending: {why}"));
                self.sup.note(format!("upgrade not now ({why})"));
                self.relook_upgrade(until, claims);
                return false;
            }
            Ready::Passive(why) => {
                codex_alab_tui::upgrade::update_pending(format!("Update pending: {why}"));
                return false;
            }
        }
        let shared = self.sup.shared();
        let target = shared
            .launch()
            .and_then(|launch| launch.upgrade_path())
            .and_then(codex_alab_tui::upgrade::named_build)
            .map_or_else(|| "the new build".to_string(), |p| p.display().to_string());
        if !self.sup.acting() {
            codex_alab_tui::upgrade::update_state("Update pending: this harness is passive");
            self.sup.note("upgrade: passive since the checks; not now");
            return false;
        }
        self.sup
            .record_act(
                upgrade::RULE_UPGRADE,
                &format!("re-exec onto {target}, resuming this session"),
            )
            .await;
        // Selection installs its own rollback identity before taking the exact timer.
        // An earlier live-thread snapshot can name a different act by this point.
        let mut handoff = UpgradeHandoff {
            autopilot: Arc::clone(self),
            owed: None,
        };
        #[cfg(test)]
        {
            let barrier = { lock(&self.before_upgrade_take).take() };
            if let Some((entered, release)) = barrier {
                let _ = entered.send(());
                let _ = release.await;
            }
        }
        let left = 'handoff: {
            let _ = self
                .take_owed_act_carrying(decided, Some(&mut handoff))
                .await;
            if decided.is_some_and(|act| !act.current(shared)) {
                break 'handoff false;
            }
            let Some(checked_policy) = checked_policy.filter(|policy| policy.upgrade) else {
                codex_alab_tui::upgrade::update_state(
                    "Update pending: automatic updates are disabled",
                );
                break 'handoff false;
            };
            // No earlier snapshot authorizes leave. Native changes throughout this fresh
            // asynchronous scan invalidate the token the actual TUI event must commit.
            let native_generation = shared.upgrade_generation();
            let facts = match decided {
                Some(_) => self.upgrade_facts_carrying(decided).await,
                None => self.upgrade_facts().await,
            };
            if decided.is_some_and(|act| !act.current(shared)) {
                break 'handoff false;
            }
            match upgrade::check(&facts) {
                UpgradeCheck::Go => {}
                UpgradeCheck::Wait(why) => {
                    codex_alab_tui::upgrade::update_pending(format!("Update pending: {why}"));
                    break 'handoff false;
                }
                UpgradeCheck::NotDue(why) => {
                    codex_alab_tui::upgrade::update_pending(format!("Update pending: {why}"));
                    break 'handoff false;
                }
            }
            if let Some(act) = &handoff.owed {
                self.sup.note(format!(
                    "upgrade: carries {} on {} (landed={})",
                    act.owed.rule.as_deref().unwrap_or("no act"),
                    act.owed.thread,
                    act.owed.landed
                ));
            }
            shared.set_owed_act(handoff.owed.as_ref().map(|act| act.owed.clone()));
            self.upgrade_requested.store(true, Ordering::SeqCst);
            // The next image can inherit only the record, not this suspended timer.
            // Refuse before teardown if its carried act has not reached that record.
            if handoff.owed.is_some() && !write_through(shared).await {
                codex_alab_tui::upgrade::update_state(
                    "Update pending: the session record could not save the carried act",
                );
                self.sup
                    .note("upgrade: the carried act was not written; keeping this session");
                break 'handoff false;
            }
            let supervisor = Arc::clone(&self.sup);
            if codex_alab_tui::upgrade::request_checked(shared, move || {
                supervisor.commit_upgrade(&checked_policy, native_generation)
            })
            .await
            {
                codex_alab_tui::upgrade::update_state(format!(
                    "Restarting on {target}, resuming this session"
                ));
                self.sup.note("upgrade: leaving like /exit for the exec");
                true
            } else {
                codex_alab_tui::upgrade::update_state(
                    "Update pending: the TUI could not leave safely; the current session keeps running",
                );
                self.sup.note("upgrade: the TUI gave no way to leave");
                false
            }
        };
        if !left
            && let Some(decided) = decided
            && handoff
                .owed
                .as_ref()
                .is_some_and(|act| Arc::ptr_eq(&act.rt, &decided.rt))
        {
            // Only this evaluator's fresh act returns to its synchronous fallback.
            // Another thread's act remains owned by the rollback guard.
            handoff.owed = None;
        }
        drop(handoff);
        drop(permits);
        left
    }

    /// Whether the upgrade's look again is pending ([`UPGRADE_RELOOK`], tests).
    #[cfg(test)]
    pub(crate) fn upgrade_relook_pending(&self) -> bool {
        self.upgrade_relook.load(Ordering::SeqCst)
    }

    /// THE UPGRADE LOOKS AGAIN at `until`, or sooner at the next move of the claims watch
    /// (`claims`, subscribed before what made it wait): a hold's lift ([`Supervisor::read_on`]),
    /// a claim taken or acting again, as every thread looks again at them (parity review R2:
    /// an upgrade that met a hold waited out its own recheck after the lift, as the acts did
    /// before 648d827a8).
    fn relook_upgrade(self: &Arc<Self>, until: Instant, mut claims: watch::Receiver<u64>) {
        if self.upgrade_relook.swap(true, Ordering::SeqCst) {
            return;
        }
        let me = Arc::clone(self);
        tokio::spawn(async move {
            tokio::select! {
                () = tokio::time::sleep_until(tokio::time::Instant::from_std(until)) => {}
                moved = claims.changed() => {
                    if moved.is_err() {
                        // The supervisor is gone: only the time is left to wait on.
                        tokio::time::sleep_until(tokio::time::Instant::from_std(until)).await;
                    }
                }
            }
            me.upgrade_relook.store(false, Ordering::SeqCst);
            me.consider_upgrade().await;
        });
    }

    /// Decide this thread's point and carry the decision out.
    pub(crate) async fn evaluate(self: Arc<Self>, rt: Arc<ThreadRt>) {
        rt.evaluations.fetch_add(1, Ordering::SeqCst);
        let Ok(turn) = Arc::clone(&rt.eval).acquire_owned().await else {
            return;
        };
        let already_owed = rt.owed_timer().is_some();
        // This evaluation decides the point: the act a timer that fired left for it is its own
        // to type (or not) from here on, and the record carries it no more.
        if lock(&rt.fired).take().is_some() {
            self.sync_record_owed();
        }
        if rt.stopped.load(Ordering::SeqCst) || rt.vanilla() {
            return;
        }
        if rt.resumed.load(Ordering::SeqCst) && !rt.scanned.load(Ordering::SeqCst) {
            return;
        }
        #[cfg(test)]
        if let Some(hook) = lock(&rt.before_park).take() {
            run_hook(hook);
        }
        let books = self.unwatched_books(&rt).await;
        if !self.park_reading(&rt, books).await {
            return;
        }
        self.reconcile_transient_goal(&rt).await;
        if !self.sup.shared().is_registered(&rt.key) {
            return;
        }
        if self.sup.policy().is_none() {
            self.sup
                .note(format!("{}: passive (no proven claim)", rt.key));
            return;
        }
        if self.upgrade_requested.load(Ordering::SeqCst) {
            self.sup.note(format!(
                "{}: the upgrade's exec is under way; no act",
                rt.key
            ));
            return;
        }
        if self.follow_renewal(&rt).await {
            return;
        }
        // WHETHER A LEAD WAITS ON THE SESSION (`@self task`), read once before the decision's
        // authority and state are taken: its first short done claims and questions wait no
        // back-off, and Land's hold is quiet.
        let managed = self.sup.managed().await;
        // THE PERSON'S LAST KEY (`status human_typed_ms=`, or another driver's hand on the tab),
        // read before the decision's authority and state are taken: it holds the owner's
        // standing go-ahead for the hour after it ([`ThreadState::person_keyed`]).
        let keyed = self
            .sup
            .person_typed_ago()
            .await
            .and_then(|ago| clock().checked_sub(ago));
        if !self.park_if_off(&rt).await {
            return;
        }
        if !self.sup.shared().is_registered(&rt.key) {
            return;
        }
        let Some(policy) = self.sup.policy() else {
            self.sup
                .note(format!("{}: passive (no proven claim)", rt.key));
            return;
        };
        if self.upgrade_requested.load(Ordering::SeqCst) {
            self.sup.note(format!(
                "{}: the upgrade's exec is under way; no act",
                rt.key
            ));
            return;
        }
        let accepted_at_decision = rt.accepted_turns.load(Ordering::SeqCst);
        let offs_at_decision = rt.off_seen.load(Ordering::SeqCst);
        let mut now = clock();
        let quota_binding = lock(&rt.quota_wall).clone();
        let quota_wall = quota_binding.as_ref().map(|binding| binding.wall.clone());
        let quota_identity = quota_binding
            .as_ref()
            .map(|binding| binding.identity.clone());
        let restored = quota_wall.as_ref().is_some_and(|wall| {
            quota_identity.is_some()
                && quota_identity == self.quota_identity()
                && self.quota_restored(&rt, wall, now)
        });
        let decided_at;
        // THE HOST'S SLEEP (2026-10-24): the point's real-time deadlines count the time slept
        // since they were last counted, and the decision reads the last wake (its settle).
        // Read before the reach: a sleep clears the usage resume's ask, so the wake's network
        // is measured again.
        let reading = self.wake_reading();
        {
            let mut state = lock(&rt.state);
            if let Some(slept) = state.slept_to(reading.map(|r| r.asleep), now) {
                self.sup.note(format!(
                    "{}: the host slept {} s; the point's real-time deadlines count it",
                    rt.key,
                    slept.as_secs()
                ));
            }
            state.woke = reading.and_then(|r| r.last).map(|wake| wake.at);
        }
        // The API's reach at the point's network wall, measured for its own route (the ask
        // renews its lease), before the state is held.
        let reach = self.reach_for(&rt);
        // The lost login at the point's login wall, read from codex's own cache (its watch armed
        // while codex-alab acts).
        let (login_at, login) = self.login_for(&rt);
        // THE LIVE UPGRADE'S NOTICE, where one is due (`upgrade_ask`; amended 2026-10-28 after
        // the parity sweep, gap 27), read before the state is held: it goes first at a clean
        // point, and the decisions after a probe or a reach's ask read it too.
        let upgrade_notice = self.upgrade_notice_for(&rt, &policy);
        let mut recovery_generation = self.sup.shared().upgrade_generation();
        // A PERSON'S HOLD OVER AUTOPILOT OFF keeps the claim and, in the policy, the word before
        // it, while the word they chose is Off: Cruise takes back no exchange the Off left
        // (`ThreadState::observe_mode_under`; review of gap 32, finding 9).
        let named_off =
            self.sup.shared().harness_word() == codex_alab_core::session::HarnessWord::Off;
        let mut decision_state;
        let mut hour_moved = false;
        let mut decision = {
            let mut state = lock(&rt.state);
            state.observe_mode_under(policy.autopilot, named_off);
            state.reach = reach;
            // Read for this point only: a turn that ended since stands at a wall of its own.
            state.login = if state.point_at() == login_at {
                login
            } else {
                LoginRead::default()
            };
            state.managed = managed;
            state.upgrade_notice = upgrade_notice;
            if let Some(key) = keyed {
                let carried = (state.go_ahead_hour_carried(now), state.go_aheads_carried());
                state.person_keyed(now.saturating_duration_since(key), now);
                // The record carries the hour the key holds and the count it reset
                // ([`Self::sync_pace`]), though nothing may be typed at this point.
                hour_moved =
                    carried != (state.go_ahead_hour_carried(now), state.go_aheads_carried());
            }
            decided_at = state.point_at();
            decision_state = state.clone();
            decide::decide_with_usage(&state, &policy, &self.timing, now, rt.jitter, restored)
        };
        if hour_moved {
            self.sync_pace(&rt);
        }
        // Leaving Land ends its episode, and Land's hold in aterm with it.
        self.tell_land(&rt);
        let authority = DecisionAuthority {
            quota_identity,
            quota: if restored { quota_wall.clone() } else { None },
            policy: policy.clone(),
            offs: offs_at_decision,
            point: decided_at,
            accepted_turns: accepted_at_decision,
            native_origin: decision_state.native_origin.clone(),
        };
        // THE REPOSITORY PROBE the decider asked for (Cruise's done claim, Land's clean
        // defense): read off the thread's working folder, then decided again. A turn end that
        // came meanwhile asks again at its own look.
        if decision.act == Act::Probe {
            let found = self
                .probe_repo(&rt, "the repository probe", repo::PROBE_LIMIT)
                .await;
            // A LANDING'S JUDGEMENT or a point's probe is settled only by a probe run with no
            // Off begun since this look parked the thread: an Off that came while it ran is the
            // person's act, judged as it lifts over the repository as it is then (the next look
            // parks it again), never over what the person may change under it.
            if !self.sup.acting()
                || offs_at_decision != self.sup.offs()
                || rt.off_seen.load(Ordering::SeqCst) != self.sup.offs()
            {
                if lock(&rt.state).judging() {
                    self.sup.note(format!(
                        "{}: an Off came during the landing's probe; judged as it lifts",
                        rt.key
                    ));
                }
                return;
            }
            now = clock();
            recovery_generation = self.sup.shared().upgrade_generation();
            let policy_current = self.sup.policy().as_ref() == Some(&policy);
            let point_moved;
            decision = {
                let mut state = lock(&rt.state);
                point_moved = rt.accepted_turns.load(Ordering::SeqCst) != accepted_at_decision
                    || self.sup.shared().turn_running_on(&rt.key)
                    || state.point_at() != decided_at;
                if !point_moved && policy_current {
                    state.probed_for(decided_at, found);
                }
                decision_state = state.clone();
                decide::decide_with_usage(&state, &policy, &self.timing, now, rt.jitter, restored)
            };
            if point_moved {
                self.sup
                    .note(format!("{}: a new point came during the probe", rt.key));
                authority.unwritten(&self, &rt);
                return;
            }
            if !policy_current {
                let _ = self.decision_current(&rt, &authority);
                return;
            }
            // The yield's repository legs, read by this probe.
            self.tell_land(&rt);
            if decision.act == Act::Probe {
                self.sup
                    .note(format!("{}: a new point came during the probe", rt.key));
                return;
            }
        }
        // THE USAGE RESUME ASKS FOR THE API'S REACH WHEN IT COMES DUE (2026-10-24): where its
        // route is one the probe reproduces and aterm's `probe_api` is on (gap 30), the first
        // due decision asks (the probe starts, and the point stands at the route from then on:
        // [`netwatch::Waiter`]) and decides again over the answer, so a resume is never typed
        // into an API measured down ([`decide::decide`]'s reach hold). A measure that moves
        // looks again at once.
        if decide::resumes_limit(&decision.act)
            && decision.not_before.is_none_or(|at| at <= now)
            && policy.probe_api
            && decision_state.net_asked.is_none()
            && matches!(
                decision_state
                    .point
                    .as_ref()
                    .and_then(|point| point.wall.as_ref()),
                Some(Wall::Usage { .. } | Wall::Entitlement)
            )
            && matches!(*lock(&rt.net), Some(netwatch::Route::Direct(_)))
        {
            {
                let mut state = lock(&rt.state);
                if rt.accepted_turns.load(Ordering::SeqCst) == accepted_at_decision
                    && !self.sup.shared().turn_running_on(&rt.key)
                    && state.point_at() == decided_at
                {
                    state.net_asked = Some(now);
                }
            }
            let reach = self.reach_for(&rt);
            let point_moved;
            decision = {
                let mut state = lock(&rt.state);
                point_moved = rt.accepted_turns.load(Ordering::SeqCst) != accepted_at_decision
                    || self.sup.shared().turn_running_on(&rt.key)
                    || state.point_at() != decided_at;
                if !point_moved {
                    state.reach = reach;
                }
                decision_state = state.clone();
                // Reachability cannot discard the fresh quota evidence that made this act due.
                decide::decide_with_usage(&state, &policy, &self.timing, now, rt.jitter, restored)
            };
            if point_moved || decision.act == Act::Probe {
                self.sup.note(format!(
                    "{}: a new point came during the reach's ask",
                    rt.key
                ));
                return;
            }
        }
        if decision.act.needs_hand()
            && let Some(until) = self.inherited_wait(&rt)
            && decision.not_before.is_none_or(|at| at < until)
        {
            decision.not_before = Some(until);
            decision.shown_wait =
                Some("the resumed automatic intent is waiting for the host to settle".into());
        }
        if !authority.point_current(&self, &rt) {
            let _ = self.decision_current(&rt, &authority);
            authority.unwritten(&self, &rt);
            return;
        }
        // Decided: whatever follows (an act, a wait, a turn end to come), the upgrade no longer
        // waits on this point ([`ThreadRt::undecided`]).
        *lock(&rt.decided) = Some(decided_at);
        // THE GO-AHEAD'S LINES (aterm's DESIGN-harness-autopilot §15.6): the decision says what
        // it does with the lines the owner's standing go-ahead answers, and the menu bar shows
        // the rest of the needs ([`ThreadState::go_ahead_judge`]).
        let shown_moved = {
            let mut state = lock(&rt.state);
            let shown = state.needs.shown_key();
            if state.point_at() == decided_at {
                state.go_ahead_judge(&decision, &policy, now);
                decision_state.needs = state.needs.clone();
            }
            state.needs.shown_key() != shown
        };
        if shown_moved {
            self.surface_needs(&rt);
        }
        // Ordinary holds are recorded quietly; a spent standing go-ahead is the one bounded
        // refusal that asks the person for the action only they can provide.
        let told = decision
            .held
            .map(|arm| decide::hold_told(&decision_state, &policy, arm, now))
            .or_else(|| {
                decision
                    .refuses()
                    .then_some(crate::replies::Told::GoAhead { arm: None })
            })
            .filter(|_| self.sup.acting());
        self.tell_held(&rt, &authority, told).await;
        if !authority.point_current(&self, &rt) {
            let _ = self.decision_current(&rt, &authority);
            authority.unwritten(&self, &rt);
            return;
        }
        if decision.act == Act::Nothing {
            // A HELD POINT OWES NOTHING: a timer armed for an earlier decision at it (a wait the
            // replies did not hold yet) is dropped with the act it owed, so neither its firing
            // nor the record a re-exec reads types the try the hold keeps back. Taken out of
            // its slot, never joined: an evaluation the timer runs took itself out already.
            let Some(waited) = self
                .with_current_runtime(&rt, || {
                    if !authority.point_current(&self, &rt) {
                        return None;
                    }
                    if decision.held.is_some() {
                        lock(&rt.fired).take();
                        if let Some(timer) = lock(&rt.timer).take() {
                            timer.handle.abort();
                        }
                    }
                    Some(self.sup.want_wait_for(&rt.key, None))
                })
                .flatten()
            else {
                let _ = self.decision_current(&rt, &authority);
                return;
            };
            if decision.held.is_some() {
                self.sync_record_owed();
            }
            // Nothing is typed (a held point included: its stop is told on its own, never as
            // a wait), so nothing is waited for: a held wall's wait comes down too, before the
            // recovery below can fail (review of 4c0168cedd, finding 10).
            if waited {
                self.sup.show_waits().await;
            }
            let held_try =
                decide::held_wall_try(&decision_state, &policy, &self.timing, now, rt.jitter);
            // A HELD QUOTA IS STILL A QUOTA A PERSON MUST LIFT: its need stands, raised here
            // where it does not (the image after a re-exec starts with none of codex-alab's own
            // needs, and its held point types no try that would raise it; review of
            // 4c0168cedd, finding 4), never written again where it stands.
            if let Some(held_try) = &held_try
                && decide::quota_try(&held_try.act)
                && self.sup.acting()
            {
                self.need_for(
                    &rt,
                    &authority,
                    "limit",
                    Some(decide::QUOTA_NEED.to_string()),
                )
                .await;
            }
            // A WALL RULING F HOLDS stays the record's failed point (stage 2): its recovery
            // carries the try its hold sets aside ([`decide::held_wall_try`]), so a re-exec
            // stands at the wall, held by the replies its record seeds, and aterm's relaunch,
            // which seeds none, decides the try afresh.
            if let Some(held_try) = held_try
                && !self
                    .sync_recovery(&rt, &held_try, &decision_state, recovery_generation)
                    .await
            {
                self.schedule(&rt, clock() + WRITE_THROUGH_RETRY);
                return;
            }
            if let Some(point) = decided_at
                && policy.autopilot == decide::Mode::Land
                && decision_state.task
                && decision_state.left
                && !decision_state.landed
                && !decision_state.compacting
                && self.sup.acting()
                && !self.sup.shared().turn_running_on(&rt.key)
            {
                let key = (accepted_at_decision, point);
                // A cancelled evaluation cannot release the gate while its queued
                // control request can still reach aterm. The publication task owns
                // it through the acknowledgement (or the client's bounded failure).
                let publisher = Arc::clone(&self);
                let thread = Arc::clone(&rt);
                let _ = tokio::spawn(async move {
                    // The live upgrader also waits on this decision after the
                    // evaluation caller is cancelled: no exec over its lease.
                    let _turn = turn;
                    publisher.record_left(thread, key).await;
                })
                .await;
            }
            // THE PERSON'S HOUR ENDS LATER (aterm's `GO_AHEAD_HOUR`): the point is decided again
            // there, though the parked agent ends no other turn.
            if let Some(at) = decision.look_again.filter(|at| *at > now) {
                self.schedule(&rt, at);
            }
            self.sup
                .note(format!("{}: nothing ({})", rt.key, decision.why));
            return;
        }
        // A live successful human answer is positive evidence beyond a previous usage
        // wall. Cruise still owes its ordinary paced continuation, but a limited goal
        // must continue through GoalService rather than a plain typed prompt. Bind this
        // exception to the goal present when that exact native human turn began.
        let human_goal_resume = if policy.autopilot == decide::Mode::Cruise
            && policy.resume_limits
            && policy.limit_wait
            && matches!(decision.act, Act::Type { rule, .. } if rule == words::RULE_CONTINUE)
            && decision_state.point.as_ref().is_some_and(|point| {
                point.by_person
                    && !point.aborted
                    && !point.restored
                    && point.wall.is_none()
                    && point
                        .text
                        .as_ref()
                        .is_some_and(|text| !text.trim().is_empty())
            }) {
            lock(&rt.human_goal_recovery)
                .as_ref()
                .filter(|(at, _)| Some(*at) == decided_at)
                .map(|(_, goal)| goal.clone())
        } else {
            None
        };
        // A wall wait is idle, but it still owns an unresolved failure. Persist that
        // point and its absolute deadlines before handing it to a timer.
        if !self
            .sync_recovery(&rt, &decision, &decision_state, recovery_generation)
            .await
        {
            self.schedule(&rt, clock() + WRITE_THROUGH_RETRY);
            return;
        }
        // THE GOAL A TURN ERROR BLOCKED AT THIS WALL IS THE WALL'S STOP (`wall_goal_after`;
        // owner 2026-10-28: "must keep continuing and making progress with retries"; FOREVER
        // CRUISE and FOREVER LAND): the wall's try waits on its ladder and resumes the goal when
        // due, as a usage-limited goal's resume does. It pinned the park before (attention, no
        // timer, the thread stopped for good).
        let bound = lock(&rt.wall_goal).clone();
        let wall_goal = match bound {
            Some(bound) if decide::wall_try(&decision.act) && self.sup.acting() => {
                match self.thread(rt.id).await {
                    Some(thread) => self.wall_goal_standing(&thread, rt.id, bound).await,
                    None => None,
                }
            }
            _ => None,
        };
        if wall_goal.is_some() {
            self.sup.note(format!(
                "{}: the goal a turn error blocked is the wall's stop; its try resumes it",
                rt.key
            ));
        }
        // A goal owns the pace before a retry wait too. Core may have blocked it when
        // this failure exhausted its retries: surface that stop now, without scheduling
        // a continuation that would only discover the same goal after the deadline.
        // A quota wall's try under a goal core stopped as usage-limited at that wall resumes
        // the goal at the act (the usage window's resume does as much): the stop is the
        // wall's, and taken for a person's it stalled the thread for ever, silent.
        if matches!(decision.act, Act::Type { .. } | Act::Compact)
            && human_goal_resume.is_none()
            && wall_goal.is_none()
            && !matches!(decision.act, Act::Type { rule, .. } if rule == words::RULE_LOGIN_BACK)
            && self.sup.acting()
            && let Some(thread) = self.thread(rt.id).await
            && !(decide::quota_try(&decision.act)
                && goal_status(&thread, rt.id).await
                    == Some(codex_state::ThreadGoalStatus::UsageLimited))
            && self.goal_owns_pace(&rt, &thread, &authority).await
        {
            return;
        }
        // THE QUOTA'S NEED, LOUD AND NEVER BLOCKING: said at once, its try on the ladder all
        // the same; a turn that ends with no wall takes it down. A need of the `owner=needs`
        // entry, never one of the `attention` reasons, whose one shown key could hide it.
        if decide::quota_try(&decision.act) && self.sup.acting() {
            self.need_for(
                &rt,
                &authority,
                "limit",
                Some(decide::QUOTA_NEED.to_string()),
            )
            .await;
        }
        if !self.decision_current(&rt, &authority) {
            return;
        }
        if let Some(at) = decision.not_before
            && at > now
        {
            self.sup.note(format!(
                "{}: wait {}s for {} ({})",
                rt.key,
                at.saturating_duration_since(now).as_secs(),
                decision.act.rule(),
                decision.why
            ));
            if let Some(words) = &decision.shown_wait {
                self.record_wait(&rt, words).await;
            }
            // The announcement can finish after a native turn or an Off retired
            // this point. Its original provider deadline belongs only to this decision.
            // ATERM'S WAITING ROWS SHOW IT (amended 2026-10-27 after the parity sweep, gap 2),
            // once the timer is armed: the entry says this wait's words, and a wait with none
            // takes an earlier wait's words down.
            if self.schedule_decided_act(&rt, &authority, &decision, at, at, true) {
                self.sup.show_waits().await;
            }
            return;
        }
        // A real-work turn can continue immediately, leaving no paced timer for
        // the minute monitor to find. Carry its decided act before starting that
        // next turn; retain this permit so the upgrade cannot race the decision.
        // A wall's try is never carried: its wait holds the upgrade ([`Autopilot::upgrade_facts`]).
        let landing = lock(&rt.state).landing;
        if !already_owed
            && let Some(owed) = owes_of(
                &decision,
                policy.autopilot,
                decision.not_before.map_or(now, |at| at.min(now)),
                landing,
                decision_state.native_origin.clone(),
            )
            .filter(|owed| !decide::is_wall_rule(owed.rule))
        {
            let decided = DecidedUpgradeAct {
                rt: Arc::clone(&rt),
                owed: TrackedOwed::from_state(owed, decision_state.asleep_mark),
                point: decided_at,
                accepted_turns: accepted_at_decision,
            };
            let upgraded = Arc::clone(&self)
                .consider_upgrade_carrying(Some(decided))
                .await;
            if upgraded
                || self.upgrade_requested.load(Ordering::SeqCst)
                || rt.accepted_turns.load(Ordering::SeqCst) != accepted_at_decision
                || lock(&rt.state).point_at() != decided_at
            {
                return;
            }
            if !self.decision_current(&rt, &authority) {
                return;
            }
            // The consideration returned normally, so this evaluator still
            // owns a decided point. Only cancellation leaves it undecided.
            *lock(&rt.decided) = Some(decided_at);
        }
        let act = decision.act.clone();
        match &act {
            Act::Attention { key, text } => {
                // The point stands (an attention answers nothing): while a turn of this thread
                // runs, its end is what the thread stands at next, so an older point says
                // nothing now.
                if self.sup.shared().turn_running_on(&rt.key) {
                    self.sup
                        .note(format!("{}: a turn runs; its end decides", rt.key));
                    return;
                }
                // Attention keeps the point, but retires its former automatic act.
                // Take its timer before yielding, under native registration's gate:
                // a newer turn must never lend this old decision cancellation authority.
                if !self.retire_decided_wait(&rt, &authority).await {
                    return;
                }
                self.attention_for(&rt, key, Some(text.clone())).await;
                return;
            }
            Act::Landed => {
                // FOREVER LAND landed: recorded, never typed; nothing more until someone
                // else's turn. A landing ends the carry-on chain as a person's turn does. A
                // landing the landed task's own wake kept is recorded again, aterm having
                // reopened it at the wake's turn (`ThreadState::reland`).
                let said = if decision_state.reland {
                    decision.why.clone()
                } else {
                    "landed: a clean defense, and the repository holds nothing no remote main \
                     has"
                    .to_string()
                };
                // A cancelled evaluator must not surrender native admission or the upgrade's
                // evaluation permit while its landing request can still reach aterm.
                let publisher = Arc::clone(&self);
                let thread = Arc::clone(&rt);
                let _ = tokio::spawn(async move {
                    let _turn = turn;
                    publisher.record_landed(thread, authority, said).await;
                })
                .await;
                return;
            }
            _ => {}
        }
        #[cfg(test)]
        if let Some((entered, release)) = { lock(&rt.before_ready).take() } {
            let _ = entered.send(());
            let _ = release.await;
        }
        let ready = self.sup.ready().await;
        if !self.decision_current(&rt, &authority) {
            return;
        }
        match ready {
            Ready::Go => {}
            Ready::Wait { until, why } => {
                // A PACED ACT A CHECK HOLDS BACK (a person's hold, a draft, a popup, a status
                // unreadable) is still the act this thread owes, due now: the record carries it
                // while it waits, so an exit aterm relaunches (S3), a crash at rest and the live
                // upgrade hand it on, as for a back-off's wait (parity review finding 4: waited
                // on as no paced act's, the record said `idle` with nothing owed, and aterm's
                // relaunch of a session killed under a hold typed nothing at its lift).
                self.defer_act(&rt, &authority, &decision, until, &why);
                return;
            }
            Ready::Passive(why) => {
                self.sup.note(format!("{}: passive ({why})", rt.key));
                return;
            }
        }
        let Some(thread) = self.thread(rt.id).await else {
            return;
        };
        if !at_rest(&thread.agent_status().await) {
            self.sup
                .note(format!("{}: not idle; its own turn end decides", rt.key));
            return;
        }
        // The record leaves rest BEFORE the act (CONTRACT §4): aterm's watch-only worker may
        // end the process at an `idle` record, and must never do so on a turn this begins.
        let shared = self.sup.shared();
        if restored
            && matches!(act, Act::ResumeLimit { .. })
            && !quota_wall
                .as_ref()
                .is_some_and(|wall| self.quota_restored(&rt, wall, clock()))
        {
            self.schedule(&rt, clock() + WRITE_THROUGH_RETRY);
            return;
        }
        // The carry-on after a relaunch is counted on the record before it goes (with its
        // `busy`), so a process that dies in it hands the next relaunch a chain it must not
        // carry on (MAX_CARRY_ONS); an act that does not go gives the count back.
        let chain = shared.carried_on();
        let carrying = matches!(&act, Act::Type { rule, .. } if *rule == words::RULE_HOST_TURN);
        if carrying {
            let before = shared.predecessor(&rt.key).map_or(0, |p| p.carried_on);
            shared.set_carried_on(before.saturating_add(1));
        }
        let _acting = shared.begin_act();
        if !write_through(shared).await {
            if carrying {
                shared.set_carried_on(chain);
            }
            self.sup.note(format!(
                "{}: the session record did not take busy; the act waits",
                rt.key
            ));
            self.schedule(&rt, clock() + WRITE_THROUGH_RETRY);
            return;
        }
        // The checks said go before the awaits above: an off read since (a standby, a
        // release) stops the act here (review of 4cdc96090, finding 1). The point stands,
        // and the threads look again when codex-alab acts again.
        if !self.sup.acting() {
            if carrying {
                shared.set_carried_on(chain);
            }
            self.sup
                .note(format!("{}: passive since the checks; no act", rt.key));
            return;
        }
        if self.upgrade_requested.load(Ordering::SeqCst) {
            if carrying {
                shared.set_carried_on(chain);
            }
            self.sup.note(format!(
                "{}: the upgrade's exec began since the checks; no act",
                rt.key
            ));
            return;
        }
        match act {
            Act::Type { rule, ref text }
                if rule != words::RULE_LIMIT_RESUME
                    && human_goal_resume.is_none()
                    && wall_goal.is_none() =>
            {
                let started = self
                    .submit(&rt, &thread, &authority, &decision, rule, text.clone())
                    .await;
                if carrying && !started {
                    shared.set_carried_on(chain);
                }
            }
            // The usage window's resume, and a quota wall's try ([`decide::quota_try`]): a
            // usage-limited goal is resumed, else the words are typed. A wall's try whose goal a
            // turn error blocked ([`wall_goal_after`]) resumes that goal.
            Act::Type { ref text, .. } | Act::ResumeLimit { ref text } => {
                if restored
                    && !quota_wall
                        .as_ref()
                        .is_some_and(|wall| self.quota_restored(&rt, wall, clock()))
                {
                    self.schedule(&rt, clock() + WRITE_THROUGH_RETRY);
                    return;
                }
                // The goal's turn can start inside `resume_goal` (the goal runtime continues
                // an idle thread at once) or after it: the record stays busy until it does
                // (CONTRACT §4). `resume_goal` books the try before that turn can begin.
                let awaited = shared.await_turn(&rt.key, ACT_TURN_WAIT);
                let stop = match &wall_goal {
                    Some(goal) => GoalStop::Wall(goal, &act),
                    None => GoalStop::Usage(human_goal_resume.as_ref()),
                };
                // The words a goal that is gone types instead: the wall's own try, else the
                // usage window's resume.
                let rule = match (&wall_goal, &act) {
                    (Some(_), Act::Type { rule, .. }) => *rule,
                    _ => words::RULE_LIMIT_RESUME,
                };
                match self
                    .resume_goal_with_origin(&rt, &thread, &authority, stop)
                    .await
                {
                    GoalResume::Resumed => {
                        self.attention_for(&rt, "goal", None).await;
                        self.sup
                            .record_act(
                                rule,
                                if wall_goal.is_some() {
                                    "resumed the goal a turn error blocked, at the wall's try"
                                } else if human_goal_resume.is_some() {
                                    "resumed the same goal after a successful human Cruise turn"
                                } else if restored {
                                    "resumed the goal after fresh included-usage permission"
                                } else if decide::quota_try(&act) {
                                    "resumed the goal on the quota's ladder"
                                } else {
                                    "resumed the goal at the usage reset"
                                },
                            )
                            .await;
                    }
                    GoalResume::Stopped => {
                        shared.cancel_await(awaited);
                        self.sup.note(format!("{}: goal recovery lost its original goal or available budget; no resume", rt.key));
                        self.wait_over(&rt).await;
                    }
                    GoalResume::TuiHeld { until, why } => {
                        shared.cancel_await(awaited);
                        self.defer_act(&rt, &authority, &decision, until, why);
                    }
                    GoalResume::NotNow => {
                        shared.cancel_await(awaited);
                        self.sup.note(format!(
                            "{}: the goal or its automatic authority changed; no goal resume",
                            rt.key
                        ));
                        self.wait_over(&rt).await;
                        // An edit can leave the goal usage-limited without starting a turn
                        // or another wake. Decide its new state afresh; the rejected
                        // preparation grants no try, and an explicit stop stays quiet. A
                        // wall's goal that moved is decided afresh too: the goal it is now
                        // owns the pace, or none does and the try is typed.
                        if authority.current(&self, &rt)
                            && (wall_goal.is_some()
                                || goal_status(&thread, rt.id).await
                                    == Some(codex_state::ThreadGoalStatus::UsageLimited))
                            && authority.current(&self, &rt)
                        {
                            if wall_goal.is_some() {
                                lock(&rt.wall_goal).take();
                                self.sync_pace(&rt);
                            }
                            self.schedule(&rt, clock());
                        }
                    }
                    GoalResume::NotLimited => {
                        shared.cancel_await(awaited);
                        let _ = self
                            .submit(&rt, &thread, &authority, &decision, rule, text.clone())
                            .await;
                    }
                    GoalResume::Failed(why) => {
                        // The goal still stands stopped (usage-limited, or blocked at the wall),
                        // so a typed turn would only be refused past it: the person is told
                        // which stop did not lift, and it is tried again later.
                        shared.cancel_await(awaited);
                        self.wait_over(&rt).await;
                        let told = if wall_goal.is_some() {
                            wall_goal_resume_failed(&why)
                        } else {
                            goal_resume_failed(&why)
                        };
                        self.attention_for(&rt, "goal", Some(told)).await;
                        self.schedule(&rt, clock() + RECHECK);
                    }
                }
            }
            Act::Compact => {
                // A goal may have taken the thread while the ready/record checks awaited; the
                // goal a turn error blocked at this full context is the wall's stop, and the
                // compaction goes past it (its continuation resumes the goal).
                if wall_goal.is_none() && self.goal_owns_pace(&rt, &thread, &authority).await {
                    return;
                }
                #[cfg(test)]
                if let Some((entered, release)) = { lock(&rt.before_submit).take() } {
                    let _ = entered.send(());
                    let _ = release.await;
                }
                let goal_runtime = thread
                    .thread_extension_data()
                    .get::<codex_goal_extension::GoalRuntimeHandle>();
                let Ok(goal_permit) = self
                    .goal_admission(
                        &rt,
                        &thread,
                        &authority,
                        goal_runtime.as_deref(),
                        wall_goal.as_ref(),
                    )
                    .await
                else {
                    return;
                };
                if !self.decision_current(&rt, &authority) {
                    return;
                }
                if self.defer_if_tui_busy(&rt, &authority, &decision) {
                    return;
                }
                let before = lock(&rt.state).clone();
                lock(&rt.state).acted(&act, now, &self.timing);
                self.sync_pace(&rt);
                // `submit` only queues the compaction: the record stays busy until its turn
                // starts (CONTRACT §4).
                let awaited = shared.await_turn(&rt.key, ACT_TURN_WAIT);
                let submitted = thread.submit(Op::Compact).await;
                // This acknowledges queue acceptance, not native task startup.
                // A later goal mutation follows that accepted operation.
                drop(goal_permit);
                match submitted {
                    Ok(_) => {
                        self.sup
                            .record_act(words::RULE_COMPACT, "compacted a full context")
                            .await;
                    }
                    Err(e) => {
                        shared.cancel_await(awaited);
                        if authority.restore_if_unanswered(&rt, before) {
                            self.surface_needs(&rt);
                        }
                        self.sup.note(format!("{}: compact refused: {e}", rt.key));
                        self.wait_over(&rt).await;
                        // The turn end this act answered is left: under the claim aterm's
                        // own rows stay quiet, so attention says so.
                        self.attention_for(
                            &rt,
                            "turn",
                            Some(turn_left(&format!("compaction refused: {e}"))),
                        )
                        .await;
                    }
                }
            }
            Act::Nothing | Act::Landed | Act::Probe | Act::Attention { .. } => {}
        }
        // A wall's try moved its ladder (or a refused act put it back).
        let _ = self.with_current_runtime(&rt, || {
            self.sync_pace(&rt);
            if lock(&rt.state).point.is_none() {
                self.clear_recovery(&rt.key);
            }
        });
        // A land act began Land's hold or nudged it (or a refused act put it back).
        self.tell_land(&rt);
    }

    #[expect(
        clippy::await_holding_invalid_type,
        reason = "this async gate must retain publication custody through the remote ACK or lease expiry; native turn admission waits on it before registering a task, and no synchronous state lock is held across that wait"
    )]
    async fn record_landed(
        self: Arc<Self>,
        rt: Arc<ThreadRt>,
        authority: DecisionAuthority,
        said: String,
    ) {
        self.tui_caught_up(&rt).await;
        let _publication = self.sup.task_face_publication().await;
        if self.with_current_runtime(&rt, || ()).is_none()
            || !self.decision_current(&rt, &authority)
            || self.sup.shared().exec_pending()
        {
            return;
        }
        let shared = self.sup.shared();
        if shared.pending_user_turn() || shared.queued_input() {
            self.sup.note(format!(
                "{}: the landing waits: a person's turn is on its way",
                rt.key
            ));
            self.schedule(&rt, clock() + WRITE_THROUGH_RETRY);
            return;
        }
        match self
            .sup
            .record_act_before_turn(words::RULE_LANDED, &said, words::LANDED_UNBOUNDED_FROM)
            .await
        {
            BeforeTurn::Recorded => {
                // Keep the registration through the local commit: a stopped runtime cannot
                // clear its replacement's carry count or publish its face. Native admission
                // still holds at the publication gate while the acknowledged point settles.
                let committed = {
                    let kept = lock(&self.threads);
                    if !Self::current_runtime(&kept, &rt)
                        || !authority.current(&self, &rt)
                        || shared.exec_pending()
                        || shared.pending_user_turn()
                        || shared.queued_input()
                    {
                        false
                    } else {
                        lock(&rt.state).acted(&Act::Landed, clock(), &self.timing);
                        self.tell_land_from(&kept, &rt);
                        if shared.is_root_thread(&rt.key) {
                            shared.set_carried_on(0);
                        }
                        true
                    }
                };
                if committed {
                    self.wait_over(&rt).await;
                } else if self.with_current_runtime(&rt, || ()).is_some()
                    && self.decision_current(&rt, &authority)
                {
                    self.schedule(&rt, clock() + WRITE_THROUGH_RETRY);
                }
            }
            // A refused or unanswered publication changes no landing, carry or hold state.
            BeforeTurn::Retry => {
                if self.with_current_runtime(&rt, || ()).is_some()
                    && self.decision_current(&rt, &authority)
                {
                    self.schedule(&rt, clock() + WRITE_THROUGH_RETRY);
                }
            }
            BeforeTurn::TooOld { aterm } => {
                // A claim/host change will rejudge this still-open point. An unsupported
                // host cannot be repaired by polling it every five seconds forever.
                if !self.newer_aterm_told.swap(true, Ordering::SeqCst) {
                    let words = words::landing_needs_newer_aterm(&aterm);
                    self.sup.note(words);
                }
            }
        }
    }

    #[expect(
        clippy::await_holding_invalid_type,
        reason = "this async gate must retain publication custody through the remote ACK or lease expiry; native turn admission waits on it before registering a task, and no synchronous state lock is held across that wait"
    )]
    async fn record_left(self: Arc<Self>, rt: Arc<ThreadRt>, key: (u64, Instant)) {
        // The person's turn's edges go before the point left to them, as a land turn's go
        // before its landing (aterm reopens either at a turn counted past its mark); waited
        // for outside the publication gate, which native turn admission waits on.
        self.tui_caught_up(&rt).await;
        let _publication = self.sup.task_face_publication().await;
        if rt.stopped.load(Ordering::SeqCst)
            || !self.sup.acting()
            || !self
                .sup
                .policy()
                .is_some_and(|policy| policy.autopilot == decide::Mode::Land)
            || self.sup.shared().exec_pending()
            || self.sup.shared().turn_running()
        {
            return;
        }
        let current = {
            let state = lock(&rt.state);
            state.task
                && state.left
                && !state.landed
                && !state.compacting
                && state.point_at() == Some(key.1)
                && rt.accepted_turns.load(Ordering::SeqCst) == key.0
        };
        if !current || *lock(&rt.left_recorded) == Some(key) {
            return;
        }
        match self
            .sup
            .record_act_before_turn(
                words::RULE_LEFT,
                "left to the person: their own exchange; nothing more until their next turn",
                words::LEFT_UNBOUNDED_FROM,
            )
            .await
        {
            BeforeTurn::Recorded => *lock(&rt.left_recorded) = Some(key),
            // Only another look is owed: a failed metadata write never
            // authorizes a turn or continuation in the person's exchange.
            BeforeTurn::Retry => self.schedule(&rt, clock() + WRITE_THROUGH_RETRY),
            // Nothing this aterm can take: the point is settled as it stands (a look owed
            // every few seconds would hold the live upgrade, a timer pending with
            // no paced act). Record the unavailable operation once.
            BeforeTurn::TooOld { aterm } => {
                *lock(&rt.left_recorded) = Some(key);
                if !self.newer_aterm_told.swap(true, Ordering::SeqCst) {
                    let words = words::left_needs_newer_aterm(&aterm);
                    self.sup.note(words);
                }
            }
        }
    }

    fn clear_recovery(&self, thread: &str) {
        let shared = self.sup.shared();
        if shared
            .recovery()
            .is_some_and(|recovery| recovery.thread == thread)
        {
            shared.set_recovery(None);
        }
    }

    async fn sync_recovery(
        &self,
        rt: &ThreadRt,
        decision: &decide::Decision,
        snapshot: &ThreadState,
        generation: u64,
    ) -> bool {
        let shared = self.sup.shared();
        let view = shared.view();
        if view.primary_thread.as_deref() != Some(&rt.key) {
            return true;
        }
        let Some(turn_id) = snapshot.point_turn_id.as_deref() else {
            return true;
        };
        // Sample monotonic first: a pause before the epoch read can only extend
        // the serialized deadline, never shorten its original lower bound.
        let now = clock();
        let epoch = now_ms();
        let measured = matches!(*lock(&rt.net), Some(netwatch::Route::Direct(_)));
        let mut recovery =
            crate::recovery::capture(&rt.key, turn_id, snapshot, decision, measured, now, epoch);
        if let Some(codex_alab_core::session::Recovery {
            wall:
                codex_alab_core::session::RecoveryWall::Usage { quota, .. }
                | codex_alab_core::session::RecoveryWall::Entitlement { quota },
            ..
        }) = &mut recovery
        {
            *quota = self.quota_scope(rt);
        }
        if let Some(recovery) = recovery {
            if shared.recovery().as_ref() != Some(&recovery) {
                let published = shared.publish_recovery(generation, recovery, || {
                    let current = lock(&rt.state);
                    current.point_at() == snapshot.point_at()
                        && current.point_turn_id == snapshot.point_turn_id
                });
                if !published {
                    return false;
                }
            }
            let budget = {
                #[cfg(test)]
                {
                    *lock(&self.recovery_write_budget)
                }
                #[cfg(not(test))]
                {
                    WRITE_THROUGH_BUDGET
                }
            };
            if !write_through_with_budget(shared, budget).await {
                self.sup.note(format!(
                    "{}: could not persist the provider recovery wait",
                    rt.key
                ));
                return false;
            }
        }
        true
    }

    /// `bound` where the thread's goal still stands as the wall left it ([`same_wall_goal`]: no
    /// person's stop of it since it was bound, the same goal, blocked), else `None` (moved,
    /// stopped by a person, unread, or gone).
    async fn wall_goal_standing(
        &self,
        thread: &Arc<CodexThread>,
        id: ThreadId,
        bound: codex_alab_core::session::WallGoal,
    ) -> Option<codex_alab_core::session::WallGoal> {
        let db = thread.state_db()?;
        let current = db.thread_goals().get_thread_goal(id).await.ok().flatten()?;
        let stops = self.sup.shared().person_goal_stops(&id.to_string());
        same_wall_goal(&bound, &current, stops).then_some(bound)
    }

    async fn goal_owns_pace(
        self: &Arc<Self>,
        rt: &Arc<ThreadRt>,
        thread: &Arc<CodexThread>,
        authority: &DecisionAuthority,
    ) -> bool {
        self.goal_read_owns_pace(rt, goal_status_read(thread, rt.id).await, authority)
            .await
    }

    /// Whether a goal owns the thread's pace, read as `read` ([`Self::goal_read_holds`]); where
    /// it does, the go-ahead decided is not typed, and its lines are back on the menu bar
    /// ([`DecisionAuthority::unwritten`]): a goal-driven park's go-ahead is a person's or the
    /// goal's to give, never typed past it.
    async fn goal_read_owns_pace(
        self: &Arc<Self>,
        rt: &Arc<ThreadRt>,
        read: Result<Option<codex_state::ThreadGoalStatus>, String>,
        authority: &DecisionAuthority,
    ) -> bool {
        let owns = self.goal_read_holds(rt, read, authority).await;
        if owns {
            authority.unwritten(self, rt);
        }
        owns
    }

    async fn goal_read_holds(
        self: &Arc<Self>,
        rt: &Arc<ThreadRt>,
        read: Result<Option<codex_state::ThreadGoalStatus>, String>,
        authority: &DecisionAuthority,
    ) -> bool {
        let status = match read {
            Ok(Some(status)) => status,
            Ok(None) => return false,
            Err(error) => {
                self.sup.note(format!(
                    "{}: could not read the goal; nothing is typed past it: {error}",
                    rt.key
                ));
                self.recheck_decided_wait(rt, authority);
                return true;
            }
        };
        if status == codex_state::ThreadGoalStatus::Complete {
            return false;
        }
        // A confirmed goal owns the pace, so the former automatic wait is no
        // longer owed. An unread goal above keeps its independent recheck.
        if !self.retire_decided_wait(rt, authority).await {
            return true;
        }
        self.sup.note(format!(
            "{}: a goal stands ({}); nothing is typed past it",
            rt.key,
            status.as_str()
        ));
        if !self.sup.shared().turn_running_on(&rt.key)
            && let Some(text) = goal_left_to_person(status)
        {
            self.attention_for(rt, "goal", Some(text)).await;
        }
        true
    }

    /// An unread goal may recheck only the native point that needed the read.
    fn recheck_decided_wait(self: &Arc<Self>, rt: &Arc<ThreadRt>, authority: &DecisionAuthority) {
        if self.with_current_runtime(rt, || {
            if !authority.current(self, rt) {
                return false;
            }
            self.arm_timer(rt, clock() + RECHECK, None);
            true
        }) == Some(true)
        {
            self.sync_record_owed();
        }
    }

    /// Retire only the wait owned by this decision. Attention and a confirmed
    /// goal keep the unresolved point, but no longer authorize its paced act.
    async fn retire_decided_wait(
        self: &Arc<Self>,
        rt: &Arc<ThreadRt>,
        authority: &DecisionAuthority,
    ) -> bool {
        let Some((pending, waited)) = self
            .with_current_runtime(rt, || {
                if !authority.current(self, rt) {
                    return None;
                }
                // A firing timer moves its owed act under these same locks.
                let mut slot = lock(&rt.timer);
                lock(&rt.fired).take();
                let pending = slot.take();
                drop(slot);
                // The wait it showed goes with it.
                Some((pending, self.sup.want_wait_for(&rt.key, None)))
            })
            .flatten()
        else {
            return false;
        };
        if let Some(timer) = &pending {
            timer.handle.abort();
        }
        self.sync_record_owed();
        if let Some(timer) = pending {
            let _ = timer.handle.await;
        }
        if waited {
            self.sup.show_waits().await;
        }
        self.decision_current(rt, authority)
    }

    /// Keep the same unspent act while a current hand holds its native admission.
    fn defer_act(
        self: &Arc<Self>,
        rt: &Arc<ThreadRt>,
        authority: &DecisionAuthority,
        decision: &decide::Decision,
        until: Instant,
        why: &str,
    ) {
        if !self.decision_current(rt, authority) {
            return;
        }
        self.sup.note(format!("{}: not now ({why})", rt.key));
        let now = clock();
        // The act still waits (on a draft, a popup, the TUI, a person's hold): the wait shown
        // stands as it was.
        self.schedule_decided_act(
            rt,
            authority,
            decision,
            until,
            decision.not_before.map_or(now, |at| at.min(now)),
            false,
        );
    }

    /// Arm only the native point and live policy that authorized this wait: `true` where it
    /// was armed. `show`: the decision's own wait, whose words (`shown_wait`, or none) become
    /// the thread's `owner=codex-alab-wait` entry, set under the same runtime gate a native
    /// turn's start takes before it takes the entry down, so a turn that starts meanwhile is
    /// never left a wait; a not-now re-arm (`false`) keeps the entry standing.
    fn schedule_decided_act(
        self: &Arc<Self>,
        rt: &Arc<ThreadRt>,
        authority: &DecisionAuthority,
        decision: &decide::Decision,
        at: Instant,
        owed_at: Instant,
        show: bool,
    ) -> bool {
        if !self.decision_current(rt, authority) {
            return false;
        }
        // Native start takes this same runtime gate before advancing its counter,
        // then cancels the old timer. Install only for the point that authorized it.
        let armed = self.with_current_runtime(rt, || {
            let state = lock(&rt.state);
            if state.point_at() != authority.point
                || rt.accepted_turns.load(Ordering::SeqCst) != authority.accepted_turns
            {
                return false;
            }
            let owes = owes_of(
                decision,
                authority.policy.autopilot,
                owed_at,
                state.landing,
                authority.native_origin.clone(),
            );
            self.arm_timer_tracking(rt, at, None, |_| {
                owes.map(|owed| TrackedOwed::from_state(owed, state.asleep_mark))
            });
            drop(state);
            if show {
                self.sup.want_wait_for(
                    &rt.key,
                    decision.shown_wait.clone().map(|words| (at, words)),
                );
            }
            true
        });
        let armed = armed == Some(true);
        if armed {
            self.sync_record_owed();
        }
        armed
    }

    /// THE LAND TURN'S EDGES GO BEFORE ITS LANDING. aterm reopens a landing at any turn its
    /// engine counts past the one `landed@v1` was recorded over (aterm `harness_task.rs`,
    /// `reopen_self_claim_landing`), and the TUI writes a turn's OSC 7501 begin edge as its chat
    /// widget takes the turn's `turn/started`, which a TUI behind under load does after this
    /// evaluation saw the turn end in the core (aterm's codex-alab lab, reland leg, 2026-10-24:
    /// the person's turn's end and the land turn's begin, written after `landed@v1`, reopened
    /// it). While this process's TUI (its frame waker set) shows `rt`'s thread, this waits until
    /// its widget took the end of the thread's latest turn
    /// ([`codex_alab_core::session::Shared::tui_handled_end`]), at most [`TUI_CATCH_UP`]. A TUI
    /// that shows another thread, or switches to one meanwhile, writes this thread's edges only
    /// at its frames, from the autopilot's own book (`turns::Frame::elsewhere`), so none comes
    /// after the turn's end, and the end its widget no longer takes is never booked: nothing
    /// waits for it. No TUI writes no edge.
    async fn tui_caught_up(&self, rt: &ThreadRt) {
        let shared = self.sup.shared();
        let Some(turn) = shared.latest_turn(&rt.key) else {
            return;
        };
        let until = clock() + TUI_CATCH_UP;
        while !shared.tui_handled_end(&rt.key, &turn) {
            if !shared.has_waker() || !shared.displays(&rt.key) {
                return;
            }
            if clock() >= until {
                self.sup.note(format!(
                    "{}: the TUI took no end of turn {turn} in {} s; the act goes on",
                    rt.key,
                    TUI_CATCH_UP.as_secs()
                ));
                return;
            }
            tokio::time::sleep(TUI_CATCH_UP_EVERY).await;
        }
    }

    /// CORE PUBLISHES AN ABORT AFTER ITS HOOKS: `on_turn_abort` runs before core sends the
    /// turn's `TurnAborted`, and the thread's status reads `Running` until that event. The
    /// decision at a person's Esc waits (at most [`ABORT_PUBLISHED`]) for the thread to rest, a
    /// new turn of its own or its stop; decided before, it found the thread busy, left the
    /// point to a turn end that had already come, and parked (seen 1 run in 10 at a grace of 0).
    async fn abort_published(&self, rt: &ThreadRt) {
        let Some(thread) = self.thread(rt.id).await else {
            return;
        };
        let until = clock() + ABORT_PUBLISHED;
        while !at_rest(&thread.agent_status().await)
            && !self.sup.shared().turn_running_on(&rt.key)
            && !rt.stopped.load(Ordering::SeqCst)
            && clock() < until
        {
            tokio::time::sleep(ABORT_PUBLISHED_EVERY).await;
        }
    }

    /// Recheck the TUI after the last ALab await, before issuing native work.
    /// Core retains its own admission semantics once the request is issued.
    fn defer_if_tui_busy(
        self: &Arc<Self>,
        rt: &Arc<ThreadRt>,
        authority: &DecisionAuthority,
        decision: &decide::Decision,
    ) -> bool {
        let Some((until, why)) = self
            .inherited_wait(rt)
            .map(|until| {
                (
                    until,
                    "the resumed automatic intent is settling after startup or sleep",
                )
            })
            .or_else(|| self.sup.tui_wait(authority.policy.human_grace))
        else {
            return false;
        };
        self.defer_act(rt, authority, decision, until, why);
        true
    }

    fn inherited_wait(&self, rt: &ThreadRt) -> Option<Instant> {
        if lock(&rt.inherited_settle).is_none() {
            return None;
        }
        let now = clock();
        let reading = self.wake_reading();
        let since = lock(&rt.inherited_settle).as_mut()?.observe(now, reading);
        let until = since
            .checked_add(self.timing.wake_settle)?
            .checked_add(rt.jitter)?;
        (until > now).then_some(until)
    }

    fn decision_current(
        self: &Arc<Self>,
        rt: &Arc<ThreadRt>,
        authority: &DecisionAuthority,
    ) -> bool {
        if authority.current(self, rt) {
            return true;
        }
        self.sup.note(format!(
            "{}: the policy or native point changed during preparation; decide again",
            rt.key
        ));
        // An active native turn owns the next look at its end; polling its
        // old point immediately would keep scheduling this same rejection.
        if !rt.stopped.load(Ordering::SeqCst)
            && !self.upgrade_requested.load(Ordering::SeqCst)
            && self.sup.acting()
            && !self.sup.shared().turn_running_on(&rt.key)
        {
            self.schedule(rt, clock());
        }
        false
    }

    /// Keep the final saved-goal read and automatic Core acceptance ordered with
    /// external goal mutation. The caller retains the runtime for the borrowed permit.
    async fn goal_admission<'a>(
        self: &Arc<Self>,
        rt: &Arc<ThreadRt>,
        thread: &Arc<CodexThread>,
        authority: &DecisionAuthority,
        runtime: Option<&'a codex_goal_extension::GoalRuntimeHandle>,
        wall_goal: Option<&codex_alab_core::session::WallGoal>,
    ) -> Result<Option<tokio::sync::SemaphorePermit<'a>>, ()> {
        let permit = match runtime {
            Some(runtime) => match runtime.alab_automatic_input_permit().await {
                Ok(permit) => Some(permit),
                Err(error) => {
                    self.sup.note(format!(
                        "{}: could not guard goal admission; automatic work waits: {error}",
                        rt.key
                    ));
                    self.recheck_decided_wait(rt, authority);
                    authority.unwritten(self, rt);
                    return Err(());
                }
            },
            None if self.goals.is_some() && thread.state_db().is_some() => {
                self.sup.note(format!(
                    "{}: the goal runtime is unavailable; automatic work waits",
                    rt.key
                ));
                self.recheck_decided_wait(rt, authority);
                authority.unwritten(self, rt);
                return Err(());
            }
            None => None,
        };
        // The goal a turn error blocked at the wall this act answers admits it while it stands
        // as that wall left it ([`wall_goal_after`]).
        if let Some(bound) = wall_goal
            && self
                .wall_goal_standing(thread, rt.id, bound.clone())
                .await
                .is_some()
        {
            return Ok(permit);
        }
        let goal = goal_status_read(thread, rt.id).await;
        if !matches!(
            &goal,
            Ok(None) | Ok(Some(codex_state::ThreadGoalStatus::Complete))
        ) {
            // Attention transport must not hold up another goal mutation.
            drop(permit);
            self.goal_read_owns_pace(rt, goal, authority).await;
            return Err(());
        }
        Ok(permit)
    }

    /// ATERM'S FACT FOR AN ACT UNDER `rule` TYPED INTO `rt` (CONTRACT §5 "aterm fact", gap 23),
    /// read only where all of these hold: `rule` is one of [`words::FACT_RIDES`] (a pre-filter:
    /// aterm judges every read), the decision's policy has `facts` on, the thread stands under no
    /// cybersecurity refusal's exclusion (its automatic prompts keep ending on
    /// [`words::POLICY_BOUNDARY`], with nothing after it), and the thread runs a background
    /// terminal or is the only live root thread the TUI registered (a title's or a memory's
    /// thread, which the person never sees, is none): aterm's jobs board keys calls by tab, and a
    /// background terminal names Codex's exec `process_id`, not the OS pid aterm's sentence names,
    /// so no closer match is possible. The reader is the thread's key, the wall its track's class
    /// word (`-` with none).
    async fn fact_for(
        &self,
        rt: &ThreadRt,
        thread: &CodexThread,
        rule: &str,
        policy: &decide::Policy,
    ) -> Option<codex_alab_aterm::Fact> {
        if !words::FACT_RIDES.contains(&rule) || !policy.facts {
            return None;
        }
        let wall = {
            let state = lock(&rt.state);
            if state.policy_continued {
                return None;
            }
            state
                .wall
                .as_ref()
                .map_or("-", decide::WallTrack::class_word)
        };
        let shared = self.sup.shared();
        let alone = !self.live_threads().iter().any(|other| {
            other.key != rt.key
                && !other.stopped.load(Ordering::SeqCst)
                && shared.is_root_thread(&other.key)
        });
        if !alone && thread.list_background_terminals().await.is_empty() {
            self.sup.note(format!(
                "{}: fact: no background terminal beside another root thread ({rule})",
                rt.key
            ));
            return None;
        }
        self.sup.fact_for(rule, &rt.key, wall).await
    }

    async fn submit(
        self: &Arc<Self>,
        rt: &Arc<ThreadRt>,
        thread: &Arc<CodexThread>,
        authority: &DecisionAuthority,
        decision: &decide::Decision,
        rule: &'static str,
        text: String,
    ) -> bool {
        let why = &decision.why;
        let goal_runtime = thread
            .thread_extension_data()
            .get::<codex_goal_extension::GoalRuntimeHandle>();
        let auth_wall = (rule == words::RULE_LOGIN_BACK)
            .then(|| {
                authority
                    .native_origin
                    .as_ref()
                    .map(|origin| origin.turn_id.clone())
            })
            .flatten();
        if rule == words::RULE_LOGIN_BACK && auth_wall.is_none() {
            self.recheck_decided_wait(rt, authority);
            return false;
        }
        let auth_context = if let Some(wall) = auth_wall.as_deref() {
            match self.renewal_goal_for_intent(rt, thread, wall).await {
                Ok(context)
                    if context.is_none()
                        || goal_runtime
                            .as_ref()
                            .is_some_and(|runtime| runtime.alab_auth_recovery_enabled()) =>
                {
                    Some(context)
                }
                _ => {
                    self.recheck_decided_wait(rt, authority);
                    return false;
                }
            }
        } else {
            None
        };
        let auth_goal = auth_context.is_some();
        let auth_goal_id = auth_context.flatten();
        let mode = thread
            .thread_settings_snapshot()
            .await
            .collaboration_mode
            .mode;
        if mode == ModeKind::Plan {
            self.sup
                .note(format!("{}: Plan mode: nothing is typed into it", rt.key));
            self.wait_over(rt).await;
            // The claim keeps aterm's own rows quiet, so a turn end codex-alab leaves is a
            // person's only once attention says so (CONTRACT §5 "left boxes").
            self.attention_for(
                rt,
                "plan",
                Some(format!(
                    "plan: Plan mode leaves this turn's end to a person ({why})"
                )),
            )
            .await;
            authority.unwritten(self, rt);
            return false;
        }
        // A goal that is not complete owns the thread's pace: Active continues itself;
        // Paused, Blocked, UsageLimited and BudgetLimited were stopped by a person, the
        // goal or the owner's budget, and a plain turn would override that.
        if !auth_goal && self.goal_owns_pace(rt, thread, authority).await {
            return false;
        }
        // ATERM'S FACT FOR THIS ACT (CONTRACT §5 "aterm fact", gap 23): read after the act's
        // checks and before goal admission, so no goal permit is held across the read. The rider
        // goes after the act's words and standing rules whole, or not at all.
        let ridden = match self.fact_for(rt, thread, rule, &authority.policy).await {
            Some(fact) => match words::with_fact(&text, &fact.rider) {
                Some(said) => {
                    self.sup.note(format!(
                        "{}: fact {} {:016x} rides {rule}",
                        rt.key, fact.kind, fact.episode
                    ));
                    Some((said, fact))
                }
                None => {
                    self.sup.note(format!(
                        "{}: fact {} {:016x} would pass {} units: {rule} typed without it",
                        rt.key,
                        fact.kind,
                        fact.episode,
                        words::VERBATIM_UNITS
                    ));
                    None
                }
            },
            None => None,
        };
        #[cfg(test)]
        if let Some((entered, release)) = { lock(&rt.before_submit).take() } {
            let _ = entered.send(());
            let _ = release.await;
        }
        let Ok(goal_permit) = (if auth_goal {
            Ok(None)
        } else {
            self.goal_admission(rt, thread, authority, goal_runtime.as_deref(), None)
                .await
        }) else {
            return false;
        };
        // Validate after every ALab preparatory await. Native core retains its
        // existing admission semantics once this request is issued.
        if !self.decision_current(rt, authority) {
            authority.unwritten(self, rt);
            return false;
        }
        if self.defer_if_tui_busy(rt, authority, decision) {
            return false;
        }
        let n = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
        let policy_scope = rule == words::RULE_POLICY_CONTINUE || lock(&rt.state).policy_continued;
        let now = clock();
        let before = lock(&rt.state).clone();
        {
            let mut state = lock(&rt.state);
            if !auth_goal {
                state.acted_on(decision, now, &self.timing);
            }
            // A hold the act begins is the act's own time's (aterm's ledger stamp).
            state.stamp_land_hold(now_ms());
        }
        // THE ACT'S PACE GOES WITH IT (a wall's rung and its owed answer, the hour's budget):
        // written through with the act, before its turn begins, as aterm's books hold the act
        // as it is typed; at the evaluation's end it reached the record only after the turn's
        // start (a refused act puts it back there).
        self.sync_pace(rt);
        // THE UPGRADE'S NOTICE COUNTS AS IT GOES (`upgrade_ask`; gap 27), before its turn can
        // end and be heard ([`upgrade_ask::Round::heard`]); a turn that does not start gives it
        // back.
        let round_before = (rule == words::RULE_UPGRADE_NOTICE).then(|| {
            let mut round = lock(&self.ask);
            let before = round.clone();
            round.noticed(&text, now);
            before
        });
        #[cfg(test)]
        if let Some((entered, release)) = { lock(&rt.before_start).take() } {
            let _ = entered.send(());
            let _ = release.await;
        }
        // THE LAST STEP BEFORE THE TURN REQUEST (CONTRACT §5 "aterm fact"): aterm's `tells_at` on
        // the reading, on this host's clock. No await stands between it and the request; a rider
        // gone stale is left off, and nothing is acknowledged.
        let (text, fact) = match ridden {
            Some((said, fact)) if fact.tells_at(i64::try_from(now_ms()).unwrap_or(i64::MAX)) => {
                (said, Some(fact))
            }
            Some((_, fact)) => {
                self.sup.note(format!(
                    "{}: fact {} {:016x} gone stale before the start: {rule} typed without it",
                    rt.key, fact.kind, fact.episode
                ));
                (text, None)
            }
            None => (text, None),
        };
        let request = TurnInputRequest::new(TurnInput::UserInput {
            content: vec![UserInput::Text {
                text: text.clone(),
                text_elements: Vec::new(),
            }],
            client_id: Some(format!("{CLIENT_PREFIX}{n}")),
        })
        .on_start(TurnStartOptions {
            turn_trigger: Some(TURN_TRIGGER.to_string()),
            cyber_access_program: policy_scope
                .then(|| *lock(&rt.cyber_access_program))
                .flatten(),
            ..Default::default()
        });
        let submitted = if auth_goal {
            match auth_wall.clone() {
                Some(wall) => {
                    thread
                        .recover_turn_if_idle(self.same_file_auth_request(
                            rt,
                            authority,
                            goal_runtime.clone(),
                            wall,
                            auth_goal_id,
                            decision.act.clone(),
                        ))
                        .await
                }
                _ => Err(codex_protocol::error::CodexErr::Fatal(
                    "auth recovery context missing".into(),
                )),
            }
        } else {
            thread.start_turn_if_idle(request).await
        };
        // Admission is complete; aterm receipts do not retain goal custody.
        drop(goal_permit);
        if !matches!(submitted, Ok(StartIfIdleSubmission::Started { .. }))
            && let Some(before) = round_before
        {
            lock(&self.ask).withdraw(before);
        }
        match submitted {
            Ok(StartIfIdleSubmission::Started { turn_id, .. }) => {
                *lock(&rt.announced) = None;
                self.sync_owed();
                self.tell_land(rt);
                // The act's message-log record keeps its fact, its why and its turn whole: where
                // it would pass aterm's 512 bytes, the act's own words give way to `…` before them.
                let rider = fact.as_ref().map_or("", |fact| fact.rider.as_str());
                let head = text.strip_suffix(rider).unwrap_or(&text);
                let recorded = self
                    .sup
                    .record_act_acknowledged(
                        rule,
                        &format!("typed \"{head}"),
                        &format!("{rider}\" ({why}; turn {turn_id})"),
                    )
                    .await;
                // The fact's ack goes once the turn started and its act was recorded (the act's
                // record carries the rider: the fact's one Messages entry).
                if let Some(fact) = &fact {
                    if recorded {
                        self.sup.fact_told(fact).await;
                    } else {
                        self.sup.note(format!(
                            "fact {} {:016x} not told: the act was not recorded",
                            fact.kind, fact.episode
                        ));
                    }
                }
                true
            }
            Ok(StartIfIdleSubmission::NotSubmitted { reason }) => {
                if authority.restore_if_unanswered(rt, before) {
                    self.surface_needs(rt);
                }
                if auth_goal {
                    self.recheck_decided_wait(rt, authority);
                    return false;
                }
                self.sup
                    .note(format!("{}: not submitted: {reason:?}", rt.key));
                self.wait_over(rt).await;
                if let Some((key, text)) = not_submitted_attention(&reason, why) {
                    self.attention_for(rt, key, Some(text)).await;
                }
                false
            }
            Err(e) => {
                if authority.restore_if_unanswered(rt, before) {
                    self.surface_needs(rt);
                }
                if auth_goal {
                    self.recheck_decided_wait(rt, authority);
                    return false;
                }
                self.sup.note(format!("{}: submit failed: {e}", rt.key));
                self.wait_over(rt).await;
                self.attention_for(
                    rt,
                    "turn",
                    Some(turn_left(&format!("the turn did not start: {e}"))),
                )
                .await;
                false
            }
        }
    }

    /// A goal stopped at the usage wall, resumed as `thread/goal/set` would at the reset.
    ///
    /// THE TRY IS BOOKED UNDER THE GOAL SERVICE'S MUTATION PERMIT, before it changes the goal
    /// ([`decide::ThreadState::goal_resumed`], its pace written through), as `submit` books a
    /// typed act before its turn starts: the
    /// goal runtime starts the goal's continuation inside `apply_runtime_effects` when the
    /// thread is idle, and that turn's start is accepted (this autopilot's `on_turn_start`)
    /// before the call returns. Booked after it, the point had moved on, the try was never
    /// counted, and a quota's ladder never climbed: each wall the goal's turn met was tried
    /// again at once (review of CX-B, CXR-1). A refused resume gives the booking back.
    #[cfg(test)]
    async fn resume_goal(
        self: &Arc<Self>,
        rt: &Arc<ThreadRt>,
        thread: &Arc<CodexThread>,
        authority: &DecisionAuthority,
    ) -> GoalResume {
        self.resume_goal_with_origin(rt, thread, authority, GoalStop::Usage(None))
            .await
    }

    async fn resume_goal_with_origin(
        self: &Arc<Self>,
        rt: &Arc<ThreadRt>,
        thread: &Arc<CodexThread>,
        authority: &DecisionAuthority,
        stop: GoalStop<'_>,
    ) -> GoalResume {
        let Some(db) = thread.state_db() else {
            return GoalResume::NotLimited;
        };
        // The stop the resume lifts, read again under the goal's mutation permit below: a goal
        // paused, replaced or edited meanwhile is no longer it.
        let stopped_here =
            |original: &codex_state::ThreadGoal, current: &codex_state::ThreadGoal| match stop {
                GoalStop::Usage(_) => same_limited_goal(current, original),
                GoalStop::Wall(bound, _) => {
                    same_wall_goal(bound, current, self.sup.shared().person_goal_stops(&rt.key))
                }
            };
        let original = match (db.thread_goals().get_thread_goal(rt.id).await, stop) {
            (Ok(Some(goal)), GoalStop::Usage(_))
                if goal.status == codex_state::ThreadGoalStatus::UsageLimited =>
            {
                goal
            }
            (Ok(Some(goal)), GoalStop::Wall(bound, _))
                if same_wall_goal(bound, &goal, self.sup.shared().person_goal_stops(&rt.key)) =>
            {
                goal
            }
            (Ok(_), GoalStop::Usage(Some(_))) => return GoalResume::Stopped,
            (Ok(_), GoalStop::Usage(None)) => return GoalResume::NotLimited,
            (Ok(_), GoalStop::Wall(..)) => return GoalResume::NotNow,
            (Err(error), _) => {
                return GoalResume::Failed(format!("could not read the goal: {error}"));
            }
        };
        if let GoalStop::Usage(Some(goal)) = stop
            && !same_limited_goal(goal, &original)
        {
            return GoalResume::Stopped;
        }
        let origin = match stop {
            GoalStop::Usage(origin) => origin,
            GoalStop::Wall(..) => None,
        };
        let Some(goals) = &self.goals else {
            return GoalResume::Failed("this build has no goal service".to_string());
        };
        #[cfg(test)]
        if let Some((entered, release)) = { lock(&rt.before_submit).take() } {
            let _ = entered.send(());
            let _ = release.await;
        }
        if !self.decision_current(rt, authority) {
            return GoalResume::NotNow;
        }
        let request = GoalSetRequest {
            thread_id: rt.id,
            objective: GoalObjectiveUpdate::Keep,
            status: Some(codex_protocol::protocol::ThreadGoalStatus::Active),
            token_budget: GoalTokenBudgetUpdate::Keep,
            max_goal_token_budget: rt.config.as_ref().and_then(|c| c.max_goal_token_budget),
        };
        let mut changed = false;
        let mut budget_stopped = false;
        let mut tui_wait = None;
        let mut before = None;
        // GoalService runs this future under the runtime's mutation permit, after any
        // earlier person's set has finished. Its initial read above is no authority to
        // reactivate a goal that was paused, replaced or edited while the permit waited.
        let result = goals
            .set_thread_goal(&db, request, async {
                let current = db
                    .thread_goals()
                    .get_thread_goal(rt.id)
                    .await
                    .map_err(|error| {
                        GoalServiceError::Internal(format!("could not recheck the goal: {error}"))
                    })?;
                if !current
                    .as_ref()
                    .is_some_and(|goal| stopped_here(&original, goal))
                    || !authority.current(self, rt)
                {
                    changed = true;
                    return Err(GoalServiceError::InvalidRequest(
                        "the goal or native decision changed during the resume".into(),
                    ));
                }
                if current.as_ref().is_some_and(|goal| {
                    goal.token_budget
                        .is_some_and(|budget| goal.tokens_used >= budget)
                }) {
                    budget_stopped = true;
                    return Err(GoalServiceError::InvalidRequest(
                        "the goal budget is exhausted".into(),
                    ));
                }
                if let Some(wait) = self
                    .inherited_wait(rt)
                    .map(|until| {
                        (
                            until,
                            "the resumed automatic intent is settling after startup or sleep",
                        )
                    })
                    .or_else(|| self.sup.tui_wait(authority.policy.human_grace))
                {
                    tui_wait = Some(wait);
                    return Err(GoalServiceError::InvalidRequest(
                        "new TUI input holds automatic goal resume".into(),
                    ));
                }
                before = Some(lock(&rt.state).clone());
                {
                    let mut state = lock(&rt.state);
                    match stop {
                        GoalStop::Usage(_) => state.goal_resumed(clock(), &self.timing),
                        // The wall's try, taken by the goal's own turn: its ladder climbs.
                        GoalStop::Wall(_, act) => {
                            state.wall_goal_resumed(act, clock(), &self.timing);
                        }
                    }
                    state.stamp_land_hold(now_ms());
                }
                self.sync_pace(rt);
                // The try is booked: a store a test has fail returns its error here, as a
                // goal write that fails after this record does, the goal unchanged.
                self.goal_set_fault()
            })
            .await;
        match result {
            Ok(outcome) => {
                self.tell_land(rt);
                outcome.apply_runtime_effects(goals).await;
                if outcome.goal.status == codex_protocol::protocol::ThreadGoalStatus::Active {
                    GoalResume::Resumed
                } else {
                    // Native storage atomically clamps Active to BudgetLimited if a
                    // concurrent accounting charge exhausts it after our callback.
                    GoalResume::Stopped
                }
            }
            Err(e) => {
                if let Some((until, why)) = tui_wait {
                    return GoalResume::TuiHeld { until, why };
                }
                if budget_stopped || (changed && origin.is_some()) {
                    return GoalResume::Stopped;
                }
                if changed {
                    return GoalResume::NotNow;
                }
                if let Some(before) = before {
                    // The needs come back on the menu bar outside the thread list's lock, which
                    // showing them takes.
                    let unwritten = self.with_current_runtime(rt, || {
                        let unwritten = authority.restore_if_unanswered(rt, before);
                        self.sync_pace(rt);
                        unwritten
                    });
                    if unwritten == Some(true) {
                        self.surface_needs(rt);
                    }
                }
                self.sup
                    .note(format!("{}: goal resume failed: {e}", rt.key));
                GoalResume::Failed(e.to_string())
            }
        }
    }

    /// Say in aterm's description what this root thread is doing: its goal (any but a
    /// complete one) and the plan step in progress ([`crate::describe`]). Only while the claim
    /// is held; the supervisor writes once per change.
    pub(crate) async fn refresh_description(&self, rt: &ThreadRt) {
        let request = self
            .with_current_runtime(rt, || {
                if !codex_alab_core::active()
                    || !self.sup.shared().is_root_thread(&rt.key)
                    || self.sup.policy().is_none()
                {
                    return None;
                }
                let request = Arc::new(());
                *lock(&rt.description_request) = Arc::clone(&request);
                Some(request)
            })
            .flatten();
        let Some(request) = request else {
            return;
        };
        let Some(thread) = self.thread(rt.id).await else {
            return;
        };
        let goal = match goal_objective(&thread, rt.id).await {
            Ok(goal) => goal,
            Err(error) => {
                self.sup.note(format!(
                    "{}: description unchanged: goal unreadable: {error}",
                    rt.key
                ));
                return;
            }
        };
        #[cfg(test)]
        {
            let read = lock(&rt.after_description_read).take();
            if let Some((reached, resume)) = read {
                let _ = reached.send(());
                let _ = resume.await;
            }
        }
        let wanted = self.with_current_runtime(rt, || {
            if !codex_alab_core::active()
                || !self.sup.shared().is_root_thread(&rt.key)
                || !Arc::ptr_eq(&lock(&rt.description_request), &request)
            {
                return false;
            }
            let step = lock(&rt.plan_step).clone();
            let text = crate::describe::describe(goal.as_deref(), step.as_deref());
            self.sup.want_description_for(&rt.description_owner, text)
        });
        if wanted == Some(true) {
            self.sup.publish_description().await;
        }
    }

    /// A turn of this root thread started: a goal that reads active again (a person's `/goal`
    /// resumed it, or the usage reset did) has answered the `goal:` attention a stopped goal
    /// raised, which no person's turn end would clear (review of the inbox, finding 8).
    async fn goal_moved(&self, rt: &ThreadRt, accepted_turns: u64) {
        if !codex_alab_core::active() || !self.sup.shared().is_root_thread(&rt.key) {
            return;
        }
        let snapshot = self
            .with_current_runtime(rt, || {
                (rt.accepted_turns.load(Ordering::SeqCst) == accepted_turns)
                    .then(|| self.sup.attention_snapshot_for(&rt.key, "goal"))
            })
            .flatten();
        let Some(snapshot) = snapshot else {
            return;
        };
        let Some(thread) = self.thread(rt.id).await else {
            return;
        };
        let status = goal_status(&thread, rt.id).await;
        #[cfg(test)]
        let finished = {
            let gate = lock(&rt.after_goal_read).take();
            if let Some(gate) = gate {
                let _ = gate.read.send(status);
                let _ = gate.resume.await;
                Some(gate.finished)
            } else {
                None
            }
        };
        if goal_answers_attention(status) {
            let client = self
                .with_current_runtime(rt, || {
                    if rt.accepted_turns.load(Ordering::SeqCst) != accepted_turns
                        || !self.sup.shared().is_root_thread(&rt.key)
                    {
                        return None;
                    }
                    self.sup.want_clear_attention(&snapshot)
                })
                .flatten();
            self.sup.publish_attention(client).await;
        }
        #[cfg(test)]
        if let Some(finished) = finished {
            let _ = finished.send(());
        }
    }

    /// Record a hold once per change and publish its machine-readable state. Ordinary holds
    /// stay quiet; only the standing go-ahead's bounded refusal raises its one actionable ask.
    async fn tell_held(
        &self,
        rt: &ThreadRt,
        authority: &DecisionAuthority,
        held: Option<crate::replies::Told>,
    ) {
        use crate::replies::GO_AHEAD_STOP_KEY;
        let ask = held.as_ref().and_then(crate::replies::Told::ask);
        let arm = held.as_ref().and_then(crate::replies::Told::arm);
        let Some((changed, arm_moved, client)) = self
            .with_current_runtime(rt, || {
                if !authority.current(self, rt) {
                    return None;
                }
                let arm_moved = self.sup.want_held_for(&rt.key, arm);
                let mut told = lock(&rt.held_told);
                let changed = *told != held;
                let attention_moved = if changed {
                    let before = std::mem::replace(&mut *told, held.clone());
                    ask.is_some()
                        || before
                            .as_ref()
                            .and_then(crate::replies::Told::ask)
                            .is_some()
                } else {
                    ask.is_some() && !self.sup.attention_stands(&rt.key, GO_AHEAD_STOP_KEY)
                };
                let client = attention_moved
                    .then(|| {
                        self.sup
                            .want_attention_for(&rt.key, GO_AHEAD_STOP_KEY, ask.clone())
                    })
                    .flatten();
                Some((changed, arm_moved, client))
            })
            .flatten()
        else {
            return;
        };
        self.sup.publish_attention(client).await;
        if arm_moved {
            self.sup.show_held().await;
        }
        if changed && let Some(held) = held {
            self.sup.appnotice(&held.line()).await;
        }
    }

    /// LAND'S HOLD IN ATERM'S LANDING (amended 2026-10-28 after the parity sweep, gap 11: a
    /// codex-alab worker under Land showed no landing in aterm): the thread's hold as its memory
    /// says it now ([`decide::ThreadState::land_held`]), handed to the supervisor under the
    /// thread's state lock, so no older reading lands after a newer one, and only from the
    /// runtime that owns the registration (a stopped one's is taken out by its stop); the
    /// supervisor sends the tab's once per change, on a task of its own.
    pub(crate) fn tell_land(&self, rt: &ThreadRt) {
        let kept = lock(&self.threads);
        self.tell_land_from(&kept, rt);
    }

    /// [`Self::tell_land`] for a caller that holds the registrations already. The line's task
    /// face goes with it ([`Self::publish_face`]): every move of a landing or a left point is
    /// one this tells.
    fn tell_land_from(&self, kept: &[Weak<ThreadRt>], rt: &ThreadRt) {
        if rt.stopped.load(Ordering::SeqCst) || !Self::current_runtime(kept, rt) {
            return;
        }
        let moved = {
            let state = lock(&rt.state);
            self.sup.want_land_for(&rt.key, state.land_held())
        };
        if moved {
            self.sup.show_land_later();
        }
        self.publish_face(kept, rt);
    }

    /// THE LINE'S TASK FACE FOR THIS THREAD (gap 12 of the parity sweep, 2026-10-29: a codex tab
    /// showed none of aterm's faces): its memory's landing or left point
    /// ([`decide::ThreadState::task_face`], none while a turn of it runs), published for the
    /// TUI's line. THE REGISTRATIONS' LOCK (`_kept`, [`Autopilot::threads`]) orders it: every
    /// caller holds it, and so does every move of the running book (`Shared::turn_started`,
    /// `turn_ended`, `thread_ended`), so the running turn read here and the memory read under the
    /// thread's state lock are of one moment, and no older reading lands after a newer one
    /// (review of gap 12, finding 8: this said the state lock ordered it). Whether a turn runs
    /// is read first: no `Shared` lock is taken under the state's but the faces' own.
    fn publish_face(&self, _kept: &[Weak<ThreadRt>], rt: &ThreadRt) {
        let shared = self.sup.shared();
        let running = shared.turn_running_on(&rt.key);
        let state = lock(&rt.state);
        shared.set_task_face(&rt.key, state.task_face(running));
    }

    /// Clear the recorded hold when the point or its arm changes. A go-ahead refusal's ask is
    /// point-scoped, so a new point takes it down before that point's decision can raise it anew.
    fn point_hold_after(
        state: &ThreadState,
        rt: &ThreadRt,
    ) -> (bool, Option<crate::replies::HeldBy>) {
        let arm = state.point_held_arm();
        let mut told = lock(&rt.held_told);
        let drop_told = state.point_held_by().is_none()
            || told
                .as_ref()
                .is_some_and(|prev| prev.arm() != arm || prev.ask().is_some());
        let ask_lifted = drop_told && told.take().is_some_and(|prev| prev.ask().is_some());
        (ask_lifted, arm)
    }

    /// Record automatic waits without leaving a persistent TUI warning.
    async fn record_wait(&self, rt: &ThreadRt, words: &str) {
        {
            let mut announced = lock(&rt.announced);
            if announced.as_deref() == Some(words) {
                return;
            }
            *announced = Some(words.to_string());
        }
        self.sup.record_wait(words).await;
    }

    /// THE THREAD'S DECISION WAITS FOR NOTHING NOW (nothing to type, a hold, an attention, a
    /// landing, an act that did not go): the wait aterm shows for it comes down. Only the
    /// current runtime's: a superseded one's late outcome never takes down its successor's.
    async fn wait_over(&self, rt: &ThreadRt) {
        if self.with_current_runtime(rt, || self.sup.want_wait_for(&rt.key, None)) == Some(true) {
            self.sup.show_waits().await;
        }
    }

    /// Look again at `at` (one pending timer per thread; a newer one replaces it).
    pub(crate) fn schedule(self: &Arc<Self>, rt: &Arc<ThreadRt>, at: Instant) {
        self.schedule_act(rt, at, None);
    }

    /// Look again at `at`, for the paced act under `owes` where it waits for one (the live
    /// upgrade carries that act instead of waiting on it).
    pub(crate) fn schedule_act(
        self: &Arc<Self>,
        rt: &Arc<ThreadRt>,
        at: Instant,
        owes: Option<Owed>,
    ) {
        self.arm_timer(rt, at, owes);
        self.sync_record_owed();
    }

    fn arm_timer(self: &Arc<Self>, rt: &Arc<ThreadRt>, at: Instant, owes: Option<Owed>) {
        self.arm_timer_if_unchanged(rt, at, owes, None);
    }

    /// A rollback may fill only the timer slot it took, never a newer wait or fired act.
    fn arm_timer_if_unchanged(
        self: &Arc<Self>,
        rt: &Arc<ThreadRt>,
        at: Instant,
        owes: Option<Owed>,
        expected_generation: Option<u64>,
    ) -> bool {
        self.arm_timer_tracking(rt, at, expected_generation, |armed| {
            owes.map(|owed| TrackedOwed::new(owed, armed))
        })
    }

    fn arm_timer_tracking(
        self: &Arc<Self>,
        rt: &Arc<ThreadRt>,
        at: Instant,
        expected_generation: Option<u64>,
        prepare: impl FnOnce(Option<codex_alab_core::wake::Reading>) -> Option<TrackedOwed>,
    ) -> bool {
        let mut slot = lock(&rt.timer);
        if expected_generation.is_some_and(|expected| {
            rt.timer_seq.load(Ordering::SeqCst) != expected
                || slot.is_some()
                || lock(&rt.fired).is_some()
        }) {
            return false;
        }
        let generation = rt.timer_seq.fetch_add(1, Ordering::SeqCst) + 1;
        let me = Arc::clone(self);
        let runtime = Arc::clone(rt);
        // A WAIT OF REAL TIME (2026-10-24): the host's sleep counts toward it, so a wait the host
        // slept through looks again soon after the wake (where its decision settles and measures
        // first), never once the awake clock has made up its remainder. A decided deadline
        // retains its selection before any announcement awaited; a new recheck starts here.
        let read = lock(&self.wake).clone();
        let armed = read();
        let mut owes = prepare(armed);
        if let Some(owed) = owes.as_mut() {
            owed.establish(armed);
        }
        // The act's own deadline can predate an announcement's await. A separate
        // readiness recheck has its own, newly armed origin.
        let shares_deadline = owes.as_ref().is_some_and(|owed| owed.act.not_before == at);
        let armed = owes
            .as_ref()
            .filter(|_| shares_deadline)
            .map_or(armed, |owed| owed.selected);
        let counts_sleep = armed.is_some() || codex_alab_core::wake::supported();
        let handle = tokio::spawn(async move {
            let deadline = tokio::time::Instant::from_std(at);
            if counts_sleep {
                let mut has_baseline = armed.is_some();
                codex_alab_core::wake::sleep_until_with(
                    deadline,
                    codex_alab_core::wake::POLL,
                    armed,
                    || {
                        let reading = read();
                        let mut slot = lock(&runtime.timer);
                        let mut selected = None;
                        if let Some(timer) =
                            slot.as_mut().filter(|timer| timer.generation == generation)
                            && let Some(owed) = timer.owes.as_mut()
                        {
                            owed.establish(reading);
                            selected = owed.selected;
                        }
                        // A record sync may have established the first valid
                        // baseline while this initially unread wait was parked.
                        if shares_deadline && !has_baseline && selected.is_some() {
                            has_baseline = true;
                            return selected;
                        }
                        has_baseline |= reading.is_some();
                        reading
                    },
                )
                .await;
            } else {
                tokio::time::sleep_until(deadline).await;
            }
            {
                let mut slot = lock(&runtime.timer);
                if !slot.as_ref().is_some_and(|t| t.generation == generation) {
                    return;
                }
                // Out of the slot before acting: a turn this timer starts must not cancel
                // (and join) the very task starting it. The act it waits for stays readable
                // until an evaluation holds the thread's permit.
                if let Some(timer) = slot.take() {
                    *lock(&runtime.fired) = timer.owes;
                }
                runtime.ask_again();
            }
            me.evaluate(runtime).await;
        });
        let timer = Timer {
            generation,
            handle,
            owes,
        };
        if let Some(old) = slot.replace(timer) {
            old.handle.abort();
        }
        drop(slot);
        true
    }

    /// Cancel and join the pending timer (and forget the act a timer that fired left: a turn
    /// that starts, or a thread that stops, moved the point it was for).
    pub(crate) async fn cancel_timer(rt: &ThreadRt) {
        lock(&rt.fired).take();
        let pending = lock(&rt.timer).take();
        if let Some(timer) = pending {
            timer.handle.abort();
            let _ = timer.handle.await;
        }
    }

    /// The `Arc` this extension lives in (callbacks get `&self`).
    fn self_arc(&self) -> Option<Arc<Autopilot>> {
        self.me.upgrade()
    }

    fn rt(store: &ExtensionData) -> Option<Arc<ThreadRt>> {
        if !codex_alab_core::active() {
            return None;
        }
        store.get::<ThreadRt>().filter(|rt| !rt.vanilla())
    }

    /// A HOOK STOOD ASIDE for a turn of the thread `store` holds, or for its resume, under the
    /// vanilla experience: what it would have seen goes unseen, so the thread is unwatched
    /// until codex-alab looks at it again ([`ThreadRt::unwatched`]; edges-1, edges-2). Only in
    /// the codex-alab binary: upstream's own runs keep no runtime.
    fn skipped(store: &ExtensionData) {
        if !codex_alab_core::is_alab_binary() {
            return;
        }
        if let Some(rt) = store.get::<ThreadRt>().filter(|rt| rt.vanilla()) {
            rt.unwatched.store(true, Ordering::SeqCst);
        }
    }

    /// A TURN BEGAN WHILE ITS HOOK STOOD ASIDE ([`Self::skipped`]): it is the thread's newest
    /// native turn all the same, so its end, should codex-alab look again before it comes, is
    /// the thread's point and no late completion of the turn before it (edges-1: that end was
    /// dropped, and the thread stood at no point). Bookkeeping only, no behaviour: the turn is
    /// accepted (a decision or an act from before the spell is superseded by it, as by any
    /// native turn), and its start is timed, so its end says what it worked. Whose it was
    /// stays unread (its message was skipped too): nobody's, failing toward carrying.
    async fn skipped_turn(store: &ExtensionData, turn: &ExtensionData) {
        if !codex_alab_core::is_alab_binary() {
            return;
        }
        let Some(rt) = store.get::<ThreadRt>().filter(|rt| rt.vanilla()) else {
            return;
        };
        rt.unwatched.store(true, Ordering::SeqCst);
        let mut native_turn = rt.native_turn.lock().await;
        *native_turn = Some(turn.level_id().to_owned());
        rt.accepted_turns.fetch_add(1, Ordering::SeqCst);
        lock(&turn.get_or_init(TurnTrack::default).0).started = Some(clock());
    }

    /// THE BOOKS OF A THREAD A VANILLA SPELL COVERED, read before a decision's look takes the
    /// spell in ([`Self::park_with`], [`rewatch_bound`]): its history, and the native turns
    /// accepted as the read began (a turn accepted since makes them no point's: its own end is
    /// the point). `None` where no spell covered the thread.
    async fn unwatched_books(&self, rt: &ThreadRt) -> Option<Books> {
        if !rt.unwatched.load(Ordering::SeqCst) {
            return None;
        }
        let accepted = rt.accepted_turns.load(Ordering::SeqCst);
        let items = match self.thread(rt.id).await {
            Some(thread) => resumed_history(&thread).await,
            None => Err("the thread manager does not have it".to_string()),
        };
        #[cfg(test)]
        let items = match lock(&self.history_seam).clone() {
            Some(items) => Ok(items),
            None => items,
        };
        Some(Books { accepted, items })
    }

    pub(crate) fn resume_scan(self: Arc<Self>, rt: Arc<ThreadRt>) -> impl Future<Output = ()> {
        // Reserve synchronously, before spawn/poll, so another /resume or registry cannot
        // steal the first thread's launch authority while its sweep is still pending.
        let first = self.sup.shared().reserve_relaunch();
        self.resume_scan_owned(rt, first, SWEPT_WAIT)
    }

    async fn resume_scan_owned(
        self: Arc<Self>,
        rt: Arc<ThreadRt>,
        first: bool,
        sweep_wait: Duration,
    ) {
        {
            let _turn = rt.eval.acquire().await;
            let accepted_at_scan = rt.accepted_turns.load(Ordering::SeqCst);
            'scan: {
                if accepted_at_scan != 0 || rt.stopped.load(Ordering::SeqCst) {
                    if first {
                        self.sup.shared().spend_relaunch();
                        let _ = self.inherited_owed(&rt.key);
                    }
                    break 'scan;
                }
                // A wait timeout is unresolved evidence, never a negative relaunch verdict.
                // The background scan stays pending; native acceptance or stop supersedes it.
                let shared = self.sup.shared();
                while self.sup.in_aterm() && !shared.swept() {
                    let _ =
                        tokio::task::spawn_blocking(move || shared.wait_swept(sweep_wait)).await;
                    if rt.accepted_turns.load(Ordering::SeqCst) != accepted_at_scan
                        || rt.stopped.load(Ordering::SeqCst)
                    {
                        if first {
                            shared.spend_relaunch();
                            let _ = self.inherited_owed(&rt.key);
                        }
                        break 'scan;
                    }
                }
                // And the proof that aterm typed the line that started this process: the aterm
                // client's first `status` read (bounded; none read is no proof).
                if self.sup.in_aterm() && !shared.launch_probed() {
                    let _ = tokio::task::spawn_blocking(move || {
                        shared.wait_launch_probed(LAUNCH_PROBE_WAIT)
                    })
                    .await;
                }
                if rt.accepted_turns.load(Ordering::SeqCst) != accepted_at_scan
                    || rt.stopped.load(Ordering::SeqCst)
                {
                    if first {
                        shared.spend_relaunch();
                        let _ = self.inherited_owed(&rt.key);
                    }
                    break 'scan;
                }
                // The relaunch is judged (and spent) before the history is read, whatever the
                // read then says: a resume that learns nothing still spends it, so a later
                // `/resume` back in this process is no relaunch (review of cf084e3b7, finding 2).
                let relaunch = if first {
                    relaunch_reserved(shared, &rt.key, now_ms())
                } else {
                    Relaunch::Nothing
                };
                let relaunched = relaunch == Relaunch::CarryOn;
                let history = match self.thread(rt.id).await {
                    Some(thread) => resumed_history(&thread).await,
                    None => Err("the thread manager does not have it".to_string()),
                };
                #[cfg(test)]
                let history = match lock(&self.history_seam).clone() {
                    Some(items) => Ok(items),
                    None => history,
                };
                if let Err(why) = &history {
                    self.sup
                        .note(format!("{}: resumed, its history unread ({why})", rt.key));
                }
                let mut resume_note = None;
                // A carried network wall whose route the process before measured: read that
                // route again once the point is restored, so the wait measures it (B2).
                let mut rearm = false;
                let applied = {
                    // Registration owns the native boundary, as start and completion do.
                    // Never reacquire it from bookkeeping inside that boundary.
                    let kept = lock(&self.threads);
                    shared.apply_resume_at_rest(&rt.key, || {
                        if rt.accepted_turns.load(Ordering::SeqCst) != accepted_at_scan
                            || rt.stopped.load(Ordering::SeqCst)
                            || !Self::current_runtime(&kept, &rt)
                        {
                            return false;
                        }
                        #[cfg(test)]
                        if let Some(hook) = lock(&rt.before_resume_bookkeeping).take() {
                            run_hook(hook);
                        }
                        self.seed_workdir(&rt);
                        self.seed_pace(&rt, history.as_deref().ok());
                        // An unread history is no proof the session never worked: its first turn's
                        // end here is past its opening (`ThreadRt::books_read`).
                        let Ok(items) = history else {
                            return true;
                        };
                        rt.books_read.store(true, Ordering::SeqCst);
                        // The act the image before owed: the re-exec's record, else the dead
                        // process's at aterm's relaunch of a session at rest ([`Relaunch::Owed`]).
                        let native = crate::resume_history::latest(&items);
                        let reexec =
                            first
                                .then(|| self.inherited_owed(&rt.key))
                                .flatten()
                                .map(|_| {
                                    let inherited = self.sup.shared().inherited();
                                    inherited
                                        .as_ref()
                                        .and_then(|old| old.owed_act.as_ref())
                                        .and_then(|act| {
                                            crate::owed_origin::record(
                                                act,
                                                &native,
                                                inherited
                                                    .as_ref()
                                                    .and_then(|old| old.in_flight_retry.as_ref()),
                                            )
                                        })
                                        .unwrap_or_else(crate::owed_origin::BoundHandoff::empty)
                                });
                        let from_relaunch = !matches!(relaunch, Relaunch::Nothing);
                        let owed = reexec.clone().or_else(|| match &relaunch {
                            Relaunch::Owed(act) => Some(
                                crate::owed_origin::record(
                                    act,
                                    &native,
                                    self.sup
                                        .shared()
                                        .predecessor(&rt.key)
                                        .as_ref()
                                        .and_then(|old| old.in_flight_retry.as_ref()),
                                )
                                .unwrap_or_else(crate::owed_origin::BoundHandoff::empty),
                            ),
                            Relaunch::CarryOn | Relaunch::Recovery(_) | Relaunch::Nothing => None,
                        });
                        let recovery = reexec
                            .as_ref()
                            .and_then(|_| self.sup.shared().inherited())
                            .and_then(|old| old.recovery.clone())
                            .or_else(|| match &relaunch {
                                Relaunch::Recovery(recovery) => Some(recovery.clone()),
                                _ => None,
                            })
                            .filter(|recovery| recovery.thread == rt.key);
                        let needs_off =
                            self.carried_needs_off(&rt.key, reexec.is_some(), from_relaunch);
                        let predecessor = self.sup.shared().predecessor(&rt.key);
                        let signal_teardown = relaunched
                            && predecessor.as_ref().is_some_and(|old| {
                                matches!(old.exit.as_deref(), Some("signal:1" | "signal:15"))
                            });
                        let source = if signal_teardown {
                            crate::resume_history::ResumeSource::Signal
                        } else {
                            relaunched.into()
                        };
                        *lock(&rt.cyber_access_program) =
                            crate::resume_history::cyber_program(&items);
                        let mut state = lock(&rt.state);
                        let inherited_intent = owed.as_ref().is_some_and(
                            crate::owed_origin::BoundHandoff::carries_automatic_intent,
                        );
                        let task = resume_bound_with(
                            &mut state,
                            &items,
                            source,
                            owed.clone().or_else(|| {
                                recovery
                                    .as_ref()
                                    .map(|_| crate::owed_origin::BoundHandoff::empty())
                            }),
                            needs_off,
                            clock(),
                            |state| {
                                if let Some(recovery) = recovery {
                                    // Restore in the opposite sampling order: a pause before the
                                    // monotonic read can only extend an absolute deadline.
                                    let epoch = now_ms();
                                    let now = clock();
                                    let restored = crate::recovery::restore(
                                        state,
                                        &items,
                                        &recovery,
                                        now,
                                        epoch,
                                        &self.timing,
                                    );
                                    if !restored {
                                        self.clear_recovery(&rt.key);
                                    } else if let codex_alab_core::session::RecoveryWall::Usage { quota, .. }
                                        | codex_alab_core::session::RecoveryWall::Entitlement { quota } = &recovery.wall
                                    {
                                        self.restore_quota_scope(&rt, quota.as_ref());
                                    }
                                    rearm = restored
                                        && (crate::recovery::rearms(&recovery)
                                            || matches!(
                                                recovery.wall,
                                                codex_alab_core::session::RecoveryWall::Usage { .. }
                                                    | codex_alab_core::session::RecoveryWall::Entitlement { .. }
                                            ));
                                    restored
                                } else {
                                    self.clear_recovery(&rt.key);
                                    false
                                }
                            },
                        );
                        // These deadlines were just converted from epoch time. Earlier
                        // host sleep is already reflected in that conversion; only later
                        // sleep, including a route rearm's await, belongs to this point.
                        let mut reading = self.wake_reading();
                        state.asleep_mark = reading.map(|reading| reading.asleep);
                        let in_flight = relaunched
                            .then(|| {
                                predecessor
                                    .as_ref()
                                    .and_then(|old| old.in_flight_retry.clone())
                            })
                            .flatten()
                            .filter(|retry| retry.thread == rt.key);
                        if let Some(retry) = in_flight {
                            let epoch = now_ms();
                            let now = clock();
                            let restored = crate::in_flight_retry::restore(
                                &mut state,
                                &items,
                                &retry,
                                signal_teardown,
                                now,
                                epoch,
                            );
                            if restored {
                                reading = self.wake_reading();
                                state.asleep_mark = reading.map(|reading| reading.asleep);
                                if shared
                                    .view()
                                    .primary_thread
                                    .as_deref()
                                    .is_none_or(|primary| primary == rt.key)
                                {
                                    shared.set_in_flight_retry(Some(retry));
                                }
                            } else if shared
                                .in_flight_retry()
                                .is_some_and(|current| current.thread == retry.thread)
                            {
                                shared.set_in_flight_retry(None);
                            }
                        } else if shared
                            .in_flight_retry()
                            .is_some_and(|retry| retry.thread == rt.key)
                        {
                            shared.set_in_flight_retry(None);
                        }
                        *lock(&rt.inherited_settle) =
                            if inherited_intent && (state.owed.is_some() || state.landed) {
                                InheritedSettle::new(
                                    clock(),
                                    reading,
                                    codex_alab_core::wake::supported(),
                                )
                            } else {
                                None
                            };
                        // A POINT LEFT TO THE PERSON THAT THE IMAGE BEFORE CARRIED (edges-5) is
                        // recorded already: its LEFT record stands at aterm, which keeps the face
                        // `theirs` across the re-exec, so this image records nothing more there.
                        if state.left
                            && let Some(point) = state.point_at()
                        {
                            *lock(&rt.left_recorded) =
                                Some((rt.accepted_turns.load(Ordering::SeqCst), point));
                        }
                        let (ask_lifted, held_arm) = Self::point_hold_after(&state, &rt);
                        drop(state);
                        if ask_lifted {
                            self.sup.clear_attention_reasons(
                                &rt.key,
                                [crate::replies::GO_AHEAD_STOP_KEY],
                            );
                        }
                        if self.sup.keep_held_for(&rt.key, held_arm) {
                            self.sup.show_held_later();
                        }
                        self.tell_land_from(&kept, &rt);
                        // The record carries the act handed over (or none, where the relaunch's
                        // first records carried the dead process's act and it was not handed over).
                        self.sync_record_owed_from(&kept);
                        if (reexec.is_some() || from_relaunch) && off_still_down(&items, needs_off)
                        {
                            // Still down in this image: its record says so in turn.
                            self.sup.shared().set_needs_off(needs_off);
                        }
                        self.sync_pace(&rt);
                        self.sync_owed_from(&kept);
                        resume_note = Some(format!(
                            "{}: resumed (task={task}, relaunched={relaunched}, owed={})",
                            rt.key,
                            owed.as_ref()
                                .map_or("none", crate::owed_origin::BoundHandoff::word)
                        ));
                        true
                    })
                };
                if applied != Some(true) {
                    if first {
                        let _ = self.inherited_owed(&rt.key);
                    }
                    self.sup.note(format!(
                        "{}: native work superseded the resume scan",
                        rt.key
                    ));
                }
                if let Some(note) = resume_note {
                    self.sup.note(note);
                }
                if rearm {
                    let route = self.rearm_route(&rt).await;
                    if rt.accepted_turns.load(Ordering::SeqCst) == accepted_at_scan
                        && !rt.stopped.load(Ordering::SeqCst)
                    {
                        self.sup.note(match &route {
                            netwatch::Route::Direct(dest) if self.sup.probe_off() => format!(
                                "{}: the carried wall's reach at {}:{} is not measured \
                                 (probe_api is off)",
                                rt.key, dest.host, dest.port
                            ),
                            netwatch::Route::Direct(dest) => format!(
                                "{}: the carried wall's reach at {}:{} is measured while it \
                                 stands",
                                rt.key, dest.host, dest.port
                            ),
                            netwatch::Route::Custom(why) => format!(
                                "{}: the carried wall's reach is not measured ({why})",
                                rt.key
                            ),
                        });
                        *lock(&rt.net) = Some(route);
                    }
                }
                self.surface_needs(&rt);
            }
            rt.scanned.store(true, Ordering::SeqCst);
        }
        self.evaluate(rt).await;
    }

    /// THE REPOSITORY PROBE for thread `rt` (`repo::probe_session`) within `limit`, noted
    /// under `what`: every folder the worker touched, the last command's first, then the
    /// thread's own, with the branches it used since its first turn, each read in the
    /// repository it was named in (review of the forever probe). The thread's own folder is read
    /// whole; every other folder only where the session changed its repository since its first
    /// turn (`repo::Used::touched`; review of the forever probe at f48a215d7b, R2). The folders it
    /// read are kept as their repositories for the next probe ([`repo::Probed::kept`]); a scope
    /// that lost a folder it touched is never read clean.
    pub(crate) async fn probe_repo(
        &self,
        rt: &ThreadRt,
        what: &str,
        limit: Duration,
    ) -> repo::RepoRead {
        let scope = rt.probe_scope();
        let mut touched: Vec<std::path::PathBuf> = rt.workdir().into_iter().collect();
        for dir in scope.dirs.iter().rev() {
            if !touched.contains(dir) {
                touched.push(dir.clone());
            }
        }
        let mut dirs: Vec<std::path::PathBuf> = Vec::new();
        if let Some(thread) = self.thread(rt.id).await {
            let cwd = thread
                .thread_settings_snapshot()
                .await
                .cwd
                .as_path()
                .to_path_buf();
            touched.retain(|dir| *dir != cwd);
            dirs.push(cwd);
        }
        let used = repo::Used {
            since: scope.since,
            branches: scope.branches.clone(),
            touched,
        };
        let probed = repo::probe_session(&dirs, &used, limit).await;
        self.compact_scope(rt, &scope.dirs, &probed.kept);
        let found = if scope.overflow && probed.read.is_clean() {
            repo::RepoRead::Unread
        } else {
            probed.read
        };
        #[cfg(test)]
        if let Some(hook) = lock(&rt.after_probe).take() {
            run_hook(hook);
        }
        self.sup
            .note(format!("{}: {what}: {}", rt.key, found.as_ref().summary()));
        found
    }

    /// AN OFF STOOD since this thread's memory last looked ([`Supervisor::offs`]): the needs
    /// it took off the person's screen, and the Land episode, end in the memory too
    /// ([`ThreadState::park_off`]), so a later report that names none raises nothing again
    /// (review of 2a9241bbf, findings 4 and 12).
    ///
    /// The record the park changed is written once the thread list is let go, off the runtime
    /// within [`WRITE_THROUGH_BUDGET`] ([`write_through`]): a slow disk never holds `threads`,
    /// nor the turn's start that awaits this.
    async fn park_if_off(&self, rt: &ThreadRt) -> bool {
        self.park_reading(rt, None).await
    }

    /// [`Self::park_if_off`], with the `books` a decision's look read for a thread a vanilla
    /// spell covered ([`Self::unwatched_books`]).
    async fn park_reading(&self, rt: &ThreadRt, books: Option<Books>) -> bool {
        let parked = {
            let kept = lock(&self.threads);
            self.park_with(&kept, rt, books)
        };
        if parked == Some(true) {
            let _ = write_through(self.sup.shared()).await;
        }
        parked.is_some()
    }

    /// The park under the thread list held: `None` where the thread is stopped or no longer
    /// current, else whether it parked at an Off now (or took a vanilla spell in), so its caller
    /// owes the record a write once it lets the list go.
    fn park_if_off_from(&self, kept: &[Weak<ThreadRt>], rt: &ThreadRt) -> Option<bool> {
        self.park_with(kept, rt, None)
    }

    /// [`Self::park_if_off_from`], with the `books` a decision's look read for a thread a
    /// vanilla spell covered ([`Self::unwatched_books`]).
    fn park_with(
        &self,
        kept: &[Weak<ThreadRt>],
        rt: &ThreadRt,
        books: Option<Books>,
    ) -> Option<bool> {
        if rt.stopped.load(Ordering::SeqCst) || !Self::current_runtime(kept, rt) {
            return None;
        }
        let offs = self.sup.offs();
        let parked = rt.off_seen.swap(offs, Ordering::SeqCst) != offs;
        if parked {
            lock(&rt.transient_auth_goal).take();
            lock(&rt.state).park_off();
        }
        // A VANILLA SPELL COVERED THE THREAD (edges-1, edges-2): its books are unwatched since,
        // its point another conversation's ([`ThreadState::unwatched`]). A decision's look read
        // its history, and it stands at its own last turn end ([`rewatch_bound`]); a turn's
        // start or end is the point itself.
        let unwatched = rt.unwatched.swap(false, Ordering::SeqCst);
        let mut rewatched = false;
        if unwatched {
            let mut state = lock(&rt.state);
            match books {
                Some(Books {
                    accepted,
                    items: Ok(items),
                }) if rt.accepted_turns.load(Ordering::SeqCst) == accepted => {
                    let stands =
                        rewatch_bound(&mut state, &items, self.sup.shared().needs_off(), clock());
                    drop(state);
                    rewatched = true;
                    self.sup.note(format!(
                        "{}: back from a vanilla spell, its history read: {}",
                        rt.key,
                        if stands {
                            "stands at its last turn end"
                        } else {
                            "no turn end to stand at (one runs, or none ended)"
                        }
                    ));
                }
                Some(Books {
                    items: Err(why), ..
                }) => {
                    state.unwatched();
                    drop(state);
                    self.sup.note(format!(
                        "{}: back from a vanilla spell, its history unread ({why}): no point until a turn ends",
                        rt.key
                    ));
                }
                _ => state.unwatched(),
            }
        }
        if parked || unwatched {
            *lock(&rt.held_told) = None;
            // The park lifts the hold: its arm comes out of aterm's `status held=` too, and
            // it ends the Land episode, Land's hold with it.
            if self.sup.want_held_for(&rt.key, None) {
                self.sup.show_held_later();
            }
            self.tell_land_from(kept, rt);
            // The done checks start again: the record the caller writes next says so.
            self.sync_pace(rt);
            self.clear_recovery(&rt.key);
            // The needs the books leave standing go on the menu bar at once (or come off it).
            if rewatched {
                self.surface_needs_from(kept, rt);
            }
            return Some(true);
        }
        Some(false)
    }

    /// The record stays `busy` while any current root owes its carry-on
    /// (`Shared::set_carry_on_owed`).
    fn sync_owed(&self) {
        let kept = lock(&self.threads);
        self.sync_owed_from(&kept);
    }

    fn sync_owed_from(&self, kept: &[Weak<ThreadRt>]) {
        let shared = self.sup.shared();
        // Teardown revokes callback authority, but a relaunchable signal retains
        // the last durable carry-on custody until the next process takes it.
        if shared.leaving_for_relaunch() {
            return;
        }
        let owed = kept.iter().filter_map(Weak::upgrade).any(|rt| {
            if rt.stopped.load(Ordering::SeqCst) {
                return false;
            }
            let state = lock(&rt.state);
            state.carry_on && state.task && !state.landed
        });
        shared.set_carry_on_owed(owed);
    }

    /// What the previous image of this process left thread `key` at its re-exec: the act it
    /// owed and whether it had landed there (its record, `Shared::inherited`), as `(owed,
    /// landed)`; `(None, false)` for the thread it resumed with nothing to carry. Handed to the
    /// first resume of that thread only: `None` for any other resume.
    pub(crate) fn inherited_owed(&self, key: &str) -> Option<(Option<Owed>, bool)> {
        let inherited = self.sup.shared().inherited()?;
        let carried = inherited.owed_act.as_ref();
        let thread = carried
            .map(|c| c.thread.as_str())
            .or(inherited.session_id.as_deref())?;
        if thread != key || self.owed_spent.swap(true, Ordering::SeqCst) {
            return None;
        }
        Some(carried.map_or((None, false), |carried| {
            (Self::owed_of(carried), carried.landed)
        }))
    }

    /// The NEEDS YOU set an Off took down that the image before this one carried in its record
    /// ([`codex_alab_core::session::Shared::needs_off`]) for thread `key`: the previous image's
    /// at a re-exec (`reexec`, its record's session is `key`), else the dead process's at
    /// aterm's relaunch; `0` for none. A relaunch whose record says none (a record written
    /// before the Off reached it, or by an older build) restores what the history leaves
    /// standing: a need shown again is a look for the person, a need lost is work stopped.
    pub(crate) fn carried_needs_off(&self, key: &str, reexec: bool, relaunched: bool) -> u64 {
        let shared = self.sup.shared();
        if reexec
            && let Some(inherited) = shared.inherited()
            && inherited.session_id.as_deref() == Some(key)
        {
            return inherited.needs_off;
        }
        if relaunched {
            return shared.predecessor(key).map_or(0, |p| p.needs_off);
        }
        0
    }

    /// The folder the worker last ran a command in, for a thread whose memory is new (a
    /// resume): this process's own (a `/resume` back), else the record's of the image before
    /// this one (a re-exec) or of the dead process aterm relaunched, so the repository probe
    /// reads where the worker was working, not the thread's start folder (review of the
    /// forever autopilot, 2026-10-20). A command run since keeps its own. Its probe scope is
    /// seeded too ([`Self::seed_scope`]).
    pub(crate) fn seed_workdir(&self, rt: &ThreadRt) {
        self.seed_scope(rt);
        let shared = self.sup.shared();
        let carried = shared
            .workdir(&rt.key)
            .or_else(|| {
                shared
                    .inherited()
                    .filter(|inherited| inherited.session_id.as_deref() == Some(rt.key.as_str()))
                    .and_then(|inherited| inherited.workdir.clone())
            })
            .or_else(|| shared.predecessor(&rt.key).and_then(|p| p.workdir));
        let Some(dir) = carried else {
            return;
        };
        let mut workdir = lock(&rt.workdir);
        if workdir.is_none() {
            shared.set_workdir(&rt.key, &dir);
            *workdir = Some(dir);
        }
    }

    /// WHAT THE PROBE READ FOR A THREAD WHOSE MEMORY IS NEW (a resume): this process's own scope
    /// (a `/resume` back) and the one the record of the image before this one (a re-exec) or of
    /// the dead process aterm relaunched carried, merged into what this memory began
    /// ([`codex_alab_core::session::ProbeScope::merge`]: every folder and branch of each, the
    /// earlier start), so the folders and branches the worker touched before the re-exec are
    /// still read (review of the forever probe).
    pub(crate) fn seed_scope(&self, rt: &ThreadRt) {
        let shared = self.sup.shared();
        let carried = [
            shared.probe_scope(&rt.key),
            shared.carried_probe_scope(&rt.key),
        ];
        let mut scope = lock(&rt.scope);
        let mut moved = false;
        for other in carried.iter().flatten() {
            moved |= scope.merge(other);
        }
        if moved {
            shared.set_probe_scope(&rt.key, &scope);
        }
    }

    /// Thread `rt`'s scope moved by `change` (a folder touched, a branch named, its start): the
    /// record carries it on.
    fn move_scope(
        &self,
        rt: &ThreadRt,
        change: impl FnOnce(&mut codex_alab_core::session::ProbeScope) -> bool,
    ) {
        let mut scope = lock(&rt.scope);
        if change(&mut scope) {
            self.sup.shared().set_probe_scope(&rt.key, &scope);
        }
    }

    /// Command `command`, run in folder `cwd`, moved thread `rt`'s scope: the folder it ran in
    /// and each one it reached ([`repo::command_reach`]), and each branch it named; a walk that
    /// was cut ([`repo::Reach::cut`]) lost a folder, so the scope is never read clean.
    fn reach_scope(&self, rt: &Arc<ThreadRt>, command: &[String], cwd: Option<&std::path::Path>) {
        if let Some(dir) = cwd {
            self.move_scope(rt, |scope| scope.touch(dir));
        }
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
        let reach = repo::command_reach(command, cwd, home.as_deref());
        if !reach.folders.is_empty() || !reach.branches.is_empty() || reach.cut {
            self.move_scope(rt, |scope| {
                let mut moved = reach.cut && !scope.overflow;
                scope.overflow |= reach.cut;
                for dir in &reach.folders {
                    moved |= scope.touch(dir);
                }
                for branch in reach.branches {
                    moved |= scope.name_branch(branch);
                }
                moved
            });
        }
        self.compact_soon(rt);
    }

    /// An edit landed in folders `dirs` of thread `rt`'s session.
    fn touch_scope(&self, rt: &Arc<ThreadRt>, dirs: &[std::path::PathBuf]) {
        self.move_scope(rt, |scope| {
            let mut moved = false;
            for dir in dirs {
                moved |= scope.touch(dir);
            }
            moved
        });
        self.compact_soon(rt);
    }

    /// A SUB-AGENT'S THREAD STARTS (`source` names a non-root agent; the forever-persist lane,
    /// item d): its commands and edits are its root session's work, which Land's probe must read
    /// before it lands, but it has no runtime of its own. A sub-agent spawned by a thread
    /// (`SubAgentSource::ThreadSpawn`) names its parent: a root thread this autopilot keeps, or
    /// a sub-agent already adopted, whose root it takes. Its store carries that root
    /// ([`SubAgentOf`]), and [`Self::sub_agent_root`] folds its reach into the root's scope.
    /// A review's, a compaction's, a memory consolidation's and any other internal agent's
    /// thread names no parent, so its reach stays unfolded, as does one whose parent this
    /// process keeps no runtime for (a vanilla root's, or one spawned before codex-alab looked).
    fn adopt_sub_agent(
        &self,
        store: &ExtensionData,
        source: &codex_protocol::protocol::SessionSource,
    ) {
        let Some(parent) = source.parent_thread_id().map(|id| id.to_string()) else {
            return;
        };
        let mut sub_agents = lock(&self.sub_agents);
        let root = if self.thread_rt(&parent).is_some() {
            parent
        } else if let Some(root) = sub_agents.get(&parent) {
            root.clone()
        } else {
            return;
        };
        sub_agents.insert(store.level_id().to_owned(), root.clone());
        drop(sub_agents);
        store.insert(SubAgentOf(root));
    }

    /// The runtime of the root thread whose session the sub-agent of `store` works in
    /// ([`Self::adopt_sub_agent`]), while this autopilot keeps that root and watches it. A root
    /// stopped for aterm's relaunch is still kept (`on_thread_stop`): its own commands still move
    /// its scope, which the record carries to the relaunch, and so do its sub-agents' (review of
    /// the forever-persist lane, F4: a stopped root folded nothing, so a sub-agent's work in the
    /// relaunch's teardown was lost to the image after); a root stopped for good is kept no more.
    fn sub_agent_root(&self, store: &ExtensionData) -> Option<Arc<ThreadRt>> {
        if !codex_alab_core::active() {
            return None;
        }
        let root = store.get::<SubAgentOf>()?;
        self.thread_rt(&root.0).filter(|rt| !rt.vanilla())
    }

    /// After a probe or a compaction of thread `rt`: the folders `seen` at its start give way to
    /// the ones it `kept` (their repositories, and each folder whose read was cut), and each
    /// folder touched since stays.
    pub(crate) fn compact_scope(
        &self,
        rt: &ThreadRt,
        seen: &[std::path::PathBuf],
        kept: &[std::path::PathBuf],
    ) {
        self.move_scope(rt, |scope| {
            let added: Vec<std::path::PathBuf> = scope
                .dirs
                .iter()
                .filter(|dir| !seen.contains(dir))
                .cloned()
                .collect();
            let mut dirs: Vec<std::path::PathBuf> = Vec::new();
            for dir in kept.iter().rev().chain(added.iter()) {
                if !dirs.contains(dir) {
                    dirs.push(dir.clone());
                }
            }
            if dirs.len() > codex_alab_core::session::PROBE_SCOPE_DIRS {
                scope.overflow = true;
                dirs.drain(..dirs.len() - codex_alab_core::session::PROBE_SCOPE_DIRS);
            }
            let moved = dirs != scope.dirs;
            // However it moved, the next compaction waits for twice this many folders.
            rt.compacted_to.store(dirs.len(), Ordering::SeqCst);
            scope.dirs = dirs;
            moved
        });
    }

    /// A SCOPE THAT GREW PAST [`codex_alab_core::session::PROBE_SCOPE_COMPACT_AT`] FOLDERS, and
    /// twice what its last compaction left, IS COMPACTED NOW, beside the runtime
    /// (`repo::compact`: its folders reduced to their repositories, as a probe keeps them),
    /// rather than at the next probe (review of the forever probe at f48a215d7b, R1): a wide
    /// change across one repository's folders between two probes filled the scope and
    /// overflowed it, and an overflowed scope is never read clean, so Land never landed again.
    /// One at a time per thread; with no runtime to run it on, the next probe compacts it.
    fn compact_soon(&self, rt: &Arc<ThreadRt>) {
        let held = lock(&rt.scope).dirs.len();
        let floor = codex_alab_core::session::PROBE_SCOPE_COMPACT_AT
            .max(rt.compacted_to.load(Ordering::SeqCst).saturating_mul(2));
        if held < floor || rt.compacting.swap(true, Ordering::SeqCst) {
            return;
        }
        let (Some(me), Ok(runtime)) = (self.me.upgrade(), tokio::runtime::Handle::try_current())
        else {
            rt.compacting.store(false, Ordering::SeqCst);
            return;
        };
        let rt = Arc::clone(rt);
        runtime.spawn(async move {
            let seen = rt.probe_scope().dirs;
            let kept = repo::compact(&seen, repo::PROBE_LIMIT).await;
            me.compact_scope(&rt, &seen, &kept);
            rt.compacting.store(false, Ordering::SeqCst);
        });
    }

    /// Whether a compaction of thread `rt`'s scope between probes runs ([`Self::compact_soon`]).
    #[cfg(test)]
    pub(crate) fn compacting(rt: &ThreadRt) -> bool {
        rt.compacting.load(Ordering::SeqCst)
    }

    /// THE PACE THE RECORD CARRIES FOR A THREAD WHOSE MEMORY IS NEW (a resume): this process's
    /// own (a `/resume` back), else what the record of the image before this one (a re-exec)
    /// or of a dead process of this tab (however old, as aterm's) left, which the record
    /// writer's start took up ([`codex_alab_core::session::Shared::pace`]): the short streak,
    /// the wall's ladder, Land's hold, a person's Esc and the wall's goal go on where that
    /// process left them ([`ThreadState::seed_pace`], [`ThreadState::seed_land_hold`],
    /// [`ThreadState::seed_esc`], [`ThreadRt::wall_goal`]), as aterm's loop takes up
    /// its predecessor's (parity review S2: a relaunch started the streak at 0, and the act
    /// after an idle crash's owed one came 120 s after it where the process before would have
    /// waited 480 s; review of gap 11, finding 1: a re-exec started the hold over). A dead
    /// process's pace is this tab's only where [`pace_handed`] says so; any other is dropped,
    /// as the record writer withholds it. `history` is the thread's durable history as the
    /// resume read it (`None`: unread): the wall's goal is bound again only where it shows no
    /// person's stop of the goal since the walled turn began
    /// ([`crate::resume_history::person_goal_stop_since`]).
    pub(crate) fn seed_pace(&self, rt: &ThreadRt, history: Option<&[codex_history::RolloutItem]>) {
        let shared = self.sup.shared();
        if let Some(pace) = shared
            .pace(&rt.key)
            .filter(|pace| pace_handed(shared, &rt.key, pace))
        {
            let wall = pace.wall.as_ref().and_then(|wall| {
                decide::WallTrack::carried(
                    &wall.class,
                    instant_of(wall.first_seen_ms),
                    wall.attempts,
                    wall.last_try_ms.map(instant_of),
                    wall.since_ms.map(instant_of),
                    wall.answer_owed,
                )
            });
            let typed: Vec<Instant> = pace.typed_ms.iter().copied().map(instant_of).collect();
            let mut state = lock(&rt.state);
            if rt.accepted_turns.load(Ordering::SeqCst) == 0 {
                // A PACE THE RECORD HANDS ON is the books' streak, ladder, hold or typed acts
                // (aterm's `seed_worked`): never at rest (`Shared::set_pace` drops one), and
                // only this tab's ([`pace_handed`]).
                state.seed_worked();
                state.seed_pace(pace.short_streak, wall);
                state.seed_checks(pace.checks);
                if let Some(cut) = &pace.cut
                    && let Some(mode) = decide::Mode::of_word(&cut.mode)
                {
                    state.seed_cut(cut.text.clone(), mode);
                }
                // LAND'S HOLD OUTLIVES A RE-EXEC AND A RELAUNCH, as aterm's loop seeds its own
                // from its books at every start (`seed_land_hold`).
                if let Some(hold) = pace.land_hold {
                    state.seed_land_hold(hold.since_ms, hold.nudges);
                }
                // THE PERSON'S HOUR OUTLIVES A RE-EXEC AND A RELAUNCH, as aterm's loop seeds the
                // hour it booked at every start (`seed_go_ahead_hour`): one that ran from the end
                // of a turn the key came in is no key the window keeps.
                if let Some(ms) = pace.go_ahead_hour_ms {
                    state.seed_go_ahead_hour(instant_of(ms), clock());
                }
                // A PERSON'S ESC OUTLIVES A RE-EXEC AND A RELAUNCH (the forever-persist lane,
                // item a): the resume's restored abort of its turn holds from it, as this
                // process's point would have, and is never read as a teardown's.
                if let Some(esc) = &pace.esc {
                    state.seed_esc(instant_of(esc.at_ms), esc.turn.clone());
                }
                // SO DOES THE GOAL A TURN ERROR BLOCKED AT THE POINT'S WALL (item a): the wall's
                // try resumes it here too. The pace holds it only while no person's stop this
                // process saw came after its binding (`Shared::person_goal_set`; a carried one is
                // bound at none, where this process's count began), so its count is kept as it
                // is: a stop after this binds no more (review of the forever-persist lane, F2: a
                // count taken here forgot each stop before it). A stop the process before saw
                // but its record never wrote is in the thread's history: such a goal is a
                // person's, as is one whose history is unread or names no start of its turn.
                if let Some(goal) = &pace.wall_goal {
                    let person_stopped = history.is_none_or(|items| {
                        crate::resume_history::person_goal_stop_since(items, &goal.turn)
                    });
                    let mut bound = lock(&rt.wall_goal);
                    if bound.is_none() && !person_stopped {
                        *bound = Some(goal.clone());
                    }
                }
                // RULING F'S REPLIES OUTLIVE A RE-EXEC AND A `/resume` BACK, NEVER aterm's
                // RELAUNCH: a process launched after the books is another run of the program
                // (aterm's `launched_since_books`), so a dead predecessor's pace, not yet
                // written over by this process, seeds none; this process's own does (parity
                // review of the stage-1 port, finding 5: a `/resume` away and back forgot the
                // hold this process counted, where its streak and checks were kept).
                if shared.inherited().is_some()
                    || shared.predecessor(&rt.key).and_then(|p| p.pace).as_ref() != Some(&pace)
                {
                    let now = clock();
                    let epoch = now_ms();
                    state.seed_replies(replies_of(&pace), pace.replies_at_ms.map(instant_of));
                    state.seed_replies_reset(
                        pace.replies_reset_ms
                            .and_then(|ms| crate::recovery::instant_of(ms, now, epoch)),
                    );
                    if let Some(point) = &pace.replies_point {
                        state.seed_held_point(
                            point.text.clone(),
                            point.earlier.clone(),
                            point.questions,
                            now,
                        );
                    }
                    // THE POINT LEFT TO THE PERSON goes with them (edges-5), as aterm's next
                    // loop applies its LEFT record again.
                    if let Some(left) = &pace.left_point {
                        state.seed_left_point(
                            left.point.text.clone(),
                            left.point.earlier.clone(),
                            left.point.questions,
                            left.opens,
                            now,
                        );
                    }
                    // THE GO-AHEAD'S COUNT goes with the replies it bounds beside: a held park
                    // the image before gave its go-ahead gets none through the hold again.
                    if let Some(count) = pace.go_ahead {
                        state.seed_go_aheads(decide::GoAheads {
                            acts: count.acts,
                            first: count.first_ms.map(instant_of),
                        });
                    }
                }
            }
            // The hour's budget only ever holds an act back, so the acts the process before
            // typed count whatever this image has accepted since.
            state.seed_acts(&typed, clock(), &self.timing);
        }
        self.sync_pace(rt);
    }

    /// THE RECORD CARRIES THE THREAD'S PACE ([`codex_alab_core::session::Pace`]): its short
    /// streak, the ladder of a wall whose class outlives a process (the appearance it counts
    /// from, and whether its last try still owes its answer, included), the acts
    /// `continue_per_hour` counts within its window, their times turned into the epoch's
    /// clock, Land's hold standing ([`ThreadState::land_hold_carried`]), a person's Esc over
    /// the point ([`ThreadState::esc_carried`]) and the goal a turn error blocked at its wall
    /// ([`wall_goal_after`]), so the image after a re-exec, or aterm's relaunch after a crash,
    /// backs off, spends the hour's budget, says the landing, holds for the person's Esc and
    /// resumes the wall's goal as this process would have ([`Self::seed_pace`]).
    pub(crate) fn sync_pace(&self, rt: &ThreadRt) {
        let pace = {
            let state = lock(&rt.state);
            let now = clock();
            codex_alab_core::session::Pace {
                short_streak: state.short_streak,
                wall: state.wall.as_ref().and_then(|wall| {
                    Some(codex_alab_core::session::WallMark {
                        class: wall.carried_class()?.to_string(),
                        attempts: wall.attempts,
                        first_seen_ms: epoch_ms_of(wall.first_seen),
                        last_try_ms: wall.last_try.map(epoch_ms_of),
                        since_ms: Some(epoch_ms_of(wall.since)),
                        answer_owed: wall.answer_owed,
                    })
                }),
                typed_ms: state
                    .typed
                    .iter()
                    .filter(|at| now.saturating_duration_since(**at) < self.timing.window)
                    .map(|at| epoch_ms_of(*at))
                    .collect(),
                cut: state.cut.clone().zip(state.mode).map(|(text, mode)| {
                    codex_alab_core::session::CutAct {
                        text,
                        mode: mode.word().to_string(),
                    }
                }),
                checks: state.checks,
                land_hold: state.land_hold_carried().map(|(since_ms, nudges)| {
                    codex_alab_core::session::LandHold { since_ms, nudges }
                }),
                replies: state
                    .replies
                    .kept()
                    .map(|k| codex_alab_core::session::KeptReply {
                        mark: k.mark,
                        answers: u32::try_from(k.answers).unwrap_or(u32::MAX),
                        walled: k.walled,
                        tries: u32::try_from(k.tries).unwrap_or(u32::MAX),
                    })
                    .collect(),
                replies_run: u32::try_from(state.replies.run).unwrap_or(u32::MAX),
                replies_at_ms: state
                    .replies_at
                    .filter(|_| !state.replies.is_empty())
                    .map(epoch_ms_of),
                // The reset outlives the replies (an Off keeps it, as aterm's `park_off`
                // does), and may be still to come.
                replies_reset_ms: state
                    .replies_reset
                    .map(|at| crate::recovery::epoch_of(at, now, now_ms())),
                replies_point: state.held_point().map(|point| {
                    codex_alab_core::session::ReplyPoint {
                        text: point.text.clone(),
                        earlier: point.earlier.clone(),
                        questions: point.questions,
                    }
                }),
                left_point: state.left_point().map(|(point, opens)| {
                    codex_alab_core::session::LeftPoint {
                        point: codex_alab_core::session::ReplyPoint {
                            text: point.text.clone(),
                            earlier: point.earlier.clone(),
                            questions: point.questions,
                        },
                        opens,
                    }
                }),
                go_ahead_hour_ms: state.go_ahead_hour_carried(now).map(epoch_ms_of),
                go_ahead: state.go_aheads_carried().map(|count| {
                    codex_alab_core::session::GoAheadCount {
                        acts: count.acts,
                        first_ms: count.first.map(epoch_ms_of),
                    }
                }),
                esc: state
                    .esc_carried()
                    .map(|(at, turn)| codex_alab_core::session::EscHold {
                        at_ms: epoch_ms_of(at),
                        turn: turn.to_string(),
                    }),
                wall_goal: lock(&rt.wall_goal).clone(),
            }
        };
        self.sup.shared().set_pace(&rt.key, pace);
    }

    /// THE NEEDS ON THE PERSON'S SCREEN (NEEDS YOU, aterm owner direction 2026-10-19: "be
    /// LOUDER ABOUT IT ... but NEVER let that block you"): the needs this thread holds
    /// standing, raised (or cleared) at once when they moved, never held for a back-off or a
    /// grace. Only a root thread the TUI registered speaks for the session.
    fn surface_needs(&self, rt: &ThreadRt) {
        let kept = lock(&self.threads);
        self.surface_needs_from(&kept, rt);
    }

    fn surface_needs_from(&self, kept: &[Weak<ThreadRt>], rt: &ThreadRt) {
        // Resume's route read can outlive retirement. Keep the exact registration
        // through the synchronous update, as native completion and person input do.
        if rt.stopped.load(Ordering::SeqCst)
            || !Self::current_runtime(kept, rt)
            || !self.sup.shared().is_root_thread(&rt.key)
        {
            return;
        }
        let (standing, key) = {
            let state = lock(&rt.state);
            // THE NEEDS ON THE MENU BAR (§15.6): those standing less the lines the owner's
            // standing go-ahead answers or is answering.
            (state.needs.shown(), state.needs.shown_key())
        };
        if self
            .sup
            .want_needs_at_off(&standing, key, rt.off_seen.load(Ordering::SeqCst))
        {
            let sup = Arc::clone(&self.sup);
            tokio::spawn(async move { sup.show_needs().await });
        }
    }

    async fn finish(
        &self,
        rt: &Arc<ThreadRt>,
        turn_store: &ExtensionData,
        abort_reason: Option<codex_protocol::protocol::TurnAbortReason>,
    ) {
        // THE PERSON'S HAND THAT THE MESSAGE-TIME READ MISSED (edges-6; aterm reads the hand
        // from the reading it judges the point by): read again now, bounded as that read is and
        // before any lock, and judged as it would have been there
        // ([`decide::PERSON_TURN_SLACK`]).
        let hand_unread = turn_store
            .get::<TurnTrack>()
            .is_some_and(|track| lock(&track.0).hand_unread);
        let late_hand = if hand_unread {
            self.sup
                .person_input_ago()
                .await
                .and_then(|ago| clock().checked_sub(ago))
        } else {
            None
        };
        let native_turn = rt.native_turn.lock().await;
        let aborted = abort_reason.is_some();
        let track = turn_store.get::<TurnTrack>();
        let data = track
            .map(|t| std::mem::take(&mut *lock(&t.0)))
            .unwrap_or_default();
        let at = clock();
        // Read before the thread list is held: what the point's go-ahead foresees.
        let policy_now = self.sup.policy();
        let finish_reading = self.wake_reading();
        let earlier = turn_said(&data.said, data.final_at);
        let text = data.final_text.or(data.text);
        let success = data.wall.is_none() && !aborted;
        let withheld = data.wall.as_ref().is_some_and(upgrade_ask::withheld_by);
        if let Some(mail) = data.mail {
            // The mail's receipt: handled at a clean end, deferred at an interrupt or an
            // error; then the report with the last agent message (CONTRACT §5 "inbox").
            let verdict = if success {
                codex_alab_core::mail::Verdict::Handled
            } else {
                codex_alab_core::mail::Verdict::Deferred
            };
            // The verdict is this process's at once, before the receipt task runs: the same
            // line typed again is refused from here on.
            self.sup.hold_receipt(mail.id, verdict);
            let sup = Arc::clone(&self.sup);
            let last = text.clone();
            tokio::spawn(async move { sup.receipt(mail, verdict, last).await });
        }
        let kept = lock(&self.threads);
        if rt.stopped.load(Ordering::SeqCst) || !Self::current_runtime(&kept, rt) {
            return;
        }
        self.sup.shared().turn_ended(&rt.key, turn_store.level_id());
        if native_turn
            .as_deref()
            .is_some_and(|current| current != turn_store.level_id())
        {
            // Its exact native turn and mail still ended. It cannot replace a newer
            // turn's point, retry custody, record, needs or attention; but a turn ended here,
            // so the session is past its opening (gap 32, review finding 6: the person's
            // message after an Esc'd first job is carried as it is without the race).
            self.seed_worked(rt);
            self.sup.note(format!(
                "{}: late completion of {} leaves the newer native turn unchanged",
                rt.key,
                turn_store.level_id()
            ));
            return;
        }
        // The route the point's network, usage, or subscription wall takes, measured while
        // the point stands; no other point's.
        *lock(&rt.net) = data.route.filter(|_| {
            matches!(
                data.wall,
                Some(Wall::Network { .. } | Wall::Usage { .. } | Wall::Entitlement)
            )
        });
        let login = data
            .login
            .filter(|_| matches!(data.wall, Some(Wall::Login)));
        *lock(&rt.auth_wall_scope) = data.auth_scope;
        *lock(&rt.auth_observation) = data.auth_observation;
        *lock(&rt.auth_wall_goal_origin) = turn_store
            .get::<codex_alab_core::renewal::GoalAtTurnStart>()
            .map(|origin| origin.goal_id.clone());
        *lock(&rt.human_goal_recovery) = data
            .limited_goal_at_start
            .filter(|_| {
                success
                    && data.by_person
                    && text.as_ref().is_some_and(|text| !text.trim().is_empty())
                    && policy_now
                        .as_ref()
                        .is_some_and(|policy| policy.autopilot == decide::Mode::Cruise)
            })
            .map(|goal| (at, goal));
        let end = TurnEnd {
            at,
            by_autopilot: data.by_autopilot,
            by_person: data.by_person,
            text: text.clone(),
            earlier,
            questions: data.questions,
            worked: data
                .started
                .map_or(Duration::ZERO, |s| at.saturating_duration_since(s)),
            wall: data.wall,
            aborted,
            ran_tools: data.ran_tools,
            restored: false,
        };
        // No turn end finishes a task any more (FOREVER CRUISE: the worker's words never end
        // it; FOREVER LAND lands at a decision, `Act::Landed`).
        let real_work = data.by_autopilot && success && end.worked >= self.timing.progress;
        let Some(parked) = self.park_if_off_from(&kept, rt) else {
            return;
        };
        // THE WORD THE TURN ENDS UNDER (gap 32, `ThreadState::ended_under`): Off as the
        // supervisor names it, else the live policy's word.
        let off = self.sup.shared().harness_word() == codex_alab_core::session::HarnessWord::Off;
        let live = self.sup.policy().map(|policy| policy.autopilot);
        let ask_lifted;
        let held_arm;
        let notice_turn;
        {
            let mut state = lock(&rt.state);
            notice_turn = data.by_autopilot && state.last_rule == Some(words::RULE_UPGRADE_NOTICE);
            // In a passive spell that is no Off, Land's rule, whatever was folded last (review
            // of gap 32, finding 2: a Cruise folded before a standby carried the opening
            // exchange of a session whose word had moved to Land): Land's rule is the one that
            // may leave it, and Cruise's first decision takes back what it left
            // (`ThreadState::observe_mode`).
            state.ended_under = if off {
                None
            } else {
                live.or(Some(decide::Mode::Land))
            };
            // A RESUMED THREAD WHOSE BOOKS THE SCAN DID NOT READ before this turn (a turn beat
            // the scan, or its history was unread): past its opening, failing toward carrying
            // (`ThreadRt::books_read`; the scan applies before any turn is accepted or never,
            // both under the thread list held here).
            if rt.resumed.load(Ordering::SeqCst) && !rt.books_read.load(Ordering::SeqCst) {
                state.seed_worked();
            }
            if let Some(at) = late_hand.filter(|_| data.by_person) {
                state.observe_person_hand(at);
            }
            // The goal a turn error blocked at this point's wall, or the one the autopilot's own
            // compaction carries to its continuation.
            let carried = lock(&rt.wall_goal).take();
            *lock(&rt.wall_goal) = wall_goal_after(
                carried,
                state.compacting,
                end.wall.as_ref(),
                end.aborted,
                TurnGoal {
                    active_at_start: data.active_goal_at_start,
                    goal_tool: data.goal_tool,
                    person_stops: (
                        data.person_stops_at_start,
                        self.sup.shared().person_goal_stops(&rt.key),
                    ),
                    turn: turn_store.level_id().to_owned(),
                },
            );
            state.observe_turn_end(end, &self.timing);
            // A PERSON'S ESC (or Ctrl-C) STOPPED IT (`ThreadState::esc_at`): held for
            // `human_grace`, then decided as a turn end. A relaunch's shutdown interrupts too,
            // and is no person's.
            if abort_reason == Some(codex_protocol::protocol::TurnAbortReason::Interrupted)
                && !self.sup.shared().leaving_for_relaunch()
            {
                state.observe_esc(at);
            }
            // THE GO-AHEAD'S LINES STAY OFF THE MENU BAR FROM THE TURN'S END (§15.6): the park
            // the go-ahead answers here is held off before the decision judges it, so no line it
            // answers shows meanwhile ([`ThreadState::go_ahead_foreseen`]).
            if let Some(policy) = &policy_now {
                state.go_ahead_foreseen(policy, at);
            }
            // The login wall the point stands at, with the login as the turn met it; no other
            // point's (its watch ends with the point it was armed for).
            *lock(&rt.login) = login.map(|baseline| LoginWatch::at(at, baseline));
            // The recorded hold is refreshed at this point, including its status arm.
            (ask_lifted, held_arm) = Self::point_hold_after(&state, rt);
            // Count only sleep since this wall's deadline was materialized. A
            // successful point still receives its own finish baseline, so sleep
            // before its first asynchronous evaluation is not silently dropped.
            state.asleep_mark = data.wall_asleep;
            let _ = state.slept_to(finish_reading.map(|reading| reading.asleep), at);
            state.point_turn_id = Some(turn_store.level_id().to_string());
            state.native_origin = Some(codex_alab_core::session::NativeOrigin {
                turn_id: turn_store.level_id().to_string(),
                phase: match abort_reason {
                    None => codex_alab_core::session::NativePhase::Complete,
                    Some(codex_protocol::protocol::TurnAbortReason::Interrupted) => {
                        codex_alab_core::session::NativePhase::Interrupted
                    }
                    Some(_) => codex_alab_core::session::NativePhase::Aborted,
                },
            });
            self.clear_recovery(&rt.key);
        }
        // A land act's yield read, or the reading another turn's end leaves behind.
        self.tell_land_from(&kept, rt);
        if ask_lifted {
            self.sup
                .clear_attention_reasons(&rt.key, [crate::replies::GO_AHEAD_STOP_KEY]);
        }
        if self.sup.keep_held_for(&rt.key, held_arm) {
            self.sup.show_held_later();
        }
        self.sync_pace(rt);
        self.sync_owed_from(&kept);
        self.surface_needs_from(&kept, rt);
        // THE ROUND HEARS THE ROOT THREAD'S LATEST REPLY (`upgrade_ask`; gap 27; aterm's
        // `transcript_has_ready` over its `answers`): each turn that ends clean, the notice's own
        // or a later one, says whether the agent is ready, in ordinary language while nothing
        // but codex-alab's own acts came since the notice, by the notice's reference alone after
        // a person's message ([`upgrade_ask::Round::heard`]). A notice whose own turn met a
        // login wall or a usage limit never reached the agent, and is given back; one that met
        // another wall stands, answered by the reply to codex-alab's try (aterm's `notice_scan`;
        // review of gap 27, finding 3).
        if self.sup.shared().is_root_thread(&rt.key) {
            let mut round = lock(&self.ask);
            if notice_turn && withheld {
                round.undelivered();
            } else if success {
                round.heard(text.as_deref());
            }
        }
        let _ = self.sup.shared().set_last_turn(
            &rt.key,
            LastTurn {
                turn_id: Some(turn_store.level_id().to_string()),
                ended_at_ms: now_ms(),
                last_agent_message: text,
                by_autopilot: data.by_autopilot,
            },
        );
        // Only a root thread the TUI registered speaks for the session's walls: a side
        // thread's turn (a title, a memory) that went through must not clear the login wall
        // the main thread stands at (end-to-end run 2026-10-16: cleared 39 ms after it was
        // raised).
        let speaks = self.sup.shared().is_root_thread(&rt.key);
        if speaks && (real_work || data.by_person) {
            // A person's turn (a new task), or a turn codex-alab typed that worked for real
            // (at least `Timing::progress`), ends the carry-on chain: a later relaunch carries
            // on again (amended 2026-10-19: the done that ended it is gone with FOREVER
            // CRUISE). A short turn does not: relaunch, carry-on, stall again still waits for
            // the person, not carry on once more.
            self.sup.shared().set_carried_on(0);
        }
        if speaks && (success || data.by_person) {
            // Retire local reasons while this native completion still has custody.
            // Only reconciliation may be deferred: a later identical raise is new. A
            // `limit` retires the quota's own need too: this root's turn got past the wall.
            self.sup.clear_attention_reasons(
                &rt.key,
                ["login", "context", "wall", "api", "limit", "turn"]
                    .into_iter()
                    .filter(|_| success)
                    .chain(
                        ["question", "plan", "goal", "turn"]
                            .into_iter()
                            .filter(|_| data.by_person),
                    ),
            );
        }
        // The park's record goes once the thread list is let go, off this turn's end
        // (`park_if_off`).
        drop(kept);
        if parked {
            let shared = self.sup.shared();
            tokio::spawn(async move {
                let _ = write_through(shared).await;
            });
        }
    }
}

/// WHAT A RESUMED THREAD'S MEMORY TAKES FROM ITS HISTORY (`resume_scan`, pure): whether it
/// holds a task and the last reply ([`scan_history`]); after aterm's relaunch or this
/// process's own re-exec (`owed` is the record's handover, `Some` even with nothing carried),
/// the needs the history leaves standing ([`history_needs`]), which go back on the person's
/// screen unless they are the set an Off took down in the image before (`needs_off`, its
/// record's; [`off_still_down`]): the history shows no Off, and a need the person turned off
/// is not raised again by a new image (review of the forever autopilot, 2026-10-20); then the
/// act the old image owed or its landing ([`ThreadState::observe_owed`]). Returns whether a
/// task stands.
#[cfg(test)]
pub(crate) fn resume_into(
    state: &mut ThreadState,
    items: &[codex_history::RolloutItem],
    source: impl Into<crate::resume_history::ResumeSource>,
    owed: Option<(Option<Owed>, bool)>,
    needs_off: u64,
    at: Instant,
) -> bool {
    let native = crate::resume_history::latest(items);
    let owed = crate::owed_origin::runtime(owed, &native);
    resume_bound(state, items, source, owed, needs_off, at)
}

/// Raw record handoffs retain the validator's landing proof through this boundary.
#[cfg(test)]
pub(crate) fn resume_bound(
    state: &mut ThreadState,
    items: &[codex_history::RolloutItem],
    source: impl Into<crate::resume_history::ResumeSource>,
    owed: Option<crate::owed_origin::BoundHandoff>,
    needs_off: u64,
    at: Instant,
) -> bool {
    resume_bound_with(state, items, source, owed, needs_off, at, |_| false)
}

/// Native refusal is the fallback when no independently validated Recovery restores
/// the point. Both paths share the same source, needs and owed-action boundary.
pub(crate) fn resume_bound_with(
    state: &mut ThreadState,
    items: &[codex_history::RolloutItem],
    source: impl Into<crate::resume_history::ResumeSource>,
    owed: Option<crate::owed_origin::BoundHandoff>,
    needs_off: u64,
    at: Instant,
    restore_recovery: impl FnOnce(&mut ThreadState) -> bool,
) -> bool {
    let source = source.into();
    let relaunched = source.carries_on();
    let native = crate::resume_history::latest(items);
    let (task, last) = scan_history(items);
    state.observe_resume(task, last.as_deref(), relaunched);
    // THE SESSION WORKED BEFORE THIS PROCESS (gap 32): its history shows a turn that ended or
    // an act of codex-alab's, so a person's message here is never the one that opens it.
    if crate::resume_history::worked(items, source) {
        state.seed_worked();
    }
    state.native_origin = crate::owed_origin::origin(&native);
    if (relaunched || owed.is_some()) && !off_still_down(items, needs_off) {
        state.restore_needs(history_needs(items));
    }
    let wall_try = owed
        .as_ref()
        .is_some_and(crate::owed_origin::BoundHandoff::wall_try);
    let mut policy_owed = None;
    if let Some(owed) = owed {
        let (owed, landed) = owed.into_parts();
        // Only the bound, fixed policy instruction may keep its words and deadline
        // across native refusal restoration, which clears an ordinary carried act.
        policy_owed = owed
            .as_ref()
            .filter(|owed| owed.rule == crate::words::RULE_POLICY_CONTINUE)
            .cloned();
        // An empty or rejected snapshot is no authority to synthesize a new carry-on.
        state.carry_on = false;
        state.observe_owed(owed, landed, at);
    }
    // A validated recovery already supplies the exact failed point. A provisional
    // fatal would erase its carried wall ladder before the real wall replaces it.
    let recovered = restore_recovery(state);
    if task && !recovered && !wall_try {
        crate::resume_history::restore_terminal(state, items, source, at);
        // Keep the native Policy point even if a later mode revokes this owed act.
        // Reattaching the validated intent does not spend its unsubmitted instruction.
        if let Some(owed) = policy_owed {
            state.owed = Some(owed);
        }
    }
    // THE POINT RULING F HELD IN THE IMAGE BEFORE (a re-exec's record): stood at again where
    // nothing above restored a point and the history still rests at that completed turn, so an
    // Off and back types there.
    if matches!(
        native.point,
        crate::resume_history::NativePoint::Complete(complete)
            if !complete
                .error
                .as_ref()
                .is_some_and(codex_protocol::protocol::ErrorEvent::affects_turn_status)
    ) {
        state.observe_held(at);
    } else if state.held_seed.take().is_some() && state.task && state.owed.is_none() {
        state.replies = crate::replies::Replies::default();
    }
    // A left point the history no longer rests at is no point to stand at again, nor is a
    // person's Esc over a turn no restored abort ended.
    state.left_seed = None;
    state.esc_seed = None;
    state.policy_continued = crate::resume_history::policy_continued(items);
    if state.policy_continued && crate::resume_history::terminal_policy_exhausted(items) {
        let reply = state.resumed_reply.get_or_insert_with(String::new);
        reply.push('\n');
        reply.push_str(crate::words::POLICY_EXHAUSTED);
    }
    task
}

/// A THREAD A VANILLA SPELL COVERED, ITS BOOKS READ AGAIN by the decision's look that takes the
/// spell in (edges-1, edges-2; the owner, 2026-10-19: FOREVER CRUISE, "Cruise never parks";
/// FOREVER LAND, "LAND EVERYTHING INTO REMOTE MAIN NO EXCEPTIONS"): the memory forgets what it
/// held from before the spell ([`decide::ThreadState::unwatched`]), and stands at the history's
/// last turn end instead (`resume_history::restore_end`: the person's own last turn, a clean
/// end, an interrupt's or a wall's), so Cruise continues there and Land carries the work on
/// from there, the session past its opening, where before nothing was typed until the person
/// typed again. The needs are the ones the history leaves standing (every report folded in
/// order, a person's clean turn answering what stood, [`history_needs`]), unless they are the
/// set an Off took down (`needs_off`, [`off_still_down`]: vanilla gives back a held claim, and
/// what that Off took off the person's screen stays off until a report names it again). The
/// thread's native origin is its history's latest turn, as a resume's is. Returns whether a
/// point stands.
pub(crate) fn rewatch_bound(
    state: &mut ThreadState,
    items: &[codex_history::RolloutItem],
    needs_off: u64,
    at: Instant,
) -> bool {
    state.unwatched();
    let stands = crate::resume_history::restore_end(state, items, at);
    state.native_origin = crate::owed_origin::origin(&crate::resume_history::latest(items));
    state.needs = if off_still_down(items, needs_off) {
        decide::Needs::default()
    } else {
        history_needs(items)
    };
    stands
}

/// Whether the needs a resumed thread's history leaves standing are the set an Off took down
/// in the image before (`needs_off`, nonzero, its record's): no report named another set
/// since. A report that named the same set again raised it again, and the record then named
/// none ([`crate::Supervisor::want_needs`]).
pub(crate) fn off_still_down(items: &[codex_history::RolloutItem], needs_off: u64) -> bool {
    needs_off != 0 && history_needs(items).key == needs_off
}

/// What a resumed thread's history says: whether a person's turn exists (a `TurnStarted`
/// whose `turn_trigger` is not codex-alab's, or one that names no attribution at all and
/// holds a user message: [`Started::of`]), and the last `TurnComplete` reply.
pub fn scan_history(items: &[codex_history::RolloutItem]) -> (bool, Option<String>) {
    let mut task = false;
    let mut last = None;
    let mut unattributed = false;
    for item in items {
        let codex_history::RolloutItem::EventMsg(event) = item else {
            continue;
        };
        match event {
            _ if is_user_message(event) => task |= std::mem::take(&mut unattributed),
            EventMsg::TurnStarted(started) => match Started::of(started) {
                Started::Ours => unattributed = false,
                Started::Person | Started::Other => {
                    unattributed = false;
                    task = true;
                }
                Started::Unattributed => unattributed = true,
            },
            EventMsg::TurnComplete(complete) => {
                unattributed = false;
                last = complete.last_agent_message.clone();
            }
            EventMsg::TurnAborted(_) => unattributed = false,
            _ => {}
        }
    }
    (task, last)
}

/// WHOSE A HISTORY'S TURN IS, by its `TurnStarted`: codex-alab's own (its trigger), a
/// person's (the TUI's submission or queue, or an attribution that names no trigger), another
/// starter's (a goal's continuation), or one that names NO ATTRIBUTION AT ALL. Core attributes
/// only a regular task (`Session::emit_turn_started`): a compaction's turn (a person's
/// `/compact`, or codex-alab's own `context-compact@v1`) names none and holds no user message,
/// and the live thread reads it as nobody's, as it reads every turn no `UserMessage` item
/// opened (parity review R1). A history from before core attributed turns, and the turn a
/// realtime conversation's last words are recorded in, name none either but hold the person's
/// message: [`is_user_message`] makes such a turn a person's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Started {
    Ours,
    Person,
    Other,
    Unattributed,
}

impl Started {
    fn of(started: &codex_protocol::protocol::TurnStartedEvent) -> Self {
        let Some(attribution) = started.turn_attribution.as_ref() else {
            return Self::Unattributed;
        };
        match attribution.turn_trigger.as_deref() {
            Some(TURN_TRIGGER) => Self::Ours,
            None => Self::Person,
            Some(other) if PERSON_TRIGGERS.contains(&other) => Self::Person,
            Some(_) => Self::Other,
        }
    }
}

/// A user message in a history: a legacy history's `UserMessage` event, or a paginated
/// one's completed user message item.
fn is_user_message(event: &EventMsg) -> bool {
    match event {
        EventMsg::UserMessage(_) => true,
        EventMsg::ItemCompleted(done) => matches!(done.item, TurnItem::UserMessage(_)),
        _ => false,
    }
}

/// The `turn_trigger`s of a person's turns in a history: the TUI's own submission and its
/// queue of what the person typed while a turn ran (an attribution that names no trigger is a
/// person's too, [`Started::of`]).
const PERSON_TRIGGERS: &[&str] = &["user", "queue"];

/// THE NEEDS A RESUMED THREAD'S HISTORY LEAVES STANDING: every reply folded in order, its
/// whole turn read as the live thread folds each turn end's ([`decide::Needs::fold`] over
/// the turn's earlier agent messages, then its reply: a need said in an earlier message
/// stands beside the reply's), a person's turn clearing what stood (they answered it); a
/// goal's, a compaction's ([`Started`]) or codex-alab's own turn answers nothing. A TURN A WALL
/// ENDED (core keeps the turn's terminal error on its completion event,
/// `TurnCompleteEvent::error`, which the rollout persists whole) answers nothing, as the live
/// thread reads a turn end with a wall (`ThreadState::observe_turn_end`; parity review S4: a
/// walled person's turn read as ended, and the need it never answered was cleared), and it
/// folds the needs its complete messages said, as an interrupted turn does, whatever ended it
/// (needs-wall-turn-unfolded: Codex hands each message whole). A set a turn of codex-alab's
/// began after is taken as acked: the act that began it carried the ack. The earlier messages
/// are a legacy history's `AgentMessage` events or a paginated one's completed agent message
/// items.
pub fn history_needs(items: &[codex_history::RolloutItem]) -> decide::Needs {
    let mut needs = decide::Needs::default();
    let mut said: Vec<String> = Vec::new();
    // The turn in hand is a person's: its end answers the needs standing (aterm's point;
    // never its start), unless a wall ended it. One that named no attribution is nobody's
    // until a user message shows it a person's ([`Started`]).
    let mut person = false;
    let mut unattributed = false;
    for item in items {
        let codex_history::RolloutItem::EventMsg(event) = item else {
            continue;
        };
        match event {
            _ if is_user_message(event) => {
                // A turn that names no attribution is a person's once it holds their message
                // (a history from before core attributed turns); a compaction's holds none.
                person |= std::mem::take(&mut unattributed);
            }
            EventMsg::AgentMessage(message) => said.push(message.message.clone()),
            EventMsg::ItemCompleted(done) => {
                if let TurnItem::AgentMessage(message) = &done.item {
                    said.push(agent_text(message));
                }
            }
            EventMsg::TurnStarted(started) => {
                said.clear();
                let whose = Started::of(started);
                if whose == Started::Ours && needs.key != 0 {
                    needs.acked = Some(needs.key);
                }
                person = whose == Started::Person;
                unattributed = whose == Started::Unattributed;
            }
            EventMsg::TurnAborted(_) => {
                unattributed = false;
                if std::mem::take(&mut person) {
                    needs = decide::Needs::default();
                }
                needs.fold(turn_said(&said, Some(said.len())).as_deref());
                said.clear();
            }
            EventMsg::TurnComplete(complete) => {
                unattributed = false;
                let persons = std::mem::take(&mut person);
                let walled = complete
                    .error
                    .as_ref()
                    .is_some_and(codex_protocol::protocol::ErrorEvent::affects_turn_status);
                if persons && !walled {
                    needs = decide::Needs::default();
                }
                let reply = complete.last_agent_message.as_deref();
                let at = reply.and_then(|r| said.iter().rposition(|t| t == r));
                let earlier = turn_said(&said, at.or(Some(said.len())));
                let whole = match earlier {
                    Some(before) => Some(format!("{before}\n\n{}", reply.unwrap_or_default())),
                    None => reply.map(str::to_string),
                };
                needs.fold(whole.as_deref());
                said.clear();
            }
            _ => {}
        }
    }
    needs
}

/// THE FOLDERS AN EDIT LANDED IN: each absolute file path a file change names (and each it
/// moved one to), by its parent folder, each once in a stable order; a relative path (a host's
/// convention this one cannot place) names none.
pub(crate) fn patched_dirs(
    change: &codex_protocol::items::FileChangeItem,
) -> Vec<std::path::PathBuf> {
    let mut dirs: Vec<std::path::PathBuf> = Vec::new();
    let mut paths: Vec<&std::path::Path> = Vec::new();
    for (path, file) in &change.changes {
        paths.push(path);
        if let codex_protocol::protocol::FileChange::Update {
            move_path: Some(to),
            ..
        } = file
        {
            paths.push(to);
        }
    }
    paths.sort_unstable();
    for path in paths {
        if let Some(dir) = path.parent().filter(|_| path.is_absolute())
            && !dirs.iter().any(|kept| kept == dir)
        {
            dirs.push(dir.to_path_buf());
        }
    }
    dirs
}

/// An agent message item's words, its text parts joined (as `on_item_completed` reads them).
fn agent_text(message: &codex_protocol::items::AgentMessageItem) -> String {
    message
        .content
        .iter()
        .map(|c| match c {
            codex_protocol::items::AgentMessageContent::Text { text } => text.as_str(),
        })
        .collect()
}

/// A resumed thread's history: the thread store's, or, for a thread whose store keeps
/// paginated history (which `load_history` refuses: "unsupported operation:
/// paginated_threads", end-to-end run 2026-10-16, so no carry-on ever came), its rollout file
/// read directly. Both carry the `TurnStarted` attribution and `TurnComplete` reply the scan
/// reads.
async fn resumed_history(
    thread: &Arc<CodexThread>,
) -> Result<Vec<codex_history::RolloutItem>, String> {
    let stored = match thread.load_history(false).await {
        Ok(history) => return Ok(history.items),
        Err(e) => e.to_string(),
    };
    let Some(path) = thread.rollout_path() else {
        return Err(format!("{stored}; no rollout file"));
    };
    codex_core::RolloutRecorder::load_rollout_items(&path)
        .await
        .map(|(items, _, _)| items)
        .map_err(|e| format!("{stored}; rollout {}: {e}", path.display()))
}

fn user_text(message: &codex_protocol::items::UserMessageItem) -> String {
    message
        .content
        .iter()
        .filter_map(|input| match input {
            UserInput::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

fn idle(status: &AgentStatus) -> bool {
    matches!(
        status,
        AgentStatus::Completed(_) | AgentStatus::Interrupted | AgentStatus::Errored(_)
    )
}

/// At rest: idle, or a thread that has run no turn in this process (`PendingInit`: a fresh
/// one, or one a relaunch or a re-exec resumed), which has nothing in flight. The carry-on
/// after aterm's crash relaunch and an upgrade due right after a re-exec act at such a point
/// (end-to-end run 2026-10-16: both waited for good, "not idle", for a person's turn).
pub fn at_rest(status: &AgentStatus) -> bool {
    idle(status) || matches!(status, AgentStatus::PendingInit)
}

async fn goal_status(
    thread: &Arc<CodexThread>,
    id: ThreadId,
) -> Option<codex_state::ThreadGoalStatus> {
    goal_status_read(thread, id).await.ok().flatten()
}

/// Admission needs to distinguish an absent goal from one it could not read.
/// Callers that use goal state only as an observation retain `goal_status`.
async fn goal_status_read(
    thread: &Arc<CodexThread>,
    id: ThreadId,
) -> Result<Option<codex_state::ThreadGoalStatus>, String> {
    let Some(db) = thread.state_db() else {
        return Ok(None);
    };
    db.thread_goals()
        .get_thread_goal(id)
        .await
        .map(|goal| goal.map(|goal| goal.status))
        .map_err(|error| error.to_string())
}

/// THE ROOT THREAD A SUB-AGENT'S THREAD WORKS FOR, by its key: kept in the sub-agent's thread
/// store ([`Autopilot::adopt_sub_agent`]), so its commands and edits move the root's probe
/// scope (the forever-persist lane, item d: they never entered it, and Land landed past a
/// folder only a sub-agent touched).
struct SubAgentOf(String);

/// WHAT A TURN SHOWED OF ITS GOAL ([`wall_goal_after`]).
#[derive(Clone, Debug, Default)]
pub(crate) struct TurnGoal {
    /// The goal active when the turn began.
    pub(crate) active_at_start: Option<codex_state::ThreadGoal>,
    /// The turn called a goal tool (`update_goal`, `create_goal`).
    pub(crate) goal_tool: bool,
    /// A person's stops of the thread's goal
    /// ([`codex_alab_core::session::Shared::person_goal_stops`]) as the turn began, and as it
    /// ended.
    pub(crate) person_stops: (u64, u64),
    /// The native turn ([`codex_alab_core::session::WallGoal::turn`]).
    pub(crate) turn: String,
}

/// THE GOAL A TURN ERROR BLOCKED AT THE WALL A POINT STANDS AT (pure; owner 2026-10-28,
/// decide.rs's `never_holds`: codex-alab "must keep continuing and making progress with
/// retries" through outages; FOREVER CRUISE and FOREVER LAND). Upstream's goal extension blocks
/// an active goal at every turn error but a usage limit or a lost login (`turn-error`, so its
/// own continuation cannot loop), and codex-alab took that stop for a person's: every wall act
/// was dropped at the goal check, the thread parked for good behind `goal: the goal is
/// blocked`. The stop is the wall's where the walled turn began with the goal ACTIVE, ended on a
/// wall codex-alab retries (a server's, the network's, an overlong reply's, a full context's),
/// was not aborted, called no goal tool (the model's `update_goal` may block it at a person's
/// request), and saw no person's stop of the goal (`thread/goal/set` from the person's hand
/// while it ran, which upstream's `turn-error` stop then leaves as it is: the forever-persist
/// lane, item c); the store keeps no stop event, so those are the evidence. The autopilot's own
/// compaction turn (`compacting`) carries the goal to its continuation; any other turn's end
/// replaces it. A goal blocked before the turn, a person's pause or block, a budget's stop and
/// the other walls' stops (usage: `UsageLimited`'s own resume; a login: the renewal's) stay
/// theirs.
pub(crate) fn wall_goal_after(
    carried: Option<codex_alab_core::session::WallGoal>,
    compaction: bool,
    wall: Option<&Wall>,
    aborted: bool,
    turn: TurnGoal,
) -> Option<codex_alab_core::session::WallGoal> {
    if compaction {
        return carried;
    }
    let turn_error = matches!(
        wall,
        Some(
            Wall::Server { .. }
                | Wall::Network { .. }
                | Wall::Overlong { .. }
                | Wall::Context { .. }
        )
    );
    let (stops_at_start, stops_now) = turn.person_stops;
    let goal_tool = turn.goal_tool;
    let walled_turn = turn.turn;
    turn.active_at_start
        .filter(|goal| {
            turn_error
                && !aborted
                && !goal_tool
                && stops_now == stops_at_start
                && goal.status == codex_state::ThreadGoalStatus::Active
        })
        .map(|goal| codex_alab_core::session::WallGoal {
            goal_id: goal.goal_id,
            objective: goal.objective,
            token_budget: goal.token_budget,
            turn: walled_turn,
            person_stops: stops_at_start,
        })
}

/// Whether `current` is the goal [`wall_goal_after`] bound, stopped at that wall: no person's
/// stop of the thread's goal came since it was bound (`person_stops`, the count now:
/// [`codex_alab_core::session::Shared::person_goal_stops`]), and it is the same goal (identity,
/// objective, budget), blocked, its budget not spent.
fn same_wall_goal(
    bound: &codex_alab_core::session::WallGoal,
    current: &codex_state::ThreadGoal,
    person_stops: u64,
) -> bool {
    person_stops == bound.person_stops
        && bound.goal_id == current.goal_id
        && bound.objective == current.objective
        && bound.token_budget == current.token_budget
        && current.status == codex_state::ThreadGoalStatus::Blocked
        && !current
            .token_budget
            .is_some_and(|budget| current.tokens_used >= budget)
}

/// WHICH STOP A GOAL'S RESUME LIFTS ([`Autopilot::resume_goal_with_origin`]).
#[derive(Clone, Copy)]
enum GoalStop<'a> {
    /// A usage window's or a quota's (`UsageLimited`), bound to the goal a person's successful
    /// turn began under where one is given.
    Usage(Option<&'a codex_state::ThreadGoal>),
    /// A turn error's at the wall this try answers ([`wall_goal_after`]), booked as that try.
    Wall(&'a codex_alab_core::session::WallGoal, &'a Act),
}

/// What resuming a usage-limited goal at the reset came to.
#[derive(Debug, Clone, PartialEq, Eq)]
enum GoalResume {
    /// The bounded human recovery no longer matches, or native budget admission stopped it.
    Stopped,
    /// It is active again; its turn starts.
    Resumed,
    /// There is no usage-limited goal (none, or a person moved it): a plain turn may go.
    NotLimited,
    /// It is still usage-limited: the resume failed (`why`).
    Failed(String),
    /// New TUI input holds the same unspent act for a later readiness check.
    TuiHeld { until: Instant, why: &'static str },
    /// The goal, native point or policy moved since the checks; no goal change was admitted.
    NotNow,
}

fn same_limited_goal(a: &codex_state::ThreadGoal, b: &codex_state::ThreadGoal) -> bool {
    a.goal_id == b.goal_id
        && a.status == codex_state::ThreadGoalStatus::UsageLimited
        && b.status == codex_state::ThreadGoalStatus::UsageLimited
        && a.objective == b.objective
        && a.token_budget == b.token_budget
}

/// The attention a usage-limited goal raises when its resume at the reset failed: a typed
/// turn would be refused past it, so nothing else goes on, and the resume is tried again
/// ([`RECHECK`]; it said a person resumes it, which FOREVER CRUISE's harness never waits for).
pub fn goal_resume_failed(why: &str) -> String {
    format!("goal: the usage-limited goal did not resume at the reset ({why}); it is tried again")
}

/// The attention the goal a turn error blocked at a wall ([`wall_goal_after`]) raises when its
/// resume at the wall's try failed: no usage limit and no reset stopped it, so it says the stop
/// it was (review of the forever-cruise lane, finding 5); tried again as
/// [`goal_resume_failed`]'s is.
pub fn wall_goal_resume_failed(why: &str) -> String {
    format!(
        "goal: the goal a turn error blocked did not resume at the wall's try ({why}); it is tried again"
    )
}

/// The attention a turn end raises when the act codex-alab chose for it did not go.
pub fn turn_left(why: &str) -> String {
    format!("turn: codex-alab could not go on ({why}); this turn's end is a person's")
}

/// The attention a refused submit raises, as `(key, text)`, or `None` when another turn
/// answers the turn end instead: one is running (`NotIdle`), one already took the slot
/// (`PendingTriggerTurn`, `Superseded`), or the process is leaving (`ServerDraining`).
/// Plan mode is the `plan` attention a pre-check would have raised; every other reason (none
/// is expected of `start_turn_if_idle`) leaves the turn end to a person, and says so.
pub fn not_submitted_attention(
    reason: &codex_protocol::turn_input::NotSubmittedReason,
    why: &str,
) -> Option<(&'static str, String)> {
    use codex_protocol::turn_input::NotSubmittedReason as R;
    match reason {
        R::NotIdle | R::PendingTriggerTurn | R::Superseded | R::ServerDraining => None,
        R::PlanMode => Some((
            "plan",
            format!("plan: Plan mode leaves this turn's end to a person ({why})"),
        )),
        other => Some(("turn", turn_left(&format!("not submitted: {other:?}")))),
    }
}

/// The attention a goal codex-alab types nothing past raises, when the goal stopped by itself
/// and so waits for a person: blocked, out of its token budget, or usage-limited where the
/// point owes no usage resume (the usage window's resume and a quota wall's try resume it at
/// their act and never come here; upstream resumes no usage-limited goal by itself, so one a
/// continuation or a compaction meets would stand still with no sign). `None` for a goal a
/// person paused (theirs already), an active one (it goes on by itself) and a complete one.
pub fn goal_left_to_person(status: codex_state::ThreadGoalStatus) -> Option<String> {
    use codex_state::ThreadGoalStatus;
    match status {
        ThreadGoalStatus::Blocked => {
            Some("goal: the goal is blocked; a person decides how it goes on".to_string())
        }
        ThreadGoalStatus::BudgetLimited => {
            Some("goal: the goal spent its token budget; a person decides".to_string())
        }
        ThreadGoalStatus::UsageLimited => Some(
            "goal: the goal is usage-limited and nothing resumes it at the usage window's \
             reset; a person decides (`/goal resume` continues it)"
                .to_string(),
        ),
        ThreadGoalStatus::Active | ThreadGoalStatus::Paused | ThreadGoalStatus::Complete => None,
    }
}

/// Whether a goal that reads `status` at a turn's start has answered the `goal:` attention: an
/// active goal goes on by itself, so whatever stopped it no longer stands.
pub fn goal_answers_attention(status: Option<codex_state::ThreadGoalStatus>) -> bool {
    status == Some(codex_state::ThreadGoalStatus::Active)
}

/// The thread's goal objective, unless the goal is complete (or there is none).
async fn goal_objective(thread: &Arc<CodexThread>, id: ThreadId) -> Result<Option<String>, String> {
    let Some(db) = thread.state_db() else {
        return Ok(None);
    };
    Ok(db
        .thread_goals()
        .get_thread_goal(id)
        .await
        .map_err(|error| error.to_string())?
        .filter(|goal| goal.status != codex_state::ThreadGoalStatus::Complete)
        .map(|goal| goal.objective))
}

/// Whether this process resumed `session` for aterm's crash relaunch (the carry-on's
/// trigger): the start-up sweep found a dead predecessor's record of that session, and aterm's
/// first `status` read at this start shows aterm's relaunch ([`relaunched_by_aterm`]).
/// codex-alab's own re-exec never carries on: the old image's record carries only the paced
/// act it owed, typed at its time ([`ThreadState::observe_owed`], `upgrade`), and a point it
/// left to a person stays the person's.
///
/// Both facts are about this process's start, so they are read ONCE: the process's first
/// resume spends them ([`Shared::spend_relaunch`]), whatever it is a resume of and whatever it
/// learns after the sweep settles (including an empty sweep or unreadable history),
/// and a later resume in the same process (`/resume` away and back) is no relaunch, whatever
/// its history says. And they speak only for the start: a resume at `now_ms` more than
/// [`LAUNCH_PROBE_WINDOW`] after it is a person's, never aterm's relaunch. A process whose own
/// count already reached [`MAX_CARRY_ONS`] carries on no more. One that does not carry on
/// passes its predecessor's count on (the record keeps it), so a refusal never resets the
/// chain: only a person's turn, a turn of real work codex-alab typed, or a landing does.
///
/// [`Shared::spend_relaunch`]: codex_alab_core::session::Shared::spend_relaunch
pub fn relaunched(shared: &codex_alab_core::session::Shared, session: &str, now_ms: u64) -> bool {
    matches!(relaunch(shared, session, now_ms), Relaunch::CarryOn)
}

/// What this process's start owes thread `session` ([`relaunch`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Relaunch {
    /// aterm's crash relaunch of a session that died working: the carry-on
    /// ([`relaunched_by_aterm`]).
    CarryOn,
    /// aterm's crash relaunch of a session that died at rest owing a paced act: that act, as
    /// the dead process decided it, at its time ([`owed_by_aterm_relaunch`]).
    Owed(codex_alab_core::session::OwedAct),
    /// A provider failure waiting on its original deadline, re-decided after the exact
    /// failed rollout turn and current policy have been checked.
    Recovery(codex_alab_core::session::Recovery),
    /// Neither: a person's resume, a re-exec, a later resume in this process.
    Nothing,
}

/// THIS PROCESS'S START, judged once for the thread its first resume names (the facts are
/// spent then, [`relaunched`]): aterm's crash relaunch of a session that died working carries
/// it on ([`Relaunch::CarryOn`]).
pub fn relaunch(shared: &codex_alab_core::session::Shared, session: &str, now_ms: u64) -> Relaunch {
    if !shared.swept() || !shared.reserve_relaunch() {
        return Relaunch::Nothing;
    }
    relaunch_reserved(shared, session, now_ms)
}

fn relaunch_reserved(
    shared: &codex_alab_core::session::Shared,
    session: &str,
    now_ms: u64,
) -> Relaunch {
    if !shared.spend_relaunch() {
        return Relaunch::Nothing;
    }
    let Some(predecessor) = shared.predecessor(session) else {
        return Relaunch::Nothing;
    };
    let window = u64::try_from(LAUNCH_PROBE_WINDOW.as_millis()).unwrap_or(u64::MAX);
    let launch = shared
        .launch()
        .filter(|launch| now_ms >= launch.started_at_ms && now_ms - launch.started_at_ms <= window);
    let witness = shared.launch_witness();
    if let Some(recovery) = predecessor
        .recovery
        .as_ref()
        .filter(|recovery| recovery.thread == session)
        && launch.is_some_and(|launch| {
            aterm_typed_the_line(
                &predecessor,
                &launch.args,
                launch.started_at_ms,
                witness.as_ref(),
            )
        })
    {
        return Relaunch::Recovery(recovery.clone());
    }
    if let Some(owed) = launch.and_then(|launch| {
        owed_by_aterm_relaunch(
            &predecessor,
            session,
            &launch.args,
            launch.started_at_ms,
            witness.as_ref(),
        )
    }) {
        return Relaunch::Owed(owed.clone());
    }
    let carry = shared.carried_on() < MAX_CARRY_ONS
        && launch.is_some_and(|launch| {
            relaunched_by_aterm(
                &predecessor,
                &launch.args,
                launch.started_at_ms,
                witness.as_ref(),
            )
        });
    if !carry && predecessor.carried_on > shared.carried_on() {
        shared.seed_before_native_work(|| {
            if predecessor.carried_on > shared.carried_on() {
                shared.set_carried_on(predecessor.carried_on);
            }
        });
    }
    if carry {
        return Relaunch::CarryOn;
    }
    Relaunch::Nothing
}

/// THE PACED ACT A SESSION THAT DIED AT REST OWED, handed to aterm's relaunch of it (FOREVER
/// CRUISE / FOREVER LAND: the back-off's wait is rest, so the record says `idle` while the act
/// waits, and the carry-on, which only a session that died working gets, would leave the act
/// lost for good): the record's `owed` names a paced act of thread `session`, and aterm typed
/// the line that started this process, proven as for the carry-on ([`aterm_typed_the_line`]:
/// a crash-like exit, the record recent, this process started with its `resumeArgv`, aterm's
/// lease at the launch read). The carry-on chain does not bound it: the act is the dead
/// process's own decision, not a new continuation, and it goes at its paced time.
pub fn owed_by_aterm_relaunch<'p>(
    predecessor: &'p codex_alab_core::session::Predecessor,
    session: &str,
    args: &[std::ffi::OsString],
    started_at_ms: u64,
    witness: Option<&codex_alab_core::session::LaunchWitness>,
) -> Option<&'p codex_alab_core::session::OwedAct> {
    predecessor
        .owed_act
        .as_ref()
        .filter(|act| {
            act.thread == session
                && !act.landed
                && act.rule.as_deref().and_then(decide::carried_rule).is_some()
                && act.text.is_some()
        })
        .filter(|_| aterm_typed_the_line(predecessor, args, started_at_ms, witness))
}

/// WHETHER THE PACE `pace` THAT [`codex_alab_core::session::Shared`] HOLDS FOR `session` IS
/// THIS START'S TO TAKE UP: this process's own, or the image's before a re-exec (its own pid
/// and identity), always. A swept predecessor's (the record writer's start holds it in custody
/// either way) only where aterm's books would hand it on (CONTRACT §4 `pace`): that record was
/// written in this process's own aterm session (aterm's loop takes up a tab's books at every
/// start, however old and however the worker before ended,
/// [`codex_alab_core::session::same_aterm_session`]), or aterm's relaunch typed this process's
/// line ([`aterm_typed_the_line`]). This is the same test the writer applies before a record
/// carries that pace. A record from another tab, or a person's own `codex resume` with no
/// session to match, gets nothing. Taken up regardless of that test, a person's resume days
/// later started on the dead process's rung and streak, which the writer had judged not this
/// start's (review of CX-B, CXR-2).
pub(crate) fn pace_handed(
    shared: &codex_alab_core::session::Shared,
    session: &str,
    pace: &codex_alab_core::session::Pace,
) -> bool {
    if shared.inherited().is_some() {
        return true;
    }
    let Some(predecessor) = shared.predecessor(session) else {
        return true;
    };
    if predecessor.pace.as_ref() != Some(pace) {
        return true;
    }
    shared.launch().is_some_and(|launch| {
        codex_alab_core::session::same_aterm_session(predecessor.aterm_session.as_deref(), launch)
            || aterm_typed_the_line(
                &predecessor,
                &launch.args,
                launch.started_at_ms,
                shared.launch_witness().as_ref(),
            )
    })
}

/// RULING F'S REPLIES AS THE RECORD'S PACE CARRIES THEM
/// ([`codex_alab_core::session::Pace::replies`]), the newest [`crate::replies::REPLIES_KEPT`]
/// kept.
fn replies_of(pace: &codex_alab_core::session::Pace) -> crate::replies::Replies {
    let kept: Vec<crate::replies::Kept> = pace
        .replies
        .iter()
        .map(|k| crate::replies::Kept {
            mark: k.mark,
            answers: usize::try_from(k.answers).unwrap_or(usize::MAX),
            walled: k.walled,
            tries: usize::try_from(k.tries).unwrap_or(usize::MAX),
        })
        .collect();
    crate::replies::Replies {
        run: usize::try_from(pace.replies_run).unwrap_or(usize::MAX),
        ..crate::replies::Replies::of(&kept)
    }
}

/// How many relaunch carry-ons in a row, with no person's turn and no real work between, a
/// session gets: one (CONTRACT §5 carry-on).
pub const MAX_CARRY_ONS: u32 = codex_alab_core::session::MAX_CARRY_ONS;

/// How long before this process's start a person's keystroke reads as the person typing the
/// line that started it: aterm's relaunch types only after a person has been quiet for
/// `human_grace_s`, and a hand-typed `codex resume <id>` starts within moments of its Enter.
pub const PERSON_LINE_MARGIN: Duration = codex_alab_core::session::PERSON_LINE_MARGIN;

/// How long after this process's start the launch probe's read still speaks for the start.
pub const LAUNCH_PROBE_WINDOW: Duration = codex_alab_core::session::LAUNCH_PROBE_WINDOW;

/// Whether aterm's first `status` read at this start shows aterm's own crash relaunch typing
/// the line that started this process: the harness's hard lease on the tab (aterm takes it
/// before it types the relaunch line and holds it to the new process's first idle; a person's
/// own `codex resume <id>` has no such hand), and no person's keystroke from
/// [`PERSON_LINE_MARGIN`] before this start up to the read (the person did not type the
/// line). The lease must be named for the aterm process that answered the read (the socket's
/// kernel peer, [`LaunchWitness::server_pid`]): a lease named for another pid is not aterm's.
/// The name alone binds only the NAME to aterm: aterm's pid is public, so a client of the
/// socket could take a lease under exactly that name. So where aterm reports who took the
/// lease (`status hand_pid=`, the taking connection's kernel peer), that pid must be the
/// server's too: a lease taken by any other process is no proof, whatever its name, and
/// `hand_pid=-` (a taker aterm cannot name) is none either. An aterm that prints no
/// `hand_pid=` keeps the name check alone. A read made more than [`LAUNCH_PROBE_WINDOW`]
/// after the start, an unknown `human_ms`, an unknown server pid, another hand or none: no
/// proof (fail closed).
///
/// [`LaunchWitness::server_pid`]: codex_alab_core::session::LaunchWitness::server_pid
pub fn aterm_relaunch_seen(
    witness: &codex_alab_core::session::LaunchWitness,
    started_at_ms: u64,
) -> bool {
    witness.proves_relaunch(started_at_ms)
}

/// A swept record is aterm's crash relaunch of THIS process only when all hold:
/// - no orderly end was written: `exit` absent (a panic, SIGKILL, power loss),
///   `signal:15` (aterm's stall or memory remedy) or `signal:1` (the terminal went away:
///   aterm's own crash, which its restore relaunches); a `fatal` exit is no crash witness
///   (aterm's crash rule reads it as orderly, and with its Codex lane retired nothing
///   relaunches it), and a SIGINT is a person's;
/// - the predecessor died working (`status` `busy`): an idle or waiting session owed no
///   continuation, and a crash must not make one;
/// - it was written within [`RELAUNCH_WINDOW`] of this start (an old record is a person's
///   later resume);
/// - this process was started with exactly the record's `resumeArgv`, the line aterm types
///   (its absolute launch path first);
/// - the predecessor was not itself in a carry-on chain (its record's `carriedOn` below
///   [`MAX_CARRY_ONS`]: no carry-on since a person's turn or real work): one stall's restart
///   carries on, as aterm's remedy intends; a continuation that stalls again, at once or
///   after turns of the autopilot's own, waits for the person, so stall, restart, continue
///   never loops;
/// - aterm's first `status` read at this start shows aterm typing that line
///   ([`aterm_relaunch_seen`]): under the managed link a person's own `codex resume <id>`
///   runs that very argv (the link execs the package by its absolute path), so the argv alone
///   proves nothing. No read, no proof: the session waits for the person.
pub fn relaunched_by_aterm(
    predecessor: &codex_alab_core::session::Predecessor,
    args: &[std::ffi::OsString],
    started_at_ms: u64,
    witness: Option<&codex_alab_core::session::LaunchWitness>,
) -> bool {
    predecessor.status.as_deref() == Some("busy")
        && predecessor.carried_on < MAX_CARRY_ONS
        && aterm_typed_the_line(predecessor, args, started_at_ms, witness)
}

/// aterm's crash relaunch typed the line that started this process, as the swept record and
/// the launch read prove it ([`relaunched_by_aterm`]'s witness, less what the carry-on alone
/// asks: the busy status and the chain): no orderly end, the record within
/// [`RELAUNCH_WINDOW`], this process started with exactly its `resumeArgv`, and aterm's lease
/// at the launch read ([`aterm_relaunch_seen`]).
fn aterm_typed_the_line(
    predecessor: &codex_alab_core::session::Predecessor,
    args: &[std::ffi::OsString],
    started_at_ms: u64,
    witness: Option<&codex_alab_core::session::LaunchWitness>,
) -> bool {
    let window = u64::try_from(RELAUNCH_WINDOW.as_millis()).unwrap_or(u64::MAX);
    codex_alab_core::session::exit_relaunchable(predecessor.exit.as_deref())
        && started_at_ms.saturating_sub(predecessor.updated_at_ms) <= window
        && predecessor.resume_argv.as_ref().is_some_and(|argv| {
            argv.len() == args.len()
                && argv
                    .iter()
                    .zip(args)
                    .all(|(want, got)| got.to_str() == Some(want.as_str()))
        })
        && witness.is_some_and(|witness| aterm_relaunch_seen(witness, started_at_ms))
}

/// The act `decision` types where its wait is carried (a continuation-type act, or A WALL'S
/// TRY: a retry, a usage window's resume, a quota's try; [`decide::carried_rule`]), under the
/// Autopilot word `mode`, as the thread owes it from `not_before` on: what the record and the
/// live upgrade carry while it waits ([`Autopilot::schedule_act`]), so a crash at rest in a
/// wall's wait hands aterm's relaunch the try at its rung, as aterm's loop types it from the
/// ladder its books carry. A decision that names the act the record carries in its place
/// ([`decide::Decision::in_record`]: a measured return's `api-back@v1` a check holds back)
/// carries that one. `None` for any other act (an attention, a compaction), whose wait
/// carries nothing. A wall's try carries `landing`, the thread's
/// [`decide::ThreadState::landing`] as it was decided: whether it delivers a land act again
/// ([`Owed::landing`]), and every owed act the native point it was decided at
/// (`native_origin`, [`Owed::native_origin`]).
pub(crate) fn owes_of(
    decision: &decide::Decision,
    mode: decide::Mode,
    not_before: Instant,
    landing: bool,
    native_origin: Option<codex_alab_core::session::NativeOrigin>,
) -> Option<Owed> {
    let (rule, text, resume_goal) = match decision.in_record.as_ref().unwrap_or(&decision.act) {
        Act::Type { rule, text } => (decide::carried_rule(rule)?, text, false),
        Act::ResumeLimit { text } => (crate::words::RULE_LIMIT_RESUME, text, true),
        _ => return None,
    };
    Some(Owed {
        native_origin,
        rule,
        text: text.clone(),
        mode,
        not_before,
        repo_owed: decision.repo_owed.clone(),
        resume_goal,
        landing: landing && decide::is_wall_rule(rule),
    })
}

/// The record's form of thread `thread`'s act owed (`owes`, its time turned into the epoch's
/// clock) or its landing.
fn owed_act_of(
    thread: &str,
    owes: Option<&TrackedOwed>,
    landed: bool,
    native_origin: Option<&codex_alab_core::session::NativeOrigin>,
    reading: Option<codex_alab_core::wake::Reading>,
) -> codex_alab_core::session::OwedAct {
    let now = clock();
    let epoch = now_ms();
    let owes = owes.map(|owed| owed.at_reading(reading, now));
    owed_act_at(thread, owes.as_ref(), landed, native_origin, now, epoch)
}

fn owed_act_at(
    thread: &str,
    owes: Option<&Owed>,
    landed: bool,
    native_origin: Option<&codex_alab_core::session::NativeOrigin>,
    now: Instant,
    epoch: u64,
) -> codex_alab_core::session::OwedAct {
    let not_before_ms = owes.map_or(0, |owed| {
        crate::recovery::epoch_of(owed.not_before, now, epoch)
    });
    codex_alab_core::session::OwedAct {
        renewal: None,
        thread: thread.to_string(),
        native_origin: if landed {
            native_origin.cloned()
        } else {
            owes.and_then(|owed| owed.native_origin.clone())
        },
        rule: owes.map(|owed| owed.rule.to_string()),
        text: owes.map(|owed| owed.text.clone()),
        mode: owes.map(|owed| owed.mode.word().to_string()),
        not_before_ms,
        repo_owed: owes.and_then(|owed| owed.repo_owed.clone()),
        resume_goal: owes.is_some_and(|owed| owed.resume_goal),
        landing: owes.is_some_and(|owed| owed.landing),
        landed,
    }
}

/// The epoch's time (ms) of `at`, an instant this process's clock has passed.
fn epoch_ms_of(at: Instant) -> u64 {
    let ago = u64::try_from(clock().saturating_duration_since(at).as_millis()).unwrap_or(u64::MAX);
    now_ms().saturating_sub(ago)
}

/// This process's instant for the epoch's time `ms` (ms since the epoch), one gone by: now for
/// one this clock cannot reach back to.
fn instant_of(ms: u64) -> Instant {
    let now = clock();
    now.checked_sub(Duration::from_millis(now_ms().saturating_sub(ms)))
        .unwrap_or(now)
}

/// This session's share of the reset jitter: stable per thread and process.
fn jitter_for(key: &str, max: Duration) -> Duration {
    use std::hash::Hash;
    use std::hash::Hasher;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    let max_ms = u64::try_from(max.as_millis()).unwrap_or(0);
    if max_ms == 0 {
        return Duration::ZERO;
    }
    Duration::from_millis(hasher.finish() % max_ms)
}

/// The wall a terminal turn error names (CONTRACT §5 rows API/stream, usage, context,
/// login), or `None` for an error that is no wall.
pub fn classify(
    error: &CodexErrorInfo,
    details: &CodexErrorDetails,
    auto_compact: bool,
) -> Option<Wall> {
    let status = |code: &Option<u16>| match code {
        Some(code) => format!("an API error ({code})"),
        None => "an API error".to_string(),
    };
    // Transport wrappers retain the status even when core has exhausted its retries.
    // A permanent client refusal needs a corrected request or access, never another turn
    // on the server-error ladder. A disconnected stream can carry the same refusal.
    let http_status = match error {
        CodexErrorInfo::HttpConnectionFailed { http_status_code }
        | CodexErrorInfo::ResponseStreamConnectionFailed { http_status_code }
        | CodexErrorInfo::ResponseTooManyFailedAttempts { http_status_code }
        | CodexErrorInfo::ResponseStreamDisconnected { http_status_code } => *http_status_code,
        _ => None,
    };
    if let Some(code) = http_status {
        if code == 401 {
            return Some(Wall::Login);
        }
        if codex_alab_core::capacity::permanent_client_status(code) {
            return Some(Wall::Fatal {
                why: format!(
                    "the API refused the request ({code}); correct the request, model access or account configuration before retrying"
                ),
            });
        }
    }
    Some(match error {
        CodexErrorInfo::ContextWindowExceeded => Wall::Context { auto_compact },
        CodexErrorInfo::UsageLimitExceeded => match details {
            CodexErrorDetails::UsageLimitReached(e) => {
                let reset = e.resets_at.as_ref();
                Wall::Usage {
                    reset_at: reset.map(|at| {
                        let wait_ms = at
                            .timestamp_millis()
                            .saturating_sub(i64::try_from(now_ms()).unwrap_or(i64::MAX));
                        clock() + Duration::from_millis(u64::try_from(wait_ms).unwrap_or(0))
                    }),
                    reset_label: reset
                        .map(|at| at.with_timezone(&chrono::Local).format("%H:%M").to_string()),
                }
            }
            CodexErrorDetails::QuotaExceeded | CodexErrorDetails::UsageNotIncluded => {
                Wall::Entitlement
            }
            _ => Wall::Usage {
                reset_at: None,
                reset_label: None,
            },
        },
        CodexErrorInfo::SessionBudgetExceeded => Wall::Fatal {
            why: "the session's token budget is spent".to_string(),
        },
        CodexErrorInfo::Unauthorized => Wall::Login,
        CodexErrorInfo::CyberPolicy => Wall::Policy,
        CodexErrorInfo::BioPolicy | CodexErrorInfo::MisalignmentPolicyViolation => Wall::Fatal {
            why: "the model's usage policy refused the turn".to_string(),
        },
        CodexErrorInfo::InvalidPrompt | CodexErrorInfo::BadRequest => Wall::Fatal {
            why: "the API refused the request as invalid".to_string(),
        },
        // The provider names the model gone or out of this account's reach (core never
        // retries it): another turn on the same model meets the same answer.
        CodexErrorInfo::ModelNotFound => Wall::Fatal {
            why: "the API does not offer this model to this account; choose another model or check its access before retrying".to_string(),
        },
        CodexErrorInfo::HttpConnectionFailed {
            http_status_code: None,
        }
        | CodexErrorInfo::ResponseStreamConnectionFailed {
            http_status_code: None,
        } => Wall::Network {
            what: "an API error (the API could not be reached)".to_string(),
        },
        CodexErrorInfo::ResponseStreamDisconnected { http_status_code } => Wall::Server {
            what: match http_status_code {
                Some(code) => format!("an API error ({code}, the reply was cut off)"),
                None => "an API error (the reply was cut off)".to_string(),
            },
            cut_off: true,
            retry_at: None,
        },
        CodexErrorInfo::HttpConnectionFailed { http_status_code }
        | CodexErrorInfo::ResponseStreamConnectionFailed { http_status_code }
        | CodexErrorInfo::ResponseTooManyFailedAttempts { http_status_code } => Wall::Server {
            what: status(http_status_code),
            cut_off: false,
            retry_at: None,
        },
        CodexErrorInfo::ServerOverloaded | CodexErrorInfo::FlexUnavailable => Wall::Server {
            what: "an API error (overloaded)".to_string(),
            cut_off: false,
            retry_at: None,
        },
        CodexErrorInfo::PolicyCapacityUnavailable => Wall::Server {
            what: "an API error (policy-restricted capacity unavailable)".to_string(),
            cut_off: false,
            retry_at: None,
        },
        CodexErrorInfo::InternalServerError => Wall::Server {
            what: "an API error (500)".to_string(),
            cut_off: false,
            retry_at: None,
        },
        CodexErrorInfo::RateLimitExceeded => Wall::Server {
            what: "an API error (429)".to_string(),
            cut_off: false,
            retry_at: None,
        },
        // Core names a stream that broke after its own retries `Other`; its details say
        // what it was: a reply cut off (at once, then the network's 1/2/5 min ladder, as
        // aterm's `CutOff`), a request that timed out, or a reply past its output token
        // maximum.
        CodexErrorInfo::Other => match details {
            CodexErrorDetails::Stream(_) => Wall::Server {
                what: "an API error (the reply was cut off)".to_string(),
                cut_off: true,
                retry_at: None,
            },
            CodexErrorDetails::Timeout | CodexErrorDetails::RequestTimeout => Wall::Server {
                what: "an API error (the request timed out)".to_string(),
                cut_off: false,
                retry_at: None,
            },
            // A reply past its output token maximum (aterm's `ApiCause::Overlong`, which
            // core never retries): retried in smaller pieces. Any other reason a response
            // stopped short is no retry's.
            CodexErrorDetails::ResponseIncomplete(incomplete)
                if incomplete.reason == "max_output_tokens" =>
            {
                Wall::Overlong {
                    what: "an API error (the reply was over its output token maximum)".to_string(),
                }
            }
            CodexErrorDetails::Io(_)
            | CodexErrorDetails::Json(_)
            | CodexErrorDetails::TokioJoin(_) => Wall::Server {
                what: "an API error (processing the response failed)".to_string(),
                cut_off: false,
                retry_at: None,
            },
            CodexErrorDetails::InvalidRequest(_)
            | CodexErrorDetails::InvalidImageRequest()
            | CodexErrorDetails::ToolCollision(_) => Wall::Fatal {
                why: "Codex refused the request or tool configuration; correct it before retrying"
                    .to_string(),
            },
            CodexErrorDetails::Fatal(_) | CodexErrorDetails::EnvVar(_) => Wall::Fatal {
                why: "Codex reported a fatal configuration error".to_string(),
            },
            CodexErrorDetails::ContentFilter => Wall::Fatal {
                why: "the model's content filter stopped the reply".to_string(),
            },
            CodexErrorDetails::TurnAborted | CodexErrorDetails::Interrupted => return None,
            _ => Wall::Fatal {
                why: "Codex ended the turn with an error that needs correction before retrying"
                    .to_string(),
            },
        },
        CodexErrorInfo::TooManyDenials | CodexErrorInfo::SandboxError => Wall::Fatal {
            why: "Codex could not proceed under its approval or sandbox configuration".to_string(),
        },
        // These reject a separate thread operation, rather than end the running turn.
        CodexErrorInfo::ThreadRollbackFailed | CodexErrorInfo::ActiveTurnNotSteerable { .. } => {
            return None;
        }
    })
}

impl ThreadLifecycleContributor<Config> for Autopilot {
    fn on_thread_start<'a>(
        &'a self,
        input: ThreadStartInput<'a, Config>,
    ) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            if codex_alab_core::active() {
                self.sup.shared().invalidate_upgrade();
            }
            if !(codex_alab_core::active() || codex_alab_core::is_alab_binary()) {
                return;
            }
            if input.session_source.is_non_root_agent() {
                self.adopt_sub_agent(input.thread_store, input.session_source);
                return;
            }
            if !(self.gate)(input.session_source) {
                return;
            }
            let Ok(id) = ThreadId::from_string(input.thread_store.level_id()) else {
                return;
            };
            codex_alab_tui::session::start(self.sup.shared(), &input.config.codex_home);
            let rt = ThreadRt::new(
                id,
                Some(Arc::new(input.config.clone())),
                input
                    .config
                    .features
                    .enabled(codex_features::Feature::TokenBudget),
                jitter_for(&id.to_string(), self.timing.reset_jitter),
            );
            // An Off that stood before this thread was its memory's business only before it.
            rt.off_seen.store(self.sup.offs(), Ordering::SeqCst);
            // A THREAD THAT CARRIES ON ANOTHER'S WORK (edges-3; review of the needs-edges lane):
            // plan mode's "clear context and implement" submits the plan as this thread starts,
            // and a provider error's continuation fills its composer with "let's continue where
            // we left off"; the TUI names each as it starts it. Its source worked (the plan's
            // turn, or the one the provider's error ended, ended there), so this is no new
            // session: past its opening, as aterm sees no taskless screen before the plan's
            // turn. A new chat (`/new`, `/clear`) names none and opens its own.
            if self.sup.shared().take_continuation().is_some() {
                lock(&rt.state).seed_worked();
            }
            rt.measured_by(&self.sup);
            input.thread_store.insert(rt);
            self.sup.shared().note_root_thread(&id.to_string());
            if let Some(rt) = input.thread_store.get::<ThreadRt>() {
                let mut threads = lock(&self.threads);
                let replaces = threads
                    .iter()
                    .filter_map(Weak::upgrade)
                    .any(|old| old.key == rt.key);
                if replaces {
                    // Retire the old instance before the replacement can publish. Late
                    // callbacks retain only their old Arc, not authority over this slot.
                    for old in threads
                        .iter()
                        .filter_map(Weak::upgrade)
                        .filter(|old| old.key == rt.key)
                    {
                        old.stopped.store(true, Ordering::SeqCst);
                        self.sup.retire_description(&old.description_owner);
                    }
                    self.sup.shared().thread_ended(&rt.key);
                    self.clear_recovery(&rt.key);
                    self.sup.clear_thread_attention(&rt.key);
                }
                threads.retain(|t| t.upgrade().is_some_and(|old| old.key != rt.key));
                threads.push(Arc::downgrade(&rt));
                self.sync_owed_from(&threads);
            }
            self.sup.start();
            if let Some(me) = self.self_arc() {
                me.watch_claims();
                me.watch_launch();
            }
        })
    }

    fn on_thread_ready<'a>(
        &'a self,
        input: ThreadReadyInput<'a, Config>,
    ) -> ExtensionFuture<'a, ()> {
        // A FORK IS PAST ITS OPENING (gap 32, review finding 4): its history is a copy of a
        // conversation's that ran, which no resume scan reads (core resumes no fork), so a
        // person's message on it is never the session's first. Core awaits this hook once the
        // thread is registered, where a hook must not suspend (`emit_thread_ready_lifecycle`:
        // a cancellation there can strand the registered thread before its cleanup is
        // installed), so the thread's source is read on a task of its own: the registry's lock
        // and the session's config are taken there, never on core's path. The read ends in a
        // moment, long before a person's turn can end on the thread.
        let rt = Self::rt(input.thread_store);
        let me = self.self_arc();
        if let (Some(rt), Some(me)) = (rt, me) {
            tokio::spawn(async move {
                let Some(thread) = me.thread(rt.id).await else {
                    return;
                };
                if thread
                    .config_snapshot()
                    .await
                    .forked_from_thread_id
                    .is_some()
                {
                    me.seed_worked(&rt);
                }
            });
        }
        Box::pin(async {})
    }

    fn on_thread_resume<'a>(&'a self, input: ThreadResumeInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            let Some(rt) = Self::rt(input.thread_store) else {
                // A resume under vanilla runs no resume scan: the thread is unwatched, and the
                // first decision's look once codex-alab looks again reads its history, past its
                // opening, and stands at its last turn end (edges-2, [`rewatch_bound`]).
                Self::skipped(input.thread_store);
                return;
            };
            rt.resumed.store(true, Ordering::SeqCst);
            if let Some(owed) = self
                .sup
                .shared()
                .owed_act()
                .filter(|owed| owed.thread == rt.key && owed.renewal.is_some())
                && self.reconcile_renewal_goal(&rt, &owed).await != Ok(false)
                && self.eligible_login_scope(&rt)
                && let Some(runtime) = input
                    .thread_store
                    .get::<codex_goal_extension::GoalRuntimeHandle>()
                    .filter(|runtime| runtime.alab_auth_recovery_enabled())
                && let Some(origin) = owed.native_origin.as_ref()
            {
                let _ = runtime.alab_defer_auth_recovery(&origin.turn_id).await;
            }
            if let Some(me) = self.self_arc() {
                tokio::spawn(me.resume_scan(rt));
            }
        })
    }

    fn on_thread_idle<'a>(&'a self, input: ThreadIdleInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            let Some(rt) = Self::rt(input.thread_store) else {
                return;
            };
            if let Some(me) = self.self_arc() {
                tokio::spawn(async move {
                    Arc::clone(&me).evaluate(rt).await;
                    // A verified idle is where a waiting upgrade is looked at again.
                    me.consider_upgrade().await;
                });
            }
        })
    }

    fn on_thread_stop<'a>(&'a self, input: ThreadStopInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            if codex_alab_core::active() {
                self.sup.shared().invalidate_upgrade();
            }
            if input.thread_store.get::<SubAgentOf>().is_some() {
                lock(&self.sub_agents).remove(input.thread_store.level_id());
            }
            let Some(rt) = Self::rt(input.thread_store) else {
                return;
            };
            {
                let kept = lock(&self.threads);
                rt.stopped.store(true, Ordering::SeqCst);
                lock(&rt.inherited_settle).take();
                if Self::current_runtime(&kept, &rt) {
                    self.sup.retire_description(&rt.description_owner);
                }
            }
            Self::cancel_timer(&rt).await;
            lock(&rt.transient_auth_goal).take();
            // AN EXIT aterm MAY RELAUNCH (SIGHUP under its own crash, SIGTERM its remedy) stops
            // every thread through `/exit`'s teardown, but the conversation goes on in that
            // relaunch: the act owed at rest stays in the record the signal keeps, for the
            // relaunch to type at its time (`Relaunch::Owed`), as after a crash at rest. Nothing
            // is typed here: the timer is cancelled. Any other stop ends what was owed.
            let relaunch = self.sup.shared().leaving_for_relaunch();
            let current = {
                let mut kept = lock(&self.threads);
                let current = Self::current_runtime(&kept, &rt);
                if current {
                    self.sup.shared().thread_ended(&rt.key);
                    if !relaunch {
                        self.clear_recovery(&rt.key);
                        self.sup.clear_thread_attention(&rt.key);
                    }
                }
                if !relaunch {
                    kept.retain(|entry| entry.upgrade().is_some_and(|old| !Arc::ptr_eq(&old, &rt)));
                    self.sync_owed_from(&kept);
                }
                current
            };
            if current && !relaunch {
                self.sync_record_owed();
            }
        })
    }
}

impl TurnLifecycleContributor for Autopilot {
    #[expect(
        clippy::await_holding_invalid_type,
        reason = "the async lifecycle gate orders native start and completion through publication and durable record writes; core awaits this hook before running the turn, and no synchronous lock crosses an await"
    )]
    fn on_turn_start<'a>(&'a self, input: TurnStartInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            if codex_alab_core::is_alab_binary() && !codex_alab_core::active() {
                Self::skipped_turn(input.thread_store, input.turn_store).await;
                return;
            }
            self.sup.start();
            if let Some(me) = self.self_arc() {
                me.watch_claims();
                me.watch_launch();
            }
            let shared = self.sup.shared();
            let Some(rt) = Self::rt(input.thread_store) else {
                Self::skipped_turn(input.thread_store, input.turn_store).await;
                if codex_alab_core::active() {
                    shared.invalidate_upgrade();
                }
                return;
            };
            lock(&rt.transient_auth_goal).take();
            // THE SESSION BEGINS at the thread's first turn (review of the forever probe,
            // land-probe-head-only): the branches each worktree's `HEAD` moved from or to since
            // are ones it used. A scope the record carried keeps the earlier start.
            self.move_scope(&rt, |scope| {
                let first = scope.since.is_none();
                if first {
                    scope.since = Some(now_ms() / 1000);
                }
                first
            });
            let quota_identity = self.quota_identity();
            let mut native_turn = rt.native_turn.lock().await;
            // A person's stop counted before the goal is read: one that races the read is the
            // turn's own, so its goal is no wall's.
            let person_stops_at_start = self.sup.shared().person_goal_stops(&rt.key);
            let goal_at_start = if let Some(thread) = self.thread(rt.id).await
                && let Some(db) = thread.state_db()
            {
                db.thread_goals()
                    .get_thread_goal(rt.id)
                    .await
                    .ok()
                    .flatten()
            } else {
                None
            };
            let goal_at = |status| goal_at_start.clone().filter(|goal| goal.status == status);
            let limited_goal_at_start = goal_at(codex_state::ThreadGoalStatus::UsageLimited);
            let active_goal_at_start = goal_at(codex_state::ThreadGoalStatus::Active);
            // Only this thread's async lifecycle gate is held while waiting. Core awaits
            // this hook before the turn runs; no synchronous state lock crosses the wait.
            // An older task-face write must have settled first.
            let publication = self.sup.task_face_publication().await;
            let (started, waited, accepted_turns) = {
                let kept = lock(&self.threads);
                if rt.stopped.load(Ordering::SeqCst) || !Self::current_runtime(&kept, &rt) {
                    return;
                }
                *lock(&rt.quota_wall) = None;
                *lock(&rt.human_goal_recovery) = None;
                *lock(&rt.quota_request) = quota_identity.map(|identity| {
                    let generation = shared.quota().select_owner(
                        &identity.account,
                        identity.user.as_deref(),
                        identity.owner,
                    );
                    (identity, generation)
                });
                *native_turn = Some(input.turn_store.level_id().to_owned());
                let started = shared.turn_started(&rt.key, input.turn_store.level_id());
                let retry_thread = rt.key.clone();
                let retry_turn = input.turn_store.level_id().to_owned();
                input.turn_store.insert(codex_alab_core::capacity::InFlightRetryObserver::new(move |deadline| {
                let changed = match deadline {
                    Some(deadline) => {
                        let now = clock();
                        let epoch = now_ms();
                        shared.publish_in_flight_retry(codex_alab_core::session::InFlightRetry {
                            thread: retry_thread.clone(),
                            turn_id: retry_turn.clone(),
                            not_before_ms: crate::recovery::epoch_of(deadline, now, epoch),
                        })
                    }
                    None => shared.clear_in_flight_retry(&retry_thread, &retry_turn),
                };
                // A core retry task is off the render/input paths. Persist before it
                // reports the wait, and clear before a cancelled task can end its turn.
                if changed && !shared.flush_record() {
                    tracing::warn!(thread = %retry_thread, turn = %retry_turn, "could not persist in-turn provider retry pacing");
                }
            }));
                let accepted_turns = rt.accepted_turns.fetch_add(1, Ordering::SeqCst) + 1;
                lock(&rt.inherited_settle).take();
                let track = input.turn_store.get_or_init(TurnTrack::default);
                {
                    let mut data = lock(&track.0);
                    data.started = Some(clock());
                    data.native_epoch = Some(accepted_turns);
                    data.limited_goal_at_start = limited_goal_at_start;
                    data.active_goal_at_start = active_goal_at_start;
                    data.person_stops_at_start = person_stops_at_start;
                }
                let unread = {
                    let mut state = lock(&rt.state);
                    state.owed = None;
                    state.point_turn_id = None;
                    state.native_origin = Some(codex_alab_core::session::NativeOrigin {
                        turn_id: input.turn_store.level_id().to_string(),
                        phase: codex_alab_core::session::NativePhase::Running,
                    });
                    state.carry_on_not_before = None;
                    state.observe_turn_start()
                };
                // A TURN BEGUN REOPENS THE LINE'S LANDED OR QUIET FACE, whoever's it is, as
                // aterm reopens its own at a turn begun: the turn runs now.
                self.publish_face(&kept, &rt);
                // ANOTHER'S TURN TAKES THE LAST YIELD'S READING DOWN AS IT BEGINS (aterm's
                // `took`): Land's hold stands with no legs read while it runs.
                if unread {
                    self.tell_land_from(&kept, &rt);
                }
                self.clear_recovery(&rt.key);
                if shared.owed_act().is_some_and(|act| act.thread == rt.key) {
                    shared.set_owed_act(None);
                }
                // A turn runs, codex-alab's own act or a person's: the thread waits for
                // nothing, and its `owner=codex-alab-wait` entry comes down. Under this gate,
                // so a decision's wait armed before it is taken down and one decided before it
                // is never armed after it (`schedule_decided_act`).
                let waited = self.sup.want_wait_for(&rt.key, None);
                (started, waited, accepted_turns)
            };
            drop(publication);
            // The current native-start gate remains held. A fresh accepted ID
            // supersedes completed/orphaned auth proof from an older ID; an
            // original-ID recovery retains its own proof through admission.
            if let Some(thread) = self.thread(rt.id).await
                && let Some(db) = thread.state_db()
                && let Err(error) = db
                    .thread_goals()
                    .alab_native_auth_turn_started(rt.id, input.turn_id)
                    .await
            {
                tracing::warn!(%error, "could not retire superseded auth custody at native start");
            }
            // An accepted turn supersedes every source of the old pending act before
            // either cancellation or persistence can yield to another record write.
            Self::cancel_timer(&rt).await;
            // The record says busy before the turn runs (CONTRACT §4), a person's turn or
            // codex-alab's: a local write, bounded, never an aterm call.
            if started && !write_through(shared).await {
                self.sup.note(format!(
                    "{}: the session record did not take the turn's start",
                    rt.key
                ));
            }
            // A turn runs: the wall's measure is for no point now, and the lost login's watch
            // ends (the turn's end reads the login again).
            lock(&rt.net).take();
            lock(&rt.auth_observation).take();
            if rt.end_login_watch() {
                self.sup.note(format!(
                    "{}: a turn runs; the lost login's watch ends",
                    rt.key
                ));
            }
            self.sync_record_owed();
            // The local busy/owed record must settle before any remote attention I/O.
            // A new accepted root turn supersedes its preceding refusal even while the
            // answer is running; other attention sources retain their owners.
            if shared.is_root_thread(&rt.key) {
                self.attention_for(&rt, "wall", None).await;
            }
            // On a task of its own: core awaits this hook before the turn runs, and a wedged
            // aterm must not hold the turn (review of f134d2b2cb, finding 4).
            if waited {
                self.sup.show_waits_later();
            }
            // THE START PROBE OVER A LANDING (aterm main e0da8d68df's `wake_began_over`), once
            // the record says busy: the turn may be the landed task's own wake, and what the
            // repository holds before it runs is none of its work. Core awaits this before the
            // turn runs, so the wake's own edits are never read as someone else's; bounded by
            // `repo::START_PROBE_LIMIT`, past which it is unread and proves nothing unchanged.
            if !self.park_if_off(&rt).await {
                return;
            }
            if lock(&rt.state).wants_start_probe() {
                let found = self
                    .probe_repo(&rt, "the wake's start probe", repo::START_PROBE_LIMIT)
                    .await;
                if !self.park_if_off(&rt).await {
                    return;
                }
                if rt.accepted_turns.load(Ordering::SeqCst) == accepted_turns {
                    lock(&rt.state).wake_began_over(found);
                }
            }
            // A goal set or moved starts a turn: the description follows it, and a goal that
            // reads active again has answered the `goal:` attention it left.
            if let Some(me) = self.self_arc() {
                // Captured at this native start, before the detached description read.
                // A later turn (even reusing an ID) cannot lend it clear authority.
                let rt = Arc::clone(&rt);
                tokio::spawn(async move {
                    me.refresh_description(&rt).await;
                    me.goal_moved(&rt, accepted_turns).await;
                });
            }
        })
    }

    fn on_item_completed<'a>(
        &'a self,
        thread_store: &'a ExtensionData,
        turn_store: &'a ExtensionData,
        item: &'a TurnItem,
    ) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            let Some(rt) = Self::rt(thread_store) else {
                // A SUB-AGENT'S EDIT lands in its root session's scope ([`SubAgentOf`]).
                if let TurnItem::FileChange(change) = item
                    && let Some(root) = self.sub_agent_root(thread_store)
                {
                    self.touch_scope(&root, &patched_dirs(change));
                }
                Self::skipped(thread_store);
                return;
            };
            let track = turn_store.get_or_init(TurnTrack::default);
            match item {
                TurnItem::UserMessage(message) => {
                    let (by_person, ours) = {
                        let kept = lock(&self.threads);
                        if rt.stopped.load(Ordering::SeqCst) || !Self::current_runtime(&kept, &rt) {
                            return;
                        }
                        lock(&track.0).saw_regular_input = true;
                        // A regular message selects its own program, including Standard/None.
                        // A compaction has no user item and must not discard this selection.
                        *lock(&rt.cyber_access_program) =
                            turn_store.get::<CyberAccessProgram>().as_deref().copied();
                        // A turn the TUI submitted for a typed `Inbox:` line: its end answers the
                        // mail (only a root thread's; a mail turn is still a person's, who typed
                        // the line).
                        if self.sup.shared().is_root_thread(&rt.key)
                            && let Some(mail) =
                                self.sup.shared().take_mail_turn(&user_text(message))
                        {
                            lock(&track.0).mail = Some(mail);
                        }
                        // codex-alab's own: its submission, or the turn the TUI submitted for it
                        // when its glue accepted the plan box's yes.
                        let ours = message
                            .client_id
                            .as_deref()
                            .is_some_and(|id| id.starts_with(CLIENT_PREFIX))
                            || self.sup.shared().take_own_turn(&user_text(message));
                        let by_person = if ours {
                            lock(&track.0).by_autopilot = true;
                            false
                        } else if turn_store
                            .get::<Arc<dyn codex_analytics::TurnAnalyticsMetadata>>()
                            .is_none_or(|metadata| {
                                metadata.turn_trigger().is_none_or(|trigger| {
                                    PERSON_TRIGGERS.contains(&trigger.as_str())
                                })
                            })
                        {
                            lock(&track.0).by_person = true;
                            true
                        } else {
                            false
                        };
                        (by_person, ours)
                    };
                    // A MESSAGE THAT IS NOT CODEX-ALAB'S OWN DIRECTS THE ROOT THREAD
                    // (`upgrade_ask`; review of gap 27, finding 1; aterm's `Turn::Direction`,
                    // which no native automatic turn is): from here the round hears the
                    // notice's answer by its reference alone, and a readiness the agent said is
                    // superseded, the round asking again at once. Codex-alab's own act (a
                    // continuation, a wall's try) supersedes nothing: the agent's reply to it is
                    // heard as the notice's answer still.
                    if !ours && self.sup.shared().is_root_thread(&rt.key) {
                        lock(&self.ask).directed();
                    }
                    if by_person {
                        // THE PERSON'S HAND (aterm's `hand`): their last window gesture as
                        // `status human_ms=` names it now, read before the turn runs (core
                        // awaits this hook; bounded). A message a control write typed stamps
                        // none, and none read is no hand.
                        let hand = if self.sup.shared().is_root_thread(&rt.key) {
                            self.sup
                                .person_input_ago()
                                .await
                                .and_then(|ago| clock().checked_sub(ago))
                        } else {
                            None
                        };
                        let kept = lock(&self.threads);
                        if rt.stopped.load(Ordering::SeqCst) || !Self::current_runtime(&kept, &rt) {
                            return;
                        }
                        {
                            let mut state = lock(&rt.state);
                            state.observe_person_turn();
                            if let Some(at) = hand {
                                state.observe_person_hand(at);
                            }
                        }
                        // A READ THAT RACED THE SUPERVISOR'S CONNECTION (`codex "<question>"`
                        // submits while it connects) or ran past its bound says nothing of whose
                        // the message was: the turn's end reads the hand again (edges-6).
                        if hand.is_none() && self.sup.shared().is_root_thread(&rt.key) {
                            lock(&track.0).hand_unread = true;
                        }
                        // A person's turn ends the Land episode, and Land's hold with it.
                        self.tell_land_from(&kept, &rt);
                        // The done checks start again (and the cut act goes): a process after
                        // this one, should this turn not end here, takes up none of them.
                        self.sync_pace(&rt);
                        self.sync_owed_from(&kept);
                        self.surface_needs_from(&kept, &rt);
                        if self.sup.shared().is_root_thread(&rt.key) {
                            self.sup.shared().set_carried_on(0);
                        }
                    }
                }
                TurnItem::AgentMessage(message) => {
                    let text = agent_text(message);
                    let mut data = lock(&track.0);
                    if message.phase == Some(MessagePhase::FinalAnswer) {
                        data.final_text = Some(text.clone());
                        data.final_at = Some(data.said.len());
                    }
                    data.said.push(text.clone());
                    data.text = Some(text);
                    if message.questions.as_ref().is_some_and(|q| !q.is_empty()) {
                        data.questions = true;
                    }
                }
                // A HOSTED CALL IS A TOOL RUN (ruling F's real work, [`TurnEnd::ran_tools`]):
                // the Responses API's own web search and image generation reach core as items,
                // never through a tool start, and aterm's readers count Claude Code's
                // `⏺ Web Search(…)` row as a call row (aterm-phase's `is_tool_call`).
                TurnItem::WebSearch(_) | TurnItem::ImageGeneration(_) => {
                    lock(&track.0).ran_tools = true;
                }
                // AN EDIT LANDS IN A FOLDER THE PROBE READS (review of the forever probe,
                // land-probe-last-workdir): `apply_patch` moves no command's folder, so each
                // file it touched names its own, absolute, as core resolved it.
                TurnItem::FileChange(change) => self.touch_scope(&rt, &patched_dirs(change)),
                _ => {}
            }
        })
    }

    fn on_turn_stop<'a>(&'a self, input: TurnStopInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            if let Some(rt) = Self::rt(input.thread_store) {
                self.acknowledge_renewal_at_stop(&rt, input.turn_store)
                    .await;
                self.finish(&rt, input.turn_store, None).await;
            } else {
                Self::skipped(input.thread_store);
                if codex_alab_core::active() {
                    self.sup.shared().invalidate_upgrade();
                }
            }
        })
    }

    fn on_turn_abort<'a>(&'a self, input: TurnAbortInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            if let Some(rt) = Self::rt(input.thread_store) {
                if input.reason == codex_protocol::protocol::TurnAbortReason::Interrupted
                    && !self.sup.shared().leaving_for_relaunch()
                    && let Some(owed) = self.sup.shared().owed_act()
                    && owed.thread == rt.key
                    && owed.renewal.is_some()
                {
                    self.sup.shared().discard_renewal(&owed);
                    let _ = write_through(self.sup.shared()).await;
                }
                let interrupted = input.reason
                    == codex_protocol::protocol::TurnAbortReason::Interrupted
                    && !self.sup.shared().leaving_for_relaunch();
                self.finish(&rt, input.turn_store, Some(input.reason)).await;
                // A PERSON'S ESC ENDS IN NO IDLE (core's `abort_all_tasks` emits none): the point
                // is decided here, once core has published the abort, held for `human_grace` and
                // decided again where it ends (`decide::ESC_HOLD`), as a turn end then (owner
                // 2026-10-27: "you need to keep working even if the user interrupted a long
                // running job").
                if interrupted && let Some(me) = self.self_arc() {
                    tokio::spawn(async move {
                        me.abort_published(&rt).await;
                        me.evaluate(rt).await;
                    });
                }
            } else {
                Self::skipped(input.thread_store);
                if codex_alab_core::active() {
                    self.sup.shared().invalidate_upgrade();
                }
            }
        })
    }

    fn on_turn_error<'a>(&'a self, input: TurnErrorInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            let Some(rt) = Self::rt(input.thread_store) else {
                return;
            };
            let auto_compact = !rt.token_budget
                && input
                    .thread_store
                    .get::<codex_core::compact::AutoCompactionDisabled>()
                    .is_none();
            let mut wall = classify(&input.error, input.error_details, auto_compact);
            *lock(&rt.quota_wall) = match input.error_details {
                CodexErrorDetails::UsageLimitReached(error) => {
                    error.rate_limits.as_ref().and_then(|limits| {
                        let bucket = limits.limit_id.clone().unwrap_or_else(|| "codex".into());
                        let credit_only = limits.primary.is_none()
                            && limits.secondary.is_none()
                            && limits
                                .credits
                                .as_ref()
                                .is_some_and(|credits| !credits.has_credits && !credits.unlimited);
                        self.bind_quota_wall(&rt, bucket, clock())
                            .map(|mut binding| {
                                binding.wall = binding.wall.with_ordinary_retry(credit_only, false);
                                binding
                            })
                    })
                }
                CodexErrorDetails::QuotaExceeded | CodexErrorDetails::UsageNotIncluded => {
                    self.bind_quota_wall(&rt, "codex".into(), clock())
                        .map(|mut binding| {
                            // The provider's missing-subscription error itself proves denial,
                            // even if access returns before the first background account read.
                            if matches!(input.error_details, CodexErrorDetails::UsageNotIncluded) {
                                binding.wall = binding.wall.with_ordinary_retry(true, true);
                            }
                            binding
                        })
                }
                _ => None,
            };
            let wall_reading = self.wake_reading();
            let track = input.turn_store.get_or_init(TurnTrack::default);
            if matches!(wall, Some(Wall::Policy))
                && (!lock(&track.0).saw_regular_input
                    || lock(&rt.state).compacting
                    || input
                        .turn_store
                        .get::<codex_alab_core::compaction::Activity>()
                        .is_some_and(|activity| activity.pending()))
            {
                wall = Some(Wall::Fatal {
                    why: "a cybersecurity policy refused context compaction; automatic work cannot replay that history".into(),
                });
            }
            if let Some(Wall::Server { retry_at, .. }) = &mut wall {
                *retry_at = input
                    .turn_store
                    .get::<codex_alab_core::capacity::SamplingRetryAdvice>()
                    .and_then(|advice| advice.deadline_at(wall_reading, clock()));
            }
            let route = match &wall {
                // Usage and subscription resumes measure this route once due.
                Some(Wall::Network { .. } | Wall::Usage { .. } | Wall::Entitlement) => {
                    let route = self.route_at_wall(&rt, input.error_details).await;
                    self.sup.note(match &route {
                        netwatch::Route::Direct(dest) if self.sup.probe_off() => format!(
                            "{}: the API's reach at {}:{} is not measured (probe_api is off)",
                            rt.key, dest.host, dest.port
                        ),
                        netwatch::Route::Direct(dest) => format!(
                            "{}: the API's reach at {}:{} is measured while the wall stands",
                            rt.key, dest.host, dest.port
                        ),
                        netwatch::Route::Custom(why) => {
                            format!("{}: the API's reach is not measured ({why})", rt.key)
                        }
                    });
                    Some(route)
                }
                _ => None,
            };
            // THE LOST LOGIN'S BASELINE (gap 13 stage 1): the login as codex holds it once its
            // own recovery gave up, so only a change after the wall reads back.
            let scope = input
                .turn_store
                .get::<codex_alab_core::renewal::AuthRequestScope>();
            // Check normal ownership first. A later same-identity cache
            // publication is then visible to the monotonic login-back read.
            let strict = scope
                .as_ref()
                .is_some_and(|scope| scope.eligible(input.turn_id));
            let local_baseline = scope
                .as_ref()
                .filter(|scope| scope.turn == input.turn_id && scope.snapshot().auth.is_some())
                .zip(self.auth_manager())
                .filter(|(scope, manager)| scope.registered_to(manager))
                .map(|(scope, manager)| {
                    let attempt = scope.snapshot();
                    (
                        login::Baseline::from_attempt(&manager, scope, &attempt),
                        manager,
                        attempt,
                    )
                });
            let local_back = matches!(wall, Some(Wall::Login))
                && local_baseline
                    .as_ref()
                    .is_some_and(|(baseline, manager, _)| {
                        baseline.read(manager) == login::Read::Back
                    });
            // Retain one exact causal observation across every error-hook
            // boundary, including a cache that disappears after this hook.
            // Actual current ownership / fresh Back remain separate authority.
            let observation = matches!(wall, Some(Wall::Login))
                .then_some(())
                .filter(|_| self.login_provider_eligible(&rt))
                .and_then(|_| local_baseline.as_ref().zip(scope.as_ref()))
                .filter(|((_, manager, attempt), _)| {
                    attempt.credentials_changed().is_some()
                        && attempt.auth.as_ref().is_some_and(|expected| {
                            login::request_profile(manager, expected) != Some(false)
                        })
                })
                .map(|((baseline, manager, attempt), scope)| {
                    let current_manager = Arc::clone(manager);
                    let expected = attempt.auth.clone();
                    let cause = Arc::new(codex_alab_core::renewal::AuthObservation::new(
                        scope,
                        attempt,
                        move || {
                            expected.as_ref().is_some_and(|expected| {
                                login::request_profile(&current_manager, expected) != Some(false)
                            })
                        },
                    ));
                    input.turn_store.insert_arc(Arc::clone(&cause));
                    Arc::new(AuthWallObservation {
                        scope: Arc::clone(scope),
                        cause,
                        manager: Arc::clone(manager),
                        baseline: baseline.clone(),
                    })
                });
            let eligible = strict || local_back;
            // Retain an explicit refusal too: an older owed receipt cannot lend
            // a later unrelated provider's wall its former eligibility.
            let auth_scope = Some(scope.filter(|_| eligible).unwrap_or_else(|| {
                Arc::new(codex_alab_core::renewal::AuthRequestScope::new(
                    input.turn_id.into(),
                    None,
                    0,
                    0,
                    || false,
                ))
            }));
            let login = (matches!(wall, Some(Wall::Login)) && eligible)
                .then(|| self.auth_manager())
                .flatten()
                .and_then(|manager| {
                    auth_scope
                        .as_ref()
                        .map(|scope| login::Baseline::from_request(&manager, scope))
                })
                .or_else(|| {
                    observation
                        .as_ref()
                        .map(|observation| observation.baseline.clone())
                });
            let mut data = lock(&track.0);
            if wall.is_some() {
                data.wall = wall;
                data.route = route;
                data.login = login;
                data.auth_scope = auth_scope;
                data.auth_observation = observation;
                data.wall_asleep = wall_reading.map(|reading| reading.asleep);
            }
        })
    }
}

impl ToolLifecycleContributor for Autopilot {
    /// A builtin command starts: the folder it runs in is where the worker works now, and one
    /// the repository probe reads (a folder of another executor's convention is none), as is
    /// each folder its script `cd`s into or points git at (`repo::command_reach`; review of the
    /// forever probe at f48a215d7b, R8); the branches it creates or checks out are ones the
    /// session used, each in the folder it was named in (`repo::Used`).
    fn on_command_start<'a>(&'a self, input: CommandStartInput<'a>) -> ToolLifecycleFuture<'a> {
        let cwd = || {
            input
                .cwd
                .to_abs_path()
                .ok()
                .map(|dir| dir.as_path().to_path_buf())
        };
        if let Some(rt) = Self::rt(input.thread_store) {
            let cwd = cwd();
            if let Some(dir) = &cwd {
                // The record carries it too, for the image after a re-exec or a relaunch.
                self.sup.shared().set_workdir(&rt.key, dir);
                *lock(&rt.workdir) = Some(dir.clone());
            }
            self.reach_scope(&rt, input.command, cwd.as_deref());
        } else if let Some(root) = self.sub_agent_root(input.thread_store) {
            // A SUB-AGENT'S COMMAND reaches into its root session's scope ([`SubAgentOf`]); the
            // root's own folder, where the probe reads first, stays its own worker's.
            self.reach_scope(&root, input.command, cwd().as_deref());
        }
        Box::pin(std::future::ready(()))
    }

    /// `update_plan`: the step in progress is what the description says the thread is doing.
    /// Any other tool but a question's is RULING F'S REAL WORK ([`TurnEnd::ran_tools`]).
    fn on_tool_start<'a>(&'a self, input: ToolStartInput<'a>) -> ToolLifecycleFuture<'a> {
        // Read synchronously, then only spawn: nothing here waits on aterm.
        // Core names every plain tool in the default namespace (`functions`: its router's
        // `ToolName::with_default_namespace`), so a plain tool is one in that namespace, as
        // upstream's goal extension reads it; an MCP server's tool of the same name is none.
        if !(input.tool_name.is_default_namespace()
            && NOT_WORK_TOOLS.contains(&input.tool_name.name.as_str()))
            && let Some(track) = input.turn_store.get::<TurnTrack>()
        {
            lock(&track.0).ran_tools = true;
        }
        if input.tool_name.is_default_namespace()
            && [
                codex_goal_extension::UPDATE_GOAL_TOOL_NAME,
                codex_goal_extension::CREATE_GOAL_TOOL_NAME,
            ]
            .contains(&input.tool_name.name.as_str())
            && let Some(track) = input.turn_store.get::<TurnTrack>()
        {
            lock(&track.0).goal_tool = true;
        }
        if input.tool_name.is_default_namespace()
            && input.tool_name.name == UPDATE_PLAN
            && let Some(rt) = Self::rt(input.thread_store)
            && let ToolPayload::Function { arguments } = input.payload
            && let Some(step) = crate::describe::plan_step(arguments)
        {
            *lock(&rt.plan_step) = step;
            if let Some(me) = self.self_arc() {
                tokio::spawn(async move { me.refresh_description(&rt).await });
            }
        }
        Box::pin(std::future::ready(()))
    }
}

impl ApprovalReviewContributor for Autopilot {
    fn decide<'a>(
        &'a self,
        input: &'a ApprovalDecisionInput<'_>,
    ) -> ExtensionFuture<'a, Option<ApprovalDecision>> {
        Box::pin(async move {
            let rt = Self::rt(input.thread_store)?;
            if !self.sup.shared().is_registered(&rt.key) {
                return None;
            }
            // The box's ids, as the TUI names it (its approval id, else its call id): a newer
            // review of the same box replaces whatever an older one left.
            let keys: Vec<&str> = [input.tool_call_id, Some(input.approval_id)]
                .into_iter()
                .flatten()
                .collect();
            let shared = self.sup.shared();
            shared.set_preapproval(&keys, None);
            let policy = self.sup.policy()?;
            if !policy.approve_all {
                self.sup.deferred(approval::Defer::ApproveOff.words());
                return None;
            }
            // Bounded: with a wedged aterm the box goes to a person at once, not 5 s later.
            let not_now = match tokio::time::timeout(APPROVAL_ATERM_BUDGET, self.sup.ready()).await
            {
                Ok(Ready::Go) => None,
                Ok(Ready::Passive(why)) => {
                    self.sup.deferred(why);
                    return None;
                }
                Ok(Ready::Wait { why, .. }) => Some(why),
                Err(_) => Some("aterm did not answer the before-act checks in time".to_string()),
            };
            let Some(config) = rt.config.as_ref() else {
                self.sup
                    .deferred("no config to read the owner's rules from".to_string());
                return None;
            };
            let facts = approval::facts(input, config, policy.approve_all).await;
            if let Some(why) = not_now {
                // Not now, but not a person's either: the box goes to the TUI, and once the
                // checks pass the autopilot answers it there with the one-shot yes (the box
                // glue, CONTRACT §5 "approvals"), as aterm's retired Codex lane pressed "Yes"
                // once after the grace (end-to-end run 2026-10-16: a box inside the grace stood
                // for good). A box the owner's limits keep is a person's, as before, and its
                // attention names the limit, not the wait.
                match approval::review(&facts) {
                    Ok(()) => {
                        shared.set_preapproval(
                            &keys,
                            Some(facts.tool.unwrap_or_else(|| "a box".to_string())),
                        );
                        self.sup.note(format!(
                            "{}: approval waits ({why}); answered in the TUI after it",
                            rt.key
                        ));
                        self.sup.deferred(why);
                    }
                    Err(defer) => self.sup.deferred(defer.words()),
                }
                return None;
            }
            match approval::review(&facts) {
                Ok(()) if !self.sup.acting() => {
                    // The facts were read after the checks: an off since leaves it to a person.
                    let why = "codex-alab went passive while the box was reviewed".to_string();
                    self.sup.note(format!("{}: approval left: {why}", rt.key));
                    self.sup.deferred(why);
                    None
                }
                Ok(()) => {
                    let sup = Arc::clone(&self.sup);
                    let what = format!(
                        "approved {} once (approve=all)",
                        facts.tool.as_deref().unwrap_or("a box")
                    );
                    tokio::spawn(async move { sup.record_act(RULE_APPROVE, &what).await });
                    Some(ApprovalDecision::Reviewed(ReviewDecision::Approved))
                }
                Err(defer) => {
                    self.sup
                        .note(format!("{}: approval left: {}", rt.key, defer.words()));
                    self.sup.deferred(defer.words());
                    None
                }
            }
        })
    }
}
