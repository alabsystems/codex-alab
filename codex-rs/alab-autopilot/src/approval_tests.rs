// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The approval UNLESS list: the base case approves, and each clause alone defers (its
//! negative control).

use codex_execpolicy::Policy as ExecPolicy;
use codex_execpolicy::PolicyParser;
use codex_protocol::config_types::ApprovalsReviewer;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::GranularApprovalConfig;
use pretty_assertions::assert_eq;

use crate::approval::ApprovalFacts;
use crate::approval::Defer;
use crate::approval::RulePrompt;
use crate::approval::action_command;
use crate::approval::review;
use crate::approval::rule_prompt;

fn base() -> ApprovalFacts {
    ApprovalFacts {
        approve_all: true,
        require_guardian: false,
        require_fresh_review: false,
        policy: AskForApproval::OnRequest,
        reviewer: ApprovalsReviewer::User,
        reviewer_from_defaults: true,
        requirements_fix_policy: false,
        requirements_fix_reviewer: false,
        tool: Some("exec_command".to_string()),
        rule_prompt: RulePrompt::No,
        purchase: false,
    }
}

fn granular(flag: &str, on: bool) -> AskForApproval {
    let mut config = GranularApprovalConfig {
        sandbox_approval: true,
        rules: true,
        skill_approval: true,
        request_permissions: true,
        mcp_elicitations: true,
    };
    match flag {
        "sandbox_approval" => config.sandbox_approval = on,
        "rules" => config.rules = on,
        "request_permissions" => config.request_permissions = on,
        "mcp_elicitations" => config.mcp_elicitations = on,
        _ => {}
    }
    AskForApproval::Granular(config)
}

#[test]
fn the_base_case_approves_once() {
    assert_eq!(review(&base()), Ok(()));
    // An explicit on-request is not an ask rule; neither is a user-chosen User reviewer.
    let explicit = ApprovalFacts {
        reviewer_from_defaults: false,
        ..base()
    };
    assert_eq!(review(&explicit), Ok(()));
    // Auto review from the packaged defaults is not the owner's choice.
    let packaged_auto = ApprovalFacts {
        reviewer: ApprovalsReviewer::AutoReview,
        ..base()
    };
    assert_eq!(review(&packaged_auto), Ok(()));
}

#[test]
fn a_fresh_review_under_the_user_reviewer_is_approved_once() {
    // An escalated exec (`require_escalated`) or a retry sets `require_fresh_review`; with a
    // person as the reviewer codex-alab's one-shot answer is that fresh review.
    let escalated = ApprovalFacts {
        require_fresh_review: true,
        ..base()
    };
    assert_eq!(review(&escalated), Ok(()));
    // Negative control: under the auto reviewer it is the guardian's synchronous review.
    let auto = ApprovalFacts {
        reviewer: ApprovalsReviewer::AutoReview,
        ..escalated
    };
    assert_eq!(review(&auto), Err(Defer::RequireFreshReview));
}

#[test]
fn each_unless_clause_defers_alone() {
    let cases: Vec<(ApprovalFacts, Defer)> = vec![
        (
            ApprovalFacts {
                approve_all: false,
                ..base()
            },
            Defer::ApproveOff,
        ),
        (
            ApprovalFacts {
                require_guardian: true,
                ..base()
            },
            Defer::RequireGuardian,
        ),
        (
            ApprovalFacts {
                require_fresh_review: true,
                reviewer: ApprovalsReviewer::AutoReview,
                ..base()
            },
            Defer::RequireFreshReview,
        ),
        (
            ApprovalFacts {
                policy: AskForApproval::UnlessTrusted,
                ..base()
            },
            Defer::UnlessTrusted,
        ),
        (
            ApprovalFacts {
                reviewer: ApprovalsReviewer::AutoReview,
                reviewer_from_defaults: false,
                ..base()
            },
            Defer::OwnerReviewer,
        ),
        (
            ApprovalFacts {
                requirements_fix_policy: true,
                ..base()
            },
            Defer::RequirementsPolicy,
        ),
        (
            ApprovalFacts {
                requirements_fix_reviewer: true,
                ..base()
            },
            Defer::RequirementsReviewer,
        ),
        (
            ApprovalFacts {
                rule_prompt: RulePrompt::Yes,
                ..base()
            },
            Defer::RulePrompt,
        ),
        (
            ApprovalFacts {
                rule_prompt: RulePrompt::Unparsed,
                ..base()
            },
            Defer::UnparsedShell,
        ),
        (
            ApprovalFacts {
                policy: granular("sandbox_approval", false),
                ..base()
            },
            Defer::GranularOff("sandbox_approval"),
        ),
        (
            ApprovalFacts {
                policy: granular("mcp_elicitations", false),
                tool: Some("mcp_tool_call".to_string()),
                ..base()
            },
            Defer::GranularOff("mcp_elicitations"),
        ),
        (
            ApprovalFacts {
                tool: Some("request_permissions".to_string()),
                ..base()
            },
            Defer::StandingGrant,
        ),
        (
            ApprovalFacts {
                tool: Some("mcp_tool_call".to_string()),
                purchase: true,
                ..base()
            },
            Defer::Purchase,
        ),
        (
            ApprovalFacts {
                policy: granular("", true),
                tool: Some("a_future_kind".to_string()),
                ..base()
            },
            Defer::UnknownKind,
        ),
    ];
    for (facts, want) in cases {
        assert_eq!(review(&facts), Err(want.clone()), "{facts:?}");
    }
    // Negative controls for granular: its flag on approves.
    for (flag, tool) in [
        ("sandbox_approval", "exec_command"),
        ("mcp_elicitations", "mcp_tool_call"),
    ] {
        let facts = ApprovalFacts {
            policy: granular(flag, true),
            tool: Some(tool.to_string()),
            ..base()
        };
        assert_eq!(review(&facts), Ok(()), "{flag}");
    }
}

