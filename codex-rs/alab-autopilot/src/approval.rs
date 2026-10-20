// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Approvals (CONTRACT §5 "approvals"): `Reviewed(Approved)`, the one-shot allow, under
//! `approve=all`, UNLESS any of the owner's own limits applies, in which case the box goes
//! to a person (`None`: the next contributor, Guardian V2, hands it to the user):
//!
//! 1. `require_guardian`, or `require_fresh_review` while the effective reviewer is
//!    `AutoReview` (core does not enforce either against a `Reviewed` answer, so codex-alab
//!    must). Under the `User` reviewer a fresh review is the box a person would answer, and
//!    codex-alab's one-shot `Reviewed(Approved)` IS that fresh review, as aterm's retired
//!    Codex lane pressed "Yes" once on the same box: an escalated exec (`require_escalated`)
//!    or a retry after a sandbox denial is approved once (end-to-end run 2026-10-16);
//! 2. the effective policy is `UnlessTrusted` (explicit, or derived from an untrusted
//!    project);
//! 3. the effective `approvals_reviewer` is `AutoReview` and not from packaged defaults (the
//!    owner chose a reviewer: in any layer, a profile or a per-server table, or at runtime);
//! 4. a requirements constraint fixed the policy or the reviewer;
//! 5. a non-heuristic execpolicy `prompt` rule matches the command (a shell wrapper this
//!    cannot parse also defers);
//! 6. a `granular` policy whose flag for this kind of box is false;
//! 7. the box is no one-shot: a `request_permissions` grant lasts the turn (a standing grant
//!    is never codex-alab's to give);
//! 8. an MCP call that reads as a purchase or a payment (never codex-alab's to make).
//!
//! An explicit `on-request` is not an ask rule. [`review`] is pure; [`facts`] gathers its
//! inputs from the thread's config.

use codex_execpolicy::Decision as RuleDecision;
use codex_execpolicy::Policy as ExecPolicy;
use codex_execpolicy::RuleMatch;
use codex_protocol::config_types::ApprovalsReviewer;
use codex_protocol::protocol::AskForApproval;

/// Whether an execpolicy rule asks about the command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RulePrompt {
    /// No owner rule prompts (or the box is not a command).
    No,
    /// A non-heuristic rule prompts (or forbids).
    Yes,
    /// The command is a shell wrapper this cannot split: deferred.
    Unparsed,
}

/// Everything [`review`] decides on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalFacts {
    /// `approve=all` (and the claim held, the before-act checks passed).
    pub approve_all: bool,
    pub require_guardian: bool,
    pub require_fresh_review: bool,
    /// The effective policy (`ApprovalDecisionInput::approval_policy`).
    pub policy: AskForApproval,
    pub reviewer: ApprovalsReviewer,
    /// `approvals_reviewer` came from no layer or the packaged defaults.
    pub reviewer_from_defaults: bool,
    /// A requirements layer constrains `approval_policy` or `approvals_reviewer`.
    pub requirements_fix_policy: bool,
    pub requirements_fix_reviewer: bool,
    /// The box's tool (`action["tool"]`: `exec_command`, `apply_patch`, …).
    pub tool: Option<String>,
    pub rule_prompt: RulePrompt,
    /// An MCP call whose names, description or arguments read as a purchase or a payment.
    pub purchase: bool,
}

/// Why a box is left to a person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Defer {
    ApproveOff,
    RequireGuardian,
    RequireFreshReview,
    UnlessTrusted,
    OwnerReviewer,
    RequirementsPolicy,
    RequirementsReviewer,
    RulePrompt,
    UnparsedShell,
    GranularOff(&'static str),
    UnknownKind,
    StandingGrant,
    Purchase,
}

impl Defer {
    /// The reason in words (the left box's attention says it).
    pub fn words(&self) -> String {
        match self {
            Defer::ApproveOff => "approve is not all".to_string(),
            Defer::RequireGuardian => "a guardian review is required".to_string(),
            Defer::RequireFreshReview => "a fresh auto review is required".to_string(),
            Defer::UnlessTrusted => "the approval policy is untrusted".to_string(),
            Defer::OwnerReviewer => "the owner chose auto_review as the reviewer".to_string(),
            Defer::RequirementsPolicy => "a requirement fixes the approval policy".to_string(),
            Defer::RequirementsReviewer => "a requirement fixes the reviewer".to_string(),
            Defer::RulePrompt => "an execpolicy rule asks about this command".to_string(),
            Defer::UnparsedShell => "a shell script the rules check cannot split".to_string(),
            Defer::GranularOff(flag) => format!("granular approval {flag} is off"),
            Defer::UnknownKind => "a box of a kind granular approval does not name".to_string(),
            Defer::StandingGrant => "a permission grant lasts the turn, not one box".to_string(),
            Defer::Purchase => "the call reads as a purchase or a payment".to_string(),
        }
    }
}

