// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The pure turn-end decider, aterm-style: [`decide`] is a function of this thread's memory
//! ([`ThreadState`]), the policy aterm answered ([`Policy`]), the timings ([`Timing`]) and
//! the clock. The runtime (`extension.rs`) feeds it what the thread did
//! ([`ThreadState::observe_turn_end`], [`ThreadState::observe_person_turn`],
//! [`ThreadState::observe_resume`]), executes the [`Decision`] after the before-every-act
//! checks, and reports back with [`ThreadState::acted`].
//!
//! Mirrors aterm's `decide_turn_end` (`supervise/policy/turn_end.rs`), cut to what Codex's
//! own events say (no screen): CONTRACT §5's rows turn end, continue, taskless, carry-on,
//! done, back-off, prose questions, API/stream, usage window, context window and login.

use std::collections::VecDeque;
use std::time::Duration;
use std::time::Instant;

use crate::words;
use crate::words::Said;

/// aterm's `TurnEndTiming`, the parts Codex needs (defaults are aterm's; tests shorten them).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Timing {
    /// The busy work that makes a turn real work: a turn that did less is short.
    pub min_work: Duration,
    /// The wait before the act after ONE short turn; each more doubles it, up to the max.
    pub short_backoff: Duration,
    pub short_backoff_max: Duration,
    /// The server's own failure (a status, an overload, a stream that broke): from each try.
    pub retry_backoff: Vec<Duration>,
    /// An API never reached (no status): short, a blind try costs only core's own retries.
    pub net_backoff: Vec<Duration>,
    /// Past a usage window's reset (aterm's `grace_for(Codex)`: 150 s capped at 60 s, since
    /// Codex has no auto-continue of its own to stay clear of).
    pub reset_grace: Duration,
    /// Up to this much more, per session, so one account's sessions do not all type at once.
    pub reset_jitter: Duration,
    /// After a continuation at a usage wall that hit the wall again.
    pub limit_backoff: Vec<Duration>,
    /// A wall no reset ends (quota, a plan without the model): never anything bought.
    pub entitlement_backoff: Vec<Duration>,
    /// The window `continue_per_hour` counts over.
    pub window: Duration,
    /// Busy work before a wall that ends its episode (aterm's `TurnEndTiming::progress`):
    /// a retry that led to real work starts the next wall's ladder over. A compaction's own
    /// run is not the worker's work.
    pub progress: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        let min = |m: u64| Duration::from_secs(m * 60);
        Timing {
            min_work: min(2),
            short_backoff: min(2),
            short_backoff_max: min(60),
            retry_backoff: vec![min(1), min(5), min(15), min(30), min(60)],
            net_backoff: vec![min(1), min(2), min(5)],
            reset_grace: Duration::from_secs(60),
            reset_jitter: Duration::from_secs(60),
            limit_backoff: vec![min(10), min(30)],
            entitlement_backoff: vec![min(30), min(120), min(360)],
            window: min(60),
            progress: min(5),
        }
    }
}

/// How a question is answered (`questions=`, resolved by aterm over `answer_questions`,
/// `question_priority` and the session's `meta questions` word).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Questions {
    Ask,
    Recommended,
    LongTerm,
    ShortTerm,
}

/// aterm's `[harness]` policy as codex-alab honours it (CONTRACT §3). Every missing or
/// unreadable value is the limit: off, no text, no act.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    pub continue_on: bool,
    pub continue_text: Option<String>,
    /// `None`: no cap. `Some(0)`: unreadable, so nothing is typed.
    pub continue_per_hour: Option<u32>,
    /// `approve=all` (`safe` reads as `none`: codex-alab has no proofs).
    pub approve_all: bool,
    pub answer_questions: bool,
    pub answer_text: Option<String>,
    pub questions: Questions,
    pub retry_api_errors: bool,
    pub resume_limits: bool,
    pub limit_wait: bool,
    pub compact_on_context_wall: bool,
    /// The live upgrade by re-exec: `upgrade` on and `upgrade_ask` off (codex-alab never asks).
    pub upgrade: bool,
    pub human_grace: Duration,
}

