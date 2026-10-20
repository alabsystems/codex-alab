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
        unresumable: None,
        threads: vec![idle("t1"), idle("sub")],
        timers_pending: false,
        left_to_person: None,
    }
}

#[test]
fn every_precondition_holding_is_go() {
    assert_eq!(check(&due()), UpgradeCheck::Go);
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
                timers_pending: true,
                ..due()
            },
            "wait is pending",
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
