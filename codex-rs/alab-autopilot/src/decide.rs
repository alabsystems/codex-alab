// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The pure turn-end decider, aterm-style: [`decide`] is a function of this thread's memory
//! ([`ThreadState`]), the policy aterm answered ([`Policy`]), the timings ([`Timing`]) and
//! the clock. The runtime (`extension.rs`) feeds it what the thread did
//! ([`ThreadState::observe_turn_end`], [`ThreadState::observe_person_turn`],
//! [`ThreadState::observe_resume`], [`ThreadState::observe_mode`]), executes the
//! [`Decision`] after the before-every-act checks, and reports back with
//! [`ThreadState::acted`].
//!
//! Mirrors aterm's `decide_turn_end` (`supervise/policy/turn_end.rs`, aterm `a355109905`;
//! its landing's rules, the reply read as the whole turn and Land's wall words as of aterm
//! main e0da8d68df), cut to what Codex's own events say (no screen; who opened a turn is
//! Codex's own word, so aterm's screen signals of an act taken have no counterpart here):
//! CONTRACT §5's rows turn end, continue, taskless, carry-on, Cruise, Land, NEEDS YOU,
//! back-off, typing stop, prose questions, API/stream, usage window, context window and login.
//!
//! FOREVER CRUISE / FOREVER LAND (aterm owner direction 2026-10-19: "Cruise is always looking
//! for more completeness and perfection. Land is trying to wrap it up to end the session
//! cleanly"; "if landing LAND EVERYTHING INTO REMOTE MAIN NO EXCEPTIONS"; "if you need
//! something from the user, be LOUDER ABOUT IT ... but NEVER let that block you"):
//!
//! - [`Mode::Cruise`] never ends a task on the worker's words: a done claim
//!   ([`words::says_done`]) gets the defend-and-find-more prompt ([`words::DONE_CHECK`]) with
//!   the open work it named and what the repository still holds as suffixes, at every claim,
//!   paced by the short streak's back-off (cost control, never a stop; RULING F's typing stop,
//!   [`ThreadState::repeats`], is the one stop: the same reply twice with no work, or six
//!   no-work answers in a row, types nothing until real work or a person's turn).
//! - [`Mode::Land`] types the land instruction ([`words::LAND`]) at an episode's first point,
//!   then a nudge ([`words::LAND_NUDGE`]) at every free point with no bound, and lands
//!   ([`Act::Landed`]) only at a land act's yield whose reply is a clean defense
//!   ([`words::clean_defense`]) over a repository that holds nothing no remote main has
//!   ([`Act::Probe`], `repo::probe`; a failed or deadline-cut read is unread, while
//!   guarded/nonlocal folders remain skipped without entering them). Then nothing is
//!   typed until A PERSON'S ACT re-arms Land (aterm main e0da8d68df, the rule of 667c926ec4):
//!   their turn opens the task again, and an Off and back judges the landing again over the
//!   repository as it is ([`Rejudge::Off`]). NOBODY'S ACT keeps it: a re-exec carries it
//!   ([`ThreadState::observe_owed`]), and the landed task's own wake (a turn nobody asked,
//!   a wall it meets included) keeps it where its words left nothing open and the repository
//!   is clean or holds only what it held as the wake began ([`Rejudge::Wake`]), recording it
//!   again for aterm, which reopened it at the wake's turn ([`ThreadState::reland`]); else
//!   Land's nudge comes. Under Land a wall's resume types Land's own words ([`words::LAND`] or the
//!   nudge) with the reversible clause and the ack, never Cruise's continuation.
//! - A NEEDS YOU line ([`crate::needs`]) never yields nothing and never attention: the needs
//!   stand ([`ThreadState::needs`], put on the person's screen by the runtime), and the point's
//!   own act carries the ack ([`crate::needs::NEEDS_ACK`]) once per new set.

use std::collections::VecDeque;
use std::time::Duration;
use std::time::Instant;

use crate::go_ahead;
use crate::go_ahead::GO_AHEAD_ACTS;
use crate::go_ahead::GO_AHEAD_HELD;
use crate::go_ahead::GO_AHEAD_SPAN;
use crate::go_ahead::GO_AHEAD_WAIT;
use crate::go_ahead::KEY_DRIFT;
use crate::needs;
use crate::netwatch::Reach;
use crate::open_work::open_work;
use crate::open_work::open_work_strict;
use crate::replies::HeldBy;
use crate::replies::Replies;
use crate::replies::mark_of;
use crate::repo::RepoRead;
use crate::repo::RepoWork;
use crate::words;
use crate::words::ApiCause;
use crate::words::RepoAsk;
use crate::words::Said;
use crate::words::Suffix;

/// How far past a turn's busy work a person's window gesture still counts as their hand in it
/// (the message sent just before the turn began): aterm's `PERSON_TURN_SLACK`.
pub const PERSON_TURN_SLACK: Duration = Duration::from_secs(30);

/// The longest answer that is still A PERSON'S OWN EXCHANGE ([`ThreadState::left`]): a
/// question or a message of theirs answered within it is theirs to answer, where a message can
/// be theirs at all (under Land only the session's opening one, under Off any, under Cruise
/// only an opening without tools or a question box: [`ThreadState::ended_under`]); a turn
/// they opened that works longer is a job,
/// and is continued (aterm's `EXCHANGE_WORK`; the owner, 2026-10-15: "these auto prompts are
/// for continuously running jobs").
pub const EXCHANGE_WORK: Duration = Duration::from_secs(10 * 60);

/// The short turns A LEAD'S WORKER waits no back-off for at a done claim or a question
/// ([`waited_on`]): aterm's `MANAGED_SHORT_TURNS`.
pub const MANAGED_SHORT_TURNS: u32 = 3;

/// aterm's `TurnEndTiming`, the parts Codex needs (defaults are aterm's; tests shorten them).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Timing {
    /// The busy work that makes a turn real work: a turn that did less is short.
    pub min_work: Duration,
    /// The wait before the act after ONE short turn; each more doubles it, up to the max.
    pub short_backoff: Duration,
    pub short_backoff_max: Duration,
    /// The server's own failure (a status, an overload, a request that timed out): from the
    /// appearance that answered each try ([`WallTrack::since`]), as every ladder counts.
    pub retry_backoff: Vec<Duration>,
    /// An API never reached (no status): short, a blind try costs only core's own retries.
    /// Also the spacing of `api-back@v1` acts the wall met again ([`WallTrack::ups`]), and of
    /// a reply cut off past its first try, which goes at once (aterm's `(ApiCause::CutOff,
    /// _)`).
    pub net_backoff: Vec<Duration>,
    /// How long nothing is typed into an API MEASURED DOWN ([`Reach::Down`]) before one try
    /// anyway (aterm's `down_hold`): the most a wrong measure holds a worker.
    pub down_hold: Duration,
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
    /// After the host's last wake, how long it must stay awake (plus this session's share of
    /// [`Timing::reset_jitter`]) before an act is typed ([`ThreadState::woke`], aterm's
    /// `WAKE_SETTLE` and `WAKE_SPREAD`): longer than a dark wake lasts.
    pub wake_settle: Duration,
    /// How long a usage window's due resume waits for its first measure of the API's reach
    /// ([`ThreadState::net_asked`]): one probe's whole budget and a second.
    pub probe_wait: Duration,
    /// How often a thread at a lost login looks at its login's file (aterm's
    /// `LOGIN_SHARED_LOOK`, 15 s): a look only stats it ([`crate::login`]), and codex reloads
    /// the login only where the file moved.
    pub login_look: Duration,
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
            down_hold: min(15),
            reset_grace: Duration::from_secs(60),
            reset_jitter: Duration::from_secs(60),
            limit_backoff: vec![min(10), min(30)],
            entitlement_backoff: vec![min(30), min(120), min(360)],
            window: min(60),
            progress: min(5),
            wake_settle: codex_alab_core::wake::WAKE_SETTLE,
            probe_wait: crate::netwatch::PROBE_BUDGET + Duration::from_secs(1),
            login_look: Duration::from_secs(15),
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

/// The session's Autopilot word as the decider reads it (`@self status autopilot=`, CONTRACT
/// §3): Off never reaches the decider (aterm's policy reads `effective=off`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Mode {
    /// FOREVER CRUISE: always looking for more completeness and correctness; the worker's
    /// words never end the task. Also the word an aterm that prints none means.
    #[default]
    Cruise,
    /// FOREVER LAND: land all of the work into the remote main branch, then end cleanly.
    Land,
}

impl Mode {
    /// The mode of aterm's `status autopilot=` word: `None` for `off`, which is no mode (Off
    /// never acts; aterm's policy read says so, and the word in force before it stands: a
    /// landing is kept, CONTRACT §2).
    pub fn of_status(word: codex_alab_aterm::status::Autopilot) -> Option<Mode> {
        match word {
            codex_alab_aterm::status::Autopilot::Land => Some(Mode::Land),
            codex_alab_aterm::status::Autopilot::Cruise => Some(Mode::Cruise),
            codex_alab_aterm::status::Autopilot::Off => None,
        }
    }

    /// The mode `word` spells ([`Mode::word`]), if it spells one.
    pub fn of_word(word: &str) -> Option<Mode> {
        match word {
            "cruise" => Some(Mode::Cruise),
            "land" => Some(Mode::Land),
            _ => None,
        }
    }

    /// The word, as aterm spells it.
    pub fn word(self) -> &'static str {
        match self {
            Mode::Cruise => "cruise",
            Mode::Land => "land",
        }
    }
}

/// THE TEXTS FOREVER CRUISE / FOREVER LAND TYPES: aterm's own, as its `harness policy` reply
/// sends them (`done_check_text`, `land_text`, `land_nudge_text` and `land_answer_text`
/// composed with the standing rules; `needs_hint_text` and `needs_ack_text` bare), else
/// codex-alab's verbatim copies of aterm's words ([`Texts::default`]). An act's suffixes go
/// before the rules a text carries, fitted as aterm fits them ([`words::fitted_act`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Texts {
    /// Cruise's defend-and-find-more prompt, without its per-point suffixes: aterm's first
    /// angle's whole act where it sent [`Texts::done_angles`].
    pub done_check: String,
    /// CRUISE'S SIX KINDS OF WORK (aterm's `DONE_ANGLES`, `done_check_angles=` one to a line),
    /// one asked at each done claim in turn ([`Texts::done_check_words`]): aterm's, where it sent
    /// them and [`Texts::done_check`] holds the first. Empty from an older aterm, whose done
    /// check is typed whole at every claim, as that aterm types it.
    pub done_angles: Vec<String>,
    /// Land's instruction.
    pub land: String,
    /// Land's nudge.
    pub land_nudge: String,
    /// Land's answer to a question.
    pub land_answer: String,
    /// LAND'S TEXT FOR A QUESTION BOX'S NOTES (`answer-land@v1`, [`crate::answer`]): aterm's
    /// `land_question_text`, which it sends bare (a free-text answer takes no standing rules),
    /// else [`words::LAND_QUESTION_TEXT`].
    pub land_question: String,
    /// The NEEDS YOU hint appended after an answer.
    pub needs_hint: String,
    /// The ack appended once per new set of needs.
    pub needs_ack: String,
    /// Whether aterm's reply sent its act texts (`done_check_text`, `land_text`,
    /// `land_nudge_text`, `land_answer_text`), which it composes with the standing rules
    /// only where some stand; `false`: these are codex-alab's verbatim copies, and an older
    /// aterm's `continue_text` is the one copy of the rules there is ([`words::standing_rules`]).
    pub from_aterm: bool,
}

impl Default for Texts {
    fn default() -> Self {
        Texts {
            done_check: words::DONE_CHECK.to_string(),
            done_angles: Vec::new(),
            land: words::LAND.to_string(),
            land_nudge: words::LAND_NUDGE.to_string(),
            land_answer: words::LAND_ANSWER.to_string(),
            land_question: words::LAND_QUESTION_TEXT.to_string(),
            needs_hint: needs::NEEDS_HINT.to_string(),
            needs_ack: needs::NEEDS_ACK.to_string(),
            from_aterm: false,
        }
    }
}

impl Texts {
    /// THE DONE CHECK'S WORDS AT A CRUISE RUN'S `checks`th CLAIM ([`ThreadState::checks`],
    /// counted from 0; the seventh is the first again), its standing rules split off: aterm's
    /// first angle's act with that angle swapped for the one in turn, as aterm's
    /// `done_check_words(n)` composes it, and the rules aterm composed it with. Where aterm sent
    /// no angles, its act whole at every claim.
    pub fn done_check_words(&self, checks: u8) -> (String, Option<&str>) {
        let (said, rules) = words::split_rules(&self.done_check);
        match self.done_angles.first() {
            Some(first) => {
                let angle = &self.done_angles[usize::from(checks) % self.done_angles.len()];
                (said.replacen(first.as_str(), angle, 1), rules)
            }
            None => (said.to_string(), rules),
        }
    }

    /// aterm's texts where its reply sends them, the verbatim copies where it does not.
    pub fn from_harness(p: &codex_alab_aterm::HarnessPolicy) -> Texts {
        let fallback = Texts::default();
        let text = |key: &str, copy: String| p.text(key).map_or(copy, str::to_string);
        let done_check = text("done_check_text", fallback.done_check);
        // aterm sends the angles beside its first angle's whole act; a reply whose act holds
        // no first angle of these is read as an older one's, its act typed whole.
        let done_angles: Vec<String> = p
            .text("done_check_angles")
            .map(|angles| {
                angles
                    .lines()
                    .map(str::trim)
                    .filter(|angle| !angle.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let done_angles = match done_angles.first() {
            Some(first) if words::split_rules(&done_check).0.contains(first.as_str()) => {
                done_angles
            }
            _ => Vec::new(),
        };
        Texts {
            done_check,
            done_angles,
            land: text("land_text", fallback.land),
            land_nudge: text("land_nudge_text", fallback.land_nudge),
            land_answer: text("land_answer_text", fallback.land_answer),
            land_question: text("land_question_text", fallback.land_question),
            needs_hint: text("needs_hint_text", fallback.needs_hint),
            needs_ack: text("needs_ack_text", fallback.needs_ack),
            // `land_question_text` is no act text here: it comes bare whatever rules stand, so
            // it says nothing of whether aterm composed its act texts with them.
            from_aterm: [
                "done_check_text",
                "land_text",
                "land_nudge_text",
                "land_answer_text",
            ]
            .iter()
            .any(|key| p.text(key).is_some()),
        }
    }
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
    /// aterm's raw `[harness] answer_questions` switch, as the prose question and go-ahead paths
    /// read it. The question overlay never reads it: it reads [`Self::questions`], which aterm
    /// resolved over the switch (`answer::decide_box`).
    pub answer_questions: bool,
    pub answer_text: Option<String>,
    pub questions: Questions,
    pub retry_api_errors: bool,
    /// THE API'S REACH MAY BE PROBED (aterm's `probe_api`, its host's own probe switch;
    /// amended 2026-10-28 after the parity sweep, gap 30): a network wall's route and a due
    /// usage resume's are measured ([`crate::netwatch`]). Off, no probe runs, the reach reads
    /// [`Reach::Unknown`], and those walls keep the time ladder, as a route the probe cannot
    /// reproduce keeps it.
    pub probe_api: bool,
    pub resume_limits: bool,
    pub limit_wait: bool,
    pub compact_on_context_wall: bool,
    /// The live upgrade by re-exec (aterm's `upgrade`).
    pub upgrade: bool,
    /// THE LIVE UPGRADE ASKS FIRST (aterm's `upgrade_ask`, its cooperative mode; amended
    /// 2026-10-28 after the parity sweep, gap 27: read as the upgrade's off before): the re-exec
    /// waits for the agent to say it is ready, asked by aterm's notice on aterm's schedule
    /// ([`crate::upgrade_ask`], [`ThreadState::upgrade_notice`]).
    pub upgrade_ask: bool,
    /// ATERM'S FACTS MAY RIDE THIS SESSION'S ACTS (aterm's `[harness] facts`, a limit; CONTRACT §3,
    /// §5 "aterm fact", gap 23): on, an act under one of [`crate::words::FACT_RIDES`] reads
    /// aterm's fact for it; off, codex-alab reads none (aterm also answers `why=facts-off`).
    pub facts: bool,
    pub human_grace: Duration,
    /// The session's Autopilot word, from `@self status autopilot=` (the supervisor reads it
    /// beside the policy; [`Policy::from_harness`] leaves Cruise).
    pub autopilot: Mode,
    /// The texts Cruise, Land and the needs type.
    pub texts: Texts,
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
            probe_api: p.switch("probe_api"),
            resume_limits: p.switch("resume_limits"),
            limit_wait: p.switch("limit_wait"),
            compact_on_context_wall: p.switch("compact_on_context_wall"),
            upgrade: p.switch("upgrade"),
            upgrade_ask: p.switch("upgrade_ask"),
            facts: p.switch("facts"),
            human_grace: p.human_grace(),
            autopilot: Mode::Cruise,
            texts: Texts::from_harness(p),
        }
    }
}

/// The wall a turn ended on, classified from Codex's terminal error (after core's own
/// retries).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Wall {
    /// A status, an overload, a request that timed out: `retry_backoff`. A reply cut off
    /// (`cut_off`, a stream that broke): at once, then `net_backoff` (aterm's `CutOff`).
    Server {
        what: String,
        cut_off: bool,
        /// Original provider deadline, retained through ordinary retry exhaustion.
        retry_at: Option<Instant>,
    },
    /// The API was never reached: `net_backoff`.
    Network { what: String },
    /// A reply past its output token maximum (aterm's `ApiCause::Overlong`; Codex's
    /// incomplete response, `max_output_tokens`): retried at once in aterm's words, which ask
    /// for smaller pieces, then on `retry_backoff`, its ladder climbing across the work each
    /// try leads to, which is the long reply the wall cuts off (aterm's `work_is_its_cause`).
    Overlong { what: String },
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
    /// The rejected action stays excluded; one bounded continuation may do independent work.
    Policy,
    /// A wall automatic continuation cannot pass (an invalid request, a budget or another policy).
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
    Policy,
    Fatal,
}

impl Wall {
    fn class(&self) -> WallClass {
        match self {
            Wall::Server { .. } | Wall::Overlong { .. } => WallClass::Server,
            Wall::Network { .. } => WallClass::Network,
            Wall::Usage { .. } => WallClass::Usage,
            Wall::Entitlement => WallClass::Entitlement,
            Wall::Context { .. } => WallClass::Context,
            Wall::Login => WallClass::Login,
            Wall::Policy => WallClass::Policy,
            Wall::Fatal { .. } => WallClass::Fatal,
        }
    }
}

impl WallClass {
    /// The class's word (`usage`, `entitlement`, `server`, `network`, `context`, `login`,
    /// `policy`, `fatal`): what a fact read names as its wall (`harness fact … wall=`), where
    /// aterm reads `usage` and `entitlement` as its usage window and its model bucket.
    fn word(self) -> &'static str {
        match self {
            WallClass::Server => "server",
            WallClass::Network => "network",
            WallClass::Usage => "usage",
            WallClass::Entitlement => "entitlement",
            WallClass::Context => "context",
            WallClass::Login => "login",
            WallClass::Policy => "policy",
            WallClass::Fatal => "fatal",
        }
    }

    /// The record's word for a class whose ladder outlives a process (aterm's `seed_retry`:
    /// the server's and the network's retries; `seed_usage_attempt`: the usage window's and
    /// the quota's resumes); `None` for the rest, which aterm seeds none of: a compaction's
    /// attempt says nothing past the process, and a sign-in or a fatal stop has no ladder.
    fn carried_word(self) -> Option<&'static str> {
        match self {
            WallClass::Server | WallClass::Network | WallClass::Usage | WallClass::Entitlement => {
                Some(self.word())
            }
            WallClass::Context | WallClass::Login | WallClass::Policy | WallClass::Fatal => None,
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
    /// THE APPEARANCE THE LADDER COUNTS FROM (aterm's `WallTrack::since`, as its `track_wall`
    /// moves it: `if retried { t.since = now }`): the wall's first appearance, moved to the end
    /// of the turn that answered codex-alab's try past it where that turn met it again
    /// ([`Self::answer_owed`]). Every class's next try is a rung from here, never from the try
    /// itself: the try's turn (core's own retries, a long reply) lies between them, and aterm
    /// counts the rung from the answer. A person's or a goal's turn that meets the wall moves
    /// it not; real work a try led to starts the ladder over from its answer
    /// ([`ThreadState::observe_turn_end`]).
    pub since: Instant,
    /// The tries typed on the API's MEASURED RETURN ([`words::RULE_API_BACK`], aterm's
    /// `WallTrack::ups`): the next is spaced on `net_backoff` by their count. This process's
    /// own: a carried ladder starts it at none, and the first measured return goes at once.
    pub ups: u32,
    /// A TRY PAST THIS WALL IS OWED ITS ANSWER (aterm's awaited retry, and its
    /// `WallTrack::owed` for a ladder a loop before it was on): set by each try, taken by the
    /// next turn that ends. In this process that is the try's own turn (a goal's, where the
    /// try resumed a goal); after a relaunch whose dead process typed the try and died in its
    /// turn, the first turn the new process sees end (its carry-on, its owed act, a person's
    /// turn), as aterm's `seed_retry` reads the wall's first reading then as the retry's
    /// next appearance. Met by the wall, that turn is the answer ([`Self::since`]); ended
    /// without it, the try was taken and the track goes.
    pub answer_owed: bool,
    /// THE LOST LOGIN'S LINE WAS TYPED PAST THIS WALL ([`words::RULE_LOGIN_BACK`], aterm's
    /// `WallTrack::login.back_typed`): set by the act, it lives with the track, so the wall met
    /// again after it types nothing more, whatever the login does next, and the ask stands.
    /// This process's own: a login's track is never carried.
    pub back_typed: bool,
}

impl WallTrack {
    /// A wall first standing at `at`.
    fn new(class: WallClass, at: Instant) -> WallTrack {
        WallTrack {
            class,
            first_seen: at,
            attempts: 0,
            last_try: None,
            since: at,
            ups: 0,
            answer_owed: false,
            back_typed: false,
        }
    }

    /// The word of the wall's class ([`WallClass::word`]): what an act at this track names as its
    /// wall in aterm's fact read (CONTRACT §5 "aterm fact").
    pub fn class_word(&self) -> &'static str {
        self.class.word()
    }

    /// The class word a re-exec or a relaunch carries this ladder under
    /// ([`ThreadState::seed_pace`]); `None` where its class's ladder ends with the process.
    pub fn carried_class(&self) -> Option<&'static str> {
        self.class.carried_word()
    }

    /// The ladder a process before this one was on, under [`Self::carried_class`]'s word
    /// `class`, counted from `since` (the appearance that answered its last try, as that
    /// process counted it), its last try owed its answer where `answer_owed`; `None` for a
    /// word no carried class has. A record from before `since` was carried names none: the
    /// ladder counts from its last try then, as that build counted it.
    pub fn carried(
        class: &str,
        first_seen: Instant,
        attempts: u32,
        last_try: Option<Instant>,
        since: Option<Instant>,
        answer_owed: bool,
    ) -> Option<WallTrack> {
        let class = [
            WallClass::Server,
            WallClass::Network,
            WallClass::Usage,
            WallClass::Entitlement,
        ]
        .into_iter()
        .find(|c| c.carried_word() == Some(class))?;
        Some(WallTrack {
            class,
            first_seen,
            attempts,
            last_try,
            since: since.or(last_try).unwrap_or(first_seen),
            ups: 0,
            answer_owed,
            back_typed: false,
        })
    }
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
    /// THE TURN'S EARLIER AGENT MESSAGES, in order, blank-line joined (aterm's `turn_said`):
    /// `None` where the turn said no more than its last. The reply is the whole turn: a need
    /// said in an earlier message stands beside its last words' ([`Needs::fold`]), keeps a
    /// clean defense from landing, and leaves a landed task's own wake open
    /// ([`wake_left_open`]).
    pub earlier: Option<String>,
    /// The agent message carried `questions: Some(..)` (`request_user_input_async`).
    pub questions: bool,
    pub worked: Duration,
    pub wall: Option<Wall>,
    pub aborted: bool,
    /// THE TURN RAN A TOOL (aterm's `ran_tools`, its tool rows): any tool core started in it
    /// but the plan's (`update_plan`) and a question's (`request_user_input`), which aterm's
    /// readers draw no call row for. Ruling F's real work ([`ThreadState::observe_turn_end`]):
    /// a turn that ran one did work, however short.
    pub ran_tools: bool,
    /// A TURN END RESTORED, NOT SEEN (the recovery's and a resumed history's synthetic point,
    /// `recovery.rs`, `resume_history.rs`, and the point a re-exec's owed act stands at,
    /// [`ThreadState::observe_owed`]): ruling F's replies neither count it nor lift at it,
    /// since no answer came there in this process.
    pub restored: bool,
}

impl TurnEnd {
    /// The turn's words whole: its earlier messages, then its last (aterm's `turn_said`
    /// before `said_tail`, as its `fold_needs` reads them), the last read on its own: a fence
    /// the earlier words leave open ends before it ([`needs::fence_closed`]; a record an older
    /// build wrote joined them unclosed).
    fn said_whole(&self) -> Option<String> {
        match (&self.earlier, &self.text) {
            (Some(before), last) => Some(format!(
                "{}\n\n{}",
                needs::fence_closed(before),
                last.as_deref().unwrap_or_default()
            )),
            (None, last) => last.clone(),
        }
    }

    /// THE GO-AHEAD THE REPLY PARKS ON (aterm's `TurnEndReading::go_ahead`,
    /// [`go_ahead::parked`]): the turn's earlier messages and its last, as the needs fold reads
    /// them. `None` where the turn was aborted, a wall cut it, or its message carried a question
    /// box ([`Self::questions`]: its options are the answer, never the go-ahead's words).
    pub fn go_ahead(&self) -> Option<go_ahead::Park> {
        if self.aborted || self.wall.is_some() || self.questions {
            return None;
        }
        go_ahead::parked(
            self.earlier.as_deref().unwrap_or_default(),
            self.text.as_deref().unwrap_or_default(),
        )
    }

    /// No earlier message of the turn names a need ([`needs::needs_you`]).
    fn earlier_names_no_need(&self) -> bool {
        self.earlier
            .as_deref()
            .is_none_or(|t| needs::needs_you(t).is_empty())
    }
}

/// WHETHER A LANDED TASK'S OWN WAKE LEFT SOMETHING OPEN, by its words (aterm's
/// `wake_left_open`): its last message or the turn's earlier ones ([`TurnEnd::earlier`]) name
/// a need ([`needs::needs_you`]), or Land's strict reading of them, needs left out, finds
/// open work ([`open_work_strict`]). Codex hands each message whole, so no read is cut short
/// of the turn's start (aterm's `said_cut` has no counterpart here).
pub fn wake_left_open(end: &TurnEnd) -> bool {
    [end.earlier.as_deref(), end.text.as_deref()]
        .into_iter()
        .flatten()
        .any(|t| {
            !needs::needs_you(t).is_empty()
                || !open_work_strict(Some(&needs::without_needs(t))).is_empty()
        })
}

/// The Land episode in hand (aterm's `LandEpisode`): whether its instruction was typed, and
/// how many nudges since (no bound: for the books and the nudges Land's hold names).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LandEpisode {
    pub typed: bool,
    pub nudges: u32,
    /// Work read by Land in this episode, refreshed by its next repository probe.
    pub repo: Option<RepoWork>,
    /// WHERE LAND'S HOLD BEGAN in this episode (aterm's `LandEpisode::hold`): what aterm's
    /// landing says as `since` and counts its nudges from ([`ThreadState::land_held`]).
    /// Carried past a re-exec and aterm's relaunch in the record's pace
    /// ([`ThreadState::seed_land_hold`]).
    pub hold: HoldStart,
    /// THE LAND ACT'S YIELD AS JUDGED (aterm's `LandEpisode::legs`, its `hold_at_yield`): what
    /// held the landing at the last yield read, until any other turn begins
    /// ([`ThreadState::observe_turn_start`]) or ends ([`ThreadState::observe_turn_end`]);
    /// `None` where none was read since.
    pub legs: Option<YieldRead>,
}