impl Policy {
    /// The policy a supported `harness policy` reply carries.
    pub fn from_harness(p: &codex_alab_aterm::HarnessPolicy) -> Policy {
        use codex_alab_aterm::policy::Approve;
        use codex_alab_aterm::policy::Questions as Q;
        Policy {
            continue_on: p.switch("continue"),
            continue_text: p.continue_text().map(str::to_string),
            continue_per_hour: p.continue_per_hour(),
            approve_all: p.approve() == Approve::All,
            answer_questions: p.switch("answer_questions"),
            answer_text: p.answer_text().map(str::to_string),
            questions: match p.questions() {
                Q::Ask => Questions::Ask,
                Q::Recommended => Questions::Recommended,
                Q::LongTerm => Questions::LongTerm,
                Q::ShortTerm => Questions::ShortTerm,
            },
            retry_api_errors: p.switch("retry_api_errors"),
            resume_limits: p.switch("resume_limits"),
            limit_wait: p.switch("limit_wait"),
            compact_on_context_wall: p.switch("compact_on_context_wall"),
            upgrade: p.switch("upgrade") && !p.switch("upgrade_ask"),
            human_grace: p.human_grace(),
        }
    }
}

/// The wall a turn ended on, classified from Codex's terminal error (after core's own
/// retries).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Wall {
    /// A status, an overload, a broken stream: `retry_backoff`.
    Server { what: String, cut_off: bool },
    /// The API was never reached: `net_backoff`.
    Network { what: String },
    /// A usage window, with the reset it names (`resets_at`) as this clock and as words.
    Usage {
        reset_at: Option<Instant>,
        reset_label: Option<String>,
    },
    /// A quota or a plan without the model: no reset ends it.
    Entitlement,
    /// The context window is full. `auto_compact`: core compacts on its own here (neither
    /// `AutoCompactionDisabled` nor token-budget mode).
    Context { auto_compact: bool },
    /// The login was refused or could not be refreshed.
    Login,
    /// A wall nothing typed gets past (a policy refusal, a bad request, a budget).
    Fatal { why: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WallClass {
    Server,
    Network,
    Usage,
    Entitlement,
    Context,
    Login,
    Fatal,
}

impl Wall {
    fn class(&self) -> WallClass {
        match self {
            Wall::Server { .. } => WallClass::Server,
            Wall::Network { .. } => WallClass::Network,
            Wall::Usage { .. } => WallClass::Usage,
            Wall::Entitlement => WallClass::Entitlement,
            Wall::Context { .. } => WallClass::Context,
            Wall::Login => WallClass::Login,
            Wall::Fatal { .. } => WallClass::Fatal,
        }
    }
}

/// One wall's episode: when it first stood and how often codex-alab tried past it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WallTrack {
    class: WallClass,
    pub first_seen: Instant,
    pub attempts: u32,
    pub last_try: Option<Instant>,
}

/// A turn this process ran and saw end (`on_turn_stop` / `on_turn_abort`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnEnd {
    pub at: Instant,
    /// codex-alab's own submission (`client_id` `alab-autopilot-<n>`).
    pub by_autopilot: bool,
    /// A person's turn: a `UserMessage` whose `client_id` is not codex-alab's.
    pub by_person: bool,
    /// The last agent message (its final answer when it marked one).
    pub text: Option<String>,
    /// The agent message carried `questions: Some(..)` (`request_user_input_async`).
    pub questions: bool,
    pub worked: Duration,
    pub wall: Option<Wall>,
    pub aborted: bool,
}

