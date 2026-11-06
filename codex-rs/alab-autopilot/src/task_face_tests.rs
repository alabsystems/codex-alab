// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! THE LINE'S AUTOPILOT FACTS (gap 12 of the parity sweep, 2026-10-29: a codex tab showed
//! `[Cruise]─[Land]─[Off]` and none of aterm's faces), against alab-aterm's in-process FAKE
//! aterm (never a real one): the primary thread's task face goes to `Shared` as its decider
//! holds it, `Landed` at a landing and `Theirs` at a point left to the person, none while a
//! turn of it runs, none after the landed task's own wake until its kept landing is recorded
//! again (as aterm reads it), none once its runtime stops, and a turn of another thread
//! reopens none; and aterm's `status land_at=` goes there at each of the session loop's reads
//! of the word (the policy's, the word's clock, a hold's while unheld), a failed read keeping
//! the last, and never at an act's own read. Each with its negative control.

use codex_alab_core::session::TaskFace;

use super::land_status_tests::begin;
use super::land_status_tests::evaluate;
use super::land_status_tests::persons_message;
use super::land_status_tests::settled;
use super::land_status_tests::shown_after_land;
use super::land_status_tests::stop;
use super::*;
use crate::decide::Mode as Word;

/// [`settled`] under Land, its task landed by the land act's clean yield and the evaluation
/// that judged it. NEGATIVE CONTROL: the clean defense alone wears no landing.
async fn landed() -> Harness {
    let h = settled().await;
    shown_after_land(&h).await;
    turn(
        &h,
        "the-yield",
        true,
        "Nothing left: the parser and the lexer are merged and pushed to origin/main, and the \
         worktrees are removed.",
    )
    .await;
    assert_eq!(h.shared.task_face(), TaskFace::Open, "not judged yet");
    evaluate(&h).await;
    assert!(h.rt.state().landed, "{:?}", h.sup.journal());
    h
}

/// A LANDING WEARS `✓ landed` ON THE LINE: the evaluation's `Act::Landed` is told, and the
/// primary thread's face reads it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_landing_wears_landed_on_the_line() {
    let h = landed().await;
    assert_eq!(
        h.shared.task_face(),
        TaskFace::Landed,
        "{:?}",
        h.sup.journal()
    );
}

/// The `landed@v1` records the fake took.
fn landings(h: &Harness) -> usize {
    h.fake
        .acts()
        .iter()
        .filter(|act| act.as_str() == crate::words::RULE_LANDED)
        .count()
}

/// A PERSON'S TURN REOPENS THE FACE FOR GOOD; THE LANDED TASK'S OWN WAKE UNTIL ITS LANDING IS
/// RECORDED AGAIN, AS ATERM READS IT (review of gap 12, finding 1): aterm reopens a proven
/// self-claim's landing at any turn begun past its mark, a goal's continuation included, and
/// reads it open until the next `landed@v1`; codex-alab's decider keeps the landing through the
/// wake and records it again once its judgement keeps it, so the line and aterm say the same
/// at every step. NEGATIVE CONTROL: the landing is recorded once, and again only after the
/// wake's judgement; a person's turn records none.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_persons_turn_reopens_the_face() {
    let h = landed().await;
    assert_eq!(landings(&h), 1, "{:?}", h.fake.acts());
    // A goal's own continuation: aterm counts its turn and reopens the landing.
    let goals = ExtensionData::new("the-goals-turn");
    begin(&h, &goals).await;
    assert_eq!(h.shared.task_face(), TaskFace::Open, "while it runs");
    stop(&h, &goals).await;
    assert!(h.rt.state().landed, "{:?}", h.sup.journal());
    // NEGATIVE CONTROL: no `landed@v1` since the wake's turn, so aterm reads it open.
    assert_eq!(landings(&h), 1, "{:?}", h.fake.acts());
    assert_eq!(h.shared.task_face(), TaskFace::Open, "judgement owed");
    // The judgement keeps it and records it again: aterm reads it landed, as the line does.
    evaluate(&h).await;
    assert_eq!(landings(&h), 2, "{:?}", h.sup.journal());
    assert_eq!(h.shared.task_face(), TaskFace::Landed, "recorded again");
    evaluate(&h).await;
    assert_eq!(landings(&h), 2, "once: {:?}", h.fake.acts());
    // A person's turn ends the landing at its message.
    let persons = ExtensionData::new("the-persons-turn");
    begin(&h, &persons).await;
    assert_eq!(h.shared.task_face(), TaskFace::Open, "while it runs");
    persons_message(&h, &persons).await;
    assert!(!h.rt.state().landed);
    stop(&h, &persons).await;
    assert_eq!(
        h.shared.task_face(),
        TaskFace::Open,
        "{:?}",
        h.sup.journal()
    );
    assert_eq!(landings(&h), 2, "{:?}", h.fake.acts());
}