/// WHERE LAND'S HOLD BEGAN (aterm's `HoldStart`): the land act that began it, the nudges counted
/// before it, and whether a landing ended it since. Land's instruction begins a hold, and so
/// does the act that first types Land's words in an episode (a compaction's continuation, a
/// carry-on or a usage resume under Land) and any land act after a landing; so, here, does a
/// land act where this image knows no beginning (after a re-exec or a relaunch whose record
/// carried none, an older build's), as the wire takes no hold without its `since`. The record
/// carries the standing one ([`ThreadState::land_hold_carried`]), which the image after takes
/// up ([`ThreadState::seed_land_hold`]), as aterm's loop seeds its own from its books.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HoldStart {
    /// When the act that began it was typed, unix ms ([`ThreadState::stamp_land_hold`]):
    /// `None` until its act is stamped.
    pub since_ms: Option<u64>,
    /// The nudges the episode counted before it.
    pub base: u32,
    /// A landing ended it: no hold until Land's next act begins one.
    pub relanded: bool,
    /// The act that began it waits for its stamp.
    pub unstamped: bool,
}

impl HoldStart {
    /// The hold a landing leaves: none, until Land's next act begins one.
    pub const LANDED: HoldStart = HoldStart {
        since_ms: None,
        base: 0,
        relanded: true,
        unstamped: false,
    };

    /// Whether this image knows where the standing hold began (its act stamped, or about to
    /// be).
    fn known(self) -> bool {
        !self.relanded && (self.since_ms.is_some() || self.unstamped)
    }
}

/// A LAND ACT'S YIELD AS JUDGED ([`LandEpisode::legs`]): the point it was read at, and what held
/// the landing there ([`codex_alab_aterm::LandRead`]: aterm's `hold_at_yield`). The reply's words
/// give the needs, Land's strict reading's open words and the defense's absence at the yield's
/// end; the repository's work comes with the probe run for that point
/// ([`ThreadState::probed_for`]), which Land reads at a done claim and a clean defense, as
/// aterm reads it at a done claim. Codex hands each message whole, so nothing is ever cut.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YieldRead {
    pub at: Instant,
    pub read: codex_alab_aterm::LandRead,
}

impl YieldRead {
    /// The yield `end`, its words read.
    fn of(end: &TurnEnd) -> YieldRead {
        use codex_alab_aterm::LandLeg;
        let tail = end.text.as_deref();
        let said_need = [end.earlier.as_deref(), tail]
            .into_iter()
            .flatten()
            .any(|t| !needs::needs_you(t).is_empty());
        let open = words::land_open_words(tail);
        let legs = [
            (LandLeg::Needs, said_need),
            (LandLeg::Open, !open.is_empty()),
            (
                LandLeg::Defense,
                tail.and_then(needs::defense_line).is_none(),
            ),
        ]
        .into_iter()
        .filter_map(|(leg, held)| held.then_some(leg))
        .collect();
        YieldRead {
            at: end.at,
            read: codex_alab_aterm::LandRead {
                legs,
                open: open
                    .iter()
                    .take(codex_alab_aterm::LAND_OPEN_WORDS)
                    .map(|word| (*word).to_string())
                    .collect(),
                ..codex_alab_aterm::LandRead::default()
            },
        }
    }

    /// The repository's work the probe for its point read (`found`): its legs and counts,
    /// replacing any an earlier probe of the same point gave.
    fn with_repo(&mut self, found: RepoRead<&RepoWork>) {
        use codex_alab_aterm::LandLeg;
        let read = &mut self.read;
        read.legs
            .retain(|leg| !matches!(leg, LandLeg::Repo | LandLeg::RepoUnread));
        let count = |n: usize| u16::try_from(n).unwrap_or(u16::MAX);
        let work = match found {
            RepoRead::Read(Some(work)) => {
                read.legs.push(LandLeg::Repo);
                Some(work)
            }
            RepoRead::Unread => {
                read.legs.push(LandLeg::RepoUnread);
                None
            }
            RepoRead::Read(None) => None,
        };
        read.legs
            .sort_by_key(|leg| LandLeg::ALL.iter().position(|each| each == leg));
        read.changed = work.map_or(0, |w| count(w.changed));
        read.untracked = work.map_or(0, |w| count(w.untracked));
        read.stashes = work.map_or(0, |w| count(w.stashes));
        // aterm's hold takes five counts: the commits on no remote main a submodule or a
        // branch the session used holds are unpushed commits too (review of the forever probe).
        read.unpushed = work.map_or(0, |w| count(w.commits_unpushed()));
        read.unlanded = work.map_or(0, |w| count(w.unlanded));
    }
}

/// What NEEDS YOU the decider holds standing (aterm's `Needs`), AND WHAT THE MENU BAR SHOWS OF
/// THEM (aterm's DESIGN-harness-autopilot §15.6, at aterm `ff13268a5b`): the lines the owner's
/// standing go-ahead answers ([`crate::go_ahead::Park::needs`]) are held off it from the decision
/// that types the go-ahead, or waits out its back-off (`answering`), and dropped for good once
/// that act was submitted, or a reply that names no need was read before it (`answered`). Any
/// other decision on a reply that names them, the go-ahead's refusal, an unsubmitted act and every
/// reset of the needs put them back ([`ThreadState::go_ahead_judged`],
/// [`ThreadState::go_ahead_unwritten`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Needs {
    /// The needs standing, as the worker's last report named them ([`needs::needs_you`]).
    pub standing: Vec<String>,
    /// Their set's key ([`needs::needs_key`]); `0` for none.
    pub key: u64,
    /// The key of the set an act was typed with the ack for: the needs it left on the menu bar.
    pub acked: Option<u64>,
    /// The lines a submitted go-ahead answered, and those of one decided but not typed before a
    /// reply that names no need was read.
    pub answered: Vec<String>,
    /// The lines the go-ahead decided at this point answers, not yet submitted.
    pub answering: Vec<String>,
    /// The last reply read named needs, and no go-ahead was submitted since.
    pub named: bool,
}

impl Needs {
    /// A NEW SET OF NEEDS STANDS UNACKED: the needs an act other than the go-ahead leaves on the
    /// menu bar are not the set last acked (aterm's `needs_ack_owed`): the point's act carries
    /// the ack (its rule unchanged). Acked by the act typed with it ([`ThreadState::acted`]); a
    /// set that changes owes its own ack.
    pub fn ack_owed(&self) -> bool {
        let held: &[String] = if self.named { &[] } else { &self.answered };
        self.ack_owed_of(&self.less(&[held]))
    }

    /// [`Self::ack_owed`] for the go-ahead that answers `past` (aterm's `needs_ack_owed_past`):
    /// the needs it leaves on the menu bar are not the set last acked.
    pub fn ack_owed_past(&self, past: &[String]) -> bool {
        self.ack_owed_of(&self.less(&[self.answered.as_slice(), past]))
    }

    /// Whether `shown` is a set of needs not acked.
    fn ack_owed_of(&self, shown: &[String]) -> bool {
        let key = needs::needs_key(shown);
        key != 0 && self.acked != Some(key)
    }

    /// The key an act that carries the ack acks, where it answers `past` too.
    fn acked_past(&self, past: &[String]) -> u64 {
        needs::needs_key(&self.less(&[self.answered.as_slice(), past]))
    }

    /// A need standing names an irreversible act: the act that acks it says to take none.
    pub fn destroy(&self) -> bool {
        needs::needs_destroy(&self.standing).is_some()
    }

    /// The standing needs less `held`, in their order.
    fn less(&self, held: &[&[String]]) -> Vec<String> {
        self.standing
            .iter()
            .filter(|n| !held.iter().any(|h| h.contains(n)))
            .cloned()
            .collect()
    }

    /// THE NEEDS ON THE MENU BAR (§15.6): those standing less the lines the owner's standing
    /// go-ahead answers or is answering.
    pub fn shown(&self) -> Vec<String> {
        self.less(&[self.answered.as_slice(), self.answering.as_slice()])
    }

    /// The key of [`Self::shown`] ([`needs::needs_key`]; `0` for none).
    pub fn shown_key(&self) -> u64 {
        needs::needs_key(&self.shown())
    }

    /// A JUDGMENT OF THE REPLY LAST READ, where it names needs ([`Self::named`]): of the lines
    /// answered before, only those its own park answers (`parks`) stay off the menu bar.
    fn keep_answered(&mut self, parks: &[String]) {
        if self.named {
            self.answered.retain(|line| parks.contains(line));
        }
    }

    /// The lines being answered, and `past`, are answered: off the menu bar for good, until a
    /// later reply names them under another decision.
    fn answer(&mut self, past: &[String]) {
        let answering = std::mem::take(&mut self.answering);
        for line in answering.into_iter().chain(past.iter().cloned()) {
            if !self.answered.contains(&line) {
                self.answered.push(line);
            }
        }
        self.named = false;
    }

    /// Fold the needs a report `tail` names (aterm's `fold_needs`): its NEEDS YOU lines
    /// replace the set; a `Nothing left:` defense replaces it with its own lines (none clears
    /// it); `NEEDS YOU: nothing` clears it. A report that says none of these leaves it, and
    /// answers the lines a go-ahead decided but did not type yet as its submission would. A line
    /// the go-ahead answered stays answered only while a reply names it.
    pub fn fold(&mut self, tail: Option<&str>) {
        let Some(t) = tail else {
            return;
        };
        let named = needs::needs_you(t);
        if named.is_empty() && needs::defense_line(t).is_none() && !needs::says_no_needs(t) {
            self.answer(&[]);
            return;
        }
        let key = needs::needs_key(&named);
        if key == 0 {
            *self = Needs::default();
            return;
        }
        if key != self.key {
            self.answered.retain(|line| named.contains(line));
            self.answering.retain(|line| named.contains(line));
            self.standing = named;
            self.key = key;
        }
        self.named = true;
    }
}

/// The repository probe for one point (`repo::probe`): `point` the turn end's instant
/// (`None`: the carry-on's point), `found` the complete reading, including unread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoCheck {
    pub point: Option<Instant>,
    pub found: RepoRead,
}

/// A LANDING JUDGED AGAIN OVER THE REPOSITORY before anything is decided over it (aterm main
/// e0da8d68df's rule, from 667c926ec4: A PERSON'S ACT re-arms Land over the repository as it
/// is; NOBODY'S ACT keeps the landing). The landing stands meanwhile: nothing is typed, the
/// decider asks for the probe ([`Act::Probe`]), and the live upgrade waits for the judgement
/// ([`ThreadState::judging`]); the probe's finding settles it ([`ThreadState::probed_for`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rejudge {
    /// AN OFF STOOD OVER THE LANDING (a person's act, aterm's `off_landing`): the person had
    /// the session in hand, so it holds only over a clean repository; over work found, the
    /// task is open again and Land's instruction comes (the Off ended the episode).
    Off,
    /// THE LANDED TASK'S OWN WAKE ENDED (nobody's turn: no `UserMessage` opened it, neither a
    /// person's nor codex-alab's; a goal's continuation): it keeps the landing where its words
    /// left nothing open (`open` false, [`wake_left_open`]) and the repository is clean, or
    /// holds the very work it held as the wake began (`began`, the start probe's finding,
    /// [`ThreadState::wake_began_over`]; `None` where no start probe ran; compared by
    /// `RepoRead::same`), with no Off since ([`ThreadState::off_landing`]): someone else's
    /// work, none of the wake's. Else the landing is over and Land's nudge is owed, naming
    /// the repository's work unless the wake began over it.
    Wake { open: bool, began: Option<RepoRead> },
}

/// An act the previous image of this process owed when it re-executed onto a new build (the
/// live upgrade under FOREVER CRUISE: a paced act no longer holds it), carried in its session
/// record: THE OLD IMAGE'S DECISION, typed by this image exactly as decided, under its rule,
/// at `not_before`. Never re-derived from the resumed reply alone: the old image read what
/// the reply cannot say again (a question's flag, whose turn it was, a land act's yield), so
/// an irreversible question's reversible answer stays that answer, and a nudge stays a nudge
/// (review of 2a9241bbf, findings 1, 8 and 16).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Owed {
    /// Native point the old decision observed, independent of the turn it would type.
    pub native_origin: Option<codex_alab_core::session::NativeOrigin>,
    /// The rule the old image would have typed under.
    pub rule: &'static str,
    /// The words it would have typed. A done claim's act that waited before the repository
    /// was read (Cruise's defend prompt, Land's nudge: [`Owed::repo_owed`]) is decided without
    /// the repository's suffix, which is read when the act goes, as a paced act's probe is
    /// ([`decide`]), and placed then ([`owed_with_repo`]).
    pub text: String,
    /// The mode it was decided under: a mode read since that differs makes it none, and the
    /// resumed reply is decided again under the new one ([`ThreadState::observe_mode`]).
    pub mode: Mode,
    /// Not before then.
    pub not_before: Instant,
    /// THE REPOSITORY'S SUFFIX OWED ([`Decision::repo_owed`]): the open work the done claim
    /// named, where the act was decided before the repository was read; this image probes
    /// once it is due and fits the words again with what the repository holds, exactly as
    /// the old image would have then ([`owed_with_repo`]). `None`: decided whole and typed
    /// as decided (a nudge over a finding already read, which leaves out work a landed
    /// task's wake began over: this image no longer knows whose that work is).
    pub repo_owed: Option<Vec<String>>,
    /// THE USAGE WINDOW'S RESUME ([`Act::ResumeLimit`]): a usage-limited goal is resumed
    /// through `GoalService` first, else the words are typed. `false` for every other act,
    /// a quota wall's try ([`words::RULE_LIMIT_RESUME`] typed as a turn) included.
    pub resume_goal: bool,
    /// A WALL'S TRY THAT DELIVERS A LAND ACT AGAIN ([`ThreadState::landing`] as the try was
    /// decided): the act the wall cut was one (the instruction, a nudge, a land answer, or a
    /// wall's try of one), so the episode had its instruction and the try's yield is a land
    /// act's, whose clean defense lands the task as the image that decided it would have
    /// judged. `false` for every other act, and in a record from a build before it.
    pub landing: bool,
}

impl Owed {
    /// The act the record hands on, as it is typed now ([`Act::ResumeLimit`] for the usage
    /// window's resume), with `text` its words.
    fn act(&self, text: String) -> Act {
        if self.resume_goal {
            Act::ResumeLimit { text }
        } else {
            Act::Type {
                rule: self.rule,
                text,
            }
        }
    }
}

/// Where [`ThreadState::checks`] counts round (aterm's `CHECKS_WRAP`): a whole number of turns of
/// aterm's six angles, after which the count is [`CHECKS_ROUND`], never 0, so the angles go on in
/// order across it.
const CHECKS_WRAP: u8 = 252;
/// One whole turn of aterm's six angles (aterm's `CHECKS_ROUND`).
const CHECKS_ROUND: u8 = 6;
const _: () = assert!(CHECKS_WRAP.is_multiple_of(CHECKS_ROUND));

/// The count once one more done check is typed after `checks` (aterm's `next_checks`).
const fn next_checks(checks: u8) -> u8 {
    match checks.checked_add(1) {
        Some(n) if n < CHECKS_WRAP => n,
        _ => CHECKS_ROUND,
    }
}

/// THE GO-AHEAD'S COUNT (aterm's `GoAheads`; owner, 2026-10-29): the [`go_ahead::RULE_GO_AHEAD`]
/// acts typed on the standing park, and when the first went since the agent last moved past it.
/// A reply that parks no more ([`ThreadState::go_ahead_read`]) and a new key of the person's
/// ([`ThreadState::person_keyed`]) start it again; real work or a wall restarts the span
/// ([`ThreadState::go_ahead_worked`]); a sleep restarts the span too.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GoAheads {
    /// The go-aheads typed on the standing park.
    pub acts: u32,
    /// When the first of them went since the agent last moved past its park.
    pub first: Option<Instant>,
}

/// One thread's memory (the runtime keeps one per root thread, in its thread store).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThreadState {
    /// A person's turn exists (no act until one does: the taskless rule).
    pub task: bool,
    /// The bounded independent-work continuation was submitted in this person's task
    /// sequence. Native attributed history restores it across a restart; successful work,
    /// mode changes and back-off never turn a refusal into repeated retry authority.
    pub policy_continued: bool,
    /// LAND LANDED: a clean defense at a land act's yield with the repository clean. Nothing
    /// is typed until a person's act: their turn, or an Off and back over work found
    /// ([`Rejudge::Off`]); the task's own wake keeps it unless the wake left work open
    /// ([`Rejudge::Wake`]). A switch to Cruise goes on. Never set under Cruise.
    pub landed: bool,
    /// AN OFF STOOD SINCE THE LANDING ([`ThreadState::park_off`], aterm's `off_landing`): the
    /// person had the session in hand, so a wake's start forgives nothing until the landing
    /// is judged again over the repository as it is.
    pub off_landing: bool,
    /// The landing's judgement owed ([`Rejudge`]): `Some` only while [`Self::landed`].
    pub rejudge: Option<Rejudge>,
    /// THE LANDED TASK'S OWN WAKE KEPT THE LANDING, AND ATERM REOPENED IT: aterm reopens a
    /// proven self-claim's landing at any turn begun past its mark (its
    /// `reopen_self_claim_landing`), the wake's included, so its face and task record read open
    /// until codex-alab records `landed@v1` again ([`Act::Landed`] at the point; amended
    /// 2026-10-29 after the review of gap 12, finding 1: the line wore `✓ landed` again while
    /// aterm said open for good). Set where the wake's judgement keeps the landing
    /// ([`ThreadState::judge_landing`]); taken by that record and by anything that ends it.
    pub reland: bool,
    /// What the repository held as the turn in hand began over a landing
    /// ([`ThreadState::wake_began_over`]): `Some(Read(None))` completed with no finding,
    /// `Some(Unread)` did not complete, `None` means no start probe ran.
    /// Taken by that turn's end.
    pub began: Option<RepoRead>,
    /// The work a landing was judged undone over at the point in hand: Land's nudge there
    /// names it (aterm's `unlanded_by_repo`). Never work the wake began over.
    pub reopened: RepoRead,
    /// The Land episode in hand.
    pub land: LandEpisode,
    /// The last act was a land act typed once the instruction was, or a wall act (or the lost
    /// login's line) that delivers one again: its yield's clean defense lands the task.
    pub landing: bool,
    /// The needs standing.
    pub needs: Needs,
    /// The repository probe for the current point, once run.
    pub repo: Option<RepoCheck>,
    /// The mode folded last ([`ThreadState::observe_mode`]).
    pub mode: Option<Mode>,
    /// The last reply a resumed thread's history holds: what the carry-on judges.
    pub resumed_reply: Option<String>,
    /// The act the previous image owed at its re-exec ([`Owed`]).
    pub owed: Option<Owed>,
    /// Short turns of codex-alab's continuations in a row.
    pub short_streak: u32,
    /// RULING F'S REPLIES (aterm's `TurnEndState::replies`): the no-work answers to acts the
    /// short streak keeps ([`Replies`]); [`ThreadState::repeats`] holds every turn-end act at
    /// the same reply again or at a run of them. Carried across a re-exec in the record's pace
    /// ([`ThreadState::seed_replies`]), never across aterm's relaunch.
    pub replies: Replies,
    /// When the newest answer counted came (aterm's `replies_at`).
    pub replies_at: Option<Instant>,
    /// The dated reset that answer showed (aterm's `replies_reset`): once it is past, the next
    /// answer finds the wall's replies answerable again ([`ThreadState::observe_turn_end`]),
    /// and while the wall's tries are spent a reset the point names that is not this one lifts
    /// the hold ([`ThreadState::repeats`]); kept by an Off.
    pub replies_reset: Option<Instant>,
    /// THE POINT THE IMAGE BEFORE A RE-EXEC HELD (its record's pace,
    /// [`ThreadState::seed_held_point`]): the resume stands at it again
    /// ([`ThreadState::observe_held`]), so an Off and back types there.
    pub held_seed: Option<TurnEnd>,
    /// THE POINT THE IMAGE BEFORE A RE-EXEC LEFT TO THE PERSON (its record's pace,
    /// [`ThreadState::seed_left_point`]), and whether its exchange opened the session: the
    /// resume stands at it again, still theirs ([`ThreadState::observe_held`]; edges-5).
    pub left_seed: Option<(TurnEnd, bool, bool)>,
    /// THE ACT AWAITING ITS ANSWER (aterm's `awaiting`): set by each act typed
    /// ([`ThreadState::acted`]; `Some(own)`, `own` where the act is a wall's own try or the lost
    /// login's line, aterm's `is_wall_retry`), never by a command (a compaction), and taken by
    /// the next turn that ends, which is its answer.
    pub awaited: Option<bool>,
    /// HOW MANY DONE CHECKS THIS CRUISE RUN TYPED (aterm's `TurnEndState::checks`): the next
    /// asks the angle after them ([`Texts::done_check_words`]). Carried across a re-exec or a
    /// relaunch in the record's pace ([`ThreadState::seed_checks`]); back to 0 at a person's
    /// turn, at an Off and on leaving Cruise, so the next run's first claim asks the first.
    pub checks: u8,
    /// When each continuation-type act was typed (for `continue_per_hour`).
    pub typed: VecDeque<Instant>,
    /// The last turn end not yet answered.
    pub point: Option<TurnEnd>,
    /// Native identity of that exact point, assigned under the same state lock.
    pub point_turn_id: Option<String>,
    /// A PERSON'S ESC STOPPED THE POINT'S TURN, at this instant (aterm's `esc_at`, read by its
    /// `person_hold`): the point is held for `human_grace` from it, then decided as a turn end
    /// the agent did not end ([`decide`]; owner 2026-10-27: "you need to keep working even if
    /// the user interrupted a long running job"). `None` at every other point, an abort no
    /// person's Esc made included (a replacement, an ended review, a goal's budget, an
    /// interrupted point a resumed history or a recovery restores): those still leave the
    /// task to a later turn. Set by [`ThreadState::observe_esc`] after the point's end, cleared
    /// by the next one. A RE-EXEC AND aterm's RELAUNCH TAKE IT UP (the record's pace,
    /// [`Self::seed_esc`]): the resume's restored abort of the same turn holds as this point did.
    pub esc_at: Option<Instant>,
    /// A PERSON'S ESC THE PROCESS BEFORE THIS ONE SAW STOP ITS POINT'S TURN (its record's pace:
    /// the image before a re-exec, or the process aterm relaunched; [`Self::seed_esc`]): when,
    /// and the native turn it stopped. The resume's restored abort of that very turn takes it as
    /// its [`Self::esc_at`] (`resume_history::restore_terminal`), where the interrupt is then
    /// the person's and never a teardown's; any other point drops it (the forever-persist lane,
    /// item a: the image after stood at an abort no Esc had made, parked for good, and a
    /// relaunch after a signal read the person's interrupt as its own teardown and typed the
    /// carry-on at once).
    pub esc_seed: Option<(Instant, String)>,
    /// Latest native lifecycle point known when a handoff decision is captured.
    pub native_origin: Option<codex_alab_core::session::NativeOrigin>,
    pub wall: Option<WallTrack>,
    /// A restored wait's original absolute deadline. Re-deciding may extend it, never
    /// shorten it; it applies only to provider retry and native usage-resume acts.
    pub recovery_not_before: Option<Instant>,
    /// The provider's in-turn wait from a witnessed busy crash. This only delays an
    /// independently authorized carry-on; it never creates a failed turn or an act.
    pub carry_on_not_before: Option<Instant>,
    /// `Op::Compact` was submitted; its idle is not a turn.
    pub compacting: bool,
    /// The compaction ended: continue.
    pub after_compact: bool,
    /// THE TEXT OF THE DEFEND PROMPT OR THE LAND ACT TYPED LAST (aterm's `cut_text`), kept
    /// while a wall's act delivers it again (a retry, a compaction, the lost login's line): a
    /// full context that cut it is owed it again, verbatim, as the compaction's continuation
    /// (aterm's `compact_owes`, `compact_owed_text`: the summary may drop it), cut as the
    /// person's hour has it then: kept whole where the hour cut [`go_ahead::NEVER_A_NEED`] out
    /// of the act ([`Decision::whole`], [`ThreadState::acted_on`]; aterm's `kept_words`). `None`
    /// after any other act, and once a turn ends with no wall or is not codex-alab's.
    pub cut: Option<String>,
    /// A resume by aterm's crash relaunch of a session that died working, with a task: the
    /// mode's act at the first verified idle (never after codex-alab's own re-exec, which
    /// carries only what it owed: [`ThreadState::owed`]).
    pub carry_on: bool,
    /// The rule of the last act typed (its yield's turn is judged by it).
    pub last_rule: Option<&'static str>,
    /// CODEX-ALAB IS CARRYING THE TASK (aterm's `carrying`): an act of its own was answered
    /// since the task began, and nobody stopped it. A person's turn meanwhile, however short,
    /// is a check-in, not an exchange of their own, unless the worker had said done. Ended by
    /// someone else's turn on a task it was not carrying, an interrupt of its act, a landing
    /// and an Off ([`ThreadState::park_off`]).
    pub carrying: bool,
    /// The point in hand ends A PERSON'S OWN EXCHANGE, or one stands at a wall whose retry
    /// delivers it again (aterm's `exchange`): their message, opened by their hand, on a task
    /// codex-alab was not carrying, answered within [`EXCHANGE_WORK`], where a message can be
    /// theirs at all (under Land only the session's opening one, under Off any, under Cruise
    /// only an opening without tools or a question box: [`Self::ended_under`], gap 32).
    pub exchange: bool,
    /// The exchange's work so far: their turn's, plus each wall act's that delivered it again.
    pub exchange_work: Duration,
    /// Tool work anywhere in the opening exchange, including a wall retry under Land.
    pub exchange_ran_tools: bool,
    /// A question box anywhere in that exchange; a later plain retry cannot erase it.
    pub exchange_questions: bool,
    /// THE EXCHANGE OPENED THE SESSION (aterm's `exchange_opens`): no turn had ended here before
    /// the person's turn that made it ([`Self::past_opening`]). Land keeps only such an
    /// exchange theirs; one an Off left past the opening is Land's to carry once the word moves
    /// to Land ([`Self::observe_mode_under`]; edges-4).
    pub exchange_opens: bool,
    /// THE POINT IS LEFT TO THE PERSON (aterm's `left_to_person`): their own exchange ended
    /// with no wall (Land's opening one, or one an Off left: [`Self::ended_under`], gap 32;
    /// Cruise preserves taskless openings, [`Self::observe_mode`]). Nothing is typed until the next turn
    /// ends, whatever the reply says.
    pub left: bool,
    /// THE SESSION IS PAST ITS OPENING (aterm 791977cae8's `first_point`, `point_seed` and
    /// `worked_before` in one; amended 2026-10-28 after the parity sweep, gap 32, and its
    /// review): a turn ended here, in this process (set as it ends, its own exchange judged by
    /// what it said before: [`Self::observe_turn_end`]; a late completion's end too), or the
    /// books show a turn ended or an act typed before this process ([`Self::seed_worked`]: the
    /// resumed thread's history, the record's pace), or the runtime seeds it failing toward
    /// carrying (`extension::Autopilot::seed_worked`): a fork (its history a conversation's
    /// that ran), a resumed thread whose history the resume scan did not read before a turn here
    /// (a turn beat the scan, or the history was unread), and a thread a vanilla spell covered
    /// (`/alab off` ... `/alab on`: turns ran that no hook saw, [`Self::unwatched`]), and a thread
    /// that carries on another's work, which the TUI names as it starts one (plan mode's "clear
    /// context and implement", which submits the plan as the thread starts, and a provider
    /// error's continuation: `Shared::continue_work`). A NEW CHAT (`/new`, `/clear`) is at its
    /// own opening, whatever this process did before, as aterm's `opened_fresh` opens the next
    /// exchange's session at a taskless screen (edges-3). Never cleared: a person's
    /// turn, an Off, a landing and a mode leave it. Under Land a person's own exchange is theirs
    /// only before it ([`Self::observe_exchange`]).
    pub past_opening: bool,
    /// THE AUTOPILOT WORD THE TURN ENDING NEXT ENDS UNDER (gap 32), set by the runtime just
    /// before each live turn's end, never carried in a record: the live policy's word (a
    /// person's hold over a held claim keeps it); in a passive spell that is no Off (a standby,
    /// a claim not yet proven or not held) Land's, whatever was folded last, since Land's rule
    /// is the one that may leave the exchange; Cruise preserves a taskless opening
    /// ([`Self::observe_mode`]); `None` under Off, as aterm's `Autopilot::Off`.
    /// [`Self::observe_exchange`] reads the supervisor's word as the turn ends: Cruise
    /// preserves a taskless opening, Land any brief opening, Off any exchange.
    pub ended_under: Option<Mode>,
    /// The point in hand is a done claim (aterm's `done == 1`): a person's turn after it is
    /// carried by nobody.
    pub claimed: bool,
    /// When codex-alab last typed an act (a keystroke before it opened nothing after it).
    pub last_act: Option<Instant>,
    /// THE API'S REACH as measured when this point was decided ([`crate::netwatch`]): set by
    /// the runtime before each decision of a point at a network wall whose route the probe
    /// reproduces, or at a usage wall whose due resume asked for it ([`Self::net_asked`]),
    /// else [`Reach::Unknown`]. Only those walls' acts read it.
    pub reach: Reach,
    /// THE LOST LOGIN as the runtime read it when this point was decided ([`LoginRead`]): set
    /// before each decision of a point at a login wall, else the default, never carried in a
    /// record. Only the login wall's act reads it.
    pub login: LoginRead,
    /// THE LIVE UPGRADE'S NOTICE DUE NOW, its words (`upgrade_ask`, [`crate::upgrade_ask`];
    /// amended 2026-10-28 after the parity sweep, gap 27): set by the runtime before each
    /// decision of the root thread on screen while the round says a notice is due, else `None`,
    /// never carried in a record. It goes first at a clean point ([`upgrade_notice_at`]), as
    /// aterm's host types its notice at an idle point before its loop's act.
    pub upgrade_notice: Option<String>,
    /// A LEAD WAITS ON THIS SESSION (aterm's `TurnEndState::managed`: a task record stands
    /// that is not `exited`), as `@self task` said it when this point was decided: set by the
    /// runtime before each decision (`Supervisor::managed`), never carried in a record. Its
    /// first short done claims and questions wait no back-off ([`waited_on`]), and Land's hold
    /// is told quietly ([`hold_told`]'s caller).
    pub managed: bool,
    /// WHEN THE POINT'S DUE USAGE RESUME FIRST ASKED FOR THE API'S REACH (2026-10-24): the
    /// runtime asks once the resume is due and its route is one the probe reproduces, and from
    /// then on measures it while the point stands ([`crate::netwatch::Waiter`]). A resume is
    /// never typed into an API measured down, nor before its first measure has had
    /// [`Timing::probe_wait`] ([`hold_for_reach`]). Cleared by the point's end and by a sleep:
    /// the network a wake brings is measured again.
    pub net_asked: Option<Instant>,
    /// THE HOST'S LAST WAKE ([`codex_alab_core::wake`]), as the runtime read it before this
    /// decision: an act goes no sooner than [`Timing::wake_settle`] after it
    /// ([`settle_after_wake`]).
    pub woke: Option<Instant>,
    /// The host's time asleep since boot when the point's absolute deadlines were last counted
    /// ([`ThreadState::slept_to`]). Live finishes and restored deadlines seed it;
    /// an unseeded state takes its first decision as the baseline.
    pub asleep_mark: Option<Duration>,
    /// A PERSON'S LAST WINDOW GESTURE as their message arrived (aterm's `status human_ms=`,
    /// which a control write never stamps; [`ThreadState::observe_person_hand`]): what says
    /// their hand opened the turn ending next. Taken by that turn's end.
    pub hand_at: Option<Instant>,
    /// THE GO-AHEAD'S COUNT on the standing park ([`GoAheads`]).
    pub go_aheads: GoAheads,
    /// Where the person's last key holds the go-ahead from ([`Self::person_keyed`]): for
    /// [`GO_AHEAD_HELD`] after it no go-ahead answers the replies, which may agree to or
    /// restate a hold the person gave.
    pub go_ahead_person: Option<Instant>,
    /// The newest key of the person's read that held the go-ahead ([`Self::person_keyed`]).
    pub person_key: Option<Instant>,
    /// What the point in hand's turn came after: the point before it or codex-alab's last act
    /// before its end, whichever was later. A key of the person's after it, within the turn's
    /// work and [`PERSON_TURN_SLACK`], came in that turn ([`Self::person_keyed`]).
    pub turn_since: Option<Instant>,
}

