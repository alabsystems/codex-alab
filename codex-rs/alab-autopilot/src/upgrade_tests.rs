// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use pretty_assertions::assert_eq;

use crate::upgrade::LoadedThread;
use crate::upgrade::UpgradeCheck;
use crate::upgrade::UpgradeFacts;
use crate::upgrade::check;

fn idle(id: &str) -> LoadedThread {
    LoadedThread {
        id: id.to_string(),
        idle: true,
        background_terminals: 0,
        goal_active: false,
    }
}

fn due() -> UpgradeFacts {
    UpgradeFacts {
        enabled: true,
        other_build: true,
        unconfirmed: None,
        unresumable: None,
        threads: vec![idle("t1"), idle("sub")],
        waits_pending: false,
        carried: 0,
        undecided: None,
        left_to_person: None,
    }
}

#[test]
fn every_precondition_holding_is_go() {
    assert_eq!(check(&due()), UpgradeCheck::Go);
}

/// UNDER `upgrade_ask` THE RE-EXEC WAITS FOR THE AGENT'S READINESS (CONTRACT §5 row "live
/// upgrade", amended 2026-10-28 after the parity sweep, gap 27), its round's words shown.
/// NEGATIVE CONTROLS: readiness in hand (`None`) goes, and the upgrade off is still not due.
#[test]
fn an_unconfirmed_readiness_holds_the_upgrade() {
    let why = "waiting for the agent to say it is ready to restart (notice 1 of 4)";
    let asking = UpgradeFacts {
        unconfirmed: Some(why.to_string()),
        ..due()
    };
    assert_eq!(
        check(&asking),
        UpgradeCheck::Wait(format!("upgrade_ask: {why}"))
    );
    assert_eq!(check(&due()), UpgradeCheck::Go);
    let off = UpgradeFacts {
        enabled: false,
        ..asking
    };
    assert!(matches!(check(&off), UpgradeCheck::NotDue(_)));
}

/// FOREVER CRUISE (aterm owner direction 2026-10-19): a paced act owed on one thread (its
/// back-off pending) crosses the re-exec in the record, so it does not hold the upgrade.
/// NEGATIVE CONTROLS: a wait the record does not carry holds it, and so do acts owed on two
/// threads (the record carries one).
#[test]
fn a_paced_act_owed_does_not_hold_the_upgrade() {
    let one = UpgradeFacts {
        carried: 1,
        ..due()
    };
    assert_eq!(check(&one), UpgradeCheck::Go);
    let wall = UpgradeFacts {
        waits_pending: true,
        carried: 1,
        ..due()
    };
    assert!(matches!(check(&wall), UpgradeCheck::Wait(why) if why.contains("wait is pending")));
    let two = UpgradeFacts {
        carried: 2,
        ..due()
    };
    assert!(matches!(check(&two), UpgradeCheck::Wait(why) if why.contains("2 threads owe")));
}

#[test]
fn off_or_the_same_build_is_not_due() {
    let off = UpgradeFacts {
        enabled: false,
        ..due()
    };
    assert!(matches!(check(&off), UpgradeCheck::NotDue(_)));
    let same = UpgradeFacts {
        other_build: false,
        ..due()
    };
    assert!(matches!(check(&same), UpgradeCheck::NotDue(_)));
}

#[test]
fn each_precondition_alone_holds_the_upgrade() {
    let cases: Vec<(UpgradeFacts, &str)> = vec![
        (
            UpgradeFacts {
                unresumable: Some("the primary thread's rollout is not on disk yet".into()),
                ..due()
            },
            "cannot be resumed",
        ),
        (
            UpgradeFacts {
                threads: vec![
                    idle("t1"),
                    LoadedThread {
                        idle: false,
                        ..idle("sub")
                    },
                ],
                ..due()
            },
            "thread sub is not idle",
        ),
        (
            UpgradeFacts {
                threads: vec![LoadedThread {
                    background_terminals: 2,
                    ..idle("t1")
                }],
                ..due()
            },
            "2 background terminal",
        ),
        (
            UpgradeFacts {
                threads: vec![LoadedThread {
                    goal_active: true,
                    ..idle("t1")
                }],
                ..due()
            },
            "active goal",
        ),
        (
            UpgradeFacts {
                waits_pending: true,
                ..due()
            },
            "wait is pending",
        ),
        (
            UpgradeFacts {
                undecided: Some("t1".into()),
                ..due()
            },
            "thread t1 stands at a point not decided yet",
        ),
        (
            UpgradeFacts {
                left_to_person: Some("question: questions=ask leaves it to a person".into()),
                ..due()
            },
            "left to a person",
        ),
    ];
    for (facts, why) in cases {
        match check(&facts) {
            UpgradeCheck::Wait(said) => assert!(said.contains(why), "{said} / {why}"),
            other => panic!("expected a wait ({why}), got {other:?}"),
        }
    }
}

#[test]
fn a_thread_that_ran_no_turn_here_is_idle_for_the_upgrade() {
    use codex_protocol::protocol::AgentStatus;
    // Right after a re-exec the resumed thread has run nothing in this image.
    assert!(crate::extension::at_rest(&AgentStatus::PendingInit));
    assert!(crate::extension::at_rest(&AgentStatus::Completed(None)));
    // Negative control: a running turn holds the upgrade.
    assert!(!crate::extension::at_rest(&AgentStatus::Running));
}
