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

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::Weak;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU64;
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
use codex_extension_api::ExtensionData;
use codex_extension_api::ExtensionEventSink;
use codex_extension_api::ExtensionFuture;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::ExtensionWarning;
use codex_extension_api::ThreadIdleInput;
use codex_extension_api::ThreadLifecycleContributor;
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
use codex_protocol::user_input::UserInput;
use tokio::task::JoinHandle;

use crate::approval;
use crate::decide;
use crate::decide::Act;
use crate::decide::ThreadState;
use crate::decide::Timing;
use crate::decide::TurnEnd;
use crate::decide::Wall;
use crate::supervisor::RECHECK;
use crate::supervisor::Ready;
use crate::supervisor::Supervisor;
use crate::supervisor::clock;
use crate::upgrade;
use crate::upgrade::UpgradeCheck;
use crate::words;

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

/// The most the approval contributor waits for aterm (the status read and the re-confirm)
/// before it leaves the box to a person: a box is never held up by a wedged aterm.
pub const APPROVAL_ATERM_BUDGET: Duration = Duration::from_secs(1);

/// How long a resumed thread's scan waits for the start-up sweep of dead records (which
/// says whether aterm's crash relaunch brought it back); past it, no carry-on.
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

/// How long an act that only queues its turn (a compaction, a goal resumed) keeps the record
/// `busy` for that turn to start (`Shared::await_turn`).
pub const ACT_TURN_WAIT: Duration = Duration::from_secs(30);