/// THE LOST LOGIN AS THE RUNTIME READ IT through codex's own `AuthManager` ([`crate::login`];
/// amended 2026-10-28 after the parity sweep, gap 13 stage 1: a codex-alab tab stayed idle after
/// its login was renewed elsewhere), as [`ThreadState::reach`] is read for a network wall.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LoginRead {
    /// THE LOGIN IS BACK: codex holds a credential newer than the one the wall refused (its
    /// change generation past the wall's), usable (cached, with no refresh refused for it) and
    /// of the walled identity (its auth mode, and for ChatGPT its user and account). Another
    /// identity's login is not back.
    pub back: bool,
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
    /// Land landed ([`words::RULE_LANDED`]): recorded, never typed.
    Landed,
    /// Run the repository probe for this point (`repo::probe`), then decide again.
    Probe,
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
            Act::Landed => words::RULE_LANDED,
            Act::Probe => "probe",
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
    /// A wait a person should see ("waiting for the usage reset at 14:05"): an `appnotice`, and
    /// aterm's Waiting rows under `owner=codex-alab-wait kind=wait` once its timer is armed
    /// (amended 2026-10-27 after the parity sweep, gap 2).
    pub shown_wait: Option<String>,
    /// A DONE CLAIM'S ACT THAT WAITS DECIDED BEFORE THE REPOSITORY WAS READ (Cruise's defend
    /// prompt, Land's nudge at a claim not yet probed): the open work the claim named, which
    /// the live upgrade carries with the act ([`Owed::repo_owed`]) so the next image reads the
    /// repository once it is due and places what it holds as this image would have. `None`
    /// for every other decision.
    pub repo_owed: Option<Vec<String>>,
    /// THE ACT THE RECORD CARRIES IN THIS ONE'S PLACE while it waits (`owes_of`): the plain
    /// retry of the same wall for the try typed on the API's MEASURED RETURN
    /// ([`words::RULE_API_BACK`]), whose words claim a measure no relaunch has taken, so a
    /// before-every-act check that holds it back (a person's hold, a draft, a popup) hands
    /// aterm's relaunch after a crash at rest the wall's try, decided afresh at its look.
    /// `None`: the act itself, where its wait is carried at all ([`carried_rule`]).
    pub in_record: Option<Act>,
    /// RULING F HOLDS THE POINT ([`ThreadState::repeats`]): nothing is typed, and the arm
    /// that holds it is told once, quietly ([`crate::replies::stop_line`]). `None` for every
    /// other decision.
    pub held: Option<HeldBy>,
    /// THE POINT IS DECIDED AGAIN THEN, though nothing is typed now (aterm's `GO_AHEAD_HOUR`
    /// wait): the end of the person's hour that holds the point's park on a go-ahead
    /// ([`ThreadState::go_ahead_hour_ends`]), where the decision types nothing, so the parked
    /// agent, which ends no other turn, gets the go-ahead there. `None` for every other decision.
    pub look_again: Option<Instant>,
    /// THE GO-AHEAD SPENT ([`crate::go_ahead::GO_AHEAD_REFUSED`]): the agent still parks on the
    /// go-ahead its limit of acts gave, so nothing is typed and the person is told now
    /// ([`crate::replies::Told::GoAhead`]); the lines it answered go back on the menu bar.
    pub refused: bool,
    /// THE ACT'S WORDS WHOLE, where the person's hour cut [`go_ahead::NEVER_A_NEED`] out of what
    /// it types (a done check within their hour): what [`ThreadState::cut`] keeps for a full
    /// context's retype, which cuts it as the hour has it then (aterm's `kept_words`), so a check
    /// typed within the hour and again past it says the sentence. `None` for every other
    /// decision.
    pub whole: Option<String>,
    /// THE POINT'S OWN DECISION A HOST'S STEP WAS TYPED IN FRONT OF (the live upgrade's notice,
    /// [`upgrade_notice_at`]): the go-ahead's verdict judges it ([`ThreadState::go_ahead_verdict`])
    /// and its refusal is told ([`Decision::refuses`]), as aterm judges a point's decision before
    /// a host's step and keeps it through the step (§15.6, its review 9), so the lines a go-ahead
    /// answers stay off the menu bar through the notice's turn. `None` for every other decision.
    pub behind: Option<Box<Decision>>,
}

impl Decision {
    fn nothing(why: impl Into<String>) -> Decision {
        Decision {
            act: Act::Nothing,
            not_before: None,
            why: why.into(),
            shown_wait: None,
            repo_owed: None,
            in_record: None,
            held: None,
            look_again: None,
            refused: false,
            whole: None,
            behind: None,
        }
    }

    /// Ruling F holds the point by `arm` (aterm's `SAME_REPLY` step's `Nothing`).
    fn held(arm: HeldBy) -> Decision {
        Decision {
            held: Some(arm),
            ..Decision::nothing(format!(
                "{}: {}",
                crate::replies::SAME_REPLY,
                crate::replies::stop_line(arm)
            ))
        }
    }

    fn now(act: Act, why: impl Into<String>) -> Decision {
        Decision {
            act,
            not_before: None,
            why: why.into(),
            shown_wait: None,
            repo_owed: None,
            in_record: None,
            held: None,
            look_again: None,
            refused: false,
            whole: None,
            behind: None,
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
            repo_owed: None,
            in_record: None,
            held: None,
            look_again: None,
            refused: false,
            whole: None,
            behind: None,
        }
    }

    /// Whether the go-ahead is refused at this point ([`Decision::refused`]), behind a host's
    /// step too ([`Decision::behind`]): the person is told now.
    pub fn refuses(&self) -> bool {
        self.refused || self.behind.as_deref().is_some_and(Decision::refuses)
    }

    /// Whether the act waits past `now`.
    pub(crate) fn waits(&self, now: Instant) -> bool {
        self.not_before.is_some_and(|at| at > now)
    }

    /// THE GO-AHEAD SPENT ([`go_ahead::GO_AHEAD_REFUSED`], [`ThreadState::go_ahead_spent`]):
    /// nothing is typed, and the person is told now ([`crate::replies::Told::GoAhead`]).
    fn refused() -> Decision {
        Decision {
            refused: true,
            ..Decision::nothing(format!(
                "{}: the agent still parks on the go-ahead its limit of acts gave",
                go_ahead::GO_AHEAD_REFUSED
            ))
        }
    }

    /// THE PERSON'S HOUR HOLDS THE POINT ([`go_ahead::GO_AHEAD_HOUR`]): nothing now, the point
    /// decided again at `ends`.
    fn hour(ends: Instant) -> Decision {
        Decision {
            look_again: Some(ends),
            ..Decision::nothing(format!(
                "{}: {}",
                go_ahead::GO_AHEAD_HOUR,
                go_ahead::GO_AHEAD_HOUR_WHY
            ))
        }
    }
}

/// Whether `rule` is a wall act (counted as the wall's attempts, outside the per-hour cap
/// and the short streak), as aterm's `is_wall_act`.
pub fn is_wall_rule(rule: &str) -> bool {
    matches!(
        rule,
        words::RULE_API_RETRY
            | words::RULE_API_BACK
            | words::RULE_API_CUTOFF
            | words::RULE_LIMIT_RESUME
            | words::RULE_COMPACT
    )
}

/// Whether `rule` delivers again the act a wall cut, its answer judged as a wall try's (aterm's
/// `is_wall_retry`): every wall act, and the lost login's line ([`words::RULE_LOGIN_BACK`]),
/// which is no try past its wall: it counts in the hour's budget, never toward the wall's tries.
fn is_wall_retry(rule: &str) -> bool {
    is_wall_rule(rule) || rule == words::RULE_LOGIN_BACK
}

/// Whether `rule`'s yield counts toward the short streak (aterm's `is_continue_rule`): the
/// continuation, the answers, the defend prompt and Land's three acts.
fn is_continue_rule(rule: &str) -> bool {
    matches!(
        rule,
        words::RULE_CONTINUE
            | go_ahead::RULE_GO_AHEAD
            | words::RULE_POLICY_CONTINUE
            | words::RULE_ANSWER
            | words::RULE_PROSE_LONG_TERM
            | words::RULE_PROSE_SHORT_TERM
            | words::RULE_DONE_CHECK
    ) || words::is_land_rule(rule)
}

/// The paced act's rule named `name` (a continuation-type act, or the carry-on), whose wait
/// the live upgrade carries across its re-exec instead of waiting on it; `None` for any other
/// (a wall's act, an unknown word from a record).
pub fn paced_rule(name: &str) -> Option<&'static str> {
    [
        words::RULE_CONTINUE,
        go_ahead::RULE_GO_AHEAD,
        words::RULE_POLICY_CONTINUE,
        words::RULE_ANSWER,
        words::RULE_PROSE_LONG_TERM,
        words::RULE_PROSE_SHORT_TERM,
        words::RULE_DONE_CHECK,
        words::RULE_LAND,
        words::RULE_LAND_NUDGE,
        words::RULE_LAND_ANSWER,
        words::RULE_HOST_TURN,
    ]
    .into_iter()
    .find(|rule| *rule == name)
}

/// The rule named `name` of an act whose wait the record carries ([`Owed`]): a paced act's
/// ([`paced_rule`]), or A WALL'S TRY (a server's, a network's or a cut-off's retry, a usage
/// window's resume, a quota's try), whose wait is rest too: a crash there left the relaunch
/// typing nothing until a person's turn, where aterm's loop types the wall's next try at its
/// rung from the ladder its books carry (`seed_retry`, `seed_usage_attempt`). `None` for any
/// other: a compaction goes at once, and a landing or an unknown word from a record is no act.
pub fn carried_rule(name: &str) -> Option<&'static str> {
    paced_rule(name).or_else(|| {
        [
            words::RULE_API_RETRY,
            words::RULE_API_CUTOFF,
            words::RULE_LIMIT_RESUME,
        ]
        .into_iter()
        .find(|rule| *rule == name)
    })
}

