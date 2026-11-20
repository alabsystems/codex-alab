// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A USAGE WALL'S MOVE REPORTS ITS RESUMED TURN SERVED ON THE DESTINATION (`codex-rotation
//! complete`; `alab/PARALLEL-LOGINS.md` gap 5, step C2). The first session of a pool walled on
//! one account makes the destination the pool's active account, unproven, under aterm's canary
//! hold: every session walled behind it is answered `waiting` until the destination is proven
//! or the hold lapses. This process, the receipt's bound successor, is the one that ran on the
//! destination, so once its receipt's consumption is confirmed it tells the host the resumed
//! turn served, and the host proves the destination for all of them. The report grants this
//! process nothing and changes nothing of its own: a host that takes no such report, or any
//! failure, leaves the hold to lapse as it did before the report existed.

use super::*;
use codex_alab_aterm::rotation::Completion;
use std::future::Future;

/// THE WHOLE BUDGET OF ONE COMPLETION REPORT, its asks again after `waiting` included: exec's
/// report's (`alab-core/glue/exec_handoff_runtime.rs` `complete`).
const COMPLETION_BUDGET: Duration = Duration::from_secs(5);

/// How soon a report the host answered `waiting` (a busy receipt writer, sign-in or auth lock,
/// or a claimed process it could not yet read) is asked again, within [`COMPLETION_BUDGET`].
const COMPLETION_REASK: Duration = Duration::from_millis(250);

/// WHAT SHOWS A USAGE WALL'S RESUMED TURN WAS SERVED ON ITS DESTINATION, and when it ended
/// ([`Autopilot::report_completion`]). Built only from a turn that ended with no error: at its
/// stop from the turn's own facts ([`Served::at_stop`]), or from its durable terminal
/// ([`Served::in_history`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Served {
    /// When the resumed turn ended, in milliseconds of the wall clock since the Unix epoch.
    /// aterm's canary hold runs on the wall clock (`accounts::CANARY_HOLD_S` against Unix
    /// seconds), and so does every age read from this, the host's sleep included: this
    /// process's own clock ([`clock`]) stops while the host sleeps, and the hold runs on.
    ended_ms: u64,
    /// How long the resumed turn ran, where that is known (the report's journal line says it,
    /// so how often a resumed turn outlasts the canary hold can be measured).
    length: Option<Duration>,
}

impl Served {
    /// How old the resumed turn's end is at `now_ms` on the wall clock. A clock set back since
    /// reads the end as just now.
    fn age(&self, now_ms: u64) -> Duration {
        Duration::from_millis(now_ms.saturating_sub(self.ended_ms))
    }

    /// At the turn's stop, before core persists its terminal, from the turn's own facts: served
    /// where the turn met no error at all (`error_seen`, whatever `classify` made of it) and
    /// stands at no wall. Core reports some errors to the turn's lifecycle alone, a post-turn
    /// compaction's usage limit among them, and persists none of those, so the stop knows of
    /// errors its history never shows. It ended at `ended_ms` on the wall clock, and `now` on
    /// this process's clock measures how long it ran.
    fn at_stop(data: &TrackData, ended_ms: u64, now: Instant) -> Option<Self> {
        if data.error_seen || data.wall.is_some() {
            return None;
        }
        Some(Self {
            ended_ms,
            length: data
                .started
                .map(|started| now.saturating_duration_since(started)),
        })
    }

    /// From durable history: `turn`'s own clean terminal ([`crate::resume_history::served`]),
    /// ended at its `completed_at`. That is whole seconds of the wall clock, cut down, so an age
    /// read from it is the true age or up to a second more, never less. A terminal that names
    /// no completion time cannot be aged and shows nothing.
    fn in_history(items: &[RolloutItem], turn: &str) -> Option<Self> {
        let terminal = crate::resume_history::served(items, turn)?;
        let ended_ms = u64::try_from(terminal.completed_at?)
            .ok()?
            .checked_mul(1000)?;
        Some(Self {
            ended_ms,
            length: terminal
                .duration_ms
                .and_then(|ms| u64::try_from(ms).ok())
                .map(Duration::from_millis),
        })
    }
}