/// One thread's memory (the runtime keeps one per root thread, in its thread store).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThreadState {
    /// A person's turn exists (no act until one does: the taskless rule).
    pub task: bool,
    /// The task is done: nothing until a person's turn.
    pub done: bool,
    /// `DONE_CHECK` was typed for this task.
    pub done_checked: bool,
    /// Short turns of codex-alab's continuations in a row.
    pub short_streak: u32,
    /// When each continuation-type act was typed (for `continue_per_hour`).
    pub typed: VecDeque<Instant>,
    /// The last turn end not yet answered.
    pub point: Option<TurnEnd>,
    pub wall: Option<WallTrack>,
    /// `Op::Compact` was submitted; its idle is not a turn.
    pub compacting: bool,
    /// The compaction ended: continue.
    pub after_compact: bool,
    /// A resume by aterm's crash relaunch of a session that died working, with a task not
    /// done: `continue_text` at the first verified idle (never after codex-alab's own
    /// re-exec, which moves only where nothing was owed).
    pub carry_on: bool,
    /// The rule of the last act typed (its yield's turn is judged by it).
    pub last_rule: Option<&'static str>,
}

/// What to do at a point.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Act {
    Nothing,
    /// Submit `text` as a turn (`TurnInput::UserInput`, `client_id` `alab-autopilot-<n>`).
    Type {
        rule: &'static str,
        text: String,
    },
    /// `Op::Compact` at a verified idle.
    Compact,
    /// The usage window reset: a goal `UsageLimited` is resumed through `GoalService`,
    /// else `text` is typed.
    ResumeLimit {
        text: String,
    },
    /// The task is done ([`words::FINISHED`] or a second done reply).
    Done,
    /// A point codex-alab will not answer: codex-alab's keyed attention.
    Attention {
        key: &'static str,
        text: String,
    },
}

impl Act {
    /// Whether the act touches the agent (and so needs the before-every-act checks).
    pub fn needs_hand(&self) -> bool {
        matches!(
            self,
            Act::Type { .. } | Act::Compact | Act::ResumeLimit { .. }
        )
    }

    /// The rule the act is recorded under (`harness_act`).
    pub fn rule(&self) -> &'static str {
        match self {
            Act::Nothing => "none",
            Act::Type { rule, .. } => rule,
            Act::Compact => words::RULE_COMPACT,
            Act::ResumeLimit { .. } => words::RULE_LIMIT_RESUME,
            Act::Done => words::RULE_FINISHED,
            Act::Attention { .. } => "attention",
        }
    }
}

/// The decision at one point: the act, not before `not_before` (a wait), and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub act: Act,
    pub not_before: Option<Instant>,
    pub why: String,
    /// A wait a person should see ("waiting for the usage reset at 14:05"): shown as a TUI
    /// warning and an `appnotice`.
    pub shown_wait: Option<String>,
}

impl Decision {
    fn nothing(why: impl Into<String>) -> Decision {
        Decision {
            act: Act::Nothing,
            not_before: None,
            why: why.into(),
            shown_wait: None,
        }
    }

    fn now(act: Act, why: impl Into<String>) -> Decision {
        Decision {
            act,
            not_before: None,
            why: why.into(),
            shown_wait: None,
        }
    }

    fn attention(key: &'static str, text: impl Into<String>) -> Decision {
        let text = text.into();
        Decision {
            act: Act::Attention {
                key,
                text: text.clone(),
            },
            not_before: None,
            why: text,
            shown_wait: None,
        }
    }
}

/// Whether `rule` is a wall act (counted as the wall's attempts, outside the per-hour cap
/// and the short streak), as aterm's `is_wall_act`.
fn is_wall_rule(rule: &str) -> bool {
    matches!(
        rule,
        words::RULE_API_RETRY
            | words::RULE_API_CUTOFF
            | words::RULE_LIMIT_RESUME
            | words::RULE_COMPACT
    )
}

/// Whether `rule`'s yield counts toward the short streak (aterm's `is_continue_rule`).
fn is_continue_rule(rule: &str) -> bool {
    matches!(
        rule,
        words::RULE_CONTINUE
            | words::RULE_ANSWER
            | words::RULE_PROSE_LONG_TERM
            | words::RULE_PROSE_SHORT_TERM
            | words::RULE_DONE_CHECK
    )
}