/// Whether an act typed under `rule` carries the needs' ack where one is owed ([`decide`]
/// appends it to exactly these): every act of a free point and the carry-on; never a wall's
/// retry. Under Land the compaction's continuation and a usage resume before Land's
/// instruction carry it too (they type Land's words, aterm's `land_words`); under Cruise the
/// compaction's continuation is the continuation alone (aterm's `continuation_text`), and a
/// usage resume's one cleared line ([`resume_text`]) carries none, so the ack stays owed. The
/// owner's standing go-ahead carries it for the needs it leaves on the menu bar
/// ([`Needs::ack_owed_past`]).
fn carries_ack(rule: &str) -> bool {
    matches!(
        rule,
        words::RULE_CONTINUE
            | go_ahead::RULE_GO_AHEAD
            | words::RULE_ANSWER
            | words::RULE_PROSE_LONG_TERM
            | words::RULE_PROSE_SHORT_TERM
            | words::RULE_DONE_CHECK
            | words::RULE_HOST_TURN
    ) || words::is_land_rule(rule)
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
    /// THE HOST SLEPT (2026-10-24): `asleep`, the host's time asleep since boot as read now
    /// ([`codex_alab_core::wake::Reading::asleep`]). The point's deadlines that name a moment
    /// of real time (a usage window's reset, a server's Retry-After, a restored wait's
    /// deadline, a crash's provider wait) were set on the `Instant` clock, which stops while
    /// the host sleeps; each moves back by the time slept since they were last counted, so a
    /// reset the host slept through is due at the wake, never as many hours later as the host
    /// slept. A deadline the clock cannot move back that far is `now`. The first reading of a
    /// point only marks it; a sleep also clears [`Self::net_asked`] (the wake's network is
    /// measured again). A growth shorter than [`codex_alab_core::wake::MIN_SLEEP`] is the two
    /// clocks read a moment apart, and moves nothing. Returns the time slept that moved them.
    pub fn slept_to(&mut self, asleep: Option<Duration>, now: Instant) -> Option<Duration> {
        let asleep = asleep?;
        let mark = self.asleep_mark.replace(asleep)?;
        let slept = asleep
            .checked_sub(mark)
            .filter(|slept| *slept >= codex_alab_core::wake::MIN_SLEEP)?;
        let back = |at: Instant| at.checked_sub(slept).unwrap_or(now.min(at));
        if let Some(point) = self.point.as_mut() {
            match point.wall.as_mut() {
                Some(Wall::Usage {
                    reset_at: Some(reset),
                    ..
                }) => *reset = back(*reset),
                Some(Wall::Server {
                    retry_at: Some(retry),
                    ..
                }) => *retry = back(*retry),
                _ => {}
            }
        }
        if let Some(owed) = self.owed.as_mut() {
            owed.not_before = back(owed.not_before);
        }
        // THE RESET THE LAST ANSWER SHOWED MOVES AS THE POINT'S DOES (`back`, the same time
        // slept), so [`Self::repeats`] reads one reset as one reset whether or not the host
        // slept through it (`same_reset`; review of 4c0168cedd, finding 6: floored at `now`, a
        // reset slept through read as another and lifted a spent wall's hold that the same
        // reset passing awake keeps). It stays after the answer that showed it: `replies_at`,
        // a moment before the sleep, does not move, and a reset slept through still came
        // after that answer ([`Self::count_reply`]'s reset block).
        if let Some(reset) = self.replies_reset.as_mut() {
            let moved = back(*reset);
            *reset = match self.replies_at {
                Some(counted) if counted < *reset => {
                    moved.max(later(counted, Duration::from_nanos(1)))
                }
                _ => moved,
            };
        }
        self.recovery_not_before = self.recovery_not_before.map(back);
        self.carry_on_not_before = self.carry_on_not_before.map(back);
        // THE PERSON'S HOUR COUNTS THE SLEEP (aterm's review 9: the window's typing age counts
        // it), and the go-ahead's span starts again at the wake.
        self.go_ahead_person = self.go_ahead_person.map(back);
        self.person_key = self.person_key.map(back);
        self.go_aheads.first = None;
        self.net_asked = None;
        Some(slept)
    }

    /// A person's turn started (a `UserMessage` that is not codex-alab's): the task opens,
    /// and the Land episode and a landing end (a Land chosen after starts a new episode). The
    /// needs stay up while it runs: its end answers them, where it met no wall
    /// ([`ThreadState::observe_turn_end`], aterm's point). The short streak stands (aterm's `TurnEndState`):
    /// the turn's end moves it as anyone else's turn does ([`ThreadState::observe_turn_end`]),
    /// so only real work ends it.
    pub fn observe_person_turn(&mut self) {
        // Someone's turn starts the done checks' angles again (aterm's `checks`); the
        // worker's own work between its claims does not.
        self.checks = 0;
        self.policy_continued = false;
        self.recovery_not_before = None;
        self.carry_on_not_before = None;
        self.task = true;
        self.unland();
        self.land = LandEpisode::default();
        self.landing = false;
        self.repo = None;
        self.owed = None;
        self.held_seed = None;
        self.left_seed = None;
        self.resumed_reply = None;
        self.carry_on = false;
        self.cut = None;
    }

    /// A VANILLA SPELL COVERED THIS THREAD (`/alab off` ... `/alab on`, or a launch or a resume
    /// with `[tui] alab_interface = false`: edges-1, edges-2): every hook stood aside while it
    /// stood, so turns ran here that no point, exchange or opening of this memory saw, a
    /// person's among them (codex-alab types nothing while vanilla). The point before the spell
    /// is another conversation's, and nothing is decided over it, or over what was owed, held or
    /// carried there (the person answered the agent's question by hand, and an act decided at
    /// that point answered it again); a person's turn ran (its task, as
    /// [`Self::observe_person_turn`] opens one); and the session is past its opening, failing
    /// toward carrying as a resumed thread whose books were not read is (the owner, 2026-10-27:
    /// "the user question exception is only for the first message of a session"). The point
    /// codex-alab stands at next is the thread's own last turn end: the history's, read by the
    /// look that takes the spell in (`extension::rewatch_bound`), so Cruise never parks over the
    /// spell's work, or the end of the turn whose start or end that look is. The needs are left
    /// to that reader: where an Off stood (vanilla gives back a claim held at `/alab off`,
    /// `Supervisor::release_for_off`), [`Self::park_off`] took them down just before this; the
    /// history's reader sets them to what the books leave standing, a set the Off took down
    /// staying down; and a turn's end folds its own report, as every turn's end does.
    pub fn unwatched(&mut self) {
        self.observe_person_turn();
        self.point = None;
        self.point_turn_id = None;
        // A person's Esc held the point that goes with it ([`Self::esc_at`]).
        self.esc_at = None;
        self.esc_seed = None;
        // A turn running now is this conversation's newest (the turn whose start takes the spell
        // in set it just before): its native origin stays. Any other is the spell's past.
        self.native_origin = self
            .native_origin
            .take()
            .filter(|origin| origin.phase == codex_alab_core::session::NativePhase::Running);
        self.left_seed = None;
        self.awaited = None;
        self.exchange = false;
        self.exchange_work = Duration::ZERO;
        self.exchange_ran_tools = false;
        self.exchange_questions = false;
        self.left = false;
        self.carrying = false;
        self.claimed = false;
        self.past_opening = true;
    }

    /// A person's message arrived with aterm's `status human_ms=` naming their last window
    /// gesture at `at` (read then, bounded; none read is no hand): the turn ending next is
    /// theirs where `at` falls inside it ([`PERSON_TURN_SLACK`]) and after codex-alab's last
    /// point and act (aterm's `hand`).
    pub fn observe_person_hand(&mut self, at: Instant) {
        self.hand_at = Some(at);
    }

    /// A TURN BEGAN on this thread (`on_turn_start`, aterm's `took`): one no act of
    /// codex-alab's awaits ([`Self::awaited`]: a person's, a goal's own continuation, anyone
    /// else's) is another turn, and the last land act's yield read goes with it, so aterm's
    /// landing names no legs while that turn runs (review of gap 11, finding 2: the reading
    /// stood until the turn's end). Returns whether the reading went.
    pub fn observe_turn_start(&mut self) -> bool {
        self.awaited.is_none() && self.land.legs.take().is_some()
    }

    /// The session's Autopilot word, folded before each decision. Leaving Land ends its
    /// episode; a landed task read under Cruise goes on (Cruise takes nothing as the end: the
    /// landing's defense is a done claim again, owed the defend prompt at the point it kept);
    /// entering Land starts an episode, its instruction first. An act the previous image owed
    /// under the other mode is none now: the point is decided again under this one. A WALL'S
    /// TRY stays owed: the point it was owed at is the wall's, which the point the record
    /// leaves is not, and a limit's resume types the words of the mode read when it goes
    /// ([`decide`]'s `owed_act`), as aterm's wall acts read the Autopilot word at their point.
    /// Cruise preserves a brief opening without tools or a question box; other exchanges
    /// left by Land, Off or a passive spell are taken back.
    pub fn observe_mode(&mut self, mode: Mode) {
        self.observe_mode_under(mode, false);
    }

    /// [`Self::observe_mode`] at a decision made while the supervisor names Autopilot off over
    /// a kept claim (`off`: a person's hold over Off, whose policy keeps the word before it so
    /// that a landing survives the hold): the word is folded, but CRUISE TAKES NOTHING BACK
    /// while the word the person chose is Off, as aterm's `cruise_takes_exchange` runs under its
    /// Autopilot word Cruise alone, so an exchange the Off left stays theirs, and a Land read
    /// after it keeps it (review of gap 32, finding 9).
    pub fn observe_mode_under(&mut self, mode: Mode, off: bool) {
        // A brief opening exchange without tool work or a question box is still the
        // person's conversation in Cruise. Later interruptions resume the existing task.
        let takes = self.takes_exchange(mode);
        if takes && !off && std::mem::take(&mut self.exchange) {
            self.left = false;
        }
        let was = self.mode.replace(mode);
        if was == Some(mode) {
            return;
        }
        // Out of Cruise: the next Cruise run's first done claim asks the first angle.
        if mode != Mode::Cruise {
            self.checks = 0;
        }
        self.land = LandEpisode::default();
        self.landing = false;
        self.cut = None;
        if mode == Mode::Cruise {
            self.unland();
        }
        if self
            .owed
            .as_ref()
            .is_some_and(|owed| owed.mode != mode && !is_wall_rule(owed.rule))
        {
            self.owed = None;
        }
    }

    /// Whether a point left to the person stays theirs under the current policy.
    /// Publication rechecks this after waiting for the host connection.
    pub fn left_under(&self, mode: Mode) -> bool {
        self.left && !self.takes_exchange(mode)
    }

    fn takes_exchange(&self, mode: Mode) -> bool {
        match mode {
            Mode::Cruise => {
                !self.exchange_opens
                    || self.exchange_ran_tools
                    || self.exchange_questions
                    || self
                        .point
                        .as_ref()
                        .is_none_or(|end| end.ran_tools || end.questions)
            }
            Mode::Land => !self.exchange_opens,
        }
    }

    /// AN OFF STOOD (a person's or the config's Off; aterm's `park_off`): the needs it took
    /// off the person's screen are cleared here too, so the next report that names none does
    /// not raise them again, and the Land episode and a landing in hand end (Land read after
    /// it starts a new episode). A landed task stays landed, but AN OFF AND BACK RE-ARMS LAND
    /// OVER THE REPOSITORY AS IT IS (a person's act, aterm main e0da8d68df): the landing is
    /// judged again over a fresh probe before anything is decided over it ([`Rejudge::Off`];
    /// a wake's judgement already owed stays owed, and forgives nothing now). Kept on the
    /// words alone, an Off under which the person edited by hand came back landed over their
    /// uncommitted work (aterm's merge review, P6).
    pub fn park_off(&mut self) {
        self.recovery_not_before = None;
        self.checks = 0;
        // OFF'S PARK LIFTS RULING F'S HOLD (aterm's `park_off`): the replies and when the last
        // came go; the reset it showed stays, as aterm's `replies_reset` does.
        self.replies = Replies::default();
        self.replies_at = None;
        self.needs = Needs::default();
        self.land = LandEpisode::default();
        self.landing = false;
        self.cut = None;
        // The person drove the session: the task is carried no more (aterm's `park_off`).
        self.carrying = false;
        // The probe the point or landing was judged over is stale: under the Off the person may
        // have changed the repository by hand.
        self.repo = None;
        if self.landed {
            self.off_landing = true;
            if self.rejudge.is_none() {
                self.rejudge = Some(Rejudge::Off);
            }
        }
    }

    /// LAND'S HOLD BEGINS at the act being typed ([`HoldStart`]): its nudges count from here,
    /// and its `since` is the act's own once stamped ([`Self::stamp_land_hold`]).
    fn begin_land_hold(&mut self) {
        self.land.hold = HoldStart {
            since_ms: None,
            base: self.land.nudges,
            relanded: false,
            unstamped: true,
        };
    }

    /// The landing is over (or never was): its judgement and what an Off left with it go.
    fn unland(&mut self) {
        self.landed = false;
        self.off_landing = false;
        self.rejudge = None;
        self.reland = false;
    }

    /// THE LANDING WAITS ON ITS JUDGEMENT ([`Rejudge`]): it stands, but the live upgrade must
    /// not carry it across a re-exec before the probe settled it (the next image would hold
    /// it on the record's word, a false landing).
    pub fn judging(&self) -> bool {
        self.landed && self.rejudge.is_some()
    }

    /// Whether a turn starting now owes the start probe (aterm's `wake_start_owed`): a landing
    /// stands under Land with no Off since and no judgement owed, so the turn may be the
    /// landed task's own wake, and what the repository holds before it runs is what the wake
    /// did not make ([`ThreadState::wake_began_over`]). A person's turn ends the landing at its
    /// start and takes nothing from the probe.
    pub fn wants_start_probe(&self) -> bool {
        self.landed && self.mode == Some(Mode::Land) && !self.off_landing && self.rejudge.is_none()
    }

    /// THE START PROBE RAN as the turn in hand began, before it ran (`on_turn_start`, awaited
    /// by core before the turn is registered): `found` is what the repository held then
    /// (aterm's `wake_began_over`). Only a probe of this turn's own start counts: the turn's
    /// end takes it ([`ThreadState::observe_turn_end`]).
    pub fn wake_began_over(&mut self, found: impl Into<RepoRead>) {
        let found = found.into();
        if self.wants_start_probe() {
            self.began = Some(found);
        }
    }

    /// The thread was resumed (a fresh store): `task` from its history, `last_reply` its last
    /// reply, `relaunched` when aterm's crash relaunch brought it back
    /// (`extension::relaunched_by_aterm`). A relaunched task is carried on whatever its last
    /// words said (a done claim no longer stops the carry-on: Cruise never rests). The needs
    /// its history leaves standing are the runtime's to restore
    /// ([`ThreadState::restore_needs`]).
    pub fn observe_resume(&mut self, task: bool, last_reply: Option<&str>, relaunched: bool) {
        self.carry_on_not_before = None;
        self.task = task;
        self.carry_on = relaunched && task;
        self.resumed_reply = last_reply.map(str::to_string);
    }

    /// The needs a resumed thread's history leaves standing (`extension::history_needs`: every
    /// report folded in order, a person's turn clearing them), restored after aterm's crash
    /// relaunch and codex-alab's own re-exec: the process exit took them off the person's
    /// screen, and nothing the worker or a person said since cleared them (review of
    /// 2a9241bbf, findings 11 and 18).
    pub fn restore_needs(&mut self, needs: Needs) {
        if self.task {
            self.needs = needs;
        }
    }

    /// THE PACE A PROCESS BEFORE THIS ONE KEPT ON THIS THREAD (its record's: the image before a
    /// re-exec, or the dead process aterm relaunched; aterm's `seed_short`, `seed_retry` and
    /// `seed_usage_attempt`): the short streak, which the next yield of codex-alab's act
    /// lengthens as it would have there, and the wall's ladder, whose next try goes on the rung
    /// that process was on, counted from the appearance that answered its last try
    /// ([`WallTrack::since`]), that try still owed its answer where the process died in its
    /// turn ([`WallTrack::answer_owed`]). Taken only where this memory holds none of its own (a
    /// `/resume` back keeps what this process saw), and a ladder only of a class that outlives
    /// a process ([`WallTrack::carried_class`]).
    pub fn seed_pace(&mut self, short_streak: u32, wall: Option<WallTrack>) {
        if self.short_streak == 0 {
            self.short_streak = short_streak;
        }
        if self.wall.is_none() {
            self.wall = wall.filter(|w| w.carried_class().is_some());
        }
    }

    /// THE SESSION WORKED BEFORE THIS PROCESS (aterm's `seed_worked`, from its books; gap 32):
    /// the resumed thread's history holds a turn that ended or an act of codex-alab's
    /// ([`crate::resume_history::worked`]), or the record hands on a pace
    /// (`extension::Autopilot::seed_pace`); or the runtime fails toward carrying where it read
    /// no such books (`extension::Autopilot::seed_worked`). A person's message from here on is
    /// never the one that opens the session ([`Self::past_opening`]).
    pub fn seed_worked(&mut self) {
        self.past_opening = true;
    }

    /// THE DONE CHECKS A PROCESS BEFORE THIS ONE TYPED IN THIS CRUISE RUN (its record's pace;
    /// aterm's `seed_checks`): the next asks the angle after them. Taken only where this memory
    /// counted none of its own.
    pub fn seed_checks(&mut self, checks: u8) {
        if self.checks == 0 {
            self.checks = if checks < CHECKS_WRAP {
                checks
            } else {
                CHECKS_ROUND + checks % CHECKS_ROUND
            };
        }
    }

    /// THE ACT A WALL CUT IN A PROCESS BEFORE THIS ONE (its record's pace; aterm's `cut_text`),
    /// typed under `mode`: a wall's try delivers it again, and a full context's compaction
    /// types it again verbatim ([`ThreadState::cut`]), as that process would have. The mode
    /// comes with it, so the first decision under the same word keeps it ([`Self::observe_mode`]
    /// drops it under another). Taken only where this memory holds none of its own.
    pub fn seed_cut(&mut self, text: String, mode: Mode) {
        if self.cut.is_some() || self.mode.is_some_and(|m| m != mode) {
            return;
        }
        self.cut = Some(text);
        self.mode = Some(mode);
    }

    /// LAND'S HOLD IN A PROCESS BEFORE THIS ONE (its record's pace: the image before a re-exec,
    /// or the dead process aterm relaunched; aterm's `seed_land_hold`, from its books): it began
    /// at `since_ms` (unix ms) and `nudges` nudges were typed since, so aterm's landing names
    /// the same `since` and count in this image, and the next nudge counts on from them (review
    /// of gap 11, finding 1: the image after a re-exec showed no landing until its next land
    /// act, which began the hold anew). It shows once the episode's instruction is known typed
    /// ([`Self::observe_owed`]: an owed nudge or land answer, a wall's try of a land act), as
    /// aterm seeds only an episode its books say was typed; an act that first types Land's
    /// words in this image's episode begins its own. Taken only where this memory holds no hold
    /// of its own.
    pub fn seed_land_hold(&mut self, since_ms: u64, nudges: u32) {
        if self.land.typed
            || self.land.hold != HoldStart::default()
            || self.mode.is_some_and(|m| m != Mode::Land)
        {
            return;
        }
        self.mode = Some(Mode::Land);
        self.land.nudges = nudges;
        self.land.hold = HoldStart {
            since_ms: Some(since_ms),
            base: 0,
            relanded: false,
            unstamped: false,
        };
    }

    /// THE ACTS A PROCESS BEFORE THIS ONE TYPED, at `at` (its record's pace: the image before a
    /// re-exec, or the dead process aterm relaunched; aterm's `seed_acts`): counted by
    /// `continue_per_hour` like this process's own while they are within the window at `now`,
    /// so a re-exec or a relaunch spends the hour's budget where the process before left it.
    /// Taken only where this memory has typed none of its own (a `/resume` back keeps what
    /// this process typed).
    pub fn seed_acts(&mut self, at: &[Instant], now: Instant, t: &Timing) {
        if !self.typed.is_empty() {
            return;
        }
        let mut within: Vec<Instant> = at
            .iter()
            .copied()
            .filter(|at| now.saturating_duration_since(*at) < t.window)
            .collect();
        within.sort_unstable();
        self.typed = within.into();
    }

    /// The previous image of this process owed `owed` at its re-exec, or had `landed` the
    /// task (its session record; the live upgrade under FOREVER CRUISE / FOREVER LAND): this
    /// image types the act at its time as it was decided, and a landed task stays landed.
    /// Either way the point the old image stood at, its last turn's end (the resumed reply),
    /// is this thread's point again, at `at`: a landing read under Cruise goes on at it, and
    /// an owed act a mode read since makes none is decided again at it (review of 2a9241bbf,
    /// findings 2, 10 and 19). Called after [`ThreadState::observe_resume`].
    pub fn observe_owed(&mut self, owed: Option<Owed>, landed: bool, at: Instant) {
        if !self.task || (owed.is_none() && !landed) {
            return;
        }
        self.point = Some(TurnEnd {
            at,
            by_autopilot: true,
            by_person: false,
            text: self.resumed_reply.clone(),
            earlier: None,
            questions: false,
            worked: Duration::ZERO,
            wall: None,
            aborted: false,
            ran_tools: false,
            restored: true,
        });
        self.held_seed = None;
        self.left_seed = None;
        self.carry_on = false;
        if landed {
            self.landed = true;
            self.mode = Some(Mode::Land);
            return;
        }
        let Some(owed) = owed else {
            return;
        };
        self.mode = Some(owed.mode);
        // A nudge or a land answer is owed only once the episode had its instruction. A wall's
        // try says so only where it delivers a land act again ([`Owed::landing`]): a limit's
        // resume types the instruction or a nudge, and a retry delivers whichever act the wall
        // cut, its yield a land act's where that act was one. Where `seed_land_hold` already
        // seeded a standing hold from the record, the episode's instruction was already typed.
        if owed.mode == Mode::Land
            && ((owed.rule != words::RULE_LAND && !is_wall_rule(owed.rule))
                || self.land.hold.known())
        {
            self.land.typed = true;
        }
        if owed.mode == Mode::Land && owed.landing && is_wall_rule(owed.rule) {
            self.land.typed = true;
            self.landing = true;
        }
        self.owed = Some(owed);
    }

    /// A turn this process ran ended.
    pub fn observe_turn_end(&mut self, end: TurnEnd, t: &Timing) {
        self.point_turn_id = None;
        self.esc_at = None;
        self.esc_seed = None;
        self.held_seed = None;
        self.left_seed = None;
        self.recovery_not_before = None;
        self.carry_on_not_before = None;
        self.net_asked = None;
        self.asleep_mark = None;
        // The start probe was this turn's own, or none ran for it.
        let began = self.began.take();
        // A PERSON'S HAND opened this turn (aterm's `hand`): their window gesture inside it
        // and after codex-alab's last point and act. Read before the point moves.
        let since = self.point_at().max(self.last_act);
        self.turn_since = since;
        let hand = self.hand_at.take().is_some_and(|at| {
            end.at.saturating_duration_since(at) <= end.worked.saturating_add(PERSON_TURN_SLACK)
                && since.is_none_or(|since| at > since)
        });
        // THE SESSION'S OPENING (aterm's `first_point`, `point_seed`, `worked_before`): no turn
        // ended here before this one and the books showed none ([`Self::past_opening`]). Read
        // before it is set, so this turn's own end can still be the opening exchange.
        let opening = !std::mem::replace(&mut self.past_opening, true);
        if end.by_person {
            self.observe_person_turn();
        }
        self.carry_on = false;
        self.owed = None;
        self.repo = None;
        self.reopened = RepoRead::default();
        // Any turn's end leaves the last yield's reading behind (aterm's: another turn ran, or
        // a new point); a land act's yield reads its own (below).
        self.land.legs = None;
        if self.compacting {
            self.compacting = false;
            if matches!(end.wall, Some(Wall::Context { .. })) {
                // The compaction's own work is no work past the full context: its track
                // climbs on, and the next is the owner's.
                self.note_wall(&end, None);
            } else {
                self.after_compact = end.wall.is_none() && !end.aborted;
            }
            self.point = Some(end);
            return;
        }
        self.after_compact = false;
        // RULING F COUNTS EVERY ANSWER (aterm's `TurnEndState::observe`), before the point
        // moves: the act awaited is answered by this turn, unless it is a person's (Codex says
        // whose turn it is), and a restored end is no answer in this process.
        // THE UPGRADE NOTICE'S ANSWER is the host's turn's, which aterm's replies leave alone
        // (`act_in_flight` and `untaken` pass over `RULE_HOST_TURN`): it neither counts nor lifts.
        let notice_answer = end.by_autopilot && self.last_rule == Some(words::RULE_UPGRADE_NOTICE);
        if !end.restored && !notice_answer {
            let awaited = self.awaited.take().filter(|_| !end.by_person);
            self.count_reply(&end, awaited, t);
        } else if end.restored && self.land.hold.known() {
            self.land.typed = true;
        }
        // The act a wall cut stays owed only past a wall of its own turn.
        if !end.by_autopilot || end.wall.is_none() {
            self.cut = None;
        }
        self.observe_exchange(&end, hand, opening);
        let short = end.worked < t.min_work || words::says_done(end.text.as_deref());
        if end.by_autopilot && self.last_rule.is_some_and(is_continue_rule) {
            self.short_streak = if short { self.short_streak + 1 } else { 0 };
        } else if !end.by_autopilot && end.wall.is_none() && !end.aborted && !self.left {
            // Someone else's turn (a person's, or one codex-alab did not type) counts as
            // aterm's `TurnEndState` counts it: under `min_work` it is short and the act that
            // answers it backs off (2 min after one); real work ends the streak. A person's
            // own exchange left to them (Land's opening one, or an Off's) moves it not.
            self.short_streak = if end.worked < t.min_work {
                self.short_streak + 1
            } else {
                0
            };
        } else if !short {
            self.short_streak = 0;
        }
        match &end.wall {
            Some(_) => self.note_wall(&end, Some(t.progress)),
            // THE WALL HAS LEFT (aterm's point with no wall on it): gone for good once the
            // worker worked or codex-alab's try was taken (aterm's `retry_taken`), which every
            // turn that ends here is, however short: a try answered short with no wall
            // included. Kept, that try's rung went on to the next wall, however much later and
            // whatever came between, where aterm's ladder starts over at its first rung.
            None => self.wall = None,
        }
        if !end.by_autopilot {
            // Someone else's turn: no land act's yield. A person's opened the task again
            // (above). A TURN NOBODY ASKED over a landing under Land (the landed task's own
            // wake: a goal's continuation) is NOBODY'S ACT, which keeps the landing where it
            // left nothing open (aterm main e0da8d68df's `wake_keeps`): judged over the
            // repository before anything is decided at its point ([`Rejudge::Wake`]), the
            // landing standing meanwhile, a wall it met included (aterm's P4: dropped there,
            // the limit's resume was typed into the landed work at its reset). A wake's
            // judgement still owed (none decided since) folds into this one: open if either
            // left work open, and the repository compared with what it held as the first
            // began.
            self.last_rule = None;
            self.landing = false;
            if self.landed && !end.by_person && self.mode == Some(Mode::Land) {
                let open = wake_left_open(&end);
                self.rejudge = Some(match self.rejudge.take() {
                    Some(Rejudge::Wake {
                        open: before,
                        began: first,
                    }) => Rejudge::Wake {
                        open: before || open,
                        began: first,
                    },
                    _ => Rejudge::Wake { open, began },
                });
            } else {
                self.unland();
            }
        }
        // THE NEEDS the reply names, its whole turn read (a need said in an earlier message
        // stands beside its last words', aterm's `turn_said`), WHATEVER ENDED THE TURN: Codex
        // hands each agent message whole, and a wall comes from a later request of the turn, so
        // a need a walled or interrupted turn said is the owner's to see (the owner, 2026-10-19:
        // "if you need something from the user, be LOUDER ABOUT IT ... but NEVER let that block
        // you"; needs-wall-turn-unfolded: the act after the wall began another turn, and the
        // need was lost for good). A person's turn answered what stood, at its point and only
        // where it met no wall (aterm's `TurnEndState`: never at its start, so the need stays
        // on the person's screen while they answer it; parity review S4); a turn that is
        // neither theirs nor codex-alab's (a goal's own continuation) answers nothing, and only
        // its report folds (review of 2a9241bbf, finding 15).
        if end.by_person && end.wall.is_none() {
            self.needs = Needs::default();
        }
        self.needs.fold(end.said_whole().as_deref());
        // A LAND ACT'S YIELD IS READ FOR LAND'S HOLD (aterm's `hold_at_yield`, at its
        // `land_judged`): the turn that answers a land act or a wall's try of one, ended with no
        // wall, not stopped and not left to a person.
        if self.mode == Some(Mode::Land)
            && end.by_autopilot
            && self.landing
            && end.wall.is_none()
            && !end.aborted
            && !end.restored
            && !self.left
        {
            self.land.legs = Some(YieldRead::of(&end));
        }
        if end.wall.is_none() && !end.aborted {
            // THE GO-AHEAD'S COUNT ([`GoAheads`]) stands while the replies park, on any word,
            // and no new key of the person's came.
            self.go_ahead_read(end.go_ahead().is_some());
        }
        // A turn of real work, or one a wall cut, starts the go-ahead's span again.
        if !end.aborted {
            self.go_ahead_worked(end.worked, end.wall.is_some(), t);
        }
        // A PERSON'S TURN IS NO KEY OF ITS OWN (aterm's review 7): Codex calls every message not
        // codex-alab's a person's, a lead's `aterm ctl @tab send` and a worker's task prompt
        // included, which the window stamps no key for. Their typed message is a key the next
        // decision reads (`human_typed_ms=`, [`Self::person_keyed`]), from this turn's end where
        // it came in it.
        self.point = Some(end);
    }

    /// A PERSON'S ESC (or Ctrl-C) STOPPED THE TURN THAT ENDED AT `at` ([`Self::esc_at`]): the
    /// runtime tells it after [`Self::observe_turn_end`] of an aborted point, for an interrupt
    /// no relaunch's shutdown made. Any other point owes nothing to it.
    pub fn observe_esc(&mut self, at: Instant) {
        if self.point.as_ref().is_some_and(|end| end.aborted) {
            self.esc_at = Some(at);
        }
    }

    /// A PERSON'S ESC THE PROCESS BEFORE THIS ONE CARRIED ([`Self::esc_seed`]): at `at` it
    /// stopped native turn `turn`, whose abort the resume's history may restore. Taken only where
    /// this memory stands at no point of its own.
    pub fn seed_esc(&mut self, at: Instant, turn: String) {
        if self.point.is_some() || self.esc_at.is_some() {
            return;
        }
        self.esc_seed = Some((at, turn));
    }

    /// The Esc this point holds for that the record carries ([`Self::esc_at`] of a point whose
    /// native turn is named): when, and the turn.
    pub fn esc_carried(&self) -> Option<(Instant, &str)> {
        let at = self.esc_at?;
        let turn = self.point_turn_id.as_deref()?;
        self.point
            .as_ref()
            .is_some_and(|end| end.aborted)
            .then_some((at, turn))
    }

    /// A PERSON'S OWN EXCHANGE, judged at the turn's end (aterm's `observe`): someone else's
    /// turn their hand opened, on a task codex-alab was not carrying (or whose worker had said
    /// done), not stopped, answered within [`EXCHANGE_WORK`], is theirs: under Land only where
    /// it opens the session (`opening`, [`ThreadState::past_opening`]), under Off any, under
    /// Cruise only an opening without tools or a question box ([`ThreadState::ended_under`]; amended 2026-10-28 after the parity sweep, gap
    /// 32: aterm 126e259836, the owner 2026-10-26, and 791977cae8, the owner 2026-10-27: "you
    /// need to keep working even if the user interrupted a long running job. the user question
    /// exception is only for the first message of a session"). A wall's act of codex-alab's
    /// that delivers it again keeps it theirs while the answer's work in all stays within it,
    /// and any other act's answer makes the task codex-alab's. Ended with no wall, the point is
    /// left to them ([`ThreadState::left`]).
    fn observe_exchange(&mut self, end: &TurnEnd, hand: bool, opening: bool) {
        let carried = self.carrying && !self.claimed && !self.left;
        if end.by_autopilot {
            self.exchange_work = self.exchange_work.saturating_add(end.worked);
            self.exchange_ran_tools |= end.ran_tools;
            self.exchange_questions |= end.questions;
            self.exchange &= self.exchange_work < EXCHANGE_WORK
                && !(self.ended_under == Some(Mode::Cruise)
                    && (self.exchange_ran_tools || self.exchange_questions))
                && self
                    .last_rule
                    .is_some_and(|rule| !is_continue_rule(rule) && rule != words::RULE_HOST_TURN);
            self.carrying = !self.exchange && !end.aborted;
        } else {
            let theirs = !carried && !end.aborted && hand;
            self.exchange_work = end.worked;
            self.exchange_ran_tools = end.ran_tools;
            self.exchange_questions = end.questions;
            // Tool work or a question box establishes a task in Cruise. Later interruptions
            // continue the task in either mode.
            self.exchange = theirs
                && end.worked < EXCHANGE_WORK
                && match self.ended_under {
                    Some(Mode::Cruise) => opening && !end.ran_tools && !end.questions,
                    Some(Mode::Land) => opening,
                    None => true,
                };
            self.exchange_opens = opening;
            self.carrying &= carried;
        }
        let ok_to_leave = end.wall.is_none();
        self.left = self.exchange && ok_to_leave;
        self.claimed = !self.left && ok_to_leave && words::says_done(end.text.as_deref());
    }

    /// THE WALL A TURN ENDED ON, folded into its track (aterm's `track_wall`): another class
    /// opens a track at this appearance; the same class after codex-alab's try
    /// ([`WallTrack::answer_owed`]) is that try's answer, which moves the appearance the
    /// ladder counts from (`if retried { t.since = now }`), and real work the try led to (at
    /// least `progress`, [`Timing::progress`]; `None` for a compaction's turn, whose work is
    /// no work past the wall) starts the ladder over there, but for a reply past its output
    /// token maximum, whose long reply that work is (`work_is_its_cause`). A person's or a
    /// goal's turn that meets the same wall moves neither, whatever it worked (aterm's `anew`
    /// asks for the retry): their turn was no try of codex-alab's.
    fn note_wall(&mut self, end: &TurnEnd, progress: Option<Duration>) {
        let Some(wall) = &end.wall else {
            return;
        };
        let class = wall.class();
        let Some(track) = self.wall.as_mut().filter(|w| w.class == class) else {
            self.wall = Some(WallTrack::new(class, end.at));
            return;
        };
        let retried = std::mem::take(&mut track.answer_owed);
        if !retried {
            return;
        }
        if progress.is_some_and(|progress| end.worked >= progress)
            && !matches!(wall, Wall::Overlong { .. })
        {
            *track = WallTrack::new(class, end.at);
        } else {
            track.since = end.at;
        }
    }

    /// The repository probe for this point ran: retain both its finding and an unread result.
    pub fn probed(&mut self, found: impl Into<RepoRead>) {
        self.probed_for(self.point_at(), found);
    }

    /// The instant of the point in hand (`None`: the carry-on's): what a probe begun now is
    /// for ([`ThreadState::probed_for`]).
    pub fn point_at(&self) -> Option<Instant> {
        self.point.as_ref().map(|p| p.at)
    }

    /// The repository probe begun at the point `point` ([`ThreadState::point_at`]) found
    /// `found`: kept only while that point stands. A turn that ended while the probe ran is a
    /// point of its own, and the decider asks for its own probe there (the probe takes up to
    /// `repo::PROBE_LIMIT`, and a person's turn can end inside it).
    pub fn probed_for(&mut self, point: Option<Instant>, found: impl Into<RepoRead>) {
        let found = found.into();
        if self.point_at() == point {
            let theirs = self.landed && self.judge_landing(found.as_ref());
            // Work a landed task's own wake began over is someone else's: no act at this
            // point names it (aterm's `repo_theirs`, which filters the point's finding).
            let found = found.filter(|_| !theirs);
            if self.mode == Some(Mode::Land) {
                self.land.repo = match &found {
                    RepoRead::Read(work) => work.clone(),
                    RepoRead::Unread => None,
                };
                // The yield read here names what the repository holds (aterm's `Repo` and
                // `Unread` legs), the same work Land's act names.
                if let Some(legs) = self
                    .land
                    .legs
                    .as_mut()
                    .filter(|legs| point == Some(legs.at))
                {
                    legs.with_repo(found.as_ref());
                }
            }
            self.repo = Some(RepoCheck { point, found });
        }
    }

    /// THE LANDING'S JUDGEMENT ([`Rejudge`]) over the probe's finding `found`: `Read(None)`
    /// means no finding; `Unread` never proves clean. Settled before the decider looks again,
    /// so no re-exec
    /// carries a landing its judgement undid. AN OFF AND BACK keeps it only over a clean
    /// repository; THE TASK'S OWN WAKE keeps it where its words left nothing open and the
    /// repository is clean, or holds the very work it held as the wake began with no Off
    /// since (`RepoRead::same`: the state read at both ends, so a wake's edit to a file
    /// already changed is the wake's; a state either probe could not read forgives nothing).
    /// Undone, the task is open at this point: Land's act comes, its nudge naming the work
    /// found unless the wake began over it (someone else's, never the worker's to commit:
    /// aterm's `repo_theirs`). Whether the work found is someone else's so.
    fn judge_landing(&mut self, found: RepoRead<&RepoWork>) -> bool {
        let Some(rejudge) = self.rejudge.take() else {
            return false;
        };
        let off_stood = std::mem::take(&mut self.off_landing);
        let woke = matches!(rejudge, Rejudge::Wake { .. });
        // Whether the landing holds, the work Land's nudge names where it does not, and
        // whether the work found is someone else's.
        let (keeps, named, theirs) = match rejudge {
            // Land's instruction comes after an Off (it ended the episode): no nudge to name
            // work in.
            Rejudge::Off => (found.is_clean(), RepoRead::default(), false),
            Rejudge::Wake { open, began } => {
                let began_over = !off_stood
                    && began
                        .as_ref()
                        .is_some_and(|began| RepoRead::same(began.as_ref(), found));
                (
                    !open && (found.is_clean() || began_over),
                    found.filter(|_| !began_over),
                    !found.is_clean() && began_over,
                )
            }
        };
        if keeps {
            // The wake's turn reopened it in aterm: it is recorded again ([`Self::reland`]).
            self.reland |= woke;
            return theirs;
        }
        self.unland();
        self.reopened = named.cloned();
        theirs
    }

    /// The probe's finding for the point in hand: `None` where it has not run for it.
    fn repo_here(&self) -> Option<RepoRead<&RepoWork>> {
        let at = self.point.as_ref().map(|p| p.at);
        self.repo
            .as_ref()
            .filter(|c| c.point == at)
            .map(|c| c.found.as_ref())
    }

    /// The decided act was carried out at `now`. An [`Act::Attention`] answers nothing: the
    /// point stands until a turn's end replaces it, so a point whose attention a passive spell
    /// cleared (`Supervisor::stand_by`, a release) is decided, and raised, again once codex-alab
    /// acts again (review of 4cdc96090, finding 3). [`Act::Landed`] keeps the point too, so a
    /// switch to Cruise acts at it.
    pub fn acted(&mut self, act: &Act, now: Instant, t: &Timing) {
        match act {
            Act::Nothing | Act::Attention { .. } | Act::Probe => return,
            Act::Landed => {
                self.landed = true;
                self.landing = false;
                // A landed task is carried no more (aterm's `done == 2`).
                self.carrying = false;
                self.exchange = false;
                // Judged over a clean repository just now: nothing owed over it, and aterm
                // told of it.
                self.off_landing = false;
                self.rejudge = None;
                self.reland = false;
                // The landing ends the hold (aterm's `hold_landed`).
                self.land.hold = HoldStart::LANDED;
                self.land.legs = None;
                return;
            }
            // THE UPGRADE'S NOTICE IS THE HOST'S TURN (aterm's `host_typed(HostTurn::Notice)`):
            // no act of the policy's, so its answer is awaited by no reply's count, spends none
            // of the hour's budget and climbs no ladder; it ends what a wall delivered again and
            // a landing in flight, as aterm's does.
            Act::Type { rule, .. } if *rule == words::RULE_UPGRADE_NOTICE => {
                self.upgrade_notice = None;
                self.awaited = None;
                self.last_act = Some(now);
                self.cut = None;
                self.landing = false;
                self.last_rule = Some(rule);
            }
            Act::Type { rule, text } => {
                if *rule == words::RULE_POLICY_CONTINUE {
                    self.policy_continued = true;
                }
                // THE LINES THE GO-AHEAD ANSWERS, whose ack it does not owe
                // ([`Needs::ack_owed_past`]): its park's, read off the point it answers, or those
                // Land's instruction typed over the park answers as the go-ahead's would (no act
                // of the go-ahead's count: [`ThreadState::go_ahead_first`]).
                let mut past = Vec::new();
                if *rule == go_ahead::RULE_GO_AHEAD {
                    match self.point.as_ref().and_then(TurnEnd::go_ahead) {
                        Some(park) => {
                            self.go_ahead_typed(&park, now);
                            past = park.needs;
                        }
                        None => self.needs.answer(&[]),
                    }
                } else if !self.needs.answering.is_empty() {
                    past = self.needs.answering.clone();
                    self.needs.answer(&[]);
                }
                // Its answer is ruling F's to count, a wall's own try's toward its tries.
                self.awaited = Some(is_wall_retry(rule));
                self.last_act = Some(now);
                // The lost login's line is typed once per wall (aterm's `back_typed`).
                if *rule == words::RULE_LOGIN_BACK
                    && let Some(wall) = self.wall.as_mut().filter(|w| w.class == WallClass::Login)
                {
                    wall.back_typed = true;
                }
                if is_wall_rule(rule) {
                    self.tried_wall(now);
                    if *rule == words::RULE_API_BACK
                        && let Some(wall) = &mut self.wall
                    {
                        wall.ups = wall.ups.saturating_add(1);
                    }
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
                let land_mode = self.mode == Some(Mode::Land);
                let typed_before = self.land.typed;
                // A quota wall's try under Land types Land's instruction before the episode had
                // it, unless the limit cut a person's own exchange; past it, and then, it types
                // the one cleared line, which gives no instruction and carries no ack, so the
                // ack stays owed ([`resume_text`]; aterm reads `land_given` off the text too).
                let land_resume =
                    land_mode && *rule == words::RULE_LIMIT_RESUME && !words::is_cleared_line(text);
                // Under Land the compaction's continuation types Land's words with the ack, and
                // so does a quota wall's try that types the instruction.
                if (carries_ack(rule) || (land_mode && *rule == words::RULE_COMPACT) || land_resume)
                    && self.needs.key != 0
                {
                    self.needs.acked = Some(self.needs.acked_past(&past));
                }
                match *rule {
                    words::RULE_LAND => self.land.typed = true,
                    words::RULE_LAND_NUDGE => {
                        self.land.nudges = self.land.nudges.saturating_add(1);
                    }
                    // Under Land the carry-on, the compaction's continuation and the usage
                    // resume that types the instruction type a land act's words.
                    words::RULE_HOST_TURN | words::RULE_COMPACT if land_mode => {
                        self.land.typed = true;
                    }
                    words::RULE_LIMIT_RESUME if land_resume => self.land.typed = true,
                    _ => {}
                }
                if *rule == words::RULE_DONE_CHECK {
                    // The next done claim asks the next angle; a wall's retry or the
                    // compaction's that types this one again asks nothing new.
                    self.checks = next_checks(self.checks);
                }
                if *rule == words::RULE_DONE_CHECK || words::is_land_rule(rule) {
                    self.cut = Some(text.clone());
                } else if !is_wall_retry(rule) {
                    self.cut = None;
                }
                self.landing = if words::is_land_rule(rule)
                    || (land_mode
                        && matches!(
                            *rule,
                            words::RULE_HOST_TURN | words::RULE_COMPACT | words::RULE_LIMIT_RESUME
                        )) {
                    self.land.typed
                } else {
                    // A wall's retry delivers the act it retries again, and so does the lost
                    // login's line (aterm's `land_resumed`: a resume past a condition goes on
                    // with what Land asked).
                    is_wall_retry(rule) && self.landing
                };
                // LAND'S HOLD BEGINS ([`HoldStart`], aterm's): at the instruction, at the act
                // that first types Land's words in the episode, and at a land act where no
                // standing hold is known (after a landing, or a re-exec's).
                if land_mode
                    && (*rule == words::RULE_LAND
                        || (!typed_before && self.land.typed)
                        || ((words::is_land_rule(rule) || self.landing) && !self.land.hold.known()))
                {
                    self.begin_land_hold();
                }
                self.last_rule = Some(rule);
            }
            Act::ResumeLimit { text } => {
                self.awaited = Some(true);
                self.last_act = Some(now);
                self.tried_wall(now);
                // The cleared line before Land's instruction (the limit cut a person's own
                // exchange, [`resume_text`]) gives no instruction: the episode stands as it was,
                // and the answer is the person's request's, no land act's.
                if self.mode == Some(Mode::Land) && !self.land.typed && words::is_cleared_line(text)
                {
                    self.landing = false;
                } else if self.mode == Some(Mode::Land) {
                    // A usage resume that first types Land's words, or resumes with no
                    // standing hold known (after a landing, or a re-exec's), begins the hold.
                    let typed_before = self.land.typed;
                    if !typed_before || !self.land.hold.known() {
                        self.begin_land_hold();
                    }
                    self.land.typed = true;
                    self.landing = true;
                    // Land's instruction went with the ack where the episode had not had it
                    // ([`resume_text`]); past it the one cleared line carries none. A goal
                    // resumed through `GoalService` typed none of it ([`Self::goal_resumed`]).
                    if self.needs.key != 0 && !typed_before {
                        self.needs.acked = Some(self.needs.acked_past(&[]));
                    }
                }
                self.last_rule = Some(words::RULE_LIMIT_RESUME);
            }
            Act::Compact => {
                // A command does the task's work: a person's exchange is over (aterm's
                // `awaited_command`), and its answer counts for no reply.
                self.awaited = None;
                self.last_act = Some(now);
                self.exchange = false;
                self.tried_wall(now);
                self.compacting = true;
                self.last_rule = Some(words::RULE_COMPACT);
            }
        }
        self.point = None;
        self.repo = None;
        self.reopened = RepoRead::default();
        self.after_compact = false;
        self.carry_on = false;
        self.owed = None;
    }

    /// [`Self::acted`] for the decision `d` carried out at `now`: the cut text it keeps is the
    /// act's whole words ([`Decision::whole`]; aterm's `kept_words`).
    pub fn acted_on(&mut self, d: &Decision, now: Instant, t: &Timing) {
        self.acted(&d.act, now, t);
        if let (Some(whole), Act::Type { text, .. }) = (&d.whole, &d.act)
            && self.cut.as_ref() == Some(text)
        {
            self.cut = Some(whole.clone());
        }
    }

    /// The usage window's resume went through `GoalService` at `now` (a usage-limited goal set
    /// active again): booked as the try a [`Act::ResumeLimit`] is (taken, its answer owed: the
    /// goal's own turn answers it), but it typed no words, so nothing the resume's words carry
    /// ([`resume_text`]) was given: the ack stays owed for the next act that types, and Land's
    /// episode stands as it was (its instruction not given, no hold begun, no land act in
    /// flight), as a wall's goal resumed leaves it ([`Self::wall_goal_resumed`]). The goal's
    /// continuation is the upstream template, which names no Land rule: booked as the
    /// instruction given, the episode nudged at the goal's turn end and never typed the
    /// instruction (the forever-persist lane, item b).
    pub fn goal_resumed(&mut self, now: Instant, t: &Timing) {
        let (acked, land, landing) = (self.needs.acked, self.land.clone(), self.landing);
        self.acted(
            &Act::ResumeLimit {
                text: String::new(),
            },
            now,
            t,
        );
        self.needs.acked = acked;
        self.land = land;
        self.landing = landing;
    }

    /// `act`, a wall's try whose goal a turn error blocked (the runtime's `GoalStop::Wall`), went
    /// as that goal resumed through `GoalService` at `now`: booked as the try (taken, its answer
    /// owed: the goal's own turn answers it and the ladder climbs), but no words were typed, so
    /// nothing the try's words carry was given: the ack stays owed for the next act that types,
    /// and Land's episode stands as it was (its instruction not given, no hold begun, no land act
    /// in flight), so the instruction still comes at a later turn end. A full context's
    /// continuation under Land types Land's words ([`Self::acted`]); booked as typed, its episode
    /// said the instruction was given when the goal's continuation went instead (review of the
    /// forever-cruise lane, finding 3).
    pub fn wall_goal_resumed(&mut self, act: &Act, now: Instant, t: &Timing) {
        let (acked, land, landing) = (self.needs.acked, self.land.clone(), self.landing);
        self.acted(act, now, t);
        self.needs.acked = acked;
        self.land = land;
        self.landing = landing;
    }

    /// RULING F'S COUNT OF ONE ANSWER (aterm's `observe` arms, in its order; `awaited` the act
    /// this turn answers, `Some(own)`). Real work is a tool run ([`TurnEnd::ran_tools`]): a
    /// turn that ran none is quiet, and IDLE under [`Timing::min_work`], or, answering an act,
    /// at a wall or saying a kept reply again however long it worked (aterm's
    /// `retried_alone`). Codex's events are the tool rows aterm's tool-row readers read, so a
    /// long quiet answer is kept too (aterm's `kept_long`): said again, it is no work. An
    /// OUTAGE's answer neither counts nor lifts ([`Self::outage`]). An interrupted turn
    /// answers nothing: the person's next turn decides.
    fn count_reply(&mut self, end: &TurnEnd, awaited: Option<bool>, t: &Timing) {
        if end.aborted {
            return;
        }
        let mark = mark_of(end);
        let walled = end.wall.is_some();
        // A DATED RESET PASSED SINCE THE LAST ANSWER (aterm's block after its arms, run at the
        // point its loop reads once the reset is past, before the resume is typed; codex-alab
        // reads no point between turn ends, so the next turn end runs it FIRST, and this
        // answer is judged over the replies it leaves, as aterm's next `observe` judges the
        // resume's): the wall's replies may be answered again, each wording's tries kept and
        // its answers none, and the run starts afresh. The point aterm reads past the reset
        // still shows the wall, the newest reply kept, so its walls are kept whatever this
        // answer says (aterm's SR
        // `two_wordings_of_a_wall_by_turns_at_resets_moving_forward_keep_their_tries`). Kept
        // across, a usage window's resumes each met again would make a run of no work that
        // held the first quiet reply past the window.
        if self
            .replies_reset
            .is_some_and(|r| end.at >= r && self.replies_at.is_some_and(|counted| counted < r))
        {
            self.replies = match self.replies.last() {
                Some(k) if k.walled => {
                    let walls: Vec<crate::replies::Kept> = self
                        .replies
                        .kept()
                        .filter(|k| k.walled)
                        .map(|k| crate::replies::Kept { answers: 0, ..k })
                        .collect();
                    Replies::of(&walls)
                }
                _ => Replies::default(),
            };
        }
        let quiet = !end.ran_tools;
        let retried_alone = awaited.is_some()
            && quiet
            && !end.worked.is_zero()
            && (walled || self.replies.has(mark));
        let idle = quiet && (retried_alone || end.worked < t.min_work);
        let kept_long = quiet;
        let outage = self.never_holds(end);
        // The reset this answer showed (aterm's `answer_reset`), a dated one only: a relative
        // one ([`reset_known`]) lifts nothing once passed, so none is kept for it.
        let reset = reset_known(end).filter(|(_, dated)| *dated).map(|(r, _)| r);
        match awaited {
            // AN OUTAGE'S ANSWER COUNTS FOR NO HOLD (aterm's v6 §14.17.5.2), nor the cyber
            // policy's refusal's (the owner's ruling, 2026-10-28: both are retried for ever).
            _ if outage && (idle || kept_long) => {}
            Some(own) if idle || kept_long => {
                self.replies.count(mark, walled, own, idle);
                self.replies_at = Some(end.at);
                self.replies_reset = reset;
            }
            // An act's answer that worked lifts the hold.
            Some(_) => self.replies = Replies::default(),
            // THE PROGRAM'S OWN TURN (a goal's continuation: nobody's) while a reply is kept,
            // no work in it, counts as an act's answer does.
            None if !outage
                && !end.by_person
                && !end.by_autopilot
                && self.replies.last().is_some()
                && (idle || kept_long || (quiet && walled)) =>
            {
                let no_work = idle || (quiet && walled) || (kept_long && self.replies.has(mark));
                self.replies.count(mark, walled, false, no_work);
                self.replies_at = Some(end.at);
                self.replies_reset = reset;
            }
            // ANY OTHER TURN (a person's, work of the program's own): it lifts the hold, but
            // an idle one saying the newest reply again lets exactly one more act go, its
            // wall one more try (aterm's `Kept { answers: 1, tries: min(tries, 2) }`).
            None => {
                self.replies = match self.replies.last() {
                    Some(k) if idle && k.mark == mark => Replies::of(&[crate::replies::Kept {
                        answers: 1,
                        tries: k.tries.min(crate::replies::FUTILE_AFTER_TRIES - 1),
                        ..k
                    }]),
                    _ => Replies::default(),
                };
            }
        }
    }

    /// AN OUTAGE THE TURN ENDED ON (aterm's `outage_shown`): the API never reached, the
    /// server's own failure (an overload, a 429, a 500, a timeout: the model's capacity), or a
    /// reply cut off where the API is measured down. The agent could not act on it, so its
    /// answer is no reply it gave again.
    fn outage(&self, end: &TurnEnd) -> bool {
        match &end.wall {
            Some(Wall::Network { .. }) => true,
            Some(Wall::Server { cut_off, .. }) => {
                !*cut_off || matches!(self.reach, Reach::Down { .. })
            }
            _ => false,
        }
    }

    /// A WALL WHOSE ANSWERS NEVER COUNT AND NEVER HOLD: an outage ([`Self::outage`]), or the
    /// cyber policy's refusal ([`Wall::Policy`]: aterm has no such wall). THE OWNER'S RULING
    /// (2026-10-28): codex-alab "must keep continuing and making progress with retries"
    /// through the model's capacity outages and the security classifier's refusals, whatever
    /// aterm does.
    fn never_holds(&self, end: &TurnEnd) -> bool {
        self.outage(end) || matches!(end.wall, Some(Wall::Policy))
    }

    /// THE ROUTE MEASURED BACK UP at a reply cut off (aterm's `route_back`, its `CutOff` arm;
    /// a network wall is an outage, which holds nothing): when the reach came up. Codex-alab
    /// measures the reach at network and usage walls only, so at a cut-off it reads none
    /// today; ported so a measure there lifts the hold as aterm's does.
    fn route_back(&self, end: &TurnEnd) -> Option<Instant> {
        match (&end.wall, self.reach) {
            (Some(Wall::Server { cut_off: true, .. }), Reach::Up { since }) => Some(since),
            _ => None,
        }
    }

    /// THE SAME REPLY AGAIN, A WALL'S ONCE ITS TRIES ARE SPENT, OR A RUN OF NO WORK (aterm's
    /// `TurnEndState::repeats`): the point `end` shows the newest reply kept, and it came again
    /// with no work, or a wall's [`crate::replies::FUTILE_AFTER_TRIES`] own tries are spent at
    /// it ([`crate::replies::Kept::holds`]; its tries the reply's own or the wall's track's,
    /// whichever is more), or [`crate::replies::NO_WORK_ACTS`] no-work answers came in a row
    /// (and three more at a wall whose tries are not spent); nothing is typed at it. An outage
    /// and the cyber policy's refusal hold nothing ([`Self::never_holds`]). A DATED RESET the
    /// last answer came before lifts it (the one that answer showed only while the wall's
    /// tries last: a reset moving forward with each answer keeps them), and so does the route
    /// measured back since it; a relative reset lifts only while the wall's tries last.
    pub fn repeats(&self, end: &TurnEnd) -> bool {
        if self.never_holds(end) {
            return false;
        }
        let Some(k) = self.replies.last().filter(|k| k.mark == mark_of(end)) else {
            return false;
        };
        let since = |r: Instant| self.replies_at.is_none_or(|at| at < r);
        let attempts = self
            .wall
            .as_ref()
            .map_or(0, |w| usize::try_from(w.attempts).unwrap_or(usize::MAX));
        let tries = k.tries.max(attempts);
        let futile = crate::replies::FUTILE_AFTER_TRIES;
        let (dated, relative) = match reset_known(end) {
            Some((r, true)) => (
                since(r) && (tries < futile || !crate::replies::same_reset(self.replies_reset, r)),
                false,
            ),
            Some((r, false)) => (false, since(r) && tries < futile),
            None => (false, false),
        };
        if dated || self.route_back(end).is_some_and(since) {
            return false;
        }
        let spent = !k.walled || tries >= futile;
        let run = if spent {
            crate::replies::NO_WORK_ACTS
        } else {
            crate::replies::NO_WORK_ACTS + futile
        };
        (!relative && spent && k.holds()) || self.replies.run >= run
    }

    /// Which arm of ruling F's hold holds the acts now ([`Replies::held_by`]), whether or not
    /// a decision at this point reads it (aterm's `held_by`).
    pub fn held_by(&self) -> Option<HeldBy> {
        self.replies.held_by()
    }

    /// The arm that holds the act at `end` ([`Self::repeats`]); `None` where none does.
    fn holds_at(&self, end: &TurnEnd) -> Option<HeldBy> {
        self.repeats(end).then(|| self.held_by()).flatten()
    }

    /// Which arm of ruling F's hold holds the point in hand (`None` before a point, at an
    /// aborted turn or a person's own exchange, at a wall [`wall_holds`] never holds, or where
    /// [`Self::holds_at`] holds nothing).
    pub fn point_held_by(&self) -> Option<HeldBy> {
        let end = self.point.as_ref()?;
        if !self.task || end.aborted || self.left {
            return None;
        }
        if end.wall.as_ref().is_some_and(|wall| !wall_holds(wall)) {
            return None;
        }
        self.holds_at(end)
    }

    /// WHY LAND STILL HOLDS (aterm's `TurnEndState::land_held`, CONTRACT §5 visibility, gap 11):
    /// under Land, from the act that began the hold ([`HoldStart`], once stamped), the nudges
    /// typed since it and, where a land act's yield was read and no other turn ran since
    /// ([`LandEpisode::legs`]), what held the landing there; `None` before the episode's first
    /// act, after a landing (until Land's next act), at a point left to a person (aterm's
    /// `theirs` face) and outside Land. A yield whose words name no leg (a clean defense, until
    /// its probe reads the repository) is said with none read: the wire takes no read without
    /// a leg. Where every standing need is one the go-ahead answers (none shown), the read
    /// names no needs leg.
    pub fn land_held(&self) -> Option<codex_alab_aterm::LandReport> {
        if self.mode != Some(Mode::Land)
            || !self.land.typed
            || self.land.hold.relanded
            || self.landed
            || self.left
        {
            return None;
        }
        // A NEED THE GO-AHEAD ANSWERS HOLDS NOTHING THE MENU BAR SHOWS (§15.6; aterm's
        // `land_held`, review of the port, finding 11): with every standing line answered, the
        // read names no needs leg, and a read left with no leg is none (the wire's read names
        // one).
        let answered = !self.needs.standing.is_empty() && self.needs.shown().is_empty();
        Some(codex_alab_aterm::LandReport {
            since_ms: self.land.hold.since_ms?,
            nudges: self.land.nudges.saturating_sub(self.land.hold.base),
            read: self
                .land
                .legs
                .as_ref()
                .map(|legs| {
                    let mut read = legs.read.clone();
                    if answered {
                        read.legs
                            .retain(|leg| *leg != codex_alab_aterm::LandLeg::Needs);
                    }
                    read
                })
                .filter(|read| !read.legs.is_empty()),
        })
    }

    /// THE LINE'S TASK FACE (aterm's `TaskFace`, gap 12 of the parity sweep): a landing, else a
    /// point left to a person, and neither while a turn of the thread runs (`running`), as aterm
    /// reopens both at a turn begun (`harness_task.rs`'s `reopen_self_claim_landing`,
    /// `holds_until_another_turn`). Nor after the landed task's own wake, while its judgement
    /// is owed and until its kept landing is recorded again ([`Self::reland`]): aterm reads the
    /// session open from the wake's turn until that record (amended 2026-10-29 after the review
    /// of gap 12, finding 1). No mode is read: the line reads the face under the Land word
    /// only, a Cruise unlands and takes a tasked left point back ([`Self::observe_mode_under`]),
    /// and an Off keeps the landing ([`Self::park_off`]), so Land again wears it at once.
    pub fn task_face(&self, running: bool) -> codex_alab_core::session::TaskFace {
        use codex_alab_core::session::TaskFace;
        let reopened = self.reland || matches!(self.rejudge, Some(Rejudge::Wake { .. }));
        if running || reopened {
            TaskFace::Open
        } else if self.landed {
            TaskFace::Landed
        } else if self.left {
            TaskFace::Theirs
        } else {
            TaskFace::Open
        }
    }

    /// THE ACT JUST TYPED WAS TYPED AT `at_ms` (unix ms; aterm's `ledgered`, the row's own stamp):
    /// the hold it began ([`ThreadState::acted`]) names it as its `since`. A hold begun earlier
    /// keeps its own.
    pub fn stamp_land_hold(&mut self, at_ms: u64) {
        if std::mem::take(&mut self.land.hold.unstamped) {
            self.land.hold.since_ms = Some(at_ms);
        }
    }

    /// LAND'S HOLD AS THE RECORD CARRIES IT ([`codex_alab_core::session::Pace::land_hold`]):
    /// where the standing hold began (unix ms) and the nudges typed since it, which the image
    /// after a re-exec or aterm's relaunch takes up ([`Self::seed_land_hold`]); `None` where
    /// [`Self::land_held`] says none for good (no hold stamped, a landing's, a point left to a
    /// person, another mode). One this image took up rides on before the episode's instruction
    /// is known typed, so a record written meanwhile still carries it.
    pub fn land_hold_carried(&self) -> Option<(u64, u32)> {
        let hold = self.land.hold;
        if hold.relanded || self.landed || self.left || self.mode.is_some_and(|m| m != Mode::Land) {
            return None;
        }
        Some((hold.since_ms?, self.land.nudges.saturating_sub(hold.base)))
    }

    /// Which arm of ruling F's hold the point in hand names in aterm's `status held=`
    /// ([`hold_told`], [`crate::replies::Told::arm`]): [`Self::point_held_by`], except at a usage
    /// window or a quota, whose hold is told as aterm's `limit_repeats` and names no arm.
    pub fn point_held_arm(&self) -> Option<HeldBy> {
        let arm = self.point_held_by()?;
        match self.point.as_ref().and_then(|point| point.wall.as_ref()) {
            Some(Wall::Usage { .. } | Wall::Entitlement) => None,
            _ => Some(arm),
        }
    }

    /// THE REPLIES THE IMAGE BEFORE A RE-EXEC KEPT (its record's pace; aterm's
    /// `seed_replies`), the newest answer counted at `at`: taken only where this memory keeps
    /// none of its own. Never after aterm's relaunch: a process launched after the books is
    /// another program's run (aterm's `launched_since_books`).
    pub fn seed_replies(&mut self, replies: Replies, at: Option<Instant>) {
        if !self.replies.is_empty() || replies.is_empty() {
            return;
        }
        self.replies = replies;
        self.replies_at = at;
    }

    /// THE RESET THE NEWEST REPLY SEEDED SHOWED, as the record says (aterm's
    /// `seed_replies_reset`): taken only where this memory holds none of its own.
    pub fn seed_replies_reset(&mut self, at: Option<Instant>) {
        if self.replies_reset.is_none() {
            self.replies_reset = at;
        }
    }

    /// THE POINT THE IMAGE BEFORE A RE-EXEC HELD (its record's pace): its words, which the
    /// resume restores as this thread's point ([`Self::observe_held`]) where it still holds.
    /// Taken only where this memory stands at no point of its own.
    pub fn seed_held_point(
        &mut self,
        text: Option<String>,
        earlier: Option<String>,
        questions: bool,
        at: Instant,
    ) {
        if self.point.is_some() {
            return;
        }
        self.held_seed = Some(TurnEnd {
            at,
            by_autopilot: true,
            by_person: false,
            text,
            earlier,
            questions,
            worked: Duration::ZERO,
            wall: None,
            aborted: false,
            ran_tools: false,
            restored: true,
        });
    }

    /// THE POINT THE IMAGE BEFORE A RE-EXEC LEFT TO THE PERSON (its record's pace): its words
    /// and whether its exchange opened the session, which the resume restores as this thread's
    /// point, left to them, where the history still rests at it ([`Self::observe_held`];
    /// edges-5: the live upgrade moved a left session with no notice, and the image after stood
    /// at no point, so a later switch to Cruise typed nothing until the person typed). Taken
    /// only where this memory stands at no point of its own.
    pub fn seed_left_point(&mut self, left: codex_alab_core::session::LeftPoint, at: Instant) {
        if self.point.is_some() {
            return;
        }
        self.left_seed = Some((
            TurnEnd {
                at,
                by_autopilot: false,
                by_person: true,
                text: left.point.text,
                earlier: left.point.earlier,
                questions: left.point.questions,
                worked: Duration::ZERO,
                wall: None,
                aborted: false,
                ran_tools: left.ran_tools,
                restored: true,
            },
            left.opens,
            left.had_questions,
        ));
    }

    /// THE POINT LEFT TO THE PERSON, whose words the record carries ([`Self::seed_left_point`]),
    /// and whether its exchange opened the session: this thread's point where it is left
    /// ([`Self::left`]), else the left point a re-exec seeded that the resume has not stood at
    /// yet. The snapshot retains work from the whole exchange, while its question-box state
    /// describes only the latest turn.
    pub fn left_point(&self) -> Option<codex_alab_core::session::LeftPoint> {
        let snapshot = |point: &TurnEnd, opens, ran_tools, had_questions| {
            codex_alab_core::session::LeftPoint {
                point: codex_alab_core::session::ReplyPoint {
                    text: point.text.clone(),
                    earlier: point.earlier.clone(),
                    questions: point.questions,
                },
                opens,
                ran_tools,
                had_questions,
            }
        };
        if self.left {
            return self.point.as_ref().map(|point| {
                snapshot(
                    point,
                    self.exchange_opens,
                    point.ran_tools || self.exchange_ran_tools,
                    point.questions || self.exchange_questions,
                )
            });
        }
        self.left_seed
            .as_ref()
            .map(|(point, opens, had_questions)| {
                snapshot(point, *opens, point.ran_tools, *had_questions)
            })
    }

    /// THE POINT RULING F HOLDS, whose words the record carries ([`Self::seed_held_point`]):
    /// this thread's point where a wall-free point is held ([`Self::holds_at`]), else the
    /// point a re-exec seeded that the resume has not stood at yet and that still holds. A
    /// wall's point is never carried: its words alone would not say the wall's mark.
    pub fn held_point(&self) -> Option<&TurnEnd> {
        self.point
            .as_ref()
            .or(self.held_seed.as_ref())
            .filter(|point| point.wall.is_none() && self.holds_at(point).is_some())
    }

    /// A RESUME AFTER A RE-EXEC STANDS AT THE POINT THE IMAGE BEFORE HELD
    /// ([`Self::seed_held_point`]), at `at`: a held point owes no act, so the record's owed
    /// act names none and [`Self::observe_owed`] restores no point; without this one, an Off
    /// and back in the new image typed nothing until another turn ended, where the image
    /// before typed at once (parity review of the stage-1 port, finding 1; aterm's loop that
    /// takes over a held screen types at a person's Off, its SR's takeover pin). Restored only
    /// where the history's last reply is still that point's and it still holds (its mark the
    /// newest kept reply's): a point that would not hold would type at once, and a history
    /// whose last reply moved while no loop watched drops the stale replies (aterm's
    /// `first_point` check). Called after the owed act, a recovery and a terminal wall restored
    /// theirs, and only where none did.
    pub fn observe_held(&mut self, at: Instant) {
        // A POINT THE IMAGE BEFORE LEFT TO THE PERSON stands again, theirs as it was there, where
        // the history still rests at its reply (edges-5). The current mode rechecks it:
        // Land preserves an opening; Cruise preserves only a taskless opening
        // ([`Self::observe_mode_under`]).
        if let Some((point, opens, had_questions)) = self.left_seed.take()
            && self.task
            && self.point.is_none()
            && self.owed.is_none()
            && self.resumed_reply == point.text
        {
            self.exchange_ran_tools = point.ran_tools;
            self.exchange_questions = had_questions || point.questions;
            self.point = Some(TurnEnd { at, ..point });
            self.exchange = true;
            self.exchange_work = Duration::ZERO;
            self.exchange_opens = opens;
            self.left = true;
            self.held_seed = None;
            self.carry_on = false;
            return;
        }
        let Some(point) = self.held_seed.take() else {
            return;
        };
        if !self.task || self.point.is_some() || self.owed.is_some() {
            return;
        }
        if self.resumed_reply != point.text {
            self.replies = Replies::default();
            return;
        }
        let point = TurnEnd { at, ..point };
        if self.holds_at(&point).is_none() {
            return;
        }
        if self.land.hold.known() {
            self.land.typed = true;
        }
        self.point = Some(point);
        self.carry_on = false;
    }

    fn tried_wall(&mut self, now: Instant) {
        if let Some(wall) = &mut self.wall {
            wall.attempts += 1;
            wall.last_try = Some(now);
            wall.answer_owed = true;
        }
    }
}

/// WHAT A DECISION DID WITH THE GO-AHEAD'S LINES (aterm's `GoAheadVerdict`,
/// [`ThreadState::go_ahead_judge`]; DESIGN-harness-autopilot §15.6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoAheadVerdict {
    /// The decision types the owner's standing go-ahead ([`go_ahead::RULE_GO_AHEAD`]) or waits
    /// out its back-off, or types Land's instruction over the park the go-ahead answers once it
    /// is typed: the lines it answers ([`go_ahead::Park::needs`]) are off the menu bar.
    Answers(Vec<String>),
    /// The go-ahead is spent and the person is told now ([`crate::replies::Told::GoAhead`]):
    /// every line is back.
    Refused,
    /// Any other decision: the lines answered before stay off only where the reply named no need
    /// since ([`Needs::keep_answered`]).
    Other,
}