/// Write the session record now (not coalesced), off the async runtime, within
/// [`WRITE_THROUGH_BUDGET`]; `true` once it says what the shared state says (or no record is
/// kept).
pub async fn write_through(shared: &'static codex_alab_core::session::Shared) -> bool {
    matches!(
        tokio::time::timeout(
            WRITE_THROUGH_BUDGET,
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

/// One root thread's runtime state, kept in its thread store.
pub struct ThreadRt {
    id: ThreadId,
    key: String,
    state: Mutex<ThreadState>,
    config: Option<Arc<Config>>,
    token_budget: bool,
    jitter: Duration,
    timer: Mutex<Option<(u64, JoinHandle<()>)>>,
    timer_seq: AtomicU64,
    /// One decision at a time per thread (a permit, never a guard held across I/O).
    eval: tokio::sync::Semaphore,
    stopped: AtomicBool,
    resumed: AtomicBool,
    scanned: AtomicBool,
    announced: Mutex<Option<String>>,
    evaluations: AtomicU64,
    /// The `update_plan` step in progress, as the last plan said (the description's).
    plan_step: Mutex<Option<String>>,
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
            id,
            key: id.to_string(),
            state: Mutex::new(ThreadState::default()),
            config,
            token_budget,
            jitter,
            timer: Mutex::new(None),
            timer_seq: AtomicU64::new(0),
            eval: tokio::sync::Semaphore::new(1),
            stopped: AtomicBool::new(false),
            resumed: AtomicBool::new(false),
            scanned: AtomicBool::new(false),
            announced: Mutex::new(None),
            evaluations: AtomicU64::new(0),
            plan_step: Mutex::new(None),
        }
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

    /// Whether a timer is pending.
    pub fn timer_pending(&self) -> bool {
        lock(&self.timer).is_some()
    }
}

#[derive(Default)]
struct TrackData {
    started: Option<Instant>,
    by_autopilot: bool,
    by_person: bool,
    text: Option<String>,
    final_text: Option<String>,
    questions: bool,
    wall: Option<Wall>,
    /// The mail this turn was submitted for (the TUI's native inbox): answered at its end.
    mail: Option<codex_alab_core::mail::MailTurn>,
}

/// One turn's facts, kept in its turn store.
#[derive(Default)]
struct TurnTrack(Mutex<TrackData>);

/// The autopilot extension (one per registry; the claim is the process's).
pub struct Autopilot {
    me: Weak<Autopilot>,
    sup: Arc<Supervisor>,
    thread_manager: Mutex<Option<Weak<ThreadManager>>>,
    goals: Option<Arc<GoalService>>,
    sink: Arc<dyn ExtensionEventSink>,
    timing: Timing,
    gate: SourceGate,
    threads: Mutex<Vec<Weak<ThreadRt>>>,
    seq: AtomicU64,
    watching: AtomicBool,
    watching_launch: AtomicBool,
    /// An upgrade is being considered, or was requested (then for good).
    upgrading: AtomicBool,
    /// A re-look at the upgrade is scheduled (a person's hand was on it).
    upgrade_relook: AtomicBool,
}

impl Autopilot {
    pub fn new(
        sup: Arc<Supervisor>,
        sink: Arc<dyn ExtensionEventSink>,
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
            sink,
            timing,
            gate,
            threads: Mutex::new(Vec::new()),
            seq: AtomicU64::new(0),
            watching: AtomicBool::new(false),
            watching_launch: AtomicBool::new(false),
            upgrading: AtomicBool::new(false),
            upgrade_relook: AtomicBool::new(false),
        })
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

    /// The runtime of a thread the autopilot keeps (tests read it).
    pub fn thread_rt(&self, key: &str) -> Option<Arc<ThreadRt>> {
        lock(&self.threads)
            .iter()
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
        lock(&self.threads).push(Arc::downgrade(rt));
    }

    /// Every claim transition (taken, acting again) has each live thread look again, and say
    /// again what it is doing: a standby or a release withdrew its description, and a long turn
    /// has no start or plan step to write it back (review of 7e7622ff7, finding 8).
    pub(crate) fn watch_claims(self: &Arc<Self>) {
        if self.watching.swap(true, Ordering::SeqCst) {
            return;
        }
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
                    tokio::spawn(Arc::clone(&me).evaluate(Arc::clone(&rt)));
                    let me = Arc::clone(&me);
                    tokio::spawn(async move { me.refresh_description(&rt).await });
                }
            }
        });
    }

    /// Watch the launch path (atpkg's twin): a change may name a new build to move onto.
    /// Only inside an aterm session; the watch lives as long as this extension.
    fn watch_launch(self: &Arc<Self>) {
        if !self.sup.in_aterm() || self.watching_launch.swap(true, Ordering::SeqCst) {
            return;
        }
        let Some(argv0) = self
            .sup
            .shared()
            .launch()
            .and_then(|launch| launch.argv0.clone())
        else {
            return;
        };
        let watcher = match codex_file_watcher::FileWatcher::new() {
            Ok(watcher) => Arc::new(watcher),
            Err(e) => {
                self.sup
                    .note(format!("the launch path cannot be watched: {e}"));
                return;
            }
        };
        // The twin's directory, not the twin: a twin that is a symlink is re-pointed by a
        // rename in that directory, and a watch on the link itself follows its target.
        let Some(dir) = argv0.parent().map(std::path::Path::to_path_buf) else {
            return;
        };
        let (subscriber, mut changes) = watcher.add_subscriber();
        let registration = subscriber.register_paths(vec![codex_file_watcher::WatchPath {
            path: dir,
            recursive: false,
        }]);
        let me = Arc::downgrade(self);
        tokio::spawn(async move {
            let _watch = (watcher, subscriber, registration);
            while changes.recv().await.is_some() {
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
        if !codex_alab_core::active() {
            return;
        }
        let Some(policy) = self.sup.policy() else {
            return;
        };
        let shared = self.sup.shared();
        let Some(launch) = shared.launch() else {
            return;
        };
        if !policy.upgrade || !codex_alab_tui::upgrade::launch_names_other_build(launch) {
            return;
        }
        if self.upgrading.swap(true, Ordering::SeqCst) {
            return;
        }
        if !self.try_upgrade().await {
            self.upgrading.store(false, Ordering::SeqCst);
        }
    }

    async fn upgrade_facts(&self) -> upgrade::UpgradeFacts {
        let shared = self.sup.shared();
        let mut threads = Vec::new();
        let manager = lock(&self.thread_manager)
            .clone()
            .and_then(|weak| weak.upgrade());
        let unresumable = match &manager {
            Some(_) => codex_alab_tui::upgrade::exec_plan(shared).err(),
            None => Some("no thread manager".to_string()),
        };
        if let Some(manager) = manager {
            for id in manager.list_thread_ids().await {
                let Ok(thread) = manager.get_thread(id).await else {
                    continue;
                };
                threads.push(upgrade::LoadedThread {
                    id: id.to_string(),
                    idle: at_rest(&thread.agent_status().await),
                    background_terminals: thread.list_background_terminals().await.len(),
                    goal_active: goal_status(&thread, id).await
                        == Some(codex_state::ThreadGoalStatus::Active),
                });
            }
        }
        let timers_pending = lock(&self.threads)
            .iter()
            .filter_map(Weak::upgrade)
            .any(|rt| rt.timer_pending());
        upgrade::UpgradeFacts {
            enabled: true,
            other_build: true,
            unresumable,
            threads,
            timers_pending,
            left_to_person: self.sup.standing_attention(),
        }
    }

    /// `true` once the TUI was asked to leave for the exec.
    async fn try_upgrade(self: &Arc<Self>) -> bool {
        let facts = self.upgrade_facts().await;
        match upgrade::check(&facts) {
            UpgradeCheck::NotDue(why) => {
                self.sup.note(format!("upgrade: {why}"));
                return false;
            }
            UpgradeCheck::Wait(why) => {
                self.sup.note(format!("upgrade waits: {why}"));
                return false;
            }
            UpgradeCheck::Go => {}
        }
        match self.sup.ready().await {
            Ready::Go => {}
            Ready::Wait { until, why } => {
                self.sup.note(format!("upgrade not now ({why})"));
                self.relook_upgrade(until);
                return false;
            }
            Ready::Passive(_) => return false,
        }
        let shared = self.sup.shared();
        let target = shared
            .launch()
            .and_then(|launch| launch.argv0.as_deref())
            .and_then(codex_alab_tui::upgrade::named_build)
            .map_or_else(|| "the new build".to_string(), |p| p.display().to_string());
        if !self.sup.acting() {
            self.sup.note("upgrade: passive since the checks; not now");
            return false;
        }
        self.sup
            .record_act(
                upgrade::RULE_UPGRADE,
                &format!("re-exec onto {target}, resuming this session"),
            )
            .await;
        if !self.sup.acting() {
            self.sup.note("upgrade: passive since the checks; not now");
            return false;
        }
        if codex_alab_tui::upgrade::request(shared) {
            self.sup.note("upgrade: leaving like /exit for the exec");
            true
        } else {
            self.sup.note("upgrade: the TUI gave no way to leave");
            false
        }
    }

    fn relook_upgrade(self: &Arc<Self>, until: Instant) {
        if self.upgrade_relook.swap(true, Ordering::SeqCst) {
            return;
        }
        let me = Arc::clone(self);
        tokio::spawn(async move {
            tokio::time::sleep_until(tokio::time::Instant::from_std(until)).await;
            me.upgrade_relook.store(false, Ordering::SeqCst);
            me.consider_upgrade().await;
        });
    }

    /// Decide this thread's point and carry the decision out.
    pub(crate) async fn evaluate(self: Arc<Self>, rt: Arc<ThreadRt>) {
        rt.evaluations.fetch_add(1, Ordering::SeqCst);
        let Ok(_turn) = rt.eval.acquire().await else {
            return;
        };
        if rt.stopped.load(Ordering::SeqCst) || !codex_alab_core::active() {
            return;
        }
        if rt.resumed.load(Ordering::SeqCst) && !rt.scanned.load(Ordering::SeqCst) {
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
        let now = clock();
        let decision = decide::decide(&lock(&rt.state), &policy, &self.timing, now, rt.jitter);
        if decision.act == Act::Nothing {
            self.sup
                .note(format!("{}: nothing ({})", rt.key, decision.why));
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
                self.show_wait(&rt, words).await;
            }
            self.schedule(&rt, at);
            return;
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
                lock(&rt.state).acted(&act, now, &self.timing);
                self.sup.attention(key, Some(text.clone())).await;
                return;
            }
            Act::Done => {
                lock(&rt.state).acted(&act, now, &self.timing);
                self.sup
                    .record_act(words::RULE_FINISHED, "the worker said the task is done")
                    .await;
                return;
            }
            _ => {}
        }
        match self.sup.ready().await {
            Ready::Go => {}
            Ready::Wait { until, why } => {
                self.sup.note(format!("{}: not now ({why})", rt.key));
                self.schedule(&rt, until);
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
        match act {
            Act::Type { rule, ref text } => {
                let started = self
                    .submit(&rt, &thread, &act, rule, text.clone(), &decision.why)
                    .await;
                if carrying && !started {
                    shared.set_carried_on(chain);
                }
            }
            Act::ResumeLimit { ref text } => {
                // The goal's turn starts after `resume_goal` returns: the record stays busy
                // until it does (CONTRACT §4).
                let awaited = shared.await_turn(&rt.key, ACT_TURN_WAIT);
                match self.resume_goal(&rt, &thread).await {
                    GoalResume::Resumed => {
                        lock(&rt.state).acted(&act, now, &self.timing);
                        self.sup.attention("goal", None).await;
                        self.sup
                            .record_act(
                                words::RULE_LIMIT_RESUME,
                                "resumed the goal at the usage reset",
                            )
                            .await;
                    }
                    GoalResume::NotNow => {
                        shared.cancel_await(awaited);
                        self.sup.note(format!(
                            "{}: passive since the checks; no goal resume",
                            rt.key
                        ));
                    }
                    GoalResume::NotLimited => {
                        shared.cancel_await(awaited);
                        let _ = self
                            .submit(
                                &rt,
                                &thread,
                                &act,
                                words::RULE_LIMIT_RESUME,
                                text.clone(),
                                &decision.why,
                            )
                            .await;
                    }
                    GoalResume::Failed(why) => {
                        // The goal still stands usage-limited, so a typed turn would only be
                        // refused past it: the person is told, and it is tried again later.
                        shared.cancel_await(awaited);
                        self.sup
                            .attention("goal", Some(goal_resume_failed(&why)))
                            .await;
                        self.schedule(&rt, clock() + RECHECK);
                    }
                }
            }
            Act::Compact => {
                let before = lock(&rt.state).clone();
                lock(&rt.state).acted(&act, now, &self.timing);
                // `submit` only queues the compaction: the record stays busy until its turn
                // starts (CONTRACT §4).
                let awaited = shared.await_turn(&rt.key, ACT_TURN_WAIT);
                match thread.submit(Op::Compact).await {
                    Ok(_) => {
                        self.sup
                            .record_act(words::RULE_COMPACT, "compacted a full context")
                            .await;
                    }
                    Err(e) => {
                        shared.cancel_await(awaited);
                        *lock(&rt.state) = before;
                        self.sup.note(format!("{}: compact refused: {e}", rt.key));
                        // The turn end this act answered is left: under the claim aterm's
                        // own rows stay quiet, so attention says so.
                        self.sup
                            .attention("turn", Some(turn_left(&format!("compaction refused: {e}"))))
                            .await;
                    }
                }
            }
            Act::Nothing | Act::Done | Act::Attention { .. } => {}
        }
    }

    async fn submit(
        &self,
        rt: &Arc<ThreadRt>,
        thread: &Arc<CodexThread>,
        act: &Act,
        rule: &'static str,
        text: String,
        why: &str,
    ) -> bool {
        let mode = thread
            .thread_settings_snapshot()
            .await
            .collaboration_mode
            .mode;
        if mode == ModeKind::Plan {
            self.sup
                .note(format!("{}: Plan mode: nothing is typed into it", rt.key));
            // The claim keeps aterm's own rows quiet, so a turn end codex-alab leaves is a
            // person's only once attention says so (CONTRACT §5 "left boxes").
            self.sup
                .attention(
                    "plan",
                    Some(format!(
                        "plan: Plan mode leaves this turn's end to a person ({why})"
                    )),
                )
                .await;
            return false;
        }
        // A goal that is not complete owns the thread's pace: Active continues itself;
        // Paused, Blocked, UsageLimited and BudgetLimited were stopped by a person, the
        // goal or the owner's budget, and a plain turn would override that.
        if let Some(status) = goal_status(thread, rt.id).await
            && status != codex_state::ThreadGoalStatus::Complete
        {
            self.sup.note(format!(
                "{}: a goal stands ({}); nothing is typed past it",
                rt.key,
                status.as_str()
            ));
            // A goal that stopped itself (blocked, or out of its budget) waits for a person;
            // one a person paused is theirs already, an active one goes on by itself, and a
            // usage-limited one is resumed at the reset.
            if let Some(text) = goal_left_to_person(status) {
                self.sup.attention("goal", Some(text)).await;
            }
            return false;
        }
        // The goal and mode reads above are awaits since the checks: still acting?
        if !self.sup.acting() {
            self.sup.note(format!(
                "{}: passive since the checks; nothing typed",
                rt.key
            ));
            return false;
        }
        let n = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
        let request = TurnInputRequest::new(TurnInput::UserInput {
            content: vec![UserInput::Text {
                text: text.clone(),
                text_elements: Vec::new(),
            }],
            client_id: Some(format!("{CLIENT_PREFIX}{n}")),
        })
        .on_start(TurnStartOptions {
            turn_trigger: Some(TURN_TRIGGER.to_string()),
            ..Default::default()
        });
        let now = clock();
        let before = lock(&rt.state).clone();
        lock(&rt.state).acted(act, now, &self.timing);
        match thread.start_turn_if_idle(request).await {
            Ok(StartIfIdleSubmission::Started { turn_id, .. }) => {
                *lock(&rt.announced) = None;
                self.sync_owed(rt);
                self.sup
                    .record_act(rule, &format!("typed \"{text}\" ({why}; turn {turn_id})"))
                    .await;
                true
            }
            Ok(StartIfIdleSubmission::NotSubmitted { reason }) => {
                *lock(&rt.state) = before;
                self.sup
                    .note(format!("{}: not submitted: {reason:?}", rt.key));
                if let Some((key, text)) = not_submitted_attention(&reason, why) {
                    self.sup.attention(key, Some(text)).await;
                }
                false
            }
            Err(e) => {
                *lock(&rt.state) = before;
                self.sup.note(format!("{}: submit failed: {e}", rt.key));
                self.sup
                    .attention(
                        "turn",
                        Some(turn_left(&format!("the turn did not start: {e}"))),
                    )
                    .await;
                false
            }
        }
    }

    /// A goal stopped at the usage wall, resumed as `thread/goal/set` would at the reset.
    async fn resume_goal(&self, rt: &ThreadRt, thread: &Arc<CodexThread>) -> GoalResume {
        if goal_status(thread, rt.id).await != Some(codex_state::ThreadGoalStatus::UsageLimited) {
            return GoalResume::NotLimited;
        }
        let Some(goals) = &self.goals else {
            return GoalResume::Failed("this build has no goal service".to_string());
        };
        let Some(db) = thread.state_db() else {
            return GoalResume::Failed("the thread has no state database".to_string());
        };
        if !self.sup.acting() {
            return GoalResume::NotNow;
        }
        let request = GoalSetRequest {
            thread_id: rt.id,
            objective: GoalObjectiveUpdate::Keep,
            status: Some(codex_protocol::protocol::ThreadGoalStatus::Active),
            token_budget: GoalTokenBudgetUpdate::Keep,
            max_goal_token_budget: rt.config.as_ref().and_then(|c| c.max_goal_token_budget),
        };
        match goals.set_thread_goal(&db, request, async { Ok(()) }).await {
            Ok(outcome) => {
                outcome.apply_runtime_effects(goals).await;
                GoalResume::Resumed
            }
            Err(e) => {
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
        if !codex_alab_core::active()
            || !self.sup.shared().is_root_thread(&rt.key)
            || self.sup.policy().is_none()
        {
            return;
        }
        let goal = match self.thread(rt.id).await {
            Some(thread) => goal_objective(&thread, rt.id).await,
            None => None,
        };
        let step = lock(&rt.plan_step).clone();
        let text = crate::describe::describe(goal.as_deref(), step.as_deref());
        self.sup.describe(text).await;
    }

    /// A turn of this root thread started: a goal that reads active again (a person's `/goal`
    /// resumed it, or the usage reset did) has answered the `goal:` attention a stopped goal
    /// raised, which no person's turn end would clear (review of the inbox, finding 8).
    async fn goal_moved(&self, rt: &ThreadRt) {
        if !codex_alab_core::active() || !self.sup.shared().is_root_thread(&rt.key) {
            return;
        }
        let Some(thread) = self.thread(rt.id).await else {
            return;
        };
        if goal_answers_attention(goal_status(&thread, rt.id).await) {
            self.sup.attention("goal", None).await;
        }
    }

    async fn show_wait(&self, rt: &ThreadRt, words: &str) {
        {
            let mut announced = lock(&rt.announced);
            if announced.as_deref() == Some(words) {
                return;
            }
            *announced = Some(words.to_string());
        }
        self.sink.emit_warning(ExtensionWarning {
            thread_id: rt.key.clone(),
            turn_id: None,
            message: format!("codex-alab: {words}"),
        });
        self.sup.record_wait(words).await;
    }

    /// Look again at `at` (one pending timer per thread; a newer one replaces it).
    pub(crate) fn schedule(self: &Arc<Self>, rt: &Arc<ThreadRt>, at: Instant) {
        let generation = rt.timer_seq.fetch_add(1, Ordering::SeqCst) + 1;
        let mut slot = lock(&rt.timer);
        let me = Arc::clone(self);
        let runtime = Arc::clone(rt);
        let handle = tokio::spawn(async move {
            tokio::time::sleep_until(tokio::time::Instant::from_std(at)).await;
            {
                let mut slot = lock(&runtime.timer);
                if !slot.as_ref().is_some_and(|(g, _)| *g == generation) {
                    return;
                }
                // Out of the slot before acting: a turn this timer starts must not cancel
                // (and join) the very task starting it.
                slot.take();
            }
            me.evaluate(runtime).await;
        });
        if let Some((_, old)) = slot.replace((generation, handle)) {
            old.abort();
        }
    }

    /// Cancel and join the pending timer.
    pub(crate) async fn cancel_timer(rt: &ThreadRt) {
        let pending = lock(&rt.timer).take();
        if let Some((_, handle)) = pending {
            handle.abort();
            let _ = handle.await;
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
        store.get::<ThreadRt>()
    }

    async fn resume_scan(self: Arc<Self>, rt: Arc<ThreadRt>) {
        {
            let _turn = rt.eval.acquire().await;
            // The crash witness comes from the record writer's start-up sweep: wait for it
            // (an event, bounded), or no predecessor is known and nothing carries on.
            let shared = self.sup.shared();
            if self.sup.in_aterm() && !shared.swept() {
                let _ = tokio::task::spawn_blocking(move || shared.wait_swept(SWEPT_WAIT)).await;
            }
            // And the proof that aterm typed the line that started this process: the aterm
            // client's first `status` read (bounded; none read is no proof).
            if self.sup.in_aterm() && !shared.launch_probed() {
                let _ = tokio::task::spawn_blocking(move || {
                    shared.wait_launch_probed(LAUNCH_PROBE_WAIT)
                })
                .await;
            }
            // The relaunch is judged (and spent) before the history is read, whatever the
            // read then says: a resume that learns nothing still spends it, so a later
            // `/resume` back in this process is no relaunch (review of cf084e3b7, finding 2).
            let relaunched = relaunched(self.sup.shared(), &rt.key, now_ms());
            let history = match self.thread(rt.id).await {
                Some(thread) => resumed_history(&thread).await,
                None => Err("the thread manager does not have it".to_string()),
            };
            if let Err(why) = &history {
                self.sup
                    .note(format!("{}: resumed, its history unread ({why})", rt.key));
            }
            if let Ok(items) = history {
                let (task, last) = scan_history(&items);
                lock(&rt.state).observe_resume(task, last.as_deref(), relaunched);
                self.sync_owed(&rt);
                self.sup.note(format!(
                    "{}: resumed (task={task}, relaunched={relaunched})",
                    rt.key
                ));
            }
            rt.scanned.store(true, Ordering::SeqCst);
        }
        self.evaluate(rt).await;
    }

    /// The record's `busy` while this thread owes its carry-on
    /// (`Shared::set_carry_on_owed`).
    fn sync_owed(&self, rt: &ThreadRt) {
        let owed = {
            let state = lock(&rt.state);
            state.carry_on && state.task && !state.done
        };
        self.sup.shared().set_carry_on_owed(owed);
    }

    fn finish(&self, rt: &Arc<ThreadRt>, turn_store: &ExtensionData, aborted: bool) {
        let track = turn_store.get::<TurnTrack>();
        let data = track
            .map(|t| std::mem::take(&mut *lock(&t.0)))
            .unwrap_or_default();
        self.sup.shared().turn_ended(&rt.key, turn_store.level_id());
        let at = clock();
        let text = data.final_text.or(data.text);
        let success = data.wall.is_none() && !aborted;
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
        let end = TurnEnd {
            at,
            by_autopilot: data.by_autopilot,
            by_person: data.by_person,
            text: text.clone(),
            questions: data.questions,
            worked: data
                .started
                .map_or(Duration::ZERO, |s| at.saturating_duration_since(s)),
            wall: data.wall,
            aborted,
        };
        // Done is what the decision makes of this end (`decide::ends_task`), not a done phrase
        // alone: a first "completed successfully" gets the done check, and the worker may carry
        // on from it (review of cf084e3b7, finding 1).
        let done = {
            let mut state = lock(&rt.state);
            state.observe_turn_end(end, &self.timing);
            success && decide::ends_task(&state)
        };
        self.sync_owed(rt);
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
        if speaks && (done || data.by_person) {
            // A person's turn (a new task) or the worker's done ends the carry-on chain: a
            // later relaunch carries on again. A turn that merely ended cleanly does not, nor
            // does a first done reply (the done check follows it): the autopilot's continue
            // after it can stall again, and its restart must then wait for the person, not
            // carry on once more.
            self.sup.shared().set_carried_on(0);
        }
        if speaks && (success || data.by_person) {
            let sup = Arc::clone(&self.sup);
            let person = data.by_person;
            tokio::spawn(async move {
                for key in ["login", "context", "wall", "api", "limit", "turn"] {
                    if success {
                        sup.attention(key, None).await;
                    }
                }
                if person {
                    for key in ["question", "plan", "goal", "turn"] {
                        sup.attention(key, None).await;
                    }
                }
            });
        }
    }
}

/// What a resumed thread's history says: whether a person's turn exists (a `TurnStarted`
/// whose `turn_trigger` is not codex-alab's; none at all is a person's), and the last
/// `TurnComplete` reply.
pub fn scan_history(items: &[codex_history::RolloutItem]) -> (bool, Option<String>) {
    let mut task = false;
    let mut last = None;
    for item in items {
        let codex_history::RolloutItem::EventMsg(event) = item else {
            continue;
        };
        match event {
            EventMsg::TurnStarted(started) => {
                let trigger = started
                    .turn_attribution
                    .as_ref()
                    .and_then(|a| a.turn_trigger.as_deref());
                if trigger != Some(TURN_TRIGGER) {
                    task = true;
                }
            }
            EventMsg::TurnComplete(complete) => {
                last = complete.last_agent_message.clone();
            }
            _ => {}
        }
    }
    (task, last)
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
    let db = thread.state_db()?;
    db.thread_goals()
        .get_thread_goal(id)
        .await
        .ok()
        .flatten()
        .map(|goal| goal.status)
}

/// What resuming a usage-limited goal at the reset came to.
#[derive(Debug, Clone, PartialEq, Eq)]
enum GoalResume {
    /// It is active again; its turn starts.
    Resumed,
    /// There is no usage-limited goal (none, or a person moved it): a plain turn may go.
    NotLimited,
    /// It is still usage-limited: the resume failed (`why`).
    Failed(String),
    /// codex-alab went passive since the checks: nothing was asked of the goal service.
    NotNow,
}

/// The attention a usage-limited goal raises when its resume at the reset failed: a typed
/// turn would be refused past it, so nothing goes on until it is resumed.
pub fn goal_resume_failed(why: &str) -> String {
    format!("goal: the usage-limited goal did not resume at the reset ({why}); a person resumes it")
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
/// and so waits for a person: blocked, or out of its token budget. `None` for a goal a person
/// paused (theirs already), an active one (it goes on by itself), a usage-limited one (resumed
/// at the reset) and a complete one.
pub fn goal_left_to_person(status: codex_state::ThreadGoalStatus) -> Option<String> {
    use codex_state::ThreadGoalStatus;
    match status {
        ThreadGoalStatus::Blocked => {
            Some("goal: the goal is blocked; a person decides how it goes on".to_string())
        }
        ThreadGoalStatus::BudgetLimited => {
            Some("goal: the goal spent its token budget; a person decides".to_string())
        }
        ThreadGoalStatus::Active
        | ThreadGoalStatus::Paused
        | ThreadGoalStatus::UsageLimited
        | ThreadGoalStatus::Complete => None,
    }
}

/// Whether a goal that reads `status` at a turn's start has answered the `goal:` attention: an
/// active goal goes on by itself, so whatever stopped it no longer stands.
pub fn goal_answers_attention(status: Option<codex_state::ThreadGoalStatus>) -> bool {
    status == Some(codex_state::ThreadGoalStatus::Active)
}

/// The thread's goal objective, unless the goal is complete (or there is none).
async fn goal_objective(thread: &Arc<CodexThread>, id: ThreadId) -> Option<String> {
    let db = thread.state_db()?;
    db.thread_goals()
        .get_thread_goal(id)
        .await
        .ok()
        .flatten()
        .filter(|goal| goal.status != codex_state::ThreadGoalStatus::Complete)
        .map(|goal| goal.objective)
}

/// Whether this process resumed `session` for aterm's crash relaunch (the carry-on's
/// trigger): the start-up sweep found a dead predecessor's record of that session, and aterm's
/// first `status` read at this start shows aterm's relaunch ([`relaunched_by_aterm`]).
/// codex-alab's own re-exec never carries on: it moves only at a quiescent point, where the
/// old image owed no act (`upgrade`), and a point it left to a person stays the person's.
///
/// Both facts are about this process's start, so they are read ONCE: the process's first
/// resume spends them ([`Shared::spend_relaunch`]), whatever it is a resume of and whatever it
/// learns (no predecessor yet because the sweep ran late, a history that could not be read),
/// and a later resume in the same process (`/resume` away and back) is no relaunch, whatever
/// its history says. And they speak only for the start: a resume at `now_ms` more than
/// [`LAUNCH_PROBE_WINDOW`] after it is a person's, never aterm's relaunch. A process whose own
/// count already reached [`MAX_CARRY_ONS`] carries on no more. One that does not carry on
/// passes its predecessor's count on (the record keeps it), so a refusal never resets the
/// chain: only a person's turn or a done does.
///
/// [`Shared::spend_relaunch`]: codex_alab_core::session::Shared::spend_relaunch
pub fn relaunched(shared: &codex_alab_core::session::Shared, session: &str, now_ms: u64) -> bool {
    if !shared.spend_relaunch() {
        return false;
    }
    let Some(predecessor) = shared.predecessor(session) else {
        return false;
    };
    let window = u64::try_from(LAUNCH_PROBE_WINDOW.as_millis()).unwrap_or(u64::MAX);
    let carry = shared.carried_on() < MAX_CARRY_ONS
        && shared.launch().is_some_and(|launch| {
            now_ms >= launch.started_at_ms
                && now_ms - launch.started_at_ms <= window
                && relaunched_by_aterm(
                    &predecessor,
                    &launch.args,
                    launch.started_at_ms,
                    shared.launch_witness().as_ref(),
                )
        });
    if !carry && predecessor.carried_on > shared.carried_on() {
        shared.set_carried_on(predecessor.carried_on);
    }
    carry
}

/// How many relaunch carry-ons in a row, with no person's turn and no done between, a session
/// gets: one (CONTRACT §5 carry-on).
pub const MAX_CARRY_ONS: u32 = 1;

/// How long before this process's start a person's keystroke reads as the person typing the
/// line that started it: aterm's relaunch types only after a person has been quiet for
/// `human_grace_s`, and a hand-typed `codex resume <id>` starts within moments of its Enter.
pub const PERSON_LINE_MARGIN: Duration = Duration::from_secs(5);

/// How long after this process's start the launch probe's read still speaks for the start.
pub const LAUNCH_PROBE_WINDOW: Duration = Duration::from_secs(60);

/// What every aterm harness's hand is named (`hand=lease:aterm-harness@<pid>`, the pid of the
/// aterm process that hosts the harness and serves the control socket).
const ATERM_HAND: &str = "lease:aterm-harness@";

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
    use codex_alab_core::session::HandPid;
    use codex_alab_core::session::HumanSeen;
    let margin = u64::try_from(PERSON_LINE_MARGIN.as_millis()).unwrap_or(u64::MAX);
    let window = u64::try_from(LAUNCH_PROBE_WINDOW.as_millis()).unwrap_or(u64::MAX);
    let aterms_hand = match (witness.hand.as_deref(), witness.server_pid) {
        (Some(hand), Some(server)) => {
            hand.strip_prefix(ATERM_HAND) == Some(server.to_string().as_str())
                && match witness.hand_pid {
                    HandPid::Absent => true,
                    HandPid::Pid(taker) => taker == server,
                    HandPid::Unknown => false,
                }
        }
        _ => false,
    };
    let person_quiet = match witness.human {
        HumanSeen::Never => true,
        HumanSeen::At(at) => at.saturating_add(margin) <= started_at_ms,
        HumanSeen::Unknown => false,
    };
    aterms_hand
        && person_quiet
        && witness.read_at_ms >= started_at_ms
        && witness.read_at_ms - started_at_ms <= window
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
///   [`MAX_CARRY_ONS`]: no carry-on since a person's turn or a done): one stall's restart
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
    let window = u64::try_from(RELAUNCH_WINDOW.as_millis()).unwrap_or(u64::MAX);
    matches!(
        predecessor.exit.as_deref(),
        None | Some("signal:15") | Some("signal:1")
    ) && predecessor.status.as_deref() == Some("busy")
        && started_at_ms.saturating_sub(predecessor.updated_at_ms) <= window
        && predecessor.resume_argv.as_ref().is_some_and(|argv| {
            argv.len() == args.len()
                && argv
                    .iter()
                    .zip(args)
                    .all(|(want, got)| got.to_str() == Some(want.as_str()))
        })
        && predecessor.carried_on < MAX_CARRY_ONS
        && witness.is_some_and(|witness| aterm_relaunch_seen(witness, started_at_ms))
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
    // A refused login reads as a status 401 (`invalid_api_key`) on any request lane: a
    // sign-in only a person can give, never a retry (end-to-end run 2026-10-16).
    if let CodexErrorInfo::HttpConnectionFailed {
        http_status_code: Some(401),
    }
    | CodexErrorInfo::ResponseStreamConnectionFailed {
        http_status_code: Some(401),
    }
    | CodexErrorInfo::ResponseTooManyFailedAttempts {
        http_status_code: Some(401),
    } = error
    {
        return Some(Wall::Login);
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
        CodexErrorInfo::CyberPolicy
        | CodexErrorInfo::BioPolicy
        | CodexErrorInfo::MisalignmentPolicyViolation => Wall::Fatal {
            why: "the model's usage policy refused the turn".to_string(),
        },
        CodexErrorInfo::InvalidPrompt | CodexErrorInfo::BadRequest => Wall::Fatal {
            why: "the API refused the request as invalid".to_string(),
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
        },
        CodexErrorInfo::HttpConnectionFailed { http_status_code }
        | CodexErrorInfo::ResponseStreamConnectionFailed { http_status_code }
        | CodexErrorInfo::ResponseTooManyFailedAttempts { http_status_code } => Wall::Server {
            what: status(http_status_code),
            cut_off: false,
        },
        CodexErrorInfo::ServerOverloaded | CodexErrorInfo::FlexUnavailable => Wall::Server {
            what: "an API error (overloaded)".to_string(),
            cut_off: false,
        },
        CodexErrorInfo::InternalServerError => Wall::Server {
            what: "an API error (500)".to_string(),
            cut_off: false,
        },
        CodexErrorInfo::RateLimitExceeded => Wall::Server {
            what: "an API error (429)".to_string(),
            cut_off: false,
        },
        // Core names a stream that broke after its own retries `Other`; its details say
        // what it was: a reply cut off (the 1/5/15/30/60 min ladder, end-to-end run
        // 2026-10-16), or a request that timed out.
        CodexErrorInfo::Other => match details {
            CodexErrorDetails::Stream(_) => Wall::Server {
                what: "an API error (the reply was cut off)".to_string(),
                cut_off: true,
            },
            CodexErrorDetails::RequestTimeout => Wall::Server {
                what: "an API error (the request timed out)".to_string(),
                cut_off: false,
            },
            _ => return None,
        },
        CodexErrorInfo::TooManyDenials
        | CodexErrorInfo::SandboxError
        | CodexErrorInfo::ThreadRollbackFailed
        | CodexErrorInfo::ActiveTurnNotSteerable { .. } => return None,
    })
}

impl ThreadLifecycleContributor<Config> for Autopilot {
    fn on_thread_start<'a>(
        &'a self,
        input: ThreadStartInput<'a, Config>,
    ) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            if !codex_alab_core::active()
                || input.session_source.is_non_root_agent()
                || !(self.gate)(input.session_source)
            {
                return;
            }
            let Ok(id) = ThreadId::from_string(input.thread_store.level_id()) else {
                return;
            };
            let rt = ThreadRt::new(
                id,
                Some(Arc::new(input.config.clone())),
                input
                    .config
                    .features
                    .enabled(codex_features::Feature::TokenBudget),
                jitter_for(&id.to_string(), self.timing.reset_jitter),
            );
            input.thread_store.insert(rt);
            self.sup.shared().note_root_thread(&id.to_string());
            if let Some(rt) = input.thread_store.get::<ThreadRt>() {
                let mut threads = lock(&self.threads);
                threads.retain(|t| t.strong_count() > 0);
                threads.push(Arc::downgrade(&rt));
            }
            self.sup.start();
            if let Some(me) = self.self_arc() {
                me.watch_claims();
                me.watch_launch();
            }
        })
    }

    fn on_thread_resume<'a>(&'a self, input: ThreadResumeInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            let Some(rt) = Self::rt(input.thread_store) else {
                return;
            };
            rt.resumed.store(true, Ordering::SeqCst);
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
            let Some(rt) = Self::rt(input.thread_store) else {
                return;
            };
            rt.stopped.store(true, Ordering::SeqCst);
            self.sup.shared().thread_ended(&rt.key);
            Self::cancel_timer(&rt).await;
        })
    }
}