fn ladder(steps: &[Duration], index: u32) -> Duration {
    let last = steps.len().saturating_sub(1);
    let i = usize::try_from(index).unwrap_or(usize::MAX).min(last);
    steps.get(i).copied().unwrap_or(Duration::from_secs(3600))
}

fn later(at: Instant, d: Duration) -> Instant {
    at.checked_add(d).unwrap_or(at)
}

impl ThreadState {
    /// A person's turn started (a `UserMessage` that is not codex-alab's): the task opens.
    pub fn observe_person_turn(&mut self) {
        self.task = true;
        self.done = false;
        self.done_checked = false;
        self.short_streak = 0;
        self.carry_on = false;
    }

    /// The thread was resumed (a fresh store): `task` from its history, `finished` when its
    /// last reply ended the task, `relaunched` when aterm's crash relaunch brought it back
    /// (`extension::relaunched_by_aterm`).
    pub fn observe_resume(&mut self, task: bool, last_reply: Option<&str>, relaunched: bool) {
        self.task = task;
        let finished = words::says_finished(last_reply) || words::says_done(last_reply);
        self.done = task && words::says_finished(last_reply);
        self.carry_on = relaunched && task && !finished;
    }

    /// A turn this process ran ended.
    pub fn observe_turn_end(&mut self, end: TurnEnd, t: &Timing) {
        if end.by_person {
            self.observe_person_turn();
        }
        self.carry_on = false;
        if self.compacting {
            self.compacting = false;
            if matches!(end.wall, Some(Wall::Context { .. })) {
                self.note_wall(&end);
            } else {
                self.after_compact = end.wall.is_none() && !end.aborted;
            }
            self.point = Some(end);
            return;
        }
        self.after_compact = false;
        let short = end.worked < t.min_work || words::says_done(end.text.as_deref());
        if end.by_autopilot && self.last_rule.is_some_and(is_continue_rule) {
            self.short_streak = if short { self.short_streak + 1 } else { 0 };
        } else if !end.by_autopilot && end.wall.is_none() && !end.aborted {
            // Someone else's turn (a person's, or one codex-alab did not type) counts as
            // aterm's `TurnEndState` counts it: under `min_work` it is short and the act that
            // answers it backs off (2 min after one); real work ends the streak. A done
            // reply here is judged by the done check, not the streak.
            self.short_streak = if end.worked < t.min_work {
                self.short_streak + 1
            } else {
                0
            };
        } else if !short {
            self.short_streak = 0;
        }
        match &end.wall {
            Some(_) => {
                if end.worked >= t.progress {
                    self.wall = None;
                }
                self.note_wall(&end);
            }
            None if end.worked >= t.min_work || !end.by_autopilot => self.wall = None,
            None => {}
        }
        if !end.by_autopilot {
            self.last_rule = None;
        }
        self.point = Some(end);
    }

    fn note_wall(&mut self, end: &TurnEnd) {
        let Some(wall) = &end.wall else {
            return;
        };
        let class = wall.class();
        if self.wall.as_ref().is_none_or(|w| w.class != class) {
            self.wall = Some(WallTrack {
                class,
                first_seen: end.at,
                attempts: 0,
                last_try: None,
            });
        }
    }