/// THE OWNER'S STANDING GO-AHEAD (go-ahead@v1; owner, 2026-10-29: "how dare you block on me for
/// go ahead! fix the cruise harness so that this cannot happen again"), aterm's at `ff13268a5b`:
/// a park on a person's go-ahead for the agent's own reversible work ([`go_ahead::parked`]) is an
/// ask the owner's standing go-ahead answers, under Cruise and Land, and nowhere else.
impl ThreadState {
    /// THE PERSON'S LAST KEY, typed `ago` before `now` (aterm's `status human_typed_ms=`, its
    /// `human_ms=` from a window that reports none, or another driver's hand on the tab: never a
    /// control write, codex-alab's or another's, which the window stamps none for, so a turn a
    /// message opened is no key of its own: aterm's review 7). It holds the go-ahead for
    /// [`GO_AHEAD_HELD`] from the later of the key and the end of the turn it came in, where it
    /// came in the point in hand's turn (within its work and [`PERSON_TURN_SLACK`], after
    /// [`Self::turn_since`]); a hold timed later stands. A key that holds nothing at `now`
    /// changes nothing. One read more than [`KEY_DRIFT`] past the newest one read is a new key:
    /// the person came back, and the go-ahead's count starts again. No word of theirs is read: a
    /// hold that outlasts the hour is the agent's to keep, and the go-ahead says so
    /// ([`go_ahead::GO_AHEAD_ANSWER`]).
    pub fn person_keyed(&mut self, ago: Duration, now: Instant) {
        let Some(key) = now.checked_sub(ago) else {
            return;
        };
        let since = self.turn_since;
        let ended = self
            .point
            .as_ref()
            .filter(|end| {
                end.at.saturating_duration_since(key)
                    <= end.worked.saturating_add(PERSON_TURN_SLACK)
                    && since.is_none_or(|since| key > since)
            })
            .map(|end| end.at);
        let from = ended.map_or(key, |end| key.max(end));
        if now >= later(from, GO_AHEAD_HELD) {
            return;
        }
        if self
            .person_key
            .is_none_or(|newest| key > later(newest, KEY_DRIFT))
        {
            self.go_aheads = GoAheads::default();
        }
        self.person_key = Some(self.person_key.map_or(key, |newest| newest.max(key)));
        self.go_ahead_person = Some(self.go_ahead_person.map_or(from, |at| at.max(from)));
    }