/// ANOTHER THREAD'S TURN REOPENS NOTHING OF THE PRIMARY THREAD'S (review of gap 12, finding
/// 16): the face reads a turn of its own thread, so a second root's turn, running while the
/// primary thread's landing is told, leaves `✓ landed`. NEGATIVE CONTROL: a turn of the
/// primary thread reopens it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn another_threads_turn_reopens_nothing_of_the_primarys() {
    let h = landed().await;
    let other = ThreadId::new().to_string();
    h.shared.note_root_thread(&other);
    assert!(h.shared.turn_started(&other, "the-other-turn"));
    assert!(h.shared.turn_running());
    h.autopilot.tell_land(&h.rt);
    assert_eq!(
        h.shared.task_face(),
        TaskFace::Landed,
        "{:?}",
        h.sup.journal()
    );
    // NEGATIVE CONTROL.
    let own = ExtensionData::new("the-primarys-turn");
    begin(&h, &own).await;
    assert_eq!(h.shared.task_face(), TaskFace::Open);
}

/// A POINT LEFT TO THE PERSON IS `⤓ quiet` UNTIL A TURN BEGINS: Land's opening exchange ends
/// theirs, as the turn's end tells it, and the next turn's start reopens the face though the
/// memory keeps the point left until that turn's end. NEGATIVE CONTROL: the memory still
/// holds the point left while the turn runs.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_point_left_to_the_person_is_quiet_until_a_turn_begins() {
    let h = settled().await;
    h.rt.with_state(|state| {
        let end = TurnEnd {
            worked: Duration::ZERO,
            ..ended(Some("Here is the answer."), None)
        };
        state.ended_under = Some(Word::Land);
        state.observe_person_hand(end.at);
        state.observe_turn_end(end, &Timing::default());
    });
    assert!(h.rt.state().left);
    // As the turn's end tells it.
    h.autopilot.tell_land(&h.rt);
    assert_eq!(h.shared.task_face(), TaskFace::Theirs);
    let next = ExtensionData::new("the-next-turn");
    begin(&h, &next).await;
    // NEGATIVE CONTROL.
    assert!(h.rt.state().left, "kept until the turn's end");
    assert_eq!(h.shared.task_face(), TaskFace::Open);
}

/// THE LINE READS ATERM'S LAND TIMER at every read of the word, as the `meta` event that moves
/// it makes the supervisor read it: armed, then cancelled. NEGATIVE CONTROL: a read that fails
/// keeps the timer last read, as it keeps the word.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_line_reads_atermss_land_timer() {
    let h = harness(&[]).await;
    assert_eq!(h.shared.land_at(), None);
    h.fake.set_status("land_at", "1767000000");
    h.fake.emit("meta field=autopilot value=cruise");
    wait_until("the timer is read", || {
        h.shared.land_at() == Some(1_767_000_000)
    })
    .await;
    // NEGATIVE CONTROL: a read that fails.
    h.fake.answer_status_with(Some("ERR status unavailable"));
    h.fake.set_status("land_at", "-");
    let reads = noted(&h, "re-read");
    h.fake.emit("meta field=autopilot value=cruise");
    wait_until("the policy is read again", || noted(&h, "re-read") > reads).await;
    assert_eq!(h.shared.land_at(), Some(1_767_000_000), "kept");
    h.fake.answer_status_with(None);
    h.fake.emit("meta field=autopilot value=cruise");
    wait_until("the cancel is read", || h.shared.land_at().is_none()).await;
}