    /// The decided act was carried out at `now`. An [`Act::Attention`] answers nothing: the
    /// point stands until a turn's end replaces it, so a point whose attention a passive spell
    /// cleared (`Supervisor::stand_by`, a release) is decided, and raised, again once codex-alab
    /// acts again (review of 4cdc96090, finding 3).
    pub fn acted(&mut self, act: &Act, now: Instant, t: &Timing) {
        match act {
            Act::Nothing | Act::Attention { .. } => return,
            Act::Type { rule, .. } => {
                if is_wall_rule(rule) {
                    self.tried_wall(now);
                } else {
                    self.typed.push_back(now);
                    while self
                        .typed
                        .front()
                        .is_some_and(|at| now.saturating_duration_since(*at) >= t.window)
                    {
                        self.typed.pop_front();
                    }
                }
                if *rule == words::RULE_DONE_CHECK {
                    self.done_checked = true;
                }
                self.last_rule = Some(rule);
            }
            Act::ResumeLimit { .. } => {
                self.tried_wall(now);
                self.last_rule = Some(words::RULE_LIMIT_RESUME);
            }
            Act::Compact => {
                self.tried_wall(now);
                self.compacting = true;
                self.last_rule = Some(words::RULE_COMPACT);
            }
            Act::Done => self.done = true,
        }
        self.point = None;
        self.after_compact = false;
        self.carry_on = false;
    }

    fn tried_wall(&mut self, now: Instant) {
        if let Some(wall) = &mut self.wall {
            wall.attempts += 1;
            wall.last_try = Some(now);
        }
    }
}

/// Why the turn end `end` finishes the task (`decide` answers it with [`Act::Done`]): the
/// worker's [`words::FINISHED`], or a done reply once [`words::DONE_CHECK`] was typed. A first
/// done reply is no finish: `decide` types the done check, and the worker may carry on.
fn done_reply(s: &ThreadState, end: &TurnEnd) -> Option<&'static str> {
    let text = end.text.as_deref();
    if words::says_finished(text) {
        Some("the worker said ATERM-TASK-DONE")
    } else if s.done_checked && words::says_done(text) {
        Some("a second done reply")
    } else {
        None
    }
}

/// Whether the point `s` stands at finishes the task: exactly where [`decide`] decides
/// [`Act::Done`] (whatever the policy, the timings or the clock), which ends the relaunch
/// carry-on chain (CONTRACT §4 `carriedOn`). A done phrase that `decide` answers with the
/// done check does not: the worker may carry on, stall, and be restarted.
pub fn ends_task(s: &ThreadState) -> bool {
    if s.compacting || s.done || !s.task || s.after_compact {
        return false;
    }
    s.point
        .as_ref()
        .is_some_and(|end| end.wall.is_none() && done_reply(s, end).is_some())
}

/// The decision at one point. Pure: the same state, policy, timings and clock give the same
/// decision. `jitter` is this session's share of [`Timing::reset_jitter`].
pub fn decide(s: &ThreadState, p: &Policy, t: &Timing, now: Instant, jitter: Duration) -> Decision {
    if s.compacting {
        return Decision::nothing("a compaction runs; its idle is not a turn");
    }
    if s.done {
        return Decision::nothing("the task is done; nothing until a person's turn");
    }
    let Some(end) = &s.point else {
        if s.carry_on && s.task {
            return match continuation(p) {
                Ok(text) => Decision::now(
                    Act::Type {
                        rule: words::RULE_HOST_TURN,
                        text,
                    },
                    "carry on after a relaunch",
                ),
                Err(why) => Decision::nothing(why),
            };
        }
        return Decision::nothing("no turn this process ran has ended here");
    };
    if !s.task {
        return Decision::nothing("taskless: no person's turn yet");
    }
    if s.after_compact {
        return match continuation(p) {
            Ok(text) => Decision::now(
                Act::Type {
                    rule: words::RULE_COMPACT,
                    text,
                },
                "continue after the compaction",
            ),
            Err(why) => Decision::nothing(why),
        };
    }
    if let Some(wall) = &end.wall {
        return decide_wall(s, end, wall, p, t, jitter);
    }
    let text = end.text.as_deref();
    if let Some(why) = done_reply(s, end) {
        return Decision::now(Act::Done, why);
    }
    if words::says_done(text) {
        return paced(
            s,
            p,
            t,
            end,
            now,
            words::RULE_DONE_CHECK,
            words::DONE_CHECK.to_string(),
        );
    }
    let said = if end.questions {
        Said::Question
    } else {
        words::classify_said(text)
    };
    match said {
        Said::Irreversible(why) => {
            if !p.answer_questions {
                return Decision::attention(
                    "question",
                    format!("question: {why}; answer_questions is off"),
                );
            }
            paced(
                s,
                p,
                t,
                end,
                now,
                words::RULE_ANSWER,
                words::REVERSIBLE_ANSWER.to_string(),
            )
        }
        Said::Stop(_) | Said::Question => {
            let why = match &said {
                Said::Stop(why) => why.clone(),
                _ => "the worker asked a question".to_string(),
            };
            if !p.answer_questions {
                return Decision::attention(
                    "question",
                    format!("question: {why}; answer_questions is off"),
                );
            }
            let (rule, answer) = match p.questions {
                Questions::Ask => {
                    return Decision::attention(
                        "question",
                        format!("question: {why}; questions=ask leaves it to a person"),
                    );
                }
                Questions::Recommended => (words::RULE_ANSWER, p.answer_text.clone()),
                Questions::LongTerm => (
                    words::RULE_PROSE_LONG_TERM,
                    Some(words::LONG_TERM_TEXT.to_string()),
                ),
                Questions::ShortTerm => (
                    words::RULE_PROSE_SHORT_TERM,
                    Some(words::SHORT_TERM_TEXT.to_string()),
                ),
            };
            match answer {
                Some(answer) => paced(s, p, t, end, now, rule, answer),
                None => Decision::attention("question", format!("question: {why}; no answer_text")),
            }
        }
        Said::Offer | Said::Plain => match continuation(p) {
            Ok(text) => paced(s, p, t, end, now, words::RULE_CONTINUE, text),
            Err(why) => Decision::nothing(why),
        },
    }
}