/// What a consume confirmation judges the resumed turn by ([`Autopilot::report_completion`]).
pub(super) enum Seen<'a> {
    /// The resumed turn's own stop, which this process is running: core persists its terminal
    /// only once the stop returns, so the stop's facts alone ([`Completions`]'s `stop`).
    Stop,
    /// A pass after the stop (a later follow pass, the retry of a lost acknowledgment, a
    /// restored process's pass): the turn's own clean terminal in this durable history, and,
    /// where this process ran the turn's stop, that stop's facts as well.
    History(&'a [RolloutItem]),
}

/// A THREAD'S COMPLETION REPORTS ([`ThreadRt`]'s `completion`). Memory only.
#[derive(Default)]
pub(crate) struct Completions {
    /// The last turn this process stopped on the thread, and what its stop showed of it
    /// ([`Served::at_stop`], `None` for not served). Every path that later confirms the consume
    /// of a move whose resumed turn it is judges the turn by this as well as by history
    /// ([`Completions::judge`]), so no path reports a turn its own stop found unserved. A later
    /// stop replaces it: a later turn is no resumed turn's, and history shows none then either.
    stop: Option<(String, Option<Served>)>,
    /// The one report sent per receipt: its receipt, and whether the host recorded it. Three
    /// paths confirm a receipt's consumption (the resumed turn's stop, a later pass that finds
    /// its terminal in history, and the retry of a lost acknowledgment), and only the first that
    /// reaches the report sends one. C2 sends one report per receipt whatever the answer; a
    /// report at the resumed turn's first served response (the critique's C3, not built) would
    /// make this one the fallback, sent only where that earlier one was not recorded, which the
    /// second field says.
    report: Option<(String, bool)>,
}

impl Completions {
    /// Turn `turn` stopped, served or not (`served`).
    fn stopped(&mut self, turn: &str, served: Option<Served>) {
        self.stop = Some((turn.to_owned(), served));
    }

    /// WHAT SHOWS `turn` SERVED, as `seen` judges it. At its stop, the stop's facts. From
    /// history, the turn's own clean terminal there, unless this process ran the turn's stop and
    /// found it unserved: core persists no error a turn met outside its terminal (a post-turn
    /// compaction's usage limit), so a terminal that reads clean does not outweigh the stop. A
    /// restored process, which ran no stop of the turn, has history alone.
    fn judge(&self, seen: &Seen<'_>, turn: &str) -> Option<Served> {
        let stop = self
            .stop
            .as_ref()
            .filter(|(stopped, _)| stopped == turn)
            .map(|(_, served)| *served);
        match seen {
            Seen::Stop => stop.flatten(),
            Seen::History(_) if stop == Some(None) => None,
            Seen::History(items) => Served::in_history(items, turn),
        }
    }

    /// Take the report for receipt `id`: false where this thread already sent one for it.
    fn claim(&mut self, id: &str) -> bool {
        if self.report.as_ref().is_some_and(|(sent, _)| sent == id) {
            return false;
        }
        self.report = Some((id.to_owned(), false));
        true
    }

    /// The host recorded receipt `id`'s report, where this still holds it.
    fn recorded(&mut self, id: &str) {
        if let Some((sent, recorded)) = self.report.as_mut()
            && sent == id
        {
            *recorded = true;
        }
    }

    /// The receipt the report was sent for, and whether the host recorded it.
    pub(crate) fn reported(&self) -> Option<(String, bool)> {
        self.report.clone()
    }
}

/// What came of one completion report, as its journal line says it.
#[derive(Debug)]
enum Reported {
    Answered(Result<Completion, codex_alab_aterm::Error>),
    /// The budget ran out while the host still answered `waiting`, or before it answered.
    OutOfBudget {
        waiting: bool,
    },
    /// The resumed turn's end passed the canary hold before an ask.
    Stale,
    /// The claim's connection is gone: there is nothing to send the report on.
    NoClaim,
}