impl TurnLifecycleContributor for Autopilot {
    fn on_turn_start<'a>(&'a self, input: TurnStartInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            let Some(rt) = Self::rt(input.thread_store) else {
                return;
            };
            let track = input.turn_store.get_or_init(TurnTrack::default);
            lock(&track.0).started = Some(clock());
            // The record says busy before the turn runs (CONTRACT §4), a person's turn or
            // codex-alab's: a local write, bounded, never an aterm call.
            let shared = self.sup.shared();
            if shared.turn_started(&rt.key, input.turn_store.level_id())
                && !write_through(shared).await
            {
                self.sup.note(format!(
                    "{}: the session record did not take the turn's start",
                    rt.key
                ));
            }
            Self::cancel_timer(&rt).await;
            // A goal set or moved starts a turn: the description follows it, and a goal that
            // reads active again has answered the `goal:` attention it left.
            if let Some(me) = self.self_arc() {
                tokio::spawn(async move {
                    me.refresh_description(&rt).await;
                    me.goal_moved(&rt).await;
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
                return;
            };
            let track = turn_store.get_or_init(TurnTrack::default);
            match item {
                TurnItem::UserMessage(message) => {
                    // A turn the TUI submitted for a typed `Inbox:` line: its end answers the
                    // mail (only a root thread's; a mail turn is still a person's, who typed
                    // the line).
                    if self.sup.shared().is_root_thread(&rt.key)
                        && let Some(mail) = self.sup.shared().take_mail_turn(&user_text(message))
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
                    let mut data = lock(&track.0);
                    if ours {
                        data.by_autopilot = true;
                    } else {
                        data.by_person = true;
                        drop(data);
                        lock(&rt.state).observe_person_turn();
                        self.sync_owed(&rt);
                        if self.sup.shared().is_root_thread(&rt.key) {
                            self.sup.shared().set_carried_on(0);
                        }
                    }
                }
                TurnItem::AgentMessage(message) => {
                    let text: String = message
                        .content
                        .iter()
                        .map(|c| match c {
                            codex_protocol::items::AgentMessageContent::Text { text } => {
                                text.as_str()
                            }
                        })
                        .collect();
                    let mut data = lock(&track.0);
                    if message.phase == Some(MessagePhase::FinalAnswer) {
                        data.final_text = Some(text.clone());
                    }
                    data.text = Some(text);
                    if message.questions.as_ref().is_some_and(|q| !q.is_empty()) {
                        data.questions = true;
                    }
                }
                _ => {}
            }
        })
    }

    fn on_turn_stop<'a>(&'a self, input: TurnStopInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            if let Some(rt) = Self::rt(input.thread_store) {
                self.finish(&rt, input.turn_store, false);
            }
        })
    }

    fn on_turn_abort<'a>(&'a self, input: TurnAbortInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            if let Some(rt) = Self::rt(input.thread_store) {
                self.finish(&rt, input.turn_store, true);
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
            let wall = classify(&input.error, input.error_details, auto_compact);
            let track = input.turn_store.get_or_init(TurnTrack::default);
            let mut data = lock(&track.0);
            if wall.is_some() {
                data.wall = wall;
            }
        })
    }
}

impl ToolLifecycleContributor for Autopilot {
    /// `update_plan`: the step in progress is what the description says the thread is doing.
    fn on_tool_start<'a>(&'a self, input: ToolStartInput<'a>) -> ToolLifecycleFuture<'a> {
        // Read synchronously, then only spawn: nothing here waits on aterm.
        if input.tool_name.namespace.is_none()
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
