// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn a_committed_mutation_returns_its_exact_before_and_after_boundary() {
    let state = Arc::new(State::default());
    let initial = state.initial_mutation_epoch().expect("birth import lease");
    let write = state.begin_mutation();
    assert!(!initial.is_current(), "intent revokes before SQL dispatch");
    assert_eq!(write.previous_mutation_epoch(), Some(initial.clone()));
    let in_progress = state.mutation_epoch();
    let receipt = write
        .commit_mutation_with_receipt()
        .expect("the exact current writer settled");
    assert_eq!(receipt.before, initial);
    assert!(receipt.after.is_current());
    assert!(
        !in_progress.is_current(),
        "settlement fences queued readers"
    );
    assert!(!receipt.after.is_initial());
    assert!(state.initial_mutation_epoch().is_none());
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
    assert!(before.is_current());
    assert!(state.initial_mutation_epoch().is_some());
}

#[test]
fn failed_and_cancelled_mutations_cannot_reopen_the_birth_lease() {
    for cancelled in [false, true] {
        let state = Arc::new(State::default());
        let birth = state.initial_mutation_epoch().expect("birth lease");
        let write = state.begin_mutation();
        if cancelled {
            drop(write);
        } else {
            write.finish(false);
        }
        assert!(!birth.is_current());
        assert!(state.initial_mutation_epoch().is_none());
        assert_eq!(state.admission(), Admission::Held);
        state.begin_mutation().commit_mutation();
        assert!(state.permits_admission());
        assert!(!birth.is_current());
        assert!(state.initial_mutation_epoch().is_none());
    }
}

#[test]
fn a_stale_writer_cannot_return_or_revoke_a_newer_commit_receipt() {
    let state = Arc::new(State::default());
    let stale = state.begin_mutation();
    let current = state
        .begin_mutation()
        .commit_mutation_with_receipt()
        .expect("newer current writer");
    assert!(stale.commit_mutation_with_receipt().is_none());
    assert!(current.after.is_current());
    assert!(state.permits_admission());
    let later = state
        .begin_mutation()
        .commit_mutation_with_receipt()
        .expect("later acknowledged write");
    assert!(!current.after.is_current());
    assert_eq!(later.before, current.after);
    assert!(later.after.is_current());
}

#[test]
fn epochs_belong_to_one_owner_and_do_not_retain_it() {
    let first = Arc::new(State::default());
    let second = Arc::new(State::default());
    let epoch = first.mutation_epoch();
    assert!(epoch.belongs_to(&first));
    assert!(!epoch.belongs_to(&second));
    assert!(!second.epoch_is_current(&epoch));
    drop(first);
    assert!(
        !epoch.is_current(),
        "a token must not retain an obsolete owner"
    );
    assert!(second.mutation_epoch().is_current());
}

#[test]
fn a_recording_failure_holds_admission_separately_from_epoch_equality() {
    let state = Arc::new(State::default());
    let epoch = state.mutation_epoch();
    state.begin().finish(false);
    assert!(epoch.is_current());
    assert_eq!(state.admission(), Admission::Held);
    state.begin().finish(true);
    assert_eq!(state.admission(), Admission::Held);
}

#[test]
fn passive_intent_revocation_cannot_change_vanilla_write_admission() {
    let state = Arc::new(State::default());
    let epoch = state.mutation_epoch();
    state.revoke_mutation_epoch();
    assert!(!epoch.is_current());
    assert!(state.initial_mutation_epoch().is_none());
    assert_eq!(state.admission(), Admission::Ready);
    assert!(!state.snapshot().writes_pending);
    state.begin().finish(false);
    let held = state.mutation_epoch();
    state.revoke_mutation_epoch();
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
        let epoch = state.mutation_epoch();
        shared.set_model_goal_custody(
            "thread",
            if pending {
                Custody::Import(epoch)
            } else {
                Custody::Known(Binding {
                    goal: goal.clone(),
                    epoch,
                })
            },
        );
        assert_eq!(
            shared
                .pace("thread")
                .and_then(|pace| pace.model_blocked_goal),
            Some(goal)
        );
        state.begin_mutation().commit_mutation();
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

#[tokio::test]
async fn a_scoped_resume_requires_its_exact_thread_owner_and_epoch() {
    use crate::model_goal_stops;

    let state = Arc::new(State::default());
    let other = Arc::new(State::default());
    let epoch = state.mutation_epoch();
    assert!(model_goal_stops::resume_unscoped());
    assert!(model_goal_stops::resume_allowed("ordinary-api", &other));
    model_goal_stops::resume_scoped("original-thread".into(), epoch.clone(), async {
        assert!(!model_goal_stops::resume_unscoped());
        assert!(model_goal_stops::resume_allowed("original-thread", &state));
        assert!(!model_goal_stops::resume_allowed("another-thread", &state));
        assert!(!model_goal_stops::resume_allowed("original-thread", &other));
        let validation = state.begin();
        assert_eq!(state.admission(), Admission::Writing);
        assert!(model_goal_stops::resume_allowed("original-thread", &state));
        validation.finish(true);
        state.begin_mutation().commit_mutation();
        assert!(!model_goal_stops::resume_allowed("original-thread", &state));
    })
    .await;
    assert!(model_goal_stops::resume_unscoped());
}