impl Reported {
    /// The journal's words for it. A refusal is its closed word, and a transport failure is
    /// named by its kind alone: the host's line, which may carry an identity, is never kept.
    fn words(&self) -> String {
        const LAPSES: &str = "its canary hold lapses as before";
        match self {
            Self::Answered(Ok(Completion::Recorded)) => {
                "recorded: the destination is proven for every session walled behind it".into()
            }
            Self::Answered(Ok(Completion::Waiting)) | Self::OutOfBudget { waiting: true } => {
                format!(
                    "still waiting after {} s; {LAPSES}",
                    COMPLETION_BUDGET.as_secs()
                )
            }
            Self::Answered(Ok(Completion::Unsupported)) => {
                format!("the host takes no such report; {LAPSES}")
            }
            Self::Answered(Err(codex_alab_aterm::Error::Refused(word))) => {
                format!("refused ({word}); {LAPSES}")
            }
            Self::Answered(Err(error)) => {
                let kind = match error {
                    codex_alab_aterm::Error::Busy { .. } => "busy",
                    codex_alab_aterm::Error::Malformed(_) => "unreadable",
                    codex_alab_aterm::Error::BadRequest(_) => "not sent",
                    _ => "not answered",
                };
                format!("{kind}; {LAPSES}")
            }
            Self::OutOfBudget { waiting: false } => format!(
                "not answered within {} s; {LAPSES}",
                COMPLETION_BUDGET.as_secs()
            ),
            Self::Stale => format!("the resumed turn's end passed the canary hold; {LAPSES}"),
            Self::NoClaim => format!("no claim to send it on; {LAPSES}"),
        }
    }
}

/// What the look before one ask finds ([`ask_completion`]).
enum Look<C> {
    /// The claim's connection, to ask on.
    Ask(C),
    /// The claim's connection is gone.
    NoClaim,
    /// The process is gone: no one is left to ask or to say what came of it.
    Gone,
}

/// THE ASKS OF ONE COMPLETION REPORT, and how many were sent: [`COMPLETION_BUDGET`] in all,
/// `waiting` asked again after [`COMPLETION_REASK`], any other answer or failure ending it.
/// Before every ask the resumed turn's end is aged again on the wall clock `wall` (milliseconds
/// since the Unix epoch), so a host that slept between two asks, which this process's own clock
/// and timers do not count, ends the report once the hold has passed. `look` finds the
/// connection for each ask, and `ask` sends it. `None`: the process is gone.
async fn ask_completion<C, F>(
    served: &Served,
    hold: Duration,
    wall: impl Fn() -> u64,
    mut look: impl FnMut() -> Look<C>,
    mut ask: impl FnMut(C) -> F,
) -> Option<(Reported, u32)>
where
    F: Future<Output = Result<Completion, codex_alab_aterm::Error>>,
{
    let mut asks = 0u32;
    let mut waiting = false;
    let answered = tokio::time::timeout(COMPLETION_BUDGET, async {
        loop {
            if served.age(wall()) > hold {
                return Some(Reported::Stale);
            }
            let client = match look() {
                Look::Ask(client) => client,
                Look::NoClaim => return Some(Reported::NoClaim),
                Look::Gone => return None,
            };
            asks += 1;
            waiting = false;
            match ask(client).await {
                Ok(Completion::Waiting) => {
                    waiting = true;
                    tokio::time::sleep(COMPLETION_REASK).await;
                }
                answer => return Some(Reported::Answered(answer)),
            }
        }
    })
    .await;
    let reported = match answered {
        Ok(reported) => reported?,
        Err(_) => Reported::OutOfBudget { waiting },
    };
    Some((reported, asks))
}

impl Autopilot {
    /// AT EVERY TURN'S STOP (`on_turn_stop`, before anything else there): what the stop shows of
    /// the turn ([`Served::at_stop`]), kept as the thread's last stop ([`Completions`]). A turn
    /// this process holds no facts for shows nothing served.
    pub(crate) fn note_turn_stop(rt: &ThreadRt, turn_store: &ExtensionData) {
        let served = turn_store
            .get::<TurnTrack>()
            .and_then(|track| Served::at_stop(&lock(&track.0), now_ms(), clock()));
        lock(&rt.completion).stopped(turn_store.level_id(), served);
    }

