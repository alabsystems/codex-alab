// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn a_committed_mutation_returns_its_exact_before_and_after_boundary() {
    let state = Arc::new(State::default());
    let initial = state.mutation_epoch().expect("birth import lease");
    assert!(initial.is_initial());
    let write = state.begin_mutation();
    assert!(!initial.is_current(), "intent revokes before SQL dispatch");
    let in_progress = state.mutation_epoch().expect("in-progress boundary");
    let transition = write
        .commit_mutation()
        .expect("the exact current writer settled");
    assert_eq!(transition.before, initial);
    assert!(transition.after.is_current());
    assert!(
        !in_progress.is_current(),
        "settlement fences queued readers"
    );
    assert!(!transition.after.is_initial());
    assert!(
        state
            .mutation_epoch()
            .is_some_and(|epoch| !epoch.is_initial())
    );
}

#[test]
fn accounting_and_healthy_validation_preserve_the_mutation_epoch() {
    let state = Arc::new(State::default());
    let before = state.mutation_epoch();
    let revision = state.snapshot().revision;
    let mut accounting = state.begin();
    accounting.mark_accounting();
    accounting.finish(true);
    state.begin().finish(true);
    assert!(state.snapshot().revision > revision);
    assert_eq!(state.mutation_epoch(), before);
    let before = before.expect("healthy epoch");
    assert!(before.is_current());
    assert!(before.is_initial());
}

#[test]
fn failed_and_cancelled_mutations_cannot_reopen_the_birth_lease() {
    for cancelled in [false, true] {
        let state = Arc::new(State::default());
        let birth = state.mutation_epoch().expect("birth lease");
        assert!(birth.is_initial());
        let write = state.begin_mutation();
        if cancelled {
            drop(write);
        } else {
            write.finish(false);
        }
        assert!(!birth.is_current());
        assert!(
            state
                .mutation_epoch()
                .is_some_and(|epoch| !epoch.is_initial())
        );
        assert_eq!(state.admission(), Admission::Held);
        assert!(state.begin_mutation().commit_mutation().is_some());
        assert!(state.permits_admission());
        assert!(!birth.is_current());
        assert!(
            state
                .mutation_epoch()
                .is_some_and(|epoch| !epoch.is_initial())
        );
    }
}

#[test]
fn an_unadmitted_overlap_cannot_revoke_the_serialized_writer() {
    let state = Arc::new(State::default());
    let first = state.begin_mutation();
    let overlapping = state.begin_mutation();
    assert!(overlapping.commit_mutation().is_none());
    let first = first
        .commit_mutation()
        .expect("the admitted serialized writer settles");
    assert!(first.after.is_current());
    assert_eq!(state.admission(), Admission::Ready);
    let later = state
        .begin_mutation()
        .commit_mutation()
        .expect("a later serialized explicit mutation settles");
    assert_eq!(later.before, first.after);
    assert!(later.after.is_current());
}

#[test]
fn epochs_belong_to_one_owner_and_do_not_retain_it() {
    let first = Arc::new(State::default());
    let second = Arc::new(State::default());
    let epoch = first.mutation_epoch().expect("first owner epoch");
    assert!(first.mutation_epoch_is_current(&epoch));
    assert!(!second.mutation_epoch_is_current(&epoch));
    drop(first);
    assert!(
        !epoch.is_current(),
        "a token must not retain an obsolete owner"
    );
    assert!(
        second
            .mutation_epoch()
            .expect("second owner epoch")
            .is_current()
    );
}

#[test]
fn a_recording_failure_holds_admission_separately_from_epoch_equality() {
    let state = Arc::new(State::default());
    let epoch = state.mutation_epoch().expect("healthy epoch");
    state.begin().finish(false);
    assert!(epoch.is_current());
    assert_eq!(state.admission(), Admission::Held);
    state.begin().finish(true);
    assert_eq!(state.admission(), Admission::Held);
}

#[test]
fn withdrawn_intent_revocation_cannot_change_write_admission() {
    let state = Arc::new(State::default());
    let epoch = state.mutation_epoch().expect("healthy epoch");
    let mut intent = state.begin_mutation();
    intent.cancel_before_dispatch();
    assert!(!epoch.is_current());
    assert!(
        state
            .mutation_epoch()
            .is_some_and(|epoch| !epoch.is_initial())
    );
    assert_eq!(state.admission(), Admission::Ready);
    assert!(!state.snapshot().writes_pending);
    state.begin().finish(false);
    let held = state
        .mutation_epoch()
        .expect("held owner still has an epoch");
    let mut intent = state.begin_mutation();
    intent.cancel_before_dispatch();
    assert!(!held.is_current());
    assert_eq!(state.admission(), Admission::Held);
    assert!(!state.snapshot().writes_pending);
}

#[test]
fn stale_or_closed_process_custody_cannot_republish_saved_goal_proof() {
    use crate::model_goal_stops::Binding;
    use crate::model_goal_stops::Custody;
    use crate::session::Pace;
    use crate::session::Shared;
    use crate::session::WallGoal;

    let mode_epoch = crate::mode_epoch().expect("initial Enhanced generation");
    for pending in [false, true] {
        let state = Arc::new(State::default());
        let shared = Shared::new();
        let goal = WallGoal {
            goal_id: "original-goal".into(),
            objective: "finish the original task".into(),
            turn: "the-native-stop".into(),
            ..WallGoal::default()
        };
        shared.carry_model_goal("thread", Some(goal.clone()));
        shared.set_pace(
            "thread",
            Pace {
                model_blocked_goal: Some(goal.clone()),
                ..Pace::default()
            },
        );
        let epoch = state.mutation_epoch().expect("live model epoch");
        shared.set_model_goal_custody(
            "thread",
            if pending {
                Custody::Import { epoch, mode_epoch }
            } else {
                Custody::Known(Binding {
                    goal: goal.clone(),
                    epoch,
                    mode_epoch,
                })
            },
        );
        assert_eq!(
            shared
                .pace("thread")
                .and_then(|pace| pace.model_blocked_goal),
            Some(goal)
        );
        assert!(state.begin_mutation().commit_mutation().is_some());
        assert!(
            shared.pace("thread").is_none(),
            "stale local and carried proof must be absent"
        );
        shared.set_pace("thread", Pace::default());
        assert!(
            shared.pace("thread").is_none(),
            "carried overlay cannot recreate stale proof"
        );
        shared.set_model_goal_custody("thread", Custody::Closed);
        assert!(shared.pace("thread").is_none());
    }
}