    /// When the hour the person's last key holds the go-ahead for ends, where it holds at `now`
    /// ([`Self::person_keyed`]).
    fn key_hour_ends(&self, now: Instant) -> Option<Instant> {
        self.go_ahead_person
            .map(|at| later(at, GO_AHEAD_HELD))
            .filter(|&ends| now < ends)
    }

    /// THE PERSON'S HOUR AS THE RECORD CARRIES IT
    /// ([`codex_alab_core::session::Pace::go_ahead_hour_ms`], aterm's `go-ahead-hour@v1`): when
    /// the hour that holds the go-ahead at `now` began ([`Self::person_keyed`]), from the key or
    /// the end of the turn it came in; `None` where none holds.
    pub fn go_ahead_hour_carried(&self, now: Instant) -> Option<Instant> {
        self.key_hour_ends(now).and(self.go_ahead_person)
    }

    /// THE PERSON'S HOUR A PROCESS BEFORE THIS ONE HELD, begun at `from` (its record's pace: the
    /// image before a re-exec, or the dead process aterm relaunched; aterm's
    /// `seed_go_ahead_hour`): it holds the go-ahead for the rest of that hour, as it would have
    /// there, though this image reads the key that began it as a key alone (its point is a
    /// restored one, with no turn's work to come in). One already over at `now` holds nothing.
    /// The key read again is no new key ([`KEY_DRIFT`]: none of that hour's is later than
    /// `from`), so the count the record carried stands ([`Self::seed_go_aheads`]).
    pub fn seed_go_ahead_hour(&mut self, from: Instant, now: Instant) {
        if now >= later(from, GO_AHEAD_HELD) {
            return;
        }
        self.go_ahead_person = Some(self.go_ahead_person.map_or(from, |at| at.max(from)));
        self.person_key = Some(self.person_key.map_or(from, |key| key.max(from)));
    }

    /// THE GO-AHEAD'S COUNT AS THE RECORD CARRIES IT ([`codex_alab_core::session::Pace::go_ahead`]):
    /// `None` where none was typed on the standing park.
    pub fn go_aheads_carried(&self) -> Option<GoAheads> {
        (self.go_aheads != GoAheads::default()).then_some(self.go_aheads)
    }

    /// THE GO-AHEAD'S COUNT A PROCESS BEFORE THIS ONE KEPT (its record's pace: the image before a
    /// re-exec, or this process before a `/resume` away): the bound goes on where it left it, so
    /// a re-exec at a held park lets no go-ahead through the same reply's hold again
    /// ([`Self::go_ahead_untried`]). Taken only where this memory counted none of its own.
    pub fn seed_go_aheads(&mut self, carried: GoAheads) {
        if self.go_aheads == GoAheads::default() {
            self.go_aheads = carried;
        }
    }

    /// THE GO-AHEAD THIS POINT IS OWED (owner, 2026-10-29: "how dare you block on me for go
    /// ahead!"): the point's park on a person's go-ahead for its own reversible work
    /// ([`TurnEnd::go_ahead`]), where Cruise answers it, or Land once its instruction was typed
    /// and the work is unlanded: the repository probe read work no remote main has, or the park
    /// is landing work by the agent's own words ([`go_ahead::Park::lands`]). Where Land's
    /// instruction is untyped, a park gets the instruction first ([`Self::go_ahead_first`]).
    /// Never while the person's last key holds it ([`Self::person_keyed`]), nor where the ask
    /// names an irreversible act other than the agent's own cleanup on a need's wrapped rows
    /// ([`go_ahead::go_ahead_answers`]: [`words::REVERSIBLE_ANSWER`] answers that).
    pub fn go_ahead_owed(&self, p: &Policy, now: Instant) -> Option<go_ahead::Park> {
        if p.autopilot == Mode::Land && !self.land.typed {
            return None;
        }
        self.go_ahead_parked(p, now)
    }

    /// [`Self::go_ahead_owed`] as if Land's instruction were typed: the park Land answers with
    /// its instruction first, and the go-ahead after it. The person's hour, the same reply's
    /// hold and its telling read this one (aterm's review 9).
    pub fn go_ahead_parked(&self, p: &Policy, now: Instant) -> Option<go_ahead::Park> {
        self.parked_unless(p, self.key_hour_ends(now).is_some())
    }

    /// [`Self::go_ahead_parked`], `held` whether the person's last key holds the go-ahead.
    fn parked_unless(&self, p: &Policy, held: bool) -> Option<go_ahead::Park> {
        if held {
            return None;
        }
        let end = self.point.as_ref()?;
        let park = end.go_ahead()?;
        let unlanded = p.autopilot == Mode::Cruise
            || matches!(self.repo_here(), Some(RepoRead::Read(Some(_))))
            || park.lands;
        (unlanded && go_ahead::go_ahead_answers(end.text.as_deref())).then_some(park)
    }

    /// A PARK UNDER LAND THAT NAMES NO LANDING WORK, the repository not read at this point: the
    /// probe reads whether the work is landed, which says whether the go-ahead is owed
    /// ([`Self::go_ahead_owed`]), whether Land's instruction goes first over it
    /// ([`Self::go_ahead_first`]) and whether the person's hour holds it to the hour's end
    /// ([`Self::go_ahead_hour_ends`]). Read at every such point, whatever the instruction or the
    /// hour, as aterm's `repo_read` reads every Land reading that parks (review of the port,
    /// finding 5: unread, a park the hour held was never decided again, and one before the
    /// instruction got Land's answer and then the same reply's hold, neither the instruction nor
    /// the go-ahead ever typed).
    fn go_ahead_probe(&self, p: &Policy) -> bool {
        p.autopilot == Mode::Land
            && p.answer_questions
            && self.repo_here().is_none()
            && self.point.as_ref().is_some_and(|end| {
                end.go_ahead().is_some_and(|park| !park.lands)
                    && go_ahead::go_ahead_answers(end.text.as_deref())
            })
    }

    /// THE PARK LAND'S INSTRUCTION ANSWERS FIRST: under Land before its instruction was typed,
    /// with answers on, the park the go-ahead answers once it is ([`Self::go_ahead_parked`]).
    /// The instruction typed over it is the act that answers the park, so its lines are off the
    /// menu bar from that decision and its ack owes none of them; a park after it gets the
    /// go-ahead.
    pub fn go_ahead_first(&self, p: &Policy, now: Instant) -> Option<go_ahead::Park> {
        (p.autopilot == Mode::Land && !self.land.typed && p.answer_questions)
            .then(|| self.go_ahead_parked(p, now))
            .flatten()
    }

    /// WHEN THE PERSON'S HOUR THAT HOLDS THIS POINT'S PARK ENDS: the end of the hour their last
    /// key holds the go-ahead for, where it holds at `now` and the point's park is owed once it
    /// ends, or Land's instruction first. The decision is made again there where it would type
    /// nothing ([`Decision::look_again`]), or back off past it, though the parked agent ends no
    /// other turn.
    pub fn go_ahead_hour_ends(&self, p: &Policy, now: Instant) -> Option<Instant> {
        let ends = self.key_hour_ends(now)?;
        self.parked_unless(p, false).is_some().then_some(ends)
    }

    /// A PERSON'S OWN EXCHANGE STANDS AT A WALL (aterm's `exchange_at_wall`): their request
    /// ([`Self::exchange`]: under Land only the session's opening one) met a wall, whose resume
    /// delivers it again, so a usage reset's resume is the one cleared line even before Land's
    /// instruction ([`resume_text`]).
    pub fn exchange_at_wall(&self) -> bool {
        self.exchange && self.wall.is_some()
    }

    /// Whether the person holds the go-ahead at `now`: their last key does
    /// ([`Self::person_keyed`]), or their own exchange stands. Cruise's continuation and done
    /// check then leave [`go_ahead::NEVER_A_NEED`] out ([`go_ahead::unless_held`]).
    pub fn go_ahead_held(&self, now: Instant) -> bool {
        self.key_hour_ends(now).is_some() || self.exchange
    }

    /// Whether the go-ahead is owed at this point, or Land's instruction before it, and none was
    /// typed on the standing park yet ([`GoAheads`]): the same reply's hold lets the point
    /// through, the instruction and then the go-ahead, after which the hold and the go-ahead's
    /// own bound count again.
    pub fn go_ahead_untried(&self, p: &Policy, now: Instant) -> bool {
        self.go_aheads.acts == 0 && self.go_ahead_parked(p, now).is_some()
    }

    /// Whether the go-ahead on the standing park is spent ([`GoAheads`]): [`GO_AHEAD_ACTS`] acts
    /// typed on it, whatever word each reply asked for and whatever work ran between them, or
    /// [`GO_AHEAD_SPAN`] since the first since the agent last moved past it
    /// ([`Self::go_ahead_worked`]). codex-alab then types nothing more and tells the person now
    /// ([`crate::replies::Told::GoAhead`]).
    pub fn go_ahead_spent(&self, now: Instant) -> bool {
        self.go_aheads.acts >= GO_AHEAD_ACTS
            || self
                .go_aheads
                .first
                .is_some_and(|first| now >= later(first, GO_AHEAD_SPAN))
    }

    /// THE GO-AHEAD'S COUNT AFTER A READ of a turn end whose turn `parks`: it starts again where
    /// the turn parks no more, whatever the act typed after; a park on any word or in prose
    /// keeps it.
    fn go_ahead_read(&mut self, parks: bool) {
        if !parks {
            self.go_aheads = GoAheads::default();
        }
    }

    /// THE AGENT MOVED PAST ITS PARK in the turn a point ends: it worked `worked`, real work at
    /// least [`Timing::min_work`], or a wall cut the turn (`walled`), whose wait is the limit's,
    /// not the park's. The go-ahead's span ([`GO_AHEAD_SPAN`]) then runs again from its next
    /// act; its acts stand ([`GO_AHEAD_ACTS`]), so an agent that parks again after work is
    /// bounded all the same.
    fn go_ahead_worked(&mut self, worked: Duration, walled: bool, t: &Timing) {
        if walled || worked >= t.min_work {
            self.go_aheads.first = None;
        }
    }

    /// THE GO-AHEAD WENT ON `park` at `now`: one more act on the standing park, or its first,
    /// and the lines it answers are off the menu bar for good ([`GoAheadVerdict::Answers`]).
    fn go_ahead_typed(&mut self, park: &go_ahead::Park, now: Instant) {
        self.needs.answer(&park.needs);
        self.go_aheads.first.get_or_insert(now);
        self.go_aheads.acts = self.go_aheads.acts.saturating_add(1);
    }

    /// WHAT `d`, decided under `p` at `now`, DID WITH THE GO-AHEAD'S LINES ([`GoAheadVerdict`]):
    /// a point answers them where it types the go-ahead owed ([`Self::go_ahead_owed`]) or waits
    /// out the back-off before it, and where it types Land's instruction over the park the
    /// go-ahead answers once it is typed ([`Self::go_ahead_first`]).
    pub fn go_ahead_verdict(&self, d: &Decision, p: &Policy, now: Instant) -> GoAheadVerdict {
        // A host's step judges the point's decision behind it ([`Decision::behind`]).
        let d = d.behind.as_deref().unwrap_or(d);
        if d.refused {
            return GoAheadVerdict::Refused;
        }
        let park = match &d.act {
            Act::Type { rule, .. } if *rule == go_ahead::RULE_GO_AHEAD => {
                self.go_ahead_owed(p, now)
            }
            Act::Type { rule, .. } if *rule == words::RULE_LAND => self.go_ahead_first(p, now),
            _ => None,
        };
        match park.filter(|_| !self.go_ahead_spent(now)) {
            Some(park) => GoAheadVerdict::Answers(park.needs),
            None => GoAheadVerdict::Other,
        }
    }

    /// THE DECISION'S [`GoAheadVerdict`], judged on the needs the menu bar shows: where the reply
    /// names needs, the lines answered before stay off only where this decision answers them too
    /// ([`Needs::keep_answered`]).
    pub fn go_ahead_judged(&mut self, verdict: GoAheadVerdict) {
        match verdict {
            GoAheadVerdict::Answers(lines) => {
                self.needs.keep_answered(&lines);
                self.needs.answering = lines;
            }
            GoAheadVerdict::Refused => {
                self.needs.answered.clear();
                self.needs.answering.clear();
            }
            GoAheadVerdict::Other => {
                self.needs.answering.clear();
                self.needs.keep_answered(&[]);
            }
        }
    }

    /// THE DECISION FORESEEN AT A TURN'S END, before it is made (the runtime's finish surfaces
    /// the needs before it decides): where the go-ahead is owed at `now` under `p` and not
    /// spent, nor held by the same reply, the lines it answers are held off the menu bar as its
    /// decision would hold them; the decision judges them again ([`Self::go_ahead_judge`]).
    pub fn go_ahead_foreseen(&mut self, p: &Policy, now: Instant) {
        if !p.answer_questions || self.go_ahead_spent(now) {
            return;
        }
        let held = self
            .point
            .as_ref()
            .is_some_and(|end| self.holds_at(end).is_some());
        if held && !self.go_ahead_untried(p, now) {
            return;
        }
        if let Some(park) = self
            .go_ahead_owed(p, now)
            .or_else(|| self.go_ahead_first(p, now))
        {
            self.go_ahead_judged(GoAheadVerdict::Answers(park.needs));
        }
    }

    /// [`Self::go_ahead_verdict`] of `d`, judged ([`Self::go_ahead_judged`]).
    pub fn go_ahead_judge(&mut self, d: &Decision, p: &Policy, now: Instant) {
        let verdict = self.go_ahead_verdict(d, p, now);
        self.go_ahead_judged(verdict);
    }

    /// THE GO-AHEAD DECIDED WAS NOT SUBMITTED: the lines it would have answered are back on the
    /// menu bar. Whether any were held for it.
    pub fn go_ahead_unwritten(&mut self) -> bool {
        if self.needs.answering.is_empty() {
            return false;
        }
        self.needs.answering.clear();
        true
    }
}

/// The ack where the needs owe one ([`Needs::ack_owed`]), else nothing.
fn ack<'a>(s: &ThreadState, p: &'a Policy) -> &'a str {
    if s.needs.ack_owed() {
        p.texts.needs_ack.as_str()
    } else {
        ""
    }
}

/// The reversible clause where a need that owes its ack names an irreversible act (aterm:
/// `ack && needs_destroy`), or where `irreversible`.
fn reversible(s: &ThreadState, irreversible: bool) -> &'static str {
    words::reversible_clause(irreversible || (s.needs.ack_owed() && s.needs.destroy()))
}

/// The open work a done claim names, in the mode's reading: Land's strict one, Cruise's own.
fn open_named(mode: Mode, text: Option<&str>) -> Vec<&'static str> {
    match mode {
        Mode::Land => words::land_open_words(text),
        Mode::Cruise => text.map_or_else(Vec::new, |t| open_work(Some(&needs::without_needs(t)))),
    }
}

/// Cruise's defend-and-find-more prompt for a done claim's words `text` (aterm's `words_act`
/// at a done): the open work the claim named, what the repository holds that no remote main
/// has (`repo`: read when the act goes, clean before), the reversible clause where its words
/// name an irreversible act (`irreversible`) or a need does, and the ack where one is owed, in
/// that order, then the standing rules, fitted as aterm fits them ([`words::fitted_act`]).
fn defend_prompt(
    s: &ThreadState,
    p: &Policy,
    checks: u8,
    text: Option<&str>,
    irreversible: bool,
    repo: RepoRead<&RepoWork>,
) -> String {
    let open = open_named(Mode::Cruise, text);
    let (said, rules) = p.texts.done_check_words(checks);
    // An angle of its own is fitted under the rules whole (the least-cut copy), as aterm fits
    // `done_check_act(n)`: the copy the first angle's act carries was cut to that act's room.
    let rules = if p.texts.done_angles.is_empty() {
        rules
    } else {
        standing_rules(p)
    };
    words::fitted_act_ruled(
        &said,
        &[
            Suffix::Open(&open),
            // Cruise's ask is the agent's own work (aterm's `RepoAsk::Own`, `f9a7dbd4c4`): the
            // probe names a person's files and another session's stash too.
            Suffix::Repo(repo, RepoAsk::Own),
            Suffix::Fixed(reversible(s, irreversible)),
            Suffix::Fixed(ack(s, p)),
        ],
        rules,
    )
}

/// The open work `open` as the record carries it ([`Owed::repo_owed`]).
fn owned(open: &[&str]) -> Vec<String> {
    open.iter().map(|word| (*word).to_string()).collect()
}

/// A done claim's act the previous image owed ([`Owed::text`], decided before the repository
/// was read: Cruise's defend prompt or Land's nudge) with what the repository holds now,
/// fitted again as ONE ACT exactly as the old image would have fitted it once due
/// ([`defend_prompt`], [`land_act`]: the words, the open work the claim named (`open`), the
/// repository's finding, the reversible clause and the ack, then the standing rules), so
/// where the act is short of room the open words are named fewer before the repository's
/// suffix gives way. The decided words are taken apart at the suffixes they were fitted
/// with (the ack, the reversible clause, the open work at however many of its words were
/// named), so the act's own words and its rules stay as decided. A record from before the
/// open work was carried names none (`open` empty): its open words stay as decided.
fn owed_with_repo(
    p: &Policy,
    prompt: &str,
    open: &[String],
    found: RepoRead<&RepoWork>,
    ask: RepoAsk,
) -> String {
    if found.is_clean() {
        return prompt.to_string();
    }
    let (said, _) = words::split_rules(prompt);
    let rules = &prompt[said.len()..];
    let ack = p.texts.needs_ack.as_str();
    let (head, ack) = match said.strip_suffix(ack).and_then(|h| h.strip_suffix(' ')) {
        Some(head) if !ack.is_empty() => (head, ack),
        _ => (said, ""),
    };
    let reversible = words::DONE_CHECK_REVERSIBLE;
    let (head, reversible) = match head
        .strip_suffix(reversible)
        .and_then(|h| h.strip_suffix(' '))
    {
        Some(head) => (head, reversible),
        None => (head, ""),
    };
    let open: Vec<&str> = open.iter().map(String::as_str).collect();
    let head = (1..=crate::open_work::OPEN_NAMED.min(open.len()))
        .rev()
        .filter_map(|most| crate::open_work::open_named(&open, most))
        .find_map(|named| {
            head.strip_suffix(named.as_str())
                .and_then(|h| h.strip_suffix(' '))
        })
        .unwrap_or(head);
    words::fitted_act(
        &format!("{head}{rules}"),
        &[
            Suffix::Open(&open),
            Suffix::Repo(found, ask),
            Suffix::Fixed(reversible),
            Suffix::Fixed(ack),
        ],
    )
}

/// The decision at one point. Pure: the same state, policy, timings and clock give the same
/// decision. `jitter` is this session's share of [`Timing::reset_jitter`].
pub fn decide(s: &ThreadState, p: &Policy, t: &Timing, now: Instant, jitter: Duration) -> Decision {
    decide_with_usage(s, p, t, now, jitter, false)
}

/// The runtime supplies a causal, account-bound usable allowance observation.
pub fn decide_with_usage(
    s: &ThreadState,
    p: &Policy,
    t: &Timing,
    now: Instant,
    jitter: Duration,
    restored: bool,
) -> Decision {
    let mut decision = decide_inner(s, p, t, now, jitter, restored);
    // A PARK THE PERSON'S HOUR HOLDS IS DECIDED AGAIN AT THE HOUR'S END (aterm's
    // `GO_AHEAD_HOUR`): nothing typed here would otherwise stand on an unchanged point past the
    // hour, as a same reply's hold does.
    if decision.act == Act::Nothing && decision.look_again.is_none() {
        decision.look_again = s.go_ahead_hour_ends(p, now);
    }
    // The upgrade's notice is an automatic prompt as any other: the policy's exclusion holds it.
    // The point's own decision stays behind it, judged for the go-ahead's lines.
    if let Some(notice) = upgrade_notice_at(s, &decision) {
        decision = Decision {
            behind: Some(Box::new(decision)),
            ..notice
        };
    }
    let exhausted = |reply: &str| {
        reply
            .lines()
            .any(|line| line.trim() == words::POLICY_EXHAUSTED)
    };
    let policy_exhausted = match &s.point {
        Some(point) => point
            .earlier
            .as_deref()
            .into_iter()
            .chain(point.text.as_deref())
            .chain(
                point
                    .restored
                    .then_some(s.resumed_reply.as_deref())
                    .flatten(),
            )
            .any(exhausted),
        None => s.resumed_reply.as_deref().is_some_and(exhausted),
    };
    if s.policy_continued
        && matches!(
            decision.act,
            Act::Type { .. } | Act::ResumeLimit { .. } | Act::Compact
        )
        && policy_exhausted
    {
        return Decision::attention(
            "wall",
            "the refused action remains excluded and no independently authorized work remains; no automatic request is sent",
        );
    }
    if s.policy_continued {
        match &mut decision.act {
            Act::Type { text, .. } | Act::ResumeLimit { text }
                if !text.ends_with(words::POLICY_BOUNDARY) =>
            {
                text.push(' ');
                text.push_str(words::POLICY_BOUNDARY);
            }
            _ => {}
        }
    }
    settle_after_wake(&mut decision, s, t, now, jitter);
    hold_for_reach(&mut decision, s, p, t, now);
    decision
}

/// THE LIVE UPGRADE'S NOTICE GOES FIRST (`upgrade_ask`; aterm's host types it at an idle point
/// before its loop decides, `IdleHost::idle_point`): where one is due
/// ([`ThreadState::upgrade_notice`]) and the point is clean, it is typed in place of whatever
/// was decided there, a wait or a hold included, which [`decide`] keeps behind it
/// ([`Decision::behind`]) for the go-ahead's verdict. A clean point has a task and an answer that
/// met no wall and was not stopped (a wall's point types nothing of the upgrade's: aterm never
/// types into a session at a usage limit), and it is not a person's own exchange, a landing, a
/// compaction or its owed continuation, nor the act the image before a re-exec owed; nor is a
/// point whose decision leaves it to a person (an attention) or lands it. A point decided to a
/// repository probe is decided again once it is read (review of gap 27, finding 12: in the
/// probe's place the notice went in at a Land yield before its landing was judged), and the
/// notice goes in place of that decision, a landing's excepted.
fn upgrade_notice_at(s: &ThreadState, decided: &Decision) -> Option<Decision> {
    let text = s.upgrade_notice.as_ref()?;
    let clean = s.task
        && s.point
            .as_ref()
            .is_some_and(|end| end.wall.is_none() && !end.aborted)
        && !s.left
        && !s.landed
        && !s.compacting
        && !s.after_compact
        && s.owed.is_none()
        && !matches!(
            decided.act,
            Act::Attention { .. } | Act::Landed | Act::Probe
        );
    clean.then(|| {
        Decision::now(
            Act::Type {
                rule: words::RULE_UPGRADE_NOTICE,
                text: text.clone(),
            },
            "the live upgrade asks first (upgrade_ask)",
        )
    })
}