/// The granular flag that governs `tool`'s box, or `None` for a kind it does not name.
fn granular_flag(
    config: codex_protocol::protocol::GranularApprovalConfig,
    tool: Option<&str>,
    rule_prompt: RulePrompt,
) -> Option<(&'static str, bool)> {
    match tool? {
        "exec_command" if rule_prompt == RulePrompt::Yes => Some(("rules", config.rules)),
        "exec_command" | "write_stdin" | "apply_patch" | "network_access" => {
            Some(("sandbox_approval", config.sandbox_approval))
        }
        "request_permissions" => Some(("request_permissions", config.request_permissions)),
        "mcp_tool_call" => Some(("mcp_elicitations", config.mcp_elicitations)),
        _ => None,
    }
}

/// The UNLESS list, in order. `Ok(())`: answer `Reviewed(Approved)`.
pub fn review(f: &ApprovalFacts) -> Result<(), Defer> {
    if !f.approve_all {
        return Err(Defer::ApproveOff);
    }
    if f.require_guardian {
        return Err(Defer::RequireGuardian);
    }
    if f.require_fresh_review && f.reviewer == ApprovalsReviewer::AutoReview {
        return Err(Defer::RequireFreshReview);
    }
    if f.policy == AskForApproval::UnlessTrusted {
        return Err(Defer::UnlessTrusted);
    }
    if f.reviewer == ApprovalsReviewer::AutoReview && !f.reviewer_from_defaults {
        return Err(Defer::OwnerReviewer);
    }
    if f.requirements_fix_policy {
        return Err(Defer::RequirementsPolicy);
    }
    if f.requirements_fix_reviewer {
        return Err(Defer::RequirementsReviewer);
    }
    if f.tool.as_deref() == Some("request_permissions") {
        return Err(Defer::StandingGrant);
    }
    if f.purchase {
        return Err(Defer::Purchase);
    }
    match f.rule_prompt {
        RulePrompt::Yes => return Err(Defer::RulePrompt),
        RulePrompt::Unparsed => return Err(Defer::UnparsedShell),
        RulePrompt::No => {}
    }
    if let AskForApproval::Granular(config) = &f.policy {
        match granular_flag(*config, f.tool.as_deref(), f.rule_prompt) {
            Some((_, true)) => {}
            Some((flag, false)) => return Err(Defer::GranularOff(flag)),
            None => return Err(Defer::UnknownKind),
        }
    }
    Ok(())
}

/// The shells whose `-c`/`-lc` script core splits before matching rules.
fn is_shell_wrapper(command: &[String]) -> bool {
    let [shell, flag, ..] = command else {
        return false;
    };
    let name = std::path::Path::new(shell)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    matches!(name, "sh" | "bash" | "zsh" | "dash" | "ksh" | "fish")
        && matches!(flag.as_str(), "-c" | "-lc" | "-l" | "-ic")
}

/// Whether `policy` has any rule that could ask: a prefix rule that prompts or forbids, or a
/// rule of a kind this cannot read (counted as one that asks).
fn has_asking_rule(policy: &ExecPolicy) -> bool {
    policy.rules().iter_all().any(|(_, rules)| {
        rules.iter().any(|rule| {
            rule.as_any()
                .downcast_ref::<codex_execpolicy::PrefixRule>()
                .is_none_or(|prefix| {
                    matches!(
                        prefix.decision,
                        RuleDecision::Prompt | RuleDecision::Forbidden
                    )
                })
        })
    })
}

/// Whether a non-heuristic rule of `policy` prompts about (or forbids) `command`. A shell
/// script this cannot split defers only when some rule could ask: with none, no command
/// hidden in it can meet one (end-to-end run 2026-10-16: `echo … >> ran.txt; echo done`
/// was left to a person under a policy with no rules at all).
pub fn rule_prompt(policy: &ExecPolicy, command: &[String]) -> RulePrompt {
    let commands = if is_shell_wrapper(command) {
        match codex_shell_command::bash::parse_shell_lc_plain_commands(command) {
            Some(commands) => commands,
            None if has_asking_rule(policy) => return RulePrompt::Unparsed,
            None => return RulePrompt::No,
        }
    } else {
        vec![command.to_vec()]
    };
    let prompts = commands.iter().any(|cmd| {
        policy
            .matches_for_command(cmd, None)
            .iter()
            .any(|m| match m {
                RuleMatch::PrefixRuleMatch { decision, .. } => {
                    matches!(decision, RuleDecision::Prompt | RuleDecision::Forbidden)
                }
                RuleMatch::HeuristicsRuleMatch { .. } => false,
            })
    });
    if prompts {
        RulePrompt::Yes
    } else {
        RulePrompt::No
    }
}

