// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::ADMISSION_RETRY;
use super::ADMISSION_STANDS;
use super::AdmissionRefusals;
use super::BOUND_RECHECK;
use super::Purpose;
use super::RECHECK;
use super::admission_retry;
use super::point_recheck;
use std::time::Duration;
use std::time::Instant;

/// A refused recovered turn goes again a second later, then two, then four, and never later
/// than a bound receipt's own recheck, however many refusals come while the reason is new.
#[test]
fn a_refused_admission_backs_off_from_a_second_to_the_bound_recheck() {
    let secs = Duration::from_secs;
    let waits: Vec<_> = (0..6)
        .map(|refusals| admission_retry(refusals, Duration::ZERO))
        .collect();
    assert_eq!(
        waits,
        [secs(1), secs(2), secs(4), secs(5), secs(5), secs(5)]
    );
    assert_eq!(admission_retry(0, Duration::ZERO), ADMISSION_RETRY);
    assert_eq!(admission_retry(u64::MAX, Duration::ZERO), BOUND_RECHECK);
    assert_eq!(
        admission_retry(u64::MAX, ADMISSION_STANDS - Duration::from_millis(1)),
        BOUND_RECHECK
    );
}

/// A REFUSAL THAT STANDS IS ASKED AGAIN A MINUTE APART (review of the admission retry,
/// finding 3). Each ask has core reserve the turn and write its settings into the rollout
/// before the admission refuses, so one reason refused in a row for a minute is a wait and no
/// longer a fact that moved: from then on the retry is the ordinary recheck, however few
/// refusals it took to get there.
#[test]
fn a_refusal_that_has_stood_a_minute_is_retried_at_the_ordinary_recheck() {
    assert_eq!(admission_retry(0, ADMISSION_STANDS), RECHECK);
    assert_eq!(admission_retry(40, ADMISSION_STANDS * 3), RECHECK);
    assert_ne!(
        admission_retry(40, ADMISSION_STANDS),
        admission_retry(40, Duration::ZERO),
        "a standing refusal is not asked as often as one that is new"
    );
}

/// A series is one walled turn's: each refused admission lengthens the next wait, a start core
/// did not take lengthens nothing, and a reason is news once in a row, so a refusal that
/// stands fills neither the log nor the journal.
#[test]
fn a_series_counts_refusals_and_says_each_reason_once_in_a_row() {
    let mut series = None;
    let at = Instant::now();
    let mut record = |turn: &str, reason: &str, refused| {
        let (before, news, _) =
            AdmissionRefusals::record(&mut series, turn.into(), reason, refused, at);
        (before, news)
    };
    assert_eq!(record("wall", "changed", true), (0, true));
    assert_eq!(record("wall", "changed", true), (1, false));
    assert_eq!(record("wall", "NotIdle", false), (2, true));
    assert_eq!(record("wall", "changed", true), (2, true));
    assert_eq!(record("wall", "receipt changed", true), (3, true));
    assert_eq!(
        record("another wall", "receipt changed", true),
        (0, true),
        "another turn's refusals do not lengthen this one's wait"
    );
}

/// A reason's row is timed from its first refusal: it is said again once, when it has stood
/// long enough for its retries to slow, and another reason (or a start core did not take)
/// starts the row over.
#[test]
fn a_row_is_timed_from_its_first_refusal_and_says_once_that_it_stands() {
    let mut series = None;
    let start = Instant::now();
    let mut record = |reason: &str, refused, after: Duration| {
        let (_, news, stood) =
            AdmissionRefusals::record(&mut series, "wall".into(), reason, refused, start + after);
        (news, stood)
    };
    let secs = Duration::from_secs;
    assert_eq!(record("changed", true, secs(0)), (true, secs(0)));
    assert_eq!(record("changed", true, secs(7)), (false, secs(7)));
    assert_eq!(
        record("changed", true, ADMISSION_STANDS),
        (true, ADMISSION_STANDS),
        "the journal says once more that the retries slow"
    );
    assert_eq!(
        record("changed", true, ADMISSION_STANDS + secs(60)),
        (false, ADMISSION_STANDS + secs(60))
    );
    assert_eq!(
        record("NotIdle", false, ADMISSION_STANDS + secs(61)),
        (true, secs(0)),
        "another reason starts the row over"
    );
    assert_eq!(
        record("NotIdle", false, ADMISSION_STANDS * 3),
        (false, ADMISSION_STANDS * 2 - secs(61)),
        "a start core did not take is said once, however long it stands"
    );
    assert_eq!(
        record("changed", true, ADMISSION_STANDS * 3),
        (true, secs(0))
    );
}

/// A BOUND RECEIPT'S POINT IS LOOKED AT AGAIN WITHIN SECONDS ON EVERY PATH THAT KEEPS IT
/// (review of the admission retry, finding 4): the recheck is decided from the purpose and
/// whether a receipt binds the intent, and the passes that could not read the goal's custody
/// or the thread's history schedule with it as the one that read the host does. Before, those
/// three waited a login's minute under a bound receipt.
#[test]
fn a_bound_receipts_point_is_rechecked_within_seconds_whatever_its_purpose() {
    assert_eq!(point_recheck(Purpose::Login, true), BOUND_RECHECK);
    assert_eq!(point_recheck(Purpose::WeeklyQuota, true), BOUND_RECHECK);
    assert_eq!(point_recheck(Purpose::LoginRotation, true), BOUND_RECHECK);
    assert_eq!(point_recheck(Purpose::Login, false), RECHECK);
    assert_eq!(
        point_recheck(Purpose::WeeklyQuota, false),
        Duration::from_secs(5)
    );
    // A lost login's move is asked about as promptly as a usage wall's: its ask is bounded.
    assert_eq!(
        point_recheck(Purpose::LoginRotation, false),
        Duration::from_secs(5)
    );
}
