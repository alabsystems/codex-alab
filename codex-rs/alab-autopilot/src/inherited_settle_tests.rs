// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Only inherited automatic intent needs an unknown-age awake origin.

use super::InheritedSettle;
use codex_alab_core::wake::Reading;
use codex_alab_core::wake::Wake;
use std::time::Duration;
use std::time::Instant;

const SEC: Duration = Duration::from_secs(1);

fn read(asleep: Duration, wake: Option<Instant>) -> Reading {
    Reading {
        asleep,
        last: wake.map(|at| Wake {
            at,
            slept: SEC,
            seq: 1,
        }),
    }
}

#[test]
fn a_fresh_resume_settles_after_old_sleep_and_restarts_after_later_sleep() {
    let now = Instant::now();
    let base = 7_200 * SEC;
    // Sleep before capture, after accepted commit, or before the first sample
    // has already entered this initial total. It proves no awake age.
    let mut hold = InheritedSettle::new(now, Some(read(base, None)), true).unwrap();
    assert_eq!(hold.observe(now + 10 * SEC, None), now);
    assert_eq!(
        hold.observe(now + 11 * SEC, Some(read(base - SEC, None))),
        now
    );
    assert_eq!(
        hold.observe(now + 12 * SEC, Some(read(base, Some(now - SEC)))),
        now,
        "an older kernel wake cannot shorten the conservative origin"
    );
    let after_sleep = now + 20 * SEC;
    assert_eq!(
        hold.observe(after_sleep, Some(read(base + 100 * SEC, None))),
        after_sleep,
        "a proven later sleep with unknown age restarts settling now"
    );
    assert_eq!(hold.observe(now + 30 * SEC, None), after_sleep);
    assert_eq!(
        hold.observe(now + 31 * SEC, Some(read(base + 100 * SEC, Some(now)))),
        after_sleep
    );
    let recent = now + 40 * SEC;
    assert_eq!(
        hold.observe(now + 45 * SEC, Some(read(base + 200 * SEC, Some(recent)))),
        recent,
        "a newer dated wake keeps its measured awake age"
    );
}

#[test]
fn known_wake_age_and_missing_samples_do_not_create_a_rolling_delay() {
    let now = Instant::now();
    let base = 5_000 * SEC;
    for age in [10 * SEC, 120 * SEC] {
        let woke = now - age;
        let mut hold = InheritedSettle::new(now, Some(read(base, Some(woke))), true).unwrap();
        assert_eq!(hold.observe(now + SEC, None), woke);
        assert_eq!(hold.observe(now + 2 * SEC, Some(read(base, None))), woke);
    }
    let mut unread = InheritedSettle::new(now, None, true).unwrap();
    assert_eq!(unread.observe(now + 5 * SEC, None), now);
    let first = now + 10 * SEC;
    assert_eq!(unread.observe(first, Some(read(base, None))), first);
    assert_eq!(
        unread.observe(now + 20 * SEC, Some(read(base, None))),
        first
    );
}

#[test]
fn a_permanently_unsupported_clock_keeps_its_existing_behavior() {
    let now = Instant::now();
    assert!(InheritedSettle::new(now, None, false).is_none());
    assert!(InheritedSettle::new(now, None, true).is_some());
    assert!(InheritedSettle::new(now, Some(read(SEC, None)), false).is_some());
}