fn continuation(p: &Policy) -> Result<String, &'static str> {
    if !p.continue_on {
        return Err("continue is off");
    }
    p.continue_text
        .clone()
        .ok_or("no continue_text: nothing to type")
}

/// A continuation-type act, paced by the short streak and `continue_per_hour`.
fn paced(
    s: &ThreadState,
    p: &Policy,
    t: &Timing,
    end: &TurnEnd,
    now: Instant,
    rule: &'static str,
    text: String,
) -> Decision {
    let mut not_before: Option<Instant> = None;
    let mut why = format!("{rule} at a turn end");
    if s.short_streak > 0 {
        let doublings = s.short_streak.saturating_sub(1).min(16);
        let wait = t
            .short_backoff
            .saturating_mul(1u32 << doublings)
            .min(t.short_backoff_max);
        not_before = Some(later(end.at, wait));
        why = format!(
            "{rule} after {} short turn(s): back off {}s",
            s.short_streak,
            wait.as_secs()
        );
    }
    match p.continue_per_hour {
        Some(0) => return Decision::nothing("continue_per_hour unreadable: nothing is typed"),
        Some(cap) => {
            let recent: Vec<Instant> = s
                .typed
                .iter()
                .copied()
                .filter(|at| now.saturating_duration_since(*at) < t.window)
                .collect();
            if recent.len() >= usize::try_from(cap).unwrap_or(usize::MAX)
                && let Some(oldest) = recent.first()
            {
                let free = later(*oldest, t.window);
                if not_before.is_none_or(|at| at < free) {
                    not_before = Some(free);
                    why = format!("{rule}: continue_per_hour={cap} reached");
                }
            }
        }
        None => {}
    }
    Decision {
        act: Act::Type { rule, text },
        not_before,
        why,
        shown_wait: None,
    }
}