/// The command of an `exec_command` box's action JSON.
pub fn action_command(action: &serde_json::Value) -> Option<Vec<String>> {
    if action.get("tool").and_then(|t| t.as_str()) != Some("exec_command") {
        return None;
    }
    action
        .get("command")?
        .as_array()?
        .iter()
        .map(|v| v.as_str().map(str::to_string))
        .collect()
}

/// Words that name buying or paying, in a tool's names, description or argument keys.
const PURCHASE_WORDS: &[&str] = &[
    "buy",
    "buys",
    "purchase",
    "purchases",
    "checkout",
    "cart",
    "order",
    "orders",
    "pay",
    "pays",
    "payment",
    "payments",
    "billing",
    "charge",
    "invoice",
    "subscribe",
    "subscription",
    "donate",
    "transfer",
    "wallet",
    "amount",
    "price",
    "currency",
    "card",
];

fn names_purchase(text: &str) -> bool {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .any(|word| {
            let word = word.to_ascii_lowercase();
            PURCHASE_WORDS.contains(&word.as_str())
        })
}

/// Whether an `mcp_tool_call` box reads as a purchase or a payment: a purchase word in its
/// server, tool, title, description or connector, or among its argument keys. Fails toward
/// a person (a sort `order` defers too).
pub fn purchase_like(action: &serde_json::Value) -> bool {
    if action.get("tool").and_then(|t| t.as_str()) != Some("mcp_tool_call") {
        return false;
    }
    let texts = [
        "server",
        "tool_name",
        "tool_title",
        "tool_description",
        "connector_id",
        "connector_name",
        "connector_description",
    ];
    texts
        .iter()
        .filter_map(|key| action.get(*key).and_then(|v| v.as_str()))
        .any(names_purchase)
        || action
            .get("arguments")
            .and_then(|args| args.as_object())
            .is_some_and(|args| args.keys().any(|key| names_purchase(key)))
}

/// Whether the effective reviewer is the packaged defaults' own: no other layer sets any
/// `approvals_reviewer` (top level, a profile's, a per-server table's), a packaged layer
/// does, and nothing changed it since the thread started (a runtime pick is the owner's).
pub fn reviewer_from_packaged_defaults(
    origins: &std::collections::HashMap<String, codex_config::ConfigLayerMetadata>,
    started_with: ApprovalsReviewer,
    effective: ApprovalsReviewer,
) -> bool {
    use codex_config::ConfigLayerSource;
    let packaged = |meta: &codex_config::ConfigLayerMetadata| {
        matches!(meta.name, ConfigLayerSource::PackagedDefaults { .. })
    };
    let reviewer_key =
        |key: &str| key == "approvals_reviewer" || key.ends_with(".approvals_reviewer");
    started_with == effective
        && origins.get("approvals_reviewer").is_some_and(packaged)
        && origins
            .iter()
            .filter(|(key, _)| reviewer_key(key))
            .all(|(_, meta)| packaged(meta))
}

/// Gather [`ApprovalFacts`] for one box from the thread's config.
pub async fn facts(
    input: &codex_extension_api::ApprovalDecisionInput<'_>,
    config: &codex_core::config::Config,
    approve_all: bool,
) -> ApprovalFacts {
    let stack = &config.config_layer_stack;
    let reviewer_from_defaults = reviewer_from_packaged_defaults(
        &stack.origins(),
        config.approvals_reviewer,
        input.approvals_reviewer,
    );
    let requirements = stack.requirements();
    let tool = input
        .action
        .get("tool")
        .and_then(|t| t.as_str())
        .map(str::to_string);
    let rule_prompt = match action_command(input.action) {
        Some(command) => match codex_core::load_exec_policy(stack).await {
            Ok(policy) => rule_prompt(&policy, &command),
            // An unreadable rules file is a rule this cannot see past.
            Err(_) => RulePrompt::Yes,
        },
        None => RulePrompt::No,
    };
    ApprovalFacts {
        approve_all,
        require_guardian: input.require_guardian,
        require_fresh_review: input.require_fresh_review,
        policy: input.approval_policy,
        reviewer: input.approvals_reviewer,
        reviewer_from_defaults,
        requirements_fix_policy: requirements.approval_policy.source.is_some(),
        requirements_fix_reviewer: requirements.approvals_reviewer.source.is_some(),
        tool,
        rule_prompt,
        purchase: purchase_like(input.action),
    }
}