fn command(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| (*w).to_string()).collect()
}

#[test]
fn owner_prompt_rules_defer_and_heuristics_do_not() {
    let mut parser = PolicyParser::new();
    parser
        .parse(
            "owner.rules",
            r#"prefix_rule(pattern=["git", "push"], decision="prompt")
prefix_rule(pattern=["ls"], decision="allow")"#,
        )
        .expect("rules parse");
    let policy = parser.build();
    assert_eq!(
        rule_prompt(&policy, &command(&["git", "push", "origin", "main"])),
        RulePrompt::Yes
    );
    assert_eq!(
        rule_prompt(&policy, &command(&["ls", "-la"])),
        RulePrompt::No
    );
    assert_eq!(
        rule_prompt(&policy, &command(&["cargo", "build"])),
        RulePrompt::No
    );
    // A shell script is split as core splits it: a prompting segment defers.
    assert_eq!(
        rule_prompt(
            &policy,
            &command(&["bash", "-lc", "ls && git push origin main"])
        ),
        RulePrompt::Yes
    );
    assert_eq!(
        rule_prompt(&policy, &command(&["bash", "-lc", "ls -la"])),
        RulePrompt::No
    );
    // A script it cannot split defers.
    assert_eq!(
        rule_prompt(
            &policy,
            &command(&["bash", "-lc", "for i in $(seq 3); do echo $i; done"])
        ),
        RulePrompt::Unparsed
    );
}

#[test]
fn a_script_it_cannot_split_defers_only_where_a_rule_could_ask() {
    let script = command(&["bash", "-lc", "echo approved-run >> ran.txt; echo done"]);
    // No rules at all (and only allow rules): nothing hidden in the script can meet one.
    assert_eq!(rule_prompt(&ExecPolicy::empty(), &script), RulePrompt::No);
    let mut parser = PolicyParser::new();
    parser
        .parse(
            "owner.rules",
            r#"prefix_rule(pattern=["ls"], decision="allow")"#,
        )
        .expect("rules parse");
    assert_eq!(rule_prompt(&parser.build(), &script), RulePrompt::No);
    // Negative control: a rule that asks makes the unsplit script a person's.
    let mut parser = PolicyParser::new();
    parser
        .parse(
            "owner.rules",
            r#"prefix_rule(pattern=["git", "push"], decision="prompt")"#,
        )
        .expect("rules parse");
    assert_eq!(rule_prompt(&parser.build(), &script), RulePrompt::Unparsed);
}

#[test]
fn the_command_comes_from_an_exec_box_only() {
    let exec = serde_json::json!({"tool": "exec_command", "command": ["ls", "-la"]});
    assert_eq!(action_command(&exec), Some(command(&["ls", "-la"])));
    let patch = serde_json::json!({"tool": "apply_patch", "patch": "..."});
    assert_eq!(action_command(&patch), None);
}