/// Whether `act` resumes past a usage window ([`Act::ResumeLimit`], or the resume's words a
/// record carried: `limit-resume@v1`).
pub fn resumes_limit(act: &Act) -> bool {
    matches!(act, Act::ResumeLimit { .. })
        || matches!(act, Act::Type { rule, .. } if *rule == words::RULE_LIMIT_RESUME)
}

/// AFTER A WAKE THE HOST SETTLES FIRST (aterm's `settle_wake`: `WAKE_SETTLE` and the session's
/// share of `WAKE_SPREAD`; 2026-10-24): an act typed into the agent goes no sooner than
/// [`Timing::wake_settle`] and this thread's share of the jitter after the host's last wake
/// ([`ThreadState::woke`]). A dark wake (the lid closed: 19 s and 2 s on 2026-10-24, the
/// network down in the first) ends before that, and the wake after it settles again, so no
/// act is typed in one; past a real wake's settle the act goes as decided. An act decided for
/// later than the settle is untouched.
fn settle_after_wake(
    decision: &mut Decision,
    s: &ThreadState,
    t: &Timing,
    now: Instant,
    jitter: Duration,
) {
    let Some(woke) = s.woke else {
        return;
    };
    if !decision.act.needs_hand() {
        return;
    }
    let settled = later(later(woke, t.wake_settle), jitter);
    if settled <= now || decision.not_before.is_some_and(|at| at >= settled) {
        return;
    }
    let words = format!(
        "the host just woke: waiting until it has been awake {} s",
        settled.saturating_duration_since(woke).as_secs()
    );
    decision.not_before = Some(settled);
    decision.why = format!("{}; {words}", decision.why);
    // The earlier wait's clock time is not when the act types now (review of f134d2b2cb,
    // finding 2): its words stay, its time goes.
    decision.shown_wait = Some(match decision.shown_wait.take() {
        Some(shown) => format!("{}; {words}", words::without_clock(&shown)),
        None => words,
    });
}

/// A USAGE WINDOW'S RESUME WAITS FOR THE API (2026-10-24: a resume fired inside a dark wake
/// with the network down, and its request sat in backoff across the sleeps after it). Due,
/// with its route measured ([`ThreadState::net_asked`]), it is not typed into an API measured
/// down: it waits for the measure to come back up (the measure's edge looks again at once) or,
/// at most, [`Timing::down_hold`] from the first ask (a wrong measure holds the resume no
/// longer than the network wall's own hold). Before the first measure is in, it waits
/// [`Timing::probe_wait`] for it. An unknown measure never holds it longer: a slow network is
/// no outage. A resume whose route the probe cannot reproduce is never asked for and goes as
/// decided, and so does every resume while aterm's `probe_api` is off ([`Policy::probe_api`]),
/// an ask made before it went off included.
fn hold_for_reach(decision: &mut Decision, s: &ThreadState, p: &Policy, t: &Timing, now: Instant) {
    if !resumes_limit(&decision.act) || decision.not_before.is_some_and(|at| at > now) {
        return;
    }
    let Some(asked) = s.net_asked.filter(|_| p.probe_api) else {
        return;
    };
    let (until, words) = match s.reach {
        Reach::Down { .. } => (
            later(asked, t.down_hold),
            "access is available, but the API is measured unreachable: resuming when it is \
             reachable, or at this try",
        ),
        Reach::Unknown => (
            later(asked, t.probe_wait),
            "access is available: measuring the API's reach before resuming",
        ),
        Reach::Up { .. } => return,
    };
    if until <= now {
        return;
    }
    decision.not_before = Some(until);
    decision.why = words.to_string();
    decision.shown_wait = Some(words.to_string());
}