fn decide_wall(
    s: &ThreadState,
    end: &TurnEnd,
    wall: &Wall,
    p: &Policy,
    t: &Timing,
    jitter: Duration,
) -> Decision {
    let track = s.wall.as_ref();
    let attempts = track.map_or(0, |w| w.attempts);
    let since = track
        .and_then(|w| w.last_try.or(Some(w.first_seen)))
        .unwrap_or(end.at);
    match wall {
        Wall::Login => Decision::attention(
            "login",
            "login: Codex's sign-in was refused or expired; sign in to Codex again (codex login)",
        ),
        Wall::Fatal { why } => Decision::attention(
            "wall",
            format!("stopped: {why}; nothing typed gets past it"),
        ),
        Wall::Context { auto_compact } => {
            if !p.compact_on_context_wall {
                return Decision::attention(
                    "context",
                    "context: the context window is full; compact_on_context_wall is off",
                );
            }
            if !auto_compact {
                return Decision::attention(
                    "context",
                    "context: the context window is full and this thread does not compact",
                );
            }
            if attempts > 0 {
                return Decision::attention(
                    "context",
                    "context: the context window is full again right after a compaction",
                );
            }
            Decision::now(Act::Compact, "the context window is full: compact")
        }
        Wall::Usage {
            reset_at,
            reset_label,
        } => {
            if !p.resume_limits || !p.limit_wait {
                return Decision::attention(
                    "limit",
                    "limit: the usage limit was reached; resume_limits or limit_wait is off",
                );
            }
            let Some(text) = p.continue_text.clone() else {
                return Decision::attention(
                    "limit",
                    "limit: the usage limit was reached; no continue_text",
                );
            };
            let (at, words) = if attempts == 0 {
                match reset_at {
                    Some(reset) => (
                        later(later(*reset, t.reset_grace), jitter),
                        match reset_label {
                            Some(label) => format!("waiting for the usage reset at {label}"),
                            None => "waiting for the usage reset".to_string(),
                        },
                    ),
                    None => (
                        later(since, ladder(&t.limit_backoff, 0)),
                        "waiting out the usage limit".to_string(),
                    ),
                }
            } else {
                (
                    later(since, ladder(&t.limit_backoff, attempts - 1)),
                    "the usage limit stood after its reset; waiting again".to_string(),
                )
            };
            Decision {
                act: Act::ResumeLimit { text },
                not_before: Some(at),
                why: words.clone(),
                shown_wait: Some(words),
            }
        }
        Wall::Entitlement => {
            if !p.resume_limits {
                return Decision::attention(
                    "limit",
                    "limit: this account's quota or plan stops the model; resume_limits is off",
                );
            }
            let Some(text) = p.continue_text.clone() else {
                return Decision::attention("limit", "limit: quota reached; no continue_text");
            };
            let at = later(since, ladder(&t.entitlement_backoff, attempts));
            let words = "the account's quota stops the model; trying again later".to_string();
            Decision {
                act: Act::Type {
                    rule: words::RULE_LIMIT_RESUME,
                    text,
                },
                not_before: Some(at),
                why: words.clone(),
                shown_wait: Some(words),
            }
        }
        Wall::Server { what, cut_off } => {
            if !p.retry_api_errors {
                return Decision::attention(
                    "api",
                    format!("api: Codex reported {what}; retry_api_errors is off"),
                );
            }
            let at = later(since, ladder(&t.retry_backoff, attempts));
            let rule = if *cut_off {
                words::RULE_API_CUTOFF
            } else {
                words::RULE_API_RETRY
            };
            let why = format!("Codex reported {what}; retrying");
            Decision {
                act: Act::Type {
                    rule,
                    text: words::wall_retry_text(what, *cut_off, false),
                },
                not_before: Some(at),
                why: why.clone(),
                shown_wait: Some(why),
            }
        }
        Wall::Network { what } => {
            if !p.retry_api_errors {
                return Decision::attention(
                    "api",
                    format!("api: Codex reported {what}; retry_api_errors is off"),
                );
            }
            let at = later(since, ladder(&t.net_backoff, attempts));
            let why = format!("Codex reported {what}; retrying");
            Decision {
                act: Act::Type {
                    rule: words::RULE_API_RETRY,
                    text: words::wall_retry_text(what, false, true),
                },
                not_before: Some(at),
                why: why.clone(),
                shown_wait: Some(why),
            }
        }
    }
}