/// THE WORD'S CLOCK AND A HOLD'S READ WHILE UNHELD READ ATERM'S TIMER TOO (review of gap 12,
/// finding 15): no `meta` event comes, and the reads the session loop makes on its own take the
/// timer, armed and then cancelled. NEGATIVE CONTROL: before either read meets it, none.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_words_clock_and_an_unheld_holds_read_take_the_timer() {
    // The word's clock: the claim's read named no word, so `status` is read again.
    let h = started(&[], |fake, sup| {
        fake.set_status("autopilot", "-");
        sup.set_word_recheck(Duration::from_millis(20));
    })
    .await;
    wait_until("the claim is proven and acted under", || h.sup.acting()).await;
    // NEGATIVE CONTROL.
    assert_eq!(h.shared.land_at(), None);
    h.fake.set_status("land_at", "1767000000");
    wait_until("the word's clock reads the timer", || {
        h.shared.land_at() == Some(1_767_000_000)
    })
    .await;
    h.fake.set_status("land_at", "-");
    wait_until("the word's clock reads the cancel", || {
        h.shared.land_at().is_none()
    })
    .await;
    // A person's hold before any claim: the read that asks whether it holds over an Off.
    let h = started(&[], |fake, _| {
        fake.set_status("hold", "1");
        fake.set_effective(false, "hold");
        fake.set_status("land_at", "1767000000");
    })
    .await;
    wait_until("the unheld hold's read takes the timer", || {
        h.shared.land_at() == Some(1_767_000_000)
    })
    .await;
    assert!(!h.sup.held(), "{:?}", h.sup.journal());
    h.fake.set_status("land_at", "-");
    h.fake.emit("meta field=autopilot value=cruise");
    wait_until("the unheld hold's read takes the cancel", || {
        h.shared.land_at().is_none()
    })
    .await;
}

/// AN ACT'S OWN READ OF THE WORD PUBLISHES NO TIMER (review of gap 12, finding 6): it runs on
/// the thread's task beside the session loop's reads, so a reply it took before a refresh's
/// could store a timer the refresh saw cancelled; the session loop's reads alone take it.
/// NEGATIVE CONTROL: the policy's read at the `meta` event takes the same timer.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_acts_own_read_publishes_no_timer() {
    let h = started(&[], |fake, sup| {
        fake.set_status("autopilot", "cruise");
        sup.set_word_recheck(Duration::from_secs(3600));
    })
    .await;
    wait_until("the claim is proven and acted under", || h.sup.acting()).await;
    h.fake.set_status("land_at", "1767000000");
    assert_eq!(h.sup.ready().await, Ready::Go, "{:?}", h.sup.journal());
    assert_eq!(h.shared.land_at(), None);
    // NEGATIVE CONTROL.
    h.fake.emit("meta field=autopilot value=cruise");
    wait_until("the policy's read takes the timer", || {
        h.shared.land_at() == Some(1_767_000_000)
    })
    .await;
}

/// A STOPPED RUNTIME TAKES ITS FACE DOWN: nothing stands for a stopped thread, so the landing's
/// face goes at its runtime's stop, and a late tell of that runtime wears none again.
/// NEGATIVE CONTROL: the face stands until the stop.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stopped_runtime_takes_its_face_down() {
    use codex_extension_api::ThreadLifecycleContributor;
    use codex_extension_api::ThreadStopInput;
    let h = landed().await;
    // NEGATIVE CONTROL.
    assert_eq!(h.shared.task_face(), TaskFace::Landed);
    h.autopilot
        .on_thread_stop(ThreadStopInput {
            session_store: &ExtensionData::new("session"),
            thread_store: &h._store,
        })
        .await;
    assert_eq!(h.shared.task_face(), TaskFace::Open);
    h.autopilot.tell_land(&h.rt);
    assert_eq!(
        h.shared.task_face(),
        TaskFace::Open,
        "a late tell wears none"
    );
}