fn decide_inner(
    s: &ThreadState,
    p: &Policy,
    t: &Timing,
    now: Instant,
    jitter: Duration,
    restored: bool,
) -> Decision {
    if s.compacting {
        return Decision::nothing("a compaction runs; its idle is not a turn");
    }
    let mode = p.autopilot;
    if s.landed && mode == Mode::Land {
        // A person's Off and back, or the task's own wake, owes the landing its judgement
        // over the repository as it is now ([`Rejudge`], settled by the probe's finding).
        if s.rejudge.is_some() && s.repo_here().is_none() {
            let why = match s.rejudge {
                Some(Rejudge::Off) => "an Off stood over the landing: probe the repository",
                _ => "the landed task's own wake ended: probe the repository",
            };
            return Decision::now(Act::Probe, why);
        }
        // The wake's judgement kept it: aterm, which reopened it at the wake's turn, is told
        // of it again ([`ThreadState::reland`]).
        if s.reland {
            return Decision::now(
                Act::Landed,
                "the landed task's own wake kept the landing: recorded again, as aterm reopened \
                 it at the wake's turn",
            );
        }
        return Decision::nothing("landed; nothing until a person's act");
    }
    // The previous image's own decision, owed at its re-exec under this very mode (a wall's
    // try under any: [`ThreadState::observe_mode`]).
    if let Some(owed) = s
        .owed
        .as_ref()
        .filter(|owed| owed.mode == mode || is_wall_rule(owed.rule))
        && s.task
    {
        return owed_act(s, p, owed, now);
    }
    let Some(end) = &s.point else {
        if s.carry_on && s.task {
            return carry_on(s, p, now);
        }
        return Decision::nothing("no turn this process ran has ended here");
    };
    if !s.task {
        return Decision::nothing("taskless: no person's turn yet");
    }
    // A PERSON'S ESC HOLDS THE POINT FOR `human_grace`, THEN IT IS A TURN END (aterm's
    // `person_hold`; owner 2026-10-27: "you need to keep working even if the user interrupted a
    // long running job"; FOREVER CRUISE, FOREVER LAND): decided again where the grace ends,
    // unless the person's own turn came meanwhile, which replaces this point; past it, decided
    // as a point the agent did not end, its interrupted words read as none (they claim no done,
    // land nothing, ask nothing: aterm's "a check's turn the person stopped gave no answer").
    // Any other abort (a replacement, an ended review, a goal's budget, an interrupted point
    // restored from history) still ends this turn's authority: a later turn decides. A crash
    // relaunch has no aborted turn here and keeps its separate carry-on path.
    if end.aborted {
        let Some(esc) = s.esc_at else {
            return Decision::nothing("the turn was aborted; a later person's turn decides");
        };
        let until = later(esc, p.human_grace);
        if now < until {
            return Decision {
                look_again: Some(until),
                ..Decision::nothing(ESC_HOLD)
            };
        }
    }
    // The compaction's continuation (aterm's `continue_as(RULE_COMPACT)`): under Cruise the
    // continuation alone (`continuation_text`), no reversible clause and no ack, which stays
    // owed for the next free point's act; under Land, Land's words with both (`land_words`).
    if s.after_compact {
        // A COMMAND'S OWED CONTINUATION IS HELD TOO (aterm's `state.after`, checked after
        // `SAME_REPLY`): the reply that came again at the compaction's point.
        if let Some(arm) = s.holds_at(end) {
            if mode == Mode::Cruise && end.wall.is_none() {
                return paced(
                    s,
                    p,
                    t,
                    end,
                    now,
                    words::RULE_CONTINUE,
                    words::FORWARD_RECOVERY.to_string(),
                );
            }
            return Decision::held(arm);
        }
        // THE ACT THE FULL CONTEXT CUT, again, verbatim, without [`go_ahead::NEVER_A_NEED`]
        // while the person's hour holds the go-ahead (aterm's `compact_owed_text`).
        if let Some(text) = s.cut.as_ref().filter(|_| p.continue_on) {
            return Decision::now(
                Act::Type {
                    rule: words::RULE_COMPACT,
                    text: go_ahead::unless_held(text, s.go_ahead_held(now)).into_owned(),
                },
                "type again the act the full context cut",
            );
        }
        let (text, why) = match mode {
            Mode::Cruise => match continuation(s, p, now) {
                Ok(text) => (text, "continue after the compaction"),
                Err(why) => return Decision::nothing(why),
            },
            Mode::Land if !p.continue_on => return Decision::nothing("continue is off"),
            Mode::Land => (
                words::act_words(&land_words(s, p), &[reversible(s, false), ack(s, p)]),
                "land after the compaction",
            ),
        };
        return Decision::now(
            Act::Type {
                rule: words::RULE_COMPACT,
                text,
            },
            why,
        );
    }
    // A PERSON'S OWN EXCHANGE (aterm's `left_to_person`): theirs to answer, whatever the
    // reply says; nothing is typed until the next turn ends. An opening without task work or an
    // Off's stands here; Cruise preserves a taskless opening.
    if s.left {
        return Decision::nothing("a person's own exchange: theirs to answer");
    }
    if let Some(wall) = &end.wall {
        // RULING F HOLDS A WALL WHOSE TRIES ARE SPENT (aterm's `act_at_point`: `SAME_REPLY`
        // before `wall_action`), before its switches and its rung, so a held wall types no
        // further try ([`wall_holds`]; never an outage's or the cyber policy's).
        if wall_holds(wall)
            && let Some(arm) = s.holds_at(end)
        {
            return Decision::held(arm);
        }
        let mut decision = decide_wall(s, end, wall, p, t, now, (jitter, restored));
        if (matches!(&decision.act, Act::Type { rule, .. } if matches!(*rule, words::RULE_API_RETRY | words::RULE_API_CUTOFF | words::RULE_LIMIT_RESUME))
            || matches!(decision.act, Act::ResumeLimit { .. }))
            && !(restored && matches!(wall, Wall::Usage { .. } | Wall::Entitlement))
        {
            decision.not_before = decision.not_before.max(s.recovery_not_before);
        }
        return decision;
    }
    let text = end.text.as_deref().filter(|_| !end.aborted);
    // THE LAND ACT'S YIELD: a clean defense there lands the task once the repository holds
    // nothing no remote main has; never where an earlier message of the turn named a need
    // (the reply is the whole turn, aterm's `clean_words`).
    if mode == Mode::Land
        && end.by_autopilot
        && s.landing
        && words::clean_defense(text)
        && end.earlier_names_no_need()
    {
        return land_yield(s, p, t, end, now);
    }
    let done = words::says_done(text);
    let said = if end.questions && !end.aborted {
        Said::Question
    } else {
        words::classify_said(text)
    };
    let (asked, irreversible) = match &said {
        Said::Irreversible(why) => (Some(why.clone()), true),
        Said::Stop(why) => (Some(why.clone()), false),
        Said::Question => (Some("the worker asked a question".to_string()), false),
        Said::Plain | Said::Offer => (None, false),
    };
    // THE GO-AHEAD (owner, 2026-10-29: "how dare you block on me for go ahead!"): a park on a
    // person's go-ahead for the agent's own reversible work is an ask the owner's standing
    // go-ahead answers ([`go_ahead::go_ahead_answer`]). Under Land it needs Land's instruction
    // typed and the work unlanded, which a park that names no landing work reads off the
    // repository first.
    if s.go_ahead_probe(p) {
        return Decision::now(
            Act::Probe,
            "a park under Land names no landing work: probe the repository",
        );
    }
    let park = s.go_ahead_owed(p, now);
    let first = s.go_ahead_first(p, now);
    // THE SAME REPLY AGAIN lets a park through once ([`ThreadState::go_ahead_untried`]): the
    // answers that formed the hold were other acts', and the go-ahead (or Land's instruction
    // before it) may be what the agent waits for.
    if (park.is_some() || first.is_some())
        && !s.go_ahead_untried(p, now)
        && let Some(arm) = s.holds_at(end)
    {
        return Decision::held(arm);
    }
    // THE GO-AHEAD SPENT ([`ThreadState::go_ahead_spent`]): an agent that still parks on the
    // go-ahead its limit of acts gave gets no more; the person is told now.
    if park.is_some() && s.go_ahead_spent(now) {
        return Decision::refused();
    }
    // The gates on the worker's words (aterm's `words_gate`): a park the go-ahead answers is an
    // ask, told with the go-ahead's why where answers are off.
    let gate_ask = match park {
        Some(_) => Some(go_ahead::GO_AHEAD_WHY.to_string()),
        None => asked.clone(),
    };
    if let Some(why) = &gate_ask
        && !p.answer_questions
    {
        return Decision::attention(
            "question",
            format!("question: {why}; answer_questions is off"),
        );
    }
    if !p.continue_on && (done || gate_ask.is_none()) {
        return Decision::nothing("continue is off");
    }
    // The go-ahead outranks the done check and Land's own acts: the work it names is the work
    // they would ask for. Its ack owes none of the lines it answers.
    if let Some(park) = &park {
        let ack = s
            .needs
            .ack_owed_past(&park.needs)
            .then_some(p.texts.needs_ack.as_str());
        return paced_unheld(
            s,
            p,
            t,
            end,
            now,
            go_ahead::RULE_GO_AHEAD,
            go_ahead::go_ahead_answer(park, ack, standing_rules(p)),
        );
    }
    // Land's instruction over the park the go-ahead answers once it is typed answers that park
    // (no back-off, as the instruction waits none): its ack owes none of the park's lines,
    // which are off the menu bar.
    if let Some(first) = &first {
        let owed = s.needs.ack_owed_past(&first.needs);
        let ack = if owed { p.texts.needs_ack.as_str() } else { "" };
        return paced_unheld(
            s,
            p,
            t,
            end,
            now,
            words::RULE_LAND,
            words::act_words(
                &p.texts.land,
                &[words::reversible_clause(owed && s.needs.destroy()), ack],
            ),
        );
    }
    let held = s.go_ahead_held(now);
    if mode == Mode::Land {
        // A done claim, or a point where nothing is asked: the instruction, then nudges with
        // no bound; a question: the land answer. A done claim goes first: a done worker's
        // "what's next?" answered as a question invites invented work.
        // A done claim's nudge names what the repository holds beside it, read when the act
        // is due as Cruise's prompt reads it (aterm reads the repository at every done claim:
        // its `repo_read`); a point that claims nothing names the work a landing was judged
        // undone over ([`ThreadState::reopened`]). Neither names work a landed task's own wake
        // began over (aterm's `repo_theirs`, [`ThreadState::probed_for`]).
        if !done && asked.is_some() {
            let words_now = words::act_words(&p.texts.land_answer, &[ack(s, p)]);
            return paced(s, p, t, end, now, words::RULE_LAND_ANSWER, words_now);
        }
        let found = if done {
            s.repo_here()
        } else {
            Some(s.reopened.as_ref())
        };
        let open = open_named(mode, text);
        let (rule, words_now) = land_act(s, p, &open, found.unwrap_or_default());
        let base = paced(s, p, t, end, now, rule, words_now);
        if found.is_some() || rule != words::RULE_LAND_NUDGE || base.act == Act::Nothing {
            return base;
        }
        if base.waits(now) {
            // Decided before the repository was read: the live upgrade carries the open work
            // with it, so the next image reads the repository once due ([`Owed::repo_owed`]).
            return Decision {
                repo_owed: Some(owned(&open)),
                ..base
            };
        }
        return Decision::now(Act::Probe, "a done claim under Land: probe the repository");
    }
    if done {
        // CRUISE NEVER RESTS: the defend-and-find-more prompt at every done claim, with the
        // open work it named and what the repository still holds (probed when the act is due;
        // a wait shows the words without it, and the live upgrade carries them so).
        let prompt = defend_prompt(s, p, s.checks, text, irreversible, RepoRead::default());
        let prompt = go_ahead::unless_held(&prompt, held).into_owned();
        let base = paced(s, p, t, end, now, words::RULE_DONE_CHECK, prompt);
        if base.act == Act::Nothing {
            return base;
        }
        if base.waits(now) {
            return Decision {
                repo_owed: Some(owned(&open_named(Mode::Cruise, text))),
                ..base
            };
        }
        let Some(found) = s.repo_here() else {
            return Decision::now(Act::Probe, "a done claim: probe the repository");
        };
        let prompt = defend_prompt(s, p, s.checks, text, irreversible, found);
        let typed = go_ahead::unless_held(&prompt, held).into_owned();
        return Decision {
            whole: (typed != prompt).then_some(prompt),
            act: Act::Type {
                rule: words::RULE_DONE_CHECK,
                text: typed,
            },
            ..base
        };
    }
    let Some(why) = asked else {
        return match continuation(s, p, now) {
            Ok(text) => paced(
                s,
                p,
                t,
                end,
                now,
                words::RULE_CONTINUE,
                words::act_words(&text, &[reversible(s, false), ack(s, p)]),
            ),
            Err(why) => Decision::nothing(why),
        };
    };
    // aterm's answers carry the standing rules after their suffixes (`words_act`).
    let rules = standing_rules(p);
    if irreversible {
        return paced(
            s,
            p,
            t,
            end,
            now,
            words::RULE_ANSWER,
            words::act_words_ruled(
                words::REVERSIBLE_ANSWER,
                &[&p.texts.needs_hint, ack(s, p)],
                rules,
            ),
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
        // aterm's hint goes after the owner's answer, never inside it.
        Some(answer) => paced(
            s,
            p,
            t,
            end,
            now,
            rule,
            words::act_words_ruled(
                &answer,
                &[&p.texts.needs_hint, reversible(s, false), ack(s, p)],
                rules,
            ),
        ),
        None => Decision::attention("question", format!("question: {why}; no answer_text")),
    }
}

/// THE STANDING RULES aterm composed its texts with (`harness policy` sends `continue_text`
/// and the forever texts composed, `answer_text` bare), for the acts whose words arrive bare:
/// the least-cut copy among them ([`words::standing_rules`]: aterm's own act texts, the
/// shortest of which, `land_answer_text`, carries them whole, and `continue_text` where it
/// agrees, or alone where aterm sent no act texts); `None` where none stand: aterm's act
/// texts sent bare say so, whatever an owner's `continue_text` spells.
fn standing_rules(p: &Policy) -> Option<&str> {
    let sent = [
        p.texts.done_check.as_str(),
        p.texts.land.as_str(),
        p.texts.land_nudge.as_str(),
        p.texts.land_answer.as_str(),
    ];
    let sent: &[&str] = if p.texts.from_aterm { &sent } else { &[] };
    words::standing_rules(sent.iter().copied(), p.continue_text.as_deref())
}

/// Land's words for a free point of the episode: the instruction where it has not been
/// typed, else a nudge.
fn land_words(s: &ThreadState, p: &Policy) -> String {
    if s.land.typed {
        p.texts.land_nudge.clone()
    } else {
        p.texts.land.clone()
    }
}

/// Land's act for a free point (aterm's `land_policy`, a done claim or nothing asked): the
/// instruction where the episode has not had it, else a nudge carrying the open work the
/// claim named (`open`) and what the repository still holds (`repo`) as suffixes; the
/// reversible clause where a need names an irreversible act, the ack where one is owed.
fn land_act(
    s: &ThreadState,
    p: &Policy,
    open: &[&'static str],
    repo: RepoRead<&RepoWork>,
) -> (&'static str, String) {
    if !s.land.typed {
        return (
            words::RULE_LAND,
            words::act_words(&p.texts.land, &[reversible(s, false), ack(s, p)]),
        );
    }
    (
        words::RULE_LAND_NUDGE,
        words::fitted_act(
            &p.texts.land_nudge,
            &[
                Suffix::Open(open),
                Suffix::Repo(repo, RepoAsk::All),
                Suffix::Fixed(reversible(s, false)),
                Suffix::Fixed(ack(s, p)),
            ],
        ),
    )
}

/// A clean defense at a land act's yield: the repository probed first; only a clean
/// reading lands. Work found or an unread probe gets the next nudge, paced like any
/// land act at the point's turn end.
fn land_yield(s: &ThreadState, p: &Policy, t: &Timing, end: &TurnEnd, now: Instant) -> Decision {
    match s.repo_here() {
        None => Decision::now(
            Act::Probe,
            "a clean defense at a land act's yield: probe the repository",
        ),
        Some(RepoRead::Read(None)) => Decision::now(
            Act::Landed,
            "landed: a clean defense at a land act's yield, the repository clean",
        ),
        Some(found) => {
            let (rule, text) = land_act(s, p, &[], found);
            paced(s, p, t, end, now, rule, text)
        }
    }
}

/// WHY A POINT A PERSON'S ESC STOPPED TYPES NOTHING YET ([`ThreadState::esc_at`]): aterm's
/// `person_hold` words, held for `human_grace` from the Esc.
pub const ESC_HOLD: &str = "a person stopped the turn with Esc";
/// A usage window's attention where its switches hold its resume ([`decide_wall`],
/// [`owed_act`]).
const USAGE_OFF: &str = "limit: the usage limit was reached; resume_limits or limit_wait is off";
/// A quota wall's attention where its switch holds its try ([`decide_wall`], [`owed_act`]).
const QUOTA_OFF: &str = "limit: this account's quota or plan stops the model; resume_limits is off";
/// The quota wall's diagnostic record while its tries continue (FOREVER CRUISE: NEEDS YOU is
/// and never blocks; the owner, 2026-10-19): only a person lifts an account's quota or plan, so
/// it is said at once, as a need of the `owner=needs` entry beside the worker's own
/// ([`crate::supervisor::Supervisor::own_need_for`]), and the try goes on its ladder all the same
/// ([`quota_try`]). Whole within the first need's [`crate::supervisor::NEED_CELLS`] cells.
pub const QUOTA_NEED: &str = "a top-up or a plan change (this account's quota stops the model)";

/// Whether `act` is a quota wall's try ([`Wall::Entitlement`]: [`words::RULE_LIMIT_RESUME`]
/// typed as a turn, here or as a record handed it on). Under a goal core stopped as
/// usage-limited at that wall, the try resumes the goal, as the usage window's resume does
/// ([`Act::ResumeLimit`]): the goal's stop is the wall's, never a person's.
/// WHETHER `act` IS A WALL'S TRY A TURN ERROR'S GOAL STOP MAY RIDE (the runtime's
/// `wall_goal_after`): a server's, the network's or an overlong reply's retry, the compaction a
/// full context takes, and its continuation; never the usage window's resume or a quota's try
/// (`UsageLimited`'s own) nor the lost login's line (the renewal's).
pub fn wall_try(act: &Act) -> bool {
    match act {
        Act::Type { rule, .. } => is_wall_rule(rule) && *rule != words::RULE_LIMIT_RESUME,
        Act::Compact => true,
        _ => false,
    }
}

pub fn quota_try(act: &Act) -> bool {
    matches!(
        act,
        Act::Type {
            rule: words::RULE_LIMIT_RESUME,
            ..
        }
    )
}

/// THE ACT THE PREVIOUS IMAGE OWED AT ITS RE-EXEC ([`Owed`]), typed as it was decided, under
/// its rule, at its time. A done claim's act decided before the repository was read
/// ([`Owed::repo_owed`]: Cruise's defend prompt, Land's nudge) reads it once due, as it would
/// have in the old image, and names what it holds where aterm places the repository's suffix
/// ([`owed_with_repo`]); not yet due, it stays owed so across another re-exec. Its yield is
/// judged as any act's of that rule: a land act's yield only where the act was one, so a
/// nudge owed over a defense no land act asked for never lands that defense. A WALL'S TRY
/// ([`carried_rule`]) answers to its wall's own switches, as the wall's act does at its point
/// ([`decide_wall`]): one off types nothing and raises the wall's attention in its words (the
/// try carried past a relaunch or a re-exec is the same wall, which the person must hear of
/// as at its own point; the error's own words are not carried, so an API wall's are generic).
/// A limit's resume owed under another mode types the words of the mode read now
/// ([`resume_text`]); its yield is judged as the try's, its wall's ladder (the record's pace)
/// climbing by it.
fn is_question_rule(rule: &str) -> bool {
    matches!(
        rule,
        words::RULE_ANSWER
            | words::RULE_PROSE_LONG_TERM
            | words::RULE_PROSE_SHORT_TERM
            | words::RULE_LAND_ANSWER
    )
}

fn owed_act(s: &ThreadState, p: &Policy, owed: &Owed, now: Instant) -> Decision {
    // THE GO-AHEAD THE IMAGE BEFORE OWED IS THE GO-AHEAD STILL (review of the port, finding 4):
    // what holds it at a free point holds it here, as [`decide_inner`]'s go-ahead block reads it
    // at the decision. Answers switched off since tell it as the question it is; the person's
    // hour, from a key read since the re-exec, holds it to the hour's end, where the point is
    // decided again; the bound spent refuses it. Continue off types it all the same: it is an
    // answer.
    if owed.rule == go_ahead::RULE_GO_AHEAD {
        if !p.answer_questions {
            return Decision::attention(
                "question",
                format!(
                    "question: {}; answer_questions is off",
                    go_ahead::GO_AHEAD_WHY
                ),
            );
        }
        if let Some(ends) = s.key_hour_ends(now) {
            return Decision::hour(ends);
        }
        if s.go_ahead_spent(now) {
            return Decision::refused();
        }
    }
    let wall_off = match owed.rule {
        words::RULE_API_RETRY | words::RULE_API_CUTOFF => (!p.retry_api_errors).then_some((
            "api",
            "api: Codex reported an API error, its try carried here; retry_api_errors is off",
        )),
        words::RULE_LIMIT_RESUME if owed.resume_goal => {
            (!p.resume_limits || !p.limit_wait).then_some(("limit", USAGE_OFF))
        }
        words::RULE_LIMIT_RESUME => (!p.resume_limits).then_some(("limit", QUOTA_OFF)),
        rule if is_question_rule(rule) => {
            let text = s
                .point
                .as_ref()
                .and_then(|end| end.text.as_deref())
                .or(s.resumed_reply.as_deref());
            let why = match words::classify_said(text) {
                Said::Irreversible(why) | Said::Stop(why) => why,
                Said::Question | Said::Plain | Said::Offer => {
                    "the worker asked a question".to_string()
                }
            };
            if !p.answer_questions {
                return Decision::attention(
                    "question",
                    format!("question: {why}; answer_questions is off"),
                );
            }
            if rule != words::RULE_LAND_ANSWER
                && !owed.text.starts_with(words::REVERSIBLE_ANSWER)
                && p.questions == Questions::Ask
            {
                return Decision::attention(
                    "question",
                    format!("question: {why}; questions=ask leaves it to a person"),
                );
            }
            None
        }
        go_ahead::RULE_GO_AHEAD => None,
        _ if !p.continue_on => return Decision::nothing("continue is off"),
        _ => None,
    };
    if let Some((key, text)) = wall_off {
        return Decision::attention(key, text);
    }
    if is_continue_rule(owed.rule) && p.continue_per_hour == Some(0) {
        return Decision::nothing("continue_per_hour unreadable: nothing is typed");
    }
    let due = owed.not_before <= now;
    let mut text = owed.text.clone();
    if owed.rule == words::RULE_LIMIT_RESUME && owed.mode != p.autopilot {
        // The old image's line says how the clearing was known; a decision that typed a prompt
        // instead (Land's instruction) was a measured reset's or a blind try's alike: measured,
        // as aterm's owed continuation types it.
        let clearing = if owed.text.starts_with(words::LIMIT_MAY_HAVE_CLEARED) {
            words::Clearing::Blind
        } else {
            words::Clearing::Measured
        };
        let Some(now_words) = resume_text(s, p, clearing) else {
            return Decision::attention(
                "limit",
                if owed.resume_goal {
                    "limit: the usage limit was reached; no continue_text"
                } else {
                    "limit: quota reached; no continue_text"
                },
            );
        };
        text = now_words;
    }
    if let Some(open) = owed.repo_owed.as_ref().filter(|_| due) {
        let Some(found) = s.repo_here() else {
            return Decision::now(
                Act::Probe,
                format!("the owed {} is due: probe the repository", owed.rule),
            );
        };
        // The old image's act asks what its mode asked: Land's nudge all of it, Cruise's done
        // check the agent's own work.
        let ask = match owed.mode {
            Mode::Land => RepoAsk::All,
            Mode::Cruise => RepoAsk::Own,
        };
        text = owed_with_repo(p, &text, open, found, ask);
    }
    Decision {
        act: owed.act(text),
        not_before: (!due).then_some(owed.not_before),
        why: format!("{} the previous image owed at its re-exec", owed.rule),
        shown_wait: (!due).then(|| words::types_again_at(words::OWED_ACT, owed.not_before, now)),
        repo_owed: owed.repo_owed.clone().filter(|_| !due),
        in_record: None,
        held: None,
        look_again: None,
        refused: false,
        whole: None,
        behind: None,
    }
}

/// THE CARRY-ON after aterm's relaunch of a session that died working, mode-aware: Cruise
/// types the continuation, or the defend prompt where the resumed reply was a done claim (the
/// reversible clause where its words name an irreversible act, D1); Land types its
/// instruction, the episode's first act (a relaunch starts none in hand).
/// THE CARRY-ON AFTER ATERM'S RELAUNCH: the mode's act, its continuation or check without
/// [`go_ahead::NEVER_A_NEED`] while the person's hour holds the go-ahead at `now` (aterm's
/// `carried_text`, `go_ahead_held_by_status`).
fn carry_on(s: &ThreadState, p: &Policy, now: Instant) -> Decision {
    if !p.continue_on {
        return Decision::nothing("continue is off");
    }
    let held = s.go_ahead_held(now);
    let reply = s.resumed_reply.as_deref();
    let text = match p.autopilot {
        Mode::Land => land_act(s, p, &open_named(Mode::Land, reply), RepoRead::default()).1,
        Mode::Cruise if words::says_done(reply) => {
            let Some(found) = s.repo_here() else {
                return Decision::now(Act::Probe, "a resumed done claim: probe the repository");
            };
            let irreversible = matches!(words::classify_said(reply), Said::Irreversible(_));
            // The check the relaunch cut, by its angle: the count's last (aterm's
            // `compact_owed_text`; a count above 0 names the angle typed last), asked again under
            // the host's rule, which counts none, so the next claim asks the angle after it.
            let prompt =
                defend_prompt(s, p, s.checks.saturating_sub(1), reply, irreversible, found);
            go_ahead::unless_held(&prompt, held).into_owned()
        }
        Mode::Cruise => match &p.continue_text {
            Some(text) => words::act_words(
                &go_ahead::unless_held(text, held),
                &[reversible(s, false), ack(s, p)],
            ),
            None => return Decision::nothing("no continue_text: nothing to type"),
        },
    };
    let mut decision = Decision::now(
        Act::Type {
            rule: words::RULE_HOST_TURN,
            text,
        },
        format!("carry on after a relaunch ({})", p.autopilot.word()),
    );
    decision.not_before = s.carry_on_not_before;
    if s.carry_on_not_before.is_some() {
        decision.shown_wait =
            Some("honoring the provider's retry delay before carrying on after relaunch".into());
    }
    decision
}

/// Cruise's continuation at `now`: [`Policy::continue_text`], without
/// [`go_ahead::NEVER_A_NEED`] while the person's hour holds the go-ahead (aterm's
/// `continue_as`, [`go_ahead::unless_held`]).
fn continuation(s: &ThreadState, p: &Policy, now: Instant) -> Result<String, &'static str> {
    if !p.continue_on {
        return Err("continue is off");
    }
    p.continue_text
        .as_deref()
        .map(|text| go_ahead::unless_held(text, s.go_ahead_held(now)).into_owned())
        .ok_or("no continue_text: nothing to type")
}

/// A LEAD'S WORKER IS ANSWERED AT ONCE (aterm's `waited_on`; parity sweep gap 5, its managed
/// half): where a lead waits on the session ([`ThreadState::managed`]), a done claim or a
/// question within its first [`MANAGED_SHORT_TURNS`] short turns waits no back-off, so the
/// lead's `await task` hears of it without the ladder's minutes. A question is any ask aterm's
/// `asked()` reads (a question, a stop, an irreversible act), and the park the go-ahead `rule`
/// answers (aterm's `gate_ask`; review of the port, finding 13); an interrupted turn decides
/// nothing before this is asked. A plain continuation keeps the ladder, as do the fourth short
/// turn and a wall's point.
fn waited_on(s: &ThreadState, end: &TurnEnd, rule: &str) -> bool {
    if !s.managed || s.short_streak > MANAGED_SHORT_TURNS || end.wall.is_some() {
        return false;
    }
    let text = end.text.as_deref();
    let done = words::says_done(text);
    // The go-ahead answers an ask however its park reads (aterm's `gate_ask`: a park made of a
    // NEEDS YOU line alone reads as no question once its needs are cut).
    let asked = rule == go_ahead::RULE_GO_AHEAD
        || end.questions
        || match words::classify_said(text) {
            Said::Irreversible(_) | Said::Stop(_) | Said::Question => true,
            Said::Plain | Said::Offer => false,
        };
    done || asked
}

/// THE HOUR'S TYPING BUDGET (`continue_per_hour`, aterm's `budgeted`): `Ok(Some((free, cap)))`
/// where the acts it counts ([`ThreadState::typed`]) already fill the window, `free` when its
/// oldest leaves it; `Ok(None)` where the budget has room or no cap is set; `Err` with the
/// decision to type nothing where the cap was unreadable.
fn hour_frees(
    s: &ThreadState,
    p: &Policy,
    t: &Timing,
    now: Instant,
) -> Result<Option<(Instant, u32)>, Decision> {
    let Some(cap) = p.continue_per_hour else {
        return Ok(None);
    };
    if cap == 0 {
        return Err(Decision::nothing(
            "continue_per_hour unreadable: nothing is typed",
        ));
    }
    let recent: Vec<Instant> = s
        .typed
        .iter()
        .copied()
        .filter(|at| now.saturating_duration_since(*at) < t.window)
        .collect();
    if recent.len() >= usize::try_from(cap).unwrap_or(usize::MAX)
        && let Some(oldest) = recent.first()
    {
        return Ok(Some((later(*oldest, t.window), cap)));
    }
    Ok(None)
}

/// THE LOST LOGIN'S LINE ([`words::RULE_LOGIN_BACK`], aterm's `type_as(RULE_LOGIN_BACK,
/// login_back_text())`): typed at once where the login came back, with the standing rules after
/// it, counted in the hour's budget as aterm's `budgeted` counts it, and gated by no switch, as
/// aterm's is not. Where the budget is spent aterm escalates; here the cap holds it until the
/// window has room, its wait in aterm's Waiting-row words, as it holds every act it counts
/// ([`paced`]). Never held by ruling F: a login wall is no wall it holds ([`wall_holds`]).
pub(crate) fn login_back(s: &ThreadState, p: &Policy, t: &Timing, now: Instant) -> Decision {
    let act = Act::Type {
        rule: words::RULE_LOGIN_BACK,
        text: words::with_rules(words::LOGIN_BACK, standing_rules(p)),
    };
    match hour_frees(s, p, t, now) {
        Err(unreadable) => unreadable,
        Ok(Some((free, cap))) if free > now => {
            let why = format!(
                "{}: the login is back; continue_per_hour={cap} reached",
                words::RULE_LOGIN_BACK
            );
            Decision {
                act,
                not_before: Some(free),
                why,
                shown_wait: Some(words::types_again_at(words::HOUR_CAP, free, now)),
                repo_owed: None,
                in_record: None,
                held: None,
                look_again: None,
                refused: false,
                whole: None,
                behind: None,
            }
        }
        Ok(_) => Decision::now(act, "the lost login is back: deliver its turn again"),
    }
}

/// A continuation-type act, paced by the short streak and `continue_per_hour`. LAND'S
/// INSTRUCTION WAITS NO BACK-OFF (aterm's `instruction_due`, v6 §14.10): where the act is the
/// instruction itself, a switch to Land types it at once over a standing streak; its answers and
/// nudges keep the ladder, and the hour's budget holds it as it holds every continuation. A
/// lead's worker's first short done claims and questions wait no back-off either ([`waited_on`]).
fn paced(
    s: &ThreadState,
    p: &Policy,
    t: &Timing,
    end: &TurnEnd,
    now: Instant,
    mut rule: &'static str,
    mut text: String,
) -> Decision {
    // An ordinary Cruise reply that repeats changes approach after pacing. Land and
    // provider walls retain their own holds; recovery never grants new authority.
    if end.wall.is_none()
        && let Some(arm) = s.holds_at(end)
    {
        if p.autopilot == Mode::Cruise && p.continue_on {
            rule = words::RULE_CONTINUE;
            text = words::FORWARD_RECOVERY.to_string();
        } else {
            return Decision::held(arm);
        }
    }
    paced_unheld(s, p, t, end, now, rule, text)
}

/// [`paced`] past ruling F's hold, which the go-ahead's own path read first
/// ([`ThreadState::go_ahead_untried`]). THE GO-AHEAD'S WAIT runs from the last act typed, at most
/// [`GO_AHEAD_WAIT`]: the agent's own wakes, each a new point, restart no wait the answer owes
/// (the incident's first twenty hours, 2026-10-27). A BACK-OFF NEVER OUTLASTS THE PERSON'S HOUR
/// (aterm's review 9): where it runs past the hour's end, the point is decided again there,
/// where the park's own wait decides ([`Decision::look_again`]).
fn paced_unheld(
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
    // THE WAIT A PERSON SEES (amended 2026-10-27 after the parity sweep, gap 2: both waits here
    // showed nothing, so a backed-off codex tab looked idle): aterm's own words for it, with the
    // clock time the act is typed.
    let mut shown: Option<String> = None;
    if end.wall.is_none() && s.holds_at(end).is_some() {
        not_before = Some(later(end.at, t.short_backoff.max(Duration::from_secs(120))));
        why = "repeated report: change approach after pacing".to_string();
    }
    if s.short_streak > 0 && rule != words::RULE_LAND && !waited_on(s, end, rule) {
        let doublings = s.short_streak.saturating_sub(1).min(16);
        let wait = t
            .short_backoff
            .saturating_mul(1u32 << doublings)
            .min(t.short_backoff_max);
        let (wait, from) = if rule == go_ahead::RULE_GO_AHEAD {
            (wait.min(GO_AHEAD_WAIT), s.last_act.unwrap_or(end.at))
        } else {
            (wait, end.at)
        };
        let due = later(from, wait);
        let due = not_before.map_or(due, |at| at.max(due));
        if let Some(ends) = s.go_ahead_hour_ends(p, now)
            && due > ends
        {
            return Decision::hour(ends);
        }
        not_before = Some(due);
        why = format!(
            "{rule} after {} short turn(s): back off {}s",
            s.short_streak,
            wait.as_secs()
        );
        shown = Some(words::short_backoff_wait(s.short_streak, wait));
    }
    match hour_frees(s, p, t, now) {
        Err(unreadable) => return unreadable,
        Ok(Some((free, cap))) if not_before.is_none_or(|at| at < free) => {
            not_before = Some(free);
            why = format!("{rule}: continue_per_hour={cap} reached");
            shown = Some(words::HOUR_CAP.to_string());
        }
        Ok(_) => {}
    }
    // Only a wait still ahead is one: a rung already passed types at once.
    let shown_wait = not_before
        .filter(|at| *at > now)
        .zip(shown)
        .map(|(at, words)| words::types_again_at(&words, at, now));
    Decision {
        act: Act::Type { rule, text },
        not_before,
        why,
        shown_wait,
        repo_owed: None,
        in_record: None,
        held: None,
        look_again: None,
        refused: false,
        whole: None,
        behind: None,
    }
}

/// THE WALLS RULING F HOLDS (aterm's `act_at_point` hold, stage 2 of the port): a usage
/// window, a quota, a reply cut off and a reply past its output maximum, each once its own
/// tries are spent at the same answer ([`ThreadState::repeats`]). NEVER the server's own
/// failure (an overload, a 429, a 500, a timeout), the network, or the cyber policy's refusal:
/// their ladders go on for ever (the owner's ruling, 2026-10-28: codex-alab keeps continuing
/// through the model's capacity outages and the security classifier's refusals). The walls
/// left (a full context, a sign-in, a fatal stop) raise attention or compact, and type no
/// tries to hold.
#[must_use]
pub fn wall_holds(wall: &Wall) -> bool {
    matches!(
        wall,
        Wall::Usage { .. }
            | Wall::Entitlement
            | Wall::Server { cut_off: true, .. }
            | Wall::Overlong { .. }
    )
}

/// THE TRY A HELD WALL WOULD TYPE, ITS HOLD SET ASIDE (its rung, as [`decide`] decides it at
/// the point with no hold): what the record's recovery carries for a wall ruling F holds
/// (`extension.rs`'s held point), so the image after a re-exec stands at the wall again and is
/// held there by the replies its record seeds, an Off and back types this try, and aterm's
/// relaunch, whose memory seeds no replies, types it as aterm's own loop does at the wall it
/// finds after a relaunch. `None` where the point is no wall [`wall_holds`] at which ruling F
/// holds now.
#[must_use]
pub fn held_wall_try(
    s: &ThreadState,
    p: &Policy,
    t: &Timing,
    now: Instant,
    jitter: Duration,
) -> Option<Decision> {
    let end = s.point.as_ref()?;
    let wall = end.wall.as_ref().filter(|wall| wall_holds(wall))?;
    s.holds_at(end)?;
    Some(decide_wall(s, end, wall, p, t, now, (jitter, false)))
}

/// THE RESET A WALL NAMES (aterm's `reset_known`), and whether it is DATED (a clock or a date)
/// rather than RELATIVE (a time from its notice, `in 45s`). A usage window's `resets_at` is
/// dated: Codex's error carries the reset as epoch seconds, which is more exact than aterm's
/// reading of a clock off the screen (a minute), and as exact across answers. A reply cut off
/// with the provider's retry advice (`Retry-After`) is relative: Codex hands only a duration.
/// A quota names none, nor a usage window whose error carries no `resets_at`.
fn reset_known(end: &TurnEnd) -> Option<(Instant, bool)> {
    match &end.wall {
        Some(Wall::Usage {
            reset_at: Some(r), ..
        }) => Some((*r, true)),
        Some(Wall::Server {
            cut_off: true,
            retry_at: Some(r),
            ..
        }) => Some((*r, false)),
        _ => None,
    }
}

/// Record a held operation. Ordinary repeats and walls remain quiet under current main's
/// autonomous-recovery policy. A hold at a park the standing go-ahead answers is its bounded
/// refusal: the agent still waits on a person after the answer, so that one hold asks now.
#[must_use]
pub fn hold_told(s: &ThreadState, p: &Policy, arm: HeldBy, now: Instant) -> crate::replies::Told {
    use crate::replies::Told;
    if p.answer_questions && s.go_ahead_parked(p, now).is_some() {
        return Told::GoAhead { arm: Some(arm) };
    }
    match s.point.as_ref().and_then(|point| point.wall.as_ref()) {
        Some(Wall::Usage { reset_at, .. }) => Told::Limit {
            reset_read: reset_at.is_some(),
        },
        Some(Wall::Entitlement) => Told::Limit { reset_read: false },
        _ => Told::Stop(arm),
    }
}

/// The act at a wall (CONTRACT §5's wall rows). Every retry that types aterm's words carries
/// the standing rules after them, as aterm's wall acts do (`type_as`: `with_rules` after
/// `wall_retry_text`).
fn decide_wall(
    s: &ThreadState,
    end: &TurnEnd,
    wall: &Wall,
    p: &Policy,
    t: &Timing,
    now: Instant,
    (jitter, restored): (Duration, bool),
) -> Decision {
    let track = s.wall.as_ref().filter(|w| w.class == wall.class());
    let attempts = track.map_or(0, |w| w.attempts);
    // THE APPEARANCE THE LADDER COUNTS FROM (aterm's `since`): the first, or the one that
    // answered codex-alab's last try ([`WallTrack::since`]), whatever the class. Counted from
    // the try itself, every rung came as much sooner than aterm's as the try's turn lasted
    // (core's own retries of a server error, minutes of a broken stream).
    let since = track.map_or(end.at, |w| w.since);
    match wall {
        // THE LOGIN CAME BACK (gap 13 stage 1, aterm's `RULE_LOGIN_BACK`): its line once per
        // wall ([`WallTrack::back_typed`]), and the ask stands until that turn goes through; the
        // wall met again after it, another identity's login and a login nothing renewed keep the
        // ask, and nothing more is typed.
        Wall::Login if s.login.back && !track.is_some_and(|w| w.back_typed) => {
            login_back(s, p, t, now)
        }
        Wall::Login => Decision::attention(
            "login",
            "login: Codex's sign-in was refused or expired; sign in to Codex again (codex login)",
        ),
        Wall::Fatal { why } => Decision::attention(
            "wall",
            format!("stopped: {why}; automatic continuation will not retry this turn"),
        ),
        Wall::Policy => {
            if !p.continue_on {
                return Decision::nothing("continue is off");
            }
            if s.policy_continued {
                return Decision::attention(
                    "wall",
                    "the cybersecurity access policy (cyberPolicy) also refused the independent-work continuation; the refused action remains excluded and no further automatic request is sent",
                );
            }
            paced(
                s,
                p,
                t,
                end,
                now,
                words::RULE_POLICY_CONTINUE,
                words::POLICY_CONTINUE.to_string(),
            )
        }
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
                return Decision::attention("limit", USAGE_OFF);
            }
            // aterm's `resume_at_reset`: past the reset the wall names now, and after a
            // continuation that met it again the back-off too, whichever is later. A resume
            // that meets another limit (the weekly one past a session window's reset) waits
            // for that limit's reset, however many tries came before it, in this process or
            // in the one whose rung a relaunch took up (the record's `pace`). With no reset
            // named and no try yet, the longest back-off from the appearance (aterm's `(None,
            // None) => back.last()`: 30 min, never the first rung's 10).
            let at_reset = if restored {
                Some(now)
            } else {
                reset_at.map(|reset| later(later(reset, t.reset_grace), jitter))
            };
            // Its line says the limit has cleared where a reset was named (or the accounts
            // probe saw included usage back), else that it may have (aterm's `resume_at_reset`).
            let clearing = if at_reset.is_some() {
                words::Clearing::Measured
            } else {
                words::Clearing::Blind
            };
            let Some(text) = resume_text(s, p, clearing) else {
                return Decision::attention(
                    "limit",
                    "limit: the usage limit was reached; no continue_text",
                );
            };
            let after_retry = attempts
                .checked_sub(1)
                .map(|n| later(since, ladder(&t.limit_backoff, n)));
            let waiting_reset = || match (restored, reset_label) {
                (true, _) => {
                    "fresh included-usage permission allows a recovery attempt".to_string()
                }
                (false, Some(label)) => format!("waiting for the usage reset at {label}"),
                (false, None) => "waiting for the usage reset".to_string(),
            };
            let (at, words) = match (at_reset, after_retry) {
                (Some(r), Some(b)) if r > b => (r, waiting_reset()),
                (_, Some(b)) => (
                    b,
                    if restored {
                        "fresh included-usage permission observed; waiting for retry pacing"
                            .to_string()
                    } else {
                        "the usage limit stood after its reset; waiting again".to_string()
                    },
                ),
                (Some(r), None) => (r, waiting_reset()),
                (None, None) => (
                    later(since, ladder(&t.limit_backoff, u32::MAX)),
                    "waiting out the usage limit".to_string(),
                ),
            };
            Decision {
                act: Act::ResumeLimit { text },
                not_before: Some(at),
                why: words.clone(),
                shown_wait: Some(words),
                repo_owed: None,
                in_record: None,
                held: None,
                look_again: None,
                refused: false,
                whole: None,
                behind: None,
            }
        }
        Wall::Entitlement => {
            if !p.resume_limits {
                return Decision::attention("limit", QUOTA_OFF);
            }
            // A quota wall names no reset: its try is blind (aterm's entitlement ladder), but for
            // included usage the accounts probe saw restored.
            let clearing = if restored {
                words::Clearing::Measured
            } else {
                words::Clearing::Blind
            };
            let Some(text) = resume_text(s, p, clearing) else {
                return Decision::attention("limit", "limit: quota reached; no continue_text");
            };
            let at = if restored {
                attempts.checked_sub(1).map_or(now, |attempt| {
                    now.max(later(since, ladder(&t.entitlement_backoff, attempt)))
                })
            } else {
                later(since, ladder(&t.entitlement_backoff, attempts))
            };
            let words = if restored {
                "subscription access is restored; continuing the pending work"
            } else {
                "the account's quota stops the model; trying again later"
            }
            .to_string();
            Decision {
                act: Act::Type {
                    rule: words::RULE_LIMIT_RESUME,
                    text,
                },
                not_before: Some(at),
                why: words.clone(),
                shown_wait: Some(words),
                repo_owed: None,
                in_record: None,
                held: None,
                look_again: None,
                refused: false,
                whole: None,
                behind: None,
            }
        }
        Wall::Server {
            what,
            cut_off,
            retry_at,
        } => {
            if !p.retry_api_errors {
                return Decision::attention(
                    "api",
                    format!("api: Codex reported {what}; retry_api_errors is off"),
                );
            }
            // A REPLY CUT OFF (aterm's `(ApiCause::CutOff, _)`): continued at once, then on
            // the network's short ladder from the appearance that answered the last try. The
            // server's own failure climbs the server's ladder from its first rung.
            let local_at = if *cut_off {
                attempts
                    .checked_sub(1)
                    .map_or(since, |n| later(since, ladder(&t.net_backoff, n)))
            } else {
                later(since, ladder(&t.retry_backoff, attempts))
            };
            let at = retry_at.map_or(local_at, |provider_at| provider_at.max(local_at));
            let rule = if *cut_off {
                words::RULE_API_CUTOFF
            } else {
                words::RULE_API_RETRY
            };
            let why = format!("Codex reported {what}; retrying");
            Decision {
                act: Act::Type {
                    rule,
                    text: words::with_rules(
                        &words::wall_retry_text(
                            what,
                            if *cut_off {
                                ApiCause::CutOff
                            } else {
                                ApiCause::Server
                            },
                        ),
                        standing_rules(p),
                    ),
                },
                not_before: Some(at),
                why: why.clone(),
                shown_wait: Some(why),
                repo_owed: None,
                in_record: None,
                held: None,
                look_again: None,
                refused: false,
                whole: None,
                behind: None,
            }
        }
        Wall::Network { what } => {
            if !p.retry_api_errors {
                return Decision::attention(
                    "api",
                    format!("api: Codex reported {what}; retry_api_errors is off"),
                );
            }
            // The try typed blind: the retry's words, which claim nothing of the API's reach.
            // Every wait below types these, and the record carries them in a measured
            // return's place while a check holds it back, so it carries no measure past this
            // process ([`carried_rule`]), and the look at the wait's end decides afresh.
            let blind = Act::Type {
                rule: words::RULE_API_RETRY,
                text: words::with_rules(
                    &words::wall_retry_text(what, ApiCause::Unreachable),
                    standing_rules(p),
                ),
            };
            let retry = |at: Instant, why: String| Decision {
                act: blind.clone(),
                not_before: Some(at),
                why: why.clone(),
                shown_wait: Some(why),
                repo_owed: None,
                in_record: None,
                held: None,
                look_again: None,
                refused: false,
                whole: None,
                behind: None,
            };
            // aterm's `probe_api` off ([`Policy::probe_api`]): nothing is measured, and a
            // measure read before it went off is no measure either.
            let reach = if p.probe_api { s.reach } else { Reach::Unknown };
            match reach {
                // MEASURED REACHABLE AGAIN (aterm's `(Unreachable, Up)`): continued at once,
                // then spaced on the short ladder by this measure's own acts (meeting the wall
                // again means the measure was wrong or the API still fails). Only a fresh
                // verified reach types these words.
                Reach::Up { .. } => {
                    let ups = track.map_or(0, |w| w.ups);
                    let due = ups
                        .checked_sub(1)
                        .map_or(since, |n| later(since, ladder(&t.net_backoff, n)));
                    if due > now {
                        return retry(
                            due,
                            format!(
                                "Codex reported {what}; the API is reachable again but met \
                                 again: retry {} at its rung",
                                ups + 1
                            ),
                        );
                    }
                    Decision {
                        act: Act::Type {
                            rule: words::RULE_API_BACK,
                            text: words::with_rules(&words::api_back_text(what), standing_rules(p)),
                        },
                        not_before: None,
                        why: format!("Codex reported {what}; the API is reachable again"),
                        shown_wait: None,
                        repo_owed: None,
                        in_record: Some(blind),
                        held: None,
                        look_again: None,
                        refused: false,
                        whole: None,
                        behind: None,
                    }
                }
                // MEASURED DEFINITELY DOWN (aterm's `(Unreachable, Down)`): nothing typed into
                // it for up to the hold, then one try all the same; a measured return before
                // then is its own act.
                Reach::Down { .. } => retry(
                    later(since, t.down_hold),
                    format!(
                        "Codex reported {what}; the API is unreachable; continuing when it is \
                         reachable, or at this try"
                    ),
                ),
                Reach::Unknown => retry(
                    later(since, ladder(&t.net_backoff, attempts)),
                    format!("Codex reported {what}; retrying"),
                ),
            }
        }
        Wall::Overlong { what } => {
            if !p.retry_api_errors {
                return Decision::attention(
                    "api",
                    format!("api: Codex reported {what}; retry_api_errors is off"),
                );
            }
            // aterm's `(ApiCause::Overlong, _)`: continued at once, then on the server's
            // ladder from the appearance that answered the last try ([`WallTrack::since`]),
            // as every class counts it: the long reply between them outlasts the ladder's
            // first rungs, and the ladder is what bounds the cost.
            let at = attempts
                .checked_sub(1)
                .map(|n| later(since, ladder(&t.retry_backoff, n)));
            let why = format!("Codex reported {what}; retrying in smaller pieces");
            Decision {
                act: Act::Type {
                    rule: words::RULE_API_CUTOFF,
                    text: words::with_rules(
                        &words::wall_retry_text(what, ApiCause::Overlong),
                        standing_rules(p),
                    ),
                },
                not_before: at,
                shown_wait: at.map(|_| why.clone()),
                repo_owed: None,
                in_record: None,
                held: None,
                look_again: None,
                refused: false,
                whole: None,
                behind: None,
                why,
            }
        }
    }
}

/// What a usage wall's resume types (aterm's `continue_as` over `cleared_of`, since aterm
/// `77161d6432`, v6 §14.17.5 slice 12a; owner rule 2026-10-25: "a prompt into a turn the agent
/// did not end is spam"): THE ONE CLEARED LINE ([`words::limit_cleared`], *has cleared* where
/// `clearing` was measured, *may have cleared* on a blind try) with the standing rules, under
/// Cruise and under Land once the episode had Land's instruction; Cruise's continuation and
/// Land's nudge wait for the agent's own next turn end, and the prompt the limit cut is judged
/// as that prompt's ([`ThreadState::cut`] stays). Under Land before the instruction, the
/// instruction (with the reversible clause where a need owing its ack names an irreversible
/// act, and the ack), the one prompt, never a line and then it ([`ThreadState::acted`] marks
/// that ack given; the cleared line carries none, so it stays owed); but where the limit cut a
/// person's own exchange, which Land leaves to them ([`ThreadState::exchange_at_wall`], aterm
/// c4a86d8e97), the cleared line, so their request goes on and the instruction waits for the
/// agent's own turn end ([`ThreadState::acted`] gives no instruction for it). Cruise's line
/// still needs aterm's `continue_text` (a reply that sends none limits acting, CONTRACT §3).
fn resume_text(s: &ThreadState, p: &Policy, clearing: words::Clearing) -> Option<String> {
    let cleared = || words::with_rules(words::limit_cleared(clearing), standing_rules(p));
    match p.autopilot {
        Mode::Cruise => p.continue_text.as_ref().map(|_| cleared()),
        Mode::Land if s.land.typed || s.exchange_at_wall() => Some(cleared()),
        Mode::Land => Some(words::act_words(
            &p.texts.land,
            &[reversible(s, false), ack(s, p)],
        )),
    }
}