    /// REPORT A USAGE WALL'S RESUMED TURN SERVED, once the receipt's consumption is confirmed:
    /// at the resumed turn's stop, at a later pass that finds its terminal in history, or by the
    /// retry of a lost acknowledgment. Only a bound [`Purpose::WeeklyQuota`] receipt with its
    /// origin (a same-account renewal and a lost login's move put no canary on any account),
    /// only where `seen` shows the turn served ([`Completions::judge`]), only while its end is no
    /// older than the canary hold ([`decide::Timing::canary_hold`]: older, the hold has lapsed
    /// and the pool may since hold the destination under another canary, which this turn
    /// proves nothing of), and once per receipt ([`Completions`]).
    ///
    /// BEST EFFORT, OFF EVERY CALLER'S PATH: spawned, it holds no strong reference over a wait,
    /// and asks as [`ask_completion`] says on the claim's existing connection. It never changes
    /// the owed act, the claim's seal or the schedule, so whatever the host answers, the caller
    /// goes on exactly as it would without the report. One journal line says what came of it
    /// and how long the resumed turn ran.
    pub(super) fn report_completion(&self, rt: &Arc<ThreadRt>, owed: &OwedAct, seen: Seen<'_>) {
        let Some(renewal) = owed
            .renewal
            .as_ref()
            .filter(|renewal| renewal.purpose == Purpose::WeeklyQuota)
        else {
            return;
        };
        let (Some(id), Some(origin)) = (renewal.receipt.clone(), owed.native_origin.as_ref())
        else {
            return;
        };
        let Some(served) = lock(&rt.completion).judge(&seen, &origin.turn_id) else {
            self.sup.note(format!(
                "{}: the usage wall's move was consumed but its resumed turn did not serve on \
                 the destination; no completion is reported",
                rt.key
            ));
            return;
        };
        let hold = self.timing.canary_hold;
        let age = served.age(now_ms());
        if age > hold {
            self.sup.note(format!(
                "{}: the usage wall's move was consumed {} s after its resumed turn ended, past \
                 the host's {} s canary hold; no completion is reported",
                rt.key,
                age.as_secs(),
                hold.as_secs()
            ));
            return;
        }
        if !lock(&rt.completion).claim(&id) {
            return;
        }
        let turn = origin.turn_id.clone();
        // The receipt's own purpose, so `account_recovery::complete`'s guard holds too.
        let purpose = renewal.purpose;
        let key = rt.key.clone();
        let owner = self.me.clone();
        let thread = Arc::downgrade(rt);
        tokio::spawn(async move {
            // A strong owner only for one look, never over an ask or a sleep.
            let look = || match owner.upgrade() {
                Some(me) => me.sup.renewal_ack_client().map_or(Look::NoClaim, Look::Ask),
                None => Look::Gone,
            };
            let ask = |client: codex_alab_aterm::Client| {
                let (id, turn) = (id.clone(), turn.clone());
                async move { account_broker::complete(&client, purpose, &id, &turn).await }
            };
            let Some((reported, asks)) = ask_completion(&served, hold, now_ms, look, ask).await
            else {
                return;
            };
            let Some(me) = owner.upgrade() else {
                return;
            };
            if matches!(reported, Reported::Answered(Ok(Completion::Recorded)))
                && let Some(rt) = thread.upgrade()
            {
                lock(&rt.completion).recorded(&id);
            }
            let length = served.length.map_or_else(
                || "an unknown time".to_owned(),
                |length| format!("{} s", length.as_secs()),
            );
            me.sup.note(format!(
                "{key}: the usage wall's resumed turn served on the destination (it ran {length}); \
                 its completion report, asked {asks} time(s): {}",
                reported.words()
            ));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::Completion;
    use super::Completions;
    use super::Look;
    use super::Reported;
    use super::Seen;
    use super::Served;
    use super::TrackData;
    use super::Wall;
    use super::ask_completion;
    use super::clock;
    use super::lock;
    use crate::Autopilot;
    use crate::Supervisor;
    use crate::SupervisorOptions;
    use crate::decide::Timing;
    use crate::extension::ThreadRt;
    use codex_alab_aterm::Options as ClientOptions;
    use codex_alab_aterm::fake::FakeAterm;
    use codex_alab_aterm::fake::Mode;
    use codex_alab_core::account_handoff::Destination;
    use codex_alab_core::account_handoff::Purpose;
    use codex_alab_core::session::AccountHandoff;
    use codex_alab_core::session::Frame;
    use codex_alab_core::session::NativeOrigin;
    use codex_alab_core::session::NativePhase;
    use codex_alab_core::session::OwedAct;
    use codex_alab_core::session::ProgramState;
    use codex_alab_core::session::Shared;
    use codex_alab_core::session::now_ms;
    use codex_history::RolloutItem;
    use codex_protocol::ThreadId;
    use codex_protocol::protocol::EventMsg;
    use std::sync::Arc;
    use std::sync::atomic::AtomicU32;
    use std::sync::atomic::AtomicU64;
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    /// A RESUMED TURN IS SERVED AT ITS STOP ONLY WHERE IT MET NO ERROR AT ALL: an error that
    /// `classify` names no wall for (an interruption core reports as an error) leaves the turn
    /// at no wall, and is still not served; nor is a turn at a wall. A served turn ended at the
    /// stop's wall-clock moment, and its length runs from its start to its stop.
    #[test]
    fn a_turn_that_met_any_error_is_not_served_at_its_stop() {
        let started = clock();
        let stopped = started + Duration::from_secs(40);
        let clean = || TrackData {
            started: Some(started),
            ..TrackData::default()
        };
        let served = Served::at_stop(&clean(), 1_000, stopped)
            .unwrap_or_else(|| panic!("a clean stop is served"));
        assert_eq!(served.ended_ms, 1_000);
        assert_eq!(served.length, Some(Duration::from_secs(40)));
        assert_eq!(served.age(706_000), Duration::from_secs(705));
        assert_eq!(served.age(0), Duration::ZERO, "a clock set back");
        let error_alone = TrackData {
            error_seen: true,
            ..clean()
        };
        assert!(error_alone.wall.is_none());
        assert!(
            Served::at_stop(&error_alone, 1_000, stopped).is_none(),
            "an error with no wall is not served"
        );
        let at_wall = TrackData {
            wall: Some(Wall::Login),
            ..clean()
        };
        assert!(Served::at_stop(&at_wall, 1_000, stopped).is_none());
    }

    /// ONE REPORT PER RECEIPT, whichever path confirms its consumption first; another receipt
    /// (a later move of the same thread) is reported on its own, and the host's record of a
    /// report stays with the receipt it was for.
    #[test]
    fn a_receipt_is_reported_once_and_another_receipt_on_its_own() {
        let mut slot = Completions::default();
        let (first, second) = ("1".repeat(32), "2".repeat(32));
        assert!(slot.claim(&first), "the first path reports");
        assert!(
            !slot.claim(&first),
            "a second path for the same receipt does not"
        );
        slot.recorded(&second);
        assert_eq!(
            slot.reported(),
            Some((first.clone(), false)),
            "another receipt's record is not this one's"
        );
        slot.recorded(&first);
        assert_eq!(slot.reported(), Some((first, true)));
        assert!(slot.claim(&second), "a later receipt is its own");
        assert!(!slot.claim(&second));
    }

    /// The resumed turn: the walled turn's own id, as core names a turn.
    const TURN: &str = "01a1ded6-9e9a-7250-a071-38b80274dd2d";

    /// `TURN`'s start and its terminal, which ended at `completed_at` (whole seconds of the wall
    /// clock) after 40 s, with or without an error, as core persists them.
    fn history_at(error: bool, completed_at: u64) -> Vec<RolloutItem> {
        let event = |value| {
            RolloutItem::EventMsg(
                serde_json::from_value::<EventMsg>(value)
                    .unwrap_or_else(|err| panic!("a native event: {err}")),
            )
        };
        let mut complete = serde_json::json!({"type":"task_complete", "turn_id":TURN,
            "last_agent_message":"Original work.", "completed_at":completed_at,
            "duration_ms":40_000});
        if error {
            complete["error"] = serde_json::json!({"message":"the provider ended this turn",
                "codex_error_info":"usage_limit_exceeded"});
        }
        vec![
            event(serde_json::json!({"type":"task_started", "turn_id":TURN,
                "model_context_window":258400, "collaboration_mode_kind":"default",
                "turn_attribution": {"turn_id": TURN, "turn_trigger": "user"}})),
            event(complete),
        ]
    }

    /// [`history_at`], the terminal at 1 000 s.
    fn history(error: bool) -> Vec<RolloutItem> {
        history_at(error, 1_000)
    }

    /// THIS PROCESS'S OWN STOP OF THE RESUMED TURN IS JUDGED ON EVERY PATH: a stop that met an
    /// error core never persisted (a post-turn compaction's usage limit) is not outweighed by a
    /// terminal that reads clean. A clean stop still needs history's own clean terminal (a
    /// terminal that carries an error is none), and a restored process, or a stop of another
    /// turn, leaves history alone to judge. At the stop, the stop alone.
    #[test]
    fn this_processs_stop_of_the_resumed_turn_judges_it_beside_history() {
        let clean = history(false);
        let errored = history(true);
        let from_history = Served {
            ended_ms: 1_000_000,
            length: Some(Duration::from_secs(40)),
        };
        let at_stop = Served {
            ended_ms: 1_000_250,
            length: Some(Duration::from_secs(41)),
        };
        let restored = Completions::default();
        assert_eq!(
            restored.judge(&Seen::History(&clean), TURN),
            Some(from_history),
            "a restored process: history alone"
        );
        assert_eq!(restored.judge(&Seen::History(&errored), TURN), None);
        assert_eq!(restored.judge(&Seen::Stop, TURN), None, "no stop, no facts");
        let mut unserved = Completions::default();
        unserved.stopped(TURN, None);
        assert_eq!(
            unserved.judge(&Seen::History(&clean), TURN),
            None,
            "the stop found an error the terminal does not carry"
        );
        assert_eq!(unserved.judge(&Seen::Stop, TURN), None);
        let mut served = Completions::default();
        served.stopped(TURN, Some(at_stop));
        assert_eq!(served.judge(&Seen::Stop, TURN), Some(at_stop));
        assert_eq!(
            served.judge(&Seen::History(&clean), TURN),
            Some(from_history)
        );
        assert_eq!(
            served.judge(&Seen::History(&errored), TURN),
            None,
            "history's own error still counts"
        );
        let mut other = Completions::default();
        other.stopped("another-turn", None);
        assert_eq!(
            other.judge(&Seen::History(&clean), TURN),
            Some(from_history),
            "another turn's stop is not this one's"
        );
    }

    /// A HOST THAT SLEEPS BETWEEN TWO ASKS ENDS THE REPORT ONCE THE HOLD HAS PASSED: the host
    /// answers `waiting`, the wall clock moves twenty minutes while this process's clock and
    /// its pause before the next ask stand still (the lid closed), and the next look finds the
    /// turn's end past the hold, so nothing more is sent. POSITIVE CONTROL: with no sleep, the
    /// second ask goes and is recorded. An end already past the hold is never asked.
    #[tokio::test]
    async fn a_host_sleep_between_two_asks_counts_toward_the_hold() {
        const HOLD: Duration = Duration::from_secs(600);
        let served = Served {
            ended_ms: 1_000_000,
            length: None,
        };
        for (slept, asked) in [(Duration::from_secs(20 * 60), 1), (Duration::ZERO, 2)] {
            let wall = AtomicU64::new(served.ended_ms + 1_000);
            let sent = AtomicU32::new(0);
            let answered = ask_completion(
                &served,
                HOLD,
                || wall.load(Ordering::SeqCst),
                || Look::Ask(()),
                |()| {
                    let first = sent.fetch_add(1, Ordering::SeqCst) == 0;
                    if first {
                        let slept = u64::try_from(slept.as_millis()).unwrap_or(u64::MAX);
                        wall.fetch_add(slept, Ordering::SeqCst);
                    }
                    std::future::ready(Ok(if first {
                        Completion::Waiting
                    } else {
                        Completion::Recorded
                    }))
                },
            )
            .await;
            assert_eq!(sent.load(Ordering::SeqCst), asked, "slept {slept:?}");
            match answered {
                Some((Reported::Stale, 1)) if !slept.is_zero() => {}
                Some((Reported::Answered(Ok(Completion::Recorded)), 2)) if slept.is_zero() => {}
                other => panic!("slept {slept:?}: {other:?}"),
            }
        }
        let wall = served.ended_ms + u64::try_from(HOLD.as_millis()).unwrap_or(u64::MAX) + 1;
        let answered = ask_completion(
            &served,
            HOLD,
            || wall,
            || Look::Ask(()),
            |()| std::future::ready(Ok(Completion::Recorded)),
        )
        .await;
        assert!(
            matches!(answered, Some((Reported::Stale, 0))),
            "{answered:?}"
        );
    }

    /// TWO CONFIRMATIONS OF ONE RECEIPT'S CONSUME ASK THE HOST ONCE. The resumed turn's stop, a
    /// later follow pass and the retry of a lost acknowledgment each report once they confirm
    /// the consume, and two of them do for one receipt where both read the intent before either
    /// released it: a follow pass and the retry, whose reads the claim's one connection answers
    /// in turn. Which goes first is the host's order of answers, which no end-to-end fixture
    /// sets, so here the stop's report and a later pass's for the same receipt are made
    /// directly, against the fake host (which knows no such report): it is asked once, and a
    /// later receipt of the same thread on its own. (`tests/autopilot/rotation_complete.rs`
    /// drives each path end to end.)
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn two_confirmations_of_one_receipt_ask_the_host_once() {
        async fn wait(what: &str, done: impl Fn() -> bool) {
            let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
            while !done() {
                assert!(tokio::time::Instant::now() < deadline, "never: {what}");
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }
        codex_alab_core::activate();
        let dir = tempfile::tempdir().unwrap_or_else(|err| panic!("scratch dir: {err}"));
        let fake = FakeAterm::start(dir.path(), Mode::Current)
            .await
            .unwrap_or_else(|err| panic!("fake aterm: {err}"));
        let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
        let endpoint = fake.endpoint();
        let sup = Supervisor::new(SupervisorOptions {
            shared,
            endpoint: Arc::new(move || Some(endpoint.clone())),
            client: ClientOptions::default(),
        });
        let autopilot = Autopilot::new(Arc::clone(&sup), None, None, Timing::default(), |_| true);
        let rt = Arc::new(ThreadRt::new(ThreadId::new(), None, false, Duration::ZERO));
        autopilot.keep_thread(&rt);
        sup.start();
        shared.observe(
            &Frame {
                primary_thread: Some(&rt.key),
                displayed_thread: Some(&rt.key),
                rollout_path: None,
                cwd: std::path::Path::new("/work"),
                model: "gpt-5",
                codex_home: dir.path(),
                embedded: true,
                state: ProgramState::Idle,
                draft_empty: true,
                queued_input: false,
                popup_open: false,
                editor_open: false,
                pending_user_turn: false,
            },
            now_ms(),
        );
        shared.note_root_thread(&rt.key);
        wait("the claim", || {
            sup.acting() && sup.renewal_ack_client().is_some()
        })
        .await;
        let owed = |receipt: &str| OwedAct {
            thread: rt.key.clone(),
            native_origin: Some(NativeOrigin {
                turn_id: TURN.into(),
                phase: NativePhase::Complete,
            }),
            renewal: Some(AccountHandoff {
                purpose: Purpose::WeeklyQuota,
                destination: Some(Destination {
                    account: "d".repeat(64),
                    user: "e".repeat(64),
                }),
                limit: None,
                openai_scope: true,
                local: false,
                account: "a".repeat(64),
                user: "b".repeat(64),
                receipt: Some(receipt.into()),
                turn: Some(TURN.into()),
                accepted: true,
                starts: 1,
                goal_id: None,
            }),
            ..Default::default()
        };
        let (first, second) = ("c".repeat(32), "f".repeat(32));
        let ended = now_ms();
        lock(&rt.completion).stopped(
            TURN,
            Some(Served {
                ended_ms: ended,
                length: None,
            }),
        );
        let history = history_at(false, ended / 1000);
        let reports = || {
            fake.requests()
                .into_iter()
                .filter(|line| line.contains(" codex-rotation complete "))
                .collect::<Vec<_>>()
        };
        let said = || {
            sup.journal()
                .iter()
                .filter(|line| line.contains("its completion report, asked"))
                .count()
        };
        autopilot.report_completion(&rt, &owed(&first), Seen::Stop);
        autopilot.report_completion(&rt, &owed(&first), Seen::History(&history));
        wait("the report is said", || said() == 1).await;
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert_eq!(reports().len(), 1, "{:#?}", reports());
        assert_eq!(said(), 1, "{:#?}", sup.journal());
        assert_eq!(rt.completion_reported(), Some((first, false)));
        autopilot.report_completion(&rt, &owed(&second), Seen::History(&history));
        wait("a later receipt's report is said", || said() == 2).await;
        let reports = reports();
        assert_eq!(reports.len(), 2, "{reports:#?}");
        assert!(
            reports[1].contains(&format!("id={second} ")),
            "{reports:#?}"
        );
    }
}