#[test]
fn a_purchase_reads_from_the_calls_names_or_argument_keys() {
    use crate::approval::purchase_like;
    let call = |extra: serde_json::Value| {
        let mut call = serde_json::json!({
            "tool": "mcp_tool_call",
            "server": "files",
            "tool_name": "read_file",
            "arguments": {"path": "/tmp/a"},
        });
        if let (Some(call), Some(extra)) = (call.as_object_mut(), extra.as_object()) {
            for (key, value) in extra {
                call.insert(key.clone(), value.clone());
            }
        }
        call
    };
    assert!(!purchase_like(&call(serde_json::json!({}))));
    assert!(purchase_like(&call(
        serde_json::json!({"tool_name": "create_checkout_session"})
    )));
    assert!(purchase_like(&call(
        serde_json::json!({"server": "stripe", "tool_title": "Pay an invoice"})
    )));
    assert!(purchase_like(&call(
        serde_json::json!({"arguments": {"amount": 20, "to": "x"}})
    )));
    assert!(purchase_like(&call(
        serde_json::json!({"tool_description": "Buy credits for the account"})
    )));
    // Only an MCP call is read so: a shell box is the execpolicy's.
    assert!(!purchase_like(
        &serde_json::json!({"tool": "exec_command", "command": ["buy"]})
    ));
}

#[test]
fn only_a_packaged_default_reviewer_is_not_the_owners_choice() {
    use crate::approval::reviewer_from_packaged_defaults;
    use codex_config::AbsolutePathBuf;
    use codex_config::ConfigLayerMetadata;
    use codex_config::ConfigLayerSource;
    let packaged = ConfigLayerMetadata {
        name: ConfigLayerSource::PackagedDefaults {
            file: AbsolutePathBuf::from_absolute_path("/pkg/defaults.toml").expect("absolute path"),
        },
        version: "1".to_string(),
    };
    let flags = ConfigLayerMetadata {
        name: ConfigLayerSource::SessionFlags,
        version: "1".to_string(),
    };
    let auto = ApprovalsReviewer::AutoReview;
    let origins = |rows: &[(&str, &ConfigLayerMetadata)]| {
        rows.iter()
            .map(|(key, meta)| ((*key).to_string(), (*meta).clone()))
            .collect::<std::collections::HashMap<_, _>>()
    };
    assert!(reviewer_from_packaged_defaults(
        &origins(&[("approvals_reviewer", &packaged)]),
        auto,
        auto
    ));
    // A profile's (or a per-server table's) reviewer is the owner's, whatever the top says.
    assert!(!reviewer_from_packaged_defaults(
        &origins(&[
            ("approvals_reviewer", &packaged),
            ("profiles.work.approvals_reviewer", &flags)
        ]),
        auto,
        auto
    ));
    assert!(!reviewer_from_packaged_defaults(
        &origins(&[("approvals_reviewer", &flags)]),
        auto,
        auto
    ));
    // No layer names it: its source is unknown, so it is not taken as the defaults'.
    assert!(!reviewer_from_packaged_defaults(&origins(&[]), auto, auto));
    // Picked at runtime (the thread started with another): the owner's.
    assert!(!reviewer_from_packaged_defaults(
        &origins(&[("approvals_reviewer", &packaged)]),
        ApprovalsReviewer::User,
        auto
    ));
}

/// Every `Defer` a box can be left for, by construction: a new variant fails to compile here
/// until the audit below covers it.
fn every_defer() -> Vec<Defer> {
    let all = vec![
        Defer::ApproveOff,
        Defer::RequireGuardian,
        Defer::RequireFreshReview,
        Defer::UnlessTrusted,
        Defer::OwnerReviewer,
        Defer::RequirementsPolicy,
        Defer::RequirementsReviewer,
        Defer::RulePrompt,
        Defer::UnparsedShell,
        Defer::GranularOff("sandbox_approval"),
        Defer::UnknownKind,
        Defer::StandingGrant,
        Defer::Purchase,
    ];
    for d in &all {
        match d {
            Defer::ApproveOff
            | Defer::RequireGuardian
            | Defer::RequireFreshReview
            | Defer::UnlessTrusted
            | Defer::OwnerReviewer
            | Defer::RequirementsPolicy
            | Defer::RequirementsReviewer
            | Defer::RulePrompt
            | Defer::UnparsedShell
            | Defer::GranularOff(_)
            | Defer::UnknownKind
            | Defer::StandingGrant
            | Defer::Purchase => {}
        }
    }
    all
}

