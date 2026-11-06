// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Runtime coverage of held provider operations and quiet Land diagnostics.

use super::*;
use crate::decide::Mode as Word;
use crate::replies::Kept;
use crate::replies::Replies;
use crate::replies::mark_of;
use crate::repo::RepoWork;

const WAIT: &str = "codex-alab-wait";
const RESET: &str = "waiting for the usage reset at 15:00";

fn wait_up(h: &Harness) -> Option<String> {
    h.fake.attention().get(WAIT).cloned()
}

async fn evaluate(h: &Harness) {
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
}

/// The thread's point held as ruling F holds it: the newest reply kept is the point's, its
/// three tries spent at a wall (or, at no wall, its two no-work answers settled to it).
fn hold_point(h: &Harness) {
    h.rt.with_state(|s| {
        let end = s.point.clone().unwrap_or_else(|| panic!("a point"));
        let walled = end.wall.is_some();
        // The answer counted at the point, and the reset it showed (a usage window's).
        s.replies_at = Some(end.at);
        s.replies_reset = match &end.wall {
            Some(Wall::Usage { reset_at, .. }) => *reset_at,
            _ => None,
        };
        s.replies = Replies::of(&[Kept {
            mark: mark_of(&end),
            answers: if walled { 0 } else { 2 },
            walled,
            tries: if walled { 3 } else { 0 },
        }]);
        assert!(s.held_by().is_some(), "{:?}", s.replies);
    });
}

fn needs_sets(h: &Harness) -> usize {
    h.fake
        .requests()
        .iter()
        .filter(|r| r.contains(" meta set attention owner=needs "))
        .count()
}

/// A HELD WALL'S WAIT COMES DOWN: a usage reset an hour off is waited out, its words in
/// aterm's Waiting rows; once the wall's tries are spent at the same answer the decision holds
/// (aterm's `limit_repeats`, a fact for later), so nothing types on its own and the entry
/// comes down with the timer and the act it owed, never raising the asks' slot; the held try
/// is the record's recovery, put back by the held decision itself (each turn's end clears the
/// recovery first). NEGATIVE CONTROL: the same point decided again before its hold keeps the
/// wait, its timer and its owed resume standing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_held_walls_wait_comes_down_and_its_hold_is_never_an_ask() {
    let h = harness(&[("limit_wait", "true")]).await;
    let wall = Wall::Usage {
        reset_at: Some(Instant::now() + Duration::from_secs(3600)),
        reset_label: Some("15:00".to_string()),
    };
    point(&h, ended(None, Some(wall)));
    h.rt.with_state(|s| s.point_turn_id = Some("the-usage-turn".into()));
    evaluate(&h).await;
    wait_until("the reset's wait is shown", || {
        wait_up(&h).as_deref() == Some(RESET)
    })
    .await;
    // NEGATIVE CONTROL: decided again, unheld, the wait stands.
    evaluate(&h).await;
    assert_eq!(wait_up(&h).as_deref(), Some(RESET));
    assert!(h.rt.timer_pending());
    assert!(h.shared.owed_act().is_some(), "the resume is owed");
    hold_point(&h);
    // As a turn's end does before the next decision.
    h.shared.set_recovery(None);
    evaluate(&h).await;
    wait_until("the held wall's wait comes down", || wait_up(&h).is_none()).await;
    assert!(!h.rt.timer_pending());
    assert_eq!(h.shared.owed_act(), None, "the held try is owed nowhere");
    assert_eq!(shown(&h), None, "quiet: never the asks' slot");
    let recovery = h
        .shared
        .recovery()
        .unwrap_or_else(|| panic!("the held wall stays the record's point"));
    assert!(
        matches!(
            recovery.wall,
            codex_alab_core::session::RecoveryWall::Usage {
                reset_at_ms: Some(_),
                ..
            }
        ),
        "{recovery:?}"
    );
    assert!(
        recovery.not_before_ms >= now_ms() + 3_500_000,
        "the held try's time, past the reset: {recovery:?}"
    );
}

/// A HELD QUOTA'S NEED STAYS, RAISED ONCE: a quota wall's need goes up with its first try; held
/// once its tries are spent, the need still stands (only a person lifts a quota) and is not
/// written again; where it does not stand at the held point (the image after a re-exec starts
/// with none of codex-alab's own needs), the held decision raises it again (review of
/// 4c0168cedd, finding 4). NEGATIVE CONTROL: before the first try no need stands, so the one
/// set seen is the try's.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_held_quotas_need_stays_up_and_is_not_raised_again() {
    use crate::decide::QUOTA_NEED;
    let h = harness(&[]).await;
    assert_eq!(needs_up(&h), None);
    point(&h, ended(None, Some(Wall::Entitlement)));
    evaluate(&h).await;
    wait_until("the quota's need is raised", || {
        needs_up(&h).is_some_and(|need| need.contains(QUOTA_NEED))
    })
    .await;
    let sets = needs_sets(&h);
    hold_point(&h);
    evaluate(&h).await;
    evaluate(&h).await;
    assert!(
        needs_up(&h).is_some_and(|need| need.contains(QUOTA_NEED)),
        "{:?}",
        h.fake.attention()
    );
    assert_eq!(needs_sets(&h), sets, "{:?}", h.fake.requests());
    assert_eq!(shown(&h), None, "quiet: never the asks' slot");
    // None of codex-alab's own needs stands, as after a re-exec: the held point raises it.
    h.sup.own_need_for(h.rt.key(), "limit", None).await;
    wait_until("the quota's need is down", || needs_up(&h).is_none()).await;
    evaluate(&h).await;
    wait_until("the held quota's need is raised again", || {
        needs_up(&h).is_some_and(|need| need.contains(QUOTA_NEED))
    })
    .await;
    assert_eq!(
        h.rt.state().held_by(),
        Some(crate::replies::HeldBy::SameReply)
    );
}

const DONE: &str = "The task is finished and verified.";

/// The thread's point a done claim held as ruling F holds it, under Land, over a repository
/// Land's probe read work no remote main has in.
fn held_over_work(h: &Harness) {
    point(h, ended(Some(DONE), None));
    h.rt.with_state(|s| {
        s.observe_mode(Word::Land);
        s.probed(Some(
            RepoWork::of_git("/r", "?? new.rs\0", "", 1, 1, "origin/main").unwrap_or_default(),
        ));
        assert!(s.land.repo.is_some(), "{:?}", s.land);
    });
    hold_point(h);
}

/// Even a repeated Land report over unlanded work creates no attention request.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_land_hold_is_recorded_without_an_attention_request() {
    let h = harness(&[]).await;
    word(&h, "land").await;
    held_over_work(&h);
    evaluate(&h).await;
    assert_eq!(shown(&h), None);
    assert!(
        h.fake
            .notices()
            .iter()
            .any(|line| line.contains(crate::replies::STOPPED_TYPING))
    );
    evaluate(&h).await;
    assert_eq!(shown(&h), None);
}
