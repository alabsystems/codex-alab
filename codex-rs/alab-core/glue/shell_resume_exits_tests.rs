// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn exit(session_id: i32) -> PendingShellExit {
    PendingShellExit {
        invocation: Uuid::new_v4(),
        session_id,
        exit_code: None,
        capture_name: None,
        capture_rejected: false,
        capture_partial: None,
        tail: None,
        launch: None,
    }
}

#[test]
fn retries_refresh_original_facts_and_overflow_excludes_new_invocations() {
    let mut backlog = ResumeBacklog::default();
    let unfinished = HashSet::new();
    let original: Vec<_> = (0..12).map(exit).collect();
    assert_eq!(backlog.batch(original.clone(), &unfinished), original[..8]);

    let mut current = original[1..].to_vec(); // A poll collected the first exit.
    current[0].exit_code = Some(23);
    current.push(exit(0)); // The new process reuses the old shell session number.
    assert_eq!(backlog.batch(current.clone(), &unfinished), current[..8]);

    // After accepting that batch, only the three original overflow entries remain.
    assert_eq!(
        backlog.batch(current[8..].to_vec(), &unfinished),
        original[9..]
    );
    assert!(
        backlog
            .batch(current[11..].to_vec(), &unfinished)
            .is_empty()
    );
}

#[test]
fn initially_empty_resume_stays_drained() {
    let mut backlog = ResumeBacklog::default();
    assert!(backlog.batch(Vec::new(), &HashSet::new()).is_empty());
    assert!(backlog.batch(vec![exit(5)], &HashSet::new()).is_empty());
}

#[test]
fn a_busy_original_publication_stays_eligible_on_the_next_sampling_attempt() {
    let mut backlog = ResumeBacklog::default();
    let original = exit(5);
    let later = exit(5);
    let unfinished = HashSet::from([original.invocation]);
    assert!(backlog.batch(Vec::new(), &unfinished).is_empty());
    assert!(
        backlog
            .batch(vec![original.clone()], &unfinished)
            .is_empty()
    );
    assert_eq!(
        backlog.batch(vec![later, original.clone()], &HashSet::new()),
        vec![original]
    );
}