/// The attention audit (CONTRACT §5 "left boxes"): an approval box codex-alab leaves while it
/// holds the claim is shown to nobody but by its attention (the claim keeps aterm's own rows
/// quiet), so every reason it leaves one for must reach a person whole, within aterm's 200 B.
#[test]
fn every_left_approval_raises_attention_naming_why() {
    use codex_alab_core::session::BlockedKind;
    for d in every_defer() {
        let words = d.words();
        let text = crate::supervisor::left_box_text(BlockedKind::Permission, Some(&words));
        let sent = codex_alab_aterm::wire::single_line(&text, 200);
        assert!(sent.starts_with("approval: left to a person ("), "{sent}");
        assert!(sent.contains(&words), "{d:?}: {sent}");
    }
    // No reason recorded still raises (a box the contributor never saw).
    assert!(
        crate::supervisor::left_box_text(BlockedKind::Permission, None).contains("did not answer")
    );
    assert!(crate::supervisor::left_box_text(BlockedKind::Question, None).starts_with("question:"));
    assert!(crate::supervisor::left_box_text(BlockedKind::Auth, None).starts_with("login:"));
}

/// A goal codex-alab types nothing past: only one that stopped by itself waits for a person.
#[test]
fn a_goal_that_stopped_by_itself_raises_attention() {
    use codex_state::ThreadGoalStatus;
    for status in [
        ThreadGoalStatus::Active,
        ThreadGoalStatus::Paused,
        ThreadGoalStatus::Blocked,
        ThreadGoalStatus::UsageLimited,
        ThreadGoalStatus::BudgetLimited,
        ThreadGoalStatus::Complete,
    ] {
        let raised = crate::extension::goal_left_to_person(status);
        let expected = matches!(
            status,
            ThreadGoalStatus::Blocked | ThreadGoalStatus::BudgetLimited
        );
        assert_eq!(raised.is_some(), expected, "{status:?}");
        if let Some(text) = raised {
            assert!(text.starts_with("goal: "), "{text}");
        }
    }
}

/// Inbox finding 8: a goal that reads active again at a turn's start (a person's `/goal`
/// resumed it) has answered the `goal:` attention a stopped goal raised. Negative control:
/// a goal still stopped, paused, limited, complete or gone answers nothing.
#[test]
fn only_a_goal_active_again_answers_the_goal_attention() {
    use codex_state::ThreadGoalStatus;
    assert!(crate::extension::goal_answers_attention(Some(
        ThreadGoalStatus::Active
    )));
    for status in [
        ThreadGoalStatus::Paused,
        ThreadGoalStatus::Blocked,
        ThreadGoalStatus::UsageLimited,
        ThreadGoalStatus::BudgetLimited,
        ThreadGoalStatus::Complete,
    ] {
        assert!(
            !crate::extension::goal_answers_attention(Some(status)),
            "{status:?}"
        );
    }
    assert!(!crate::extension::goal_answers_attention(None));
}

/// A turn core would not start: left to a person (and said) unless another turn answers the
/// turn end instead.
#[test]
fn a_refused_submit_raises_attention_unless_another_turn_answers() {
    use codex_protocol::protocol::NonSteerableTurnKind;
    use codex_protocol::turn_input::NotSubmittedReason as R;
    for reason in [
        R::NotIdle,
        R::PendingTriggerTurn,
        R::Superseded,
        R::ServerDraining,
    ] {
        assert_eq!(
            crate::extension::not_submitted_attention(&reason, "x"),
            None,
            "{reason:?}"
        );
    }
    let (key, text) = crate::extension::not_submitted_attention(&R::PlanMode, "x").expect("plan");
    assert_eq!(key, "plan");
    assert!(text.starts_with("plan: "), "{text}");
    for reason in [
        R::NoActiveTurn,
        R::ExpectedTurnMismatch {
            expected: "a".to_string(),
            actual: "b".to_string(),
        },
        R::ActiveTurnNotSteerable {
            turn_kind: NonSteerableTurnKind::Review,
        },
        R::ActiveTurnOutputSchemaMismatch,
        R::EmptyInput,
    ] {
        let (key, text) = crate::extension::not_submitted_attention(&reason, "x").expect("left");
        assert_eq!(key, "turn", "{reason:?}");
        assert!(text.starts_with("turn: "), "{text}");
    }
    // A usage-limited goal whose resume failed is a person's, and says why.
    let text = crate::extension::goal_resume_failed("no state database");
    assert!(text.starts_with("goal: "), "{text}");
    assert!(text.contains("no state database"), "{text}");
}
