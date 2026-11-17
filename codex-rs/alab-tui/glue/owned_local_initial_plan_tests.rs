// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use clap::Parser;

type TestResult = Result<(), Box<dyn std::error::Error>>;
fn cli() -> Result<Cli, clap::Error> {
    Cli::try_parse_from(["codex"])
}

#[test]
fn explicit_original_cli_plans_remain_fixed_and_full_cli_is_not_consumed() -> TestResult {
    let mut actual = cli()?;
    assert!(matches!(kind(&actual), InitialSessionKind::Start));
    actual.resume_session_id = Some("actual-cli-selection-data".into());
    assert!(matches!(kind(&actual), InitialSessionKind::Resume));
    actual.fork_session_id = Some("actual-cli-fork-data".into());
    // Original ordinary TUI gives fork selection precedence over resume.
    assert!(matches!(kind(&actual), InitialSessionKind::Fork));
    assert_eq!(
        actual.resume_session_id.as_deref(),
        Some("actual-cli-selection-data")
    );
    assert_eq!(
        actual.fork_session_id.as_deref(),
        Some("actual-cli-fork-data")
    );
    Ok(())
}

#[test]
fn original_lookup_picker_and_overview_require_actual_one_shot_choice() -> TestResult {
    let flags: [fn(&mut Cli); 5] = [
        |cli| cli.resume_picker = true,
        |cli| cli.resume_last = true,
        |cli| cli.fork_picker = true,
        |cli| cli.fork_last = true,
        |cli| cli.agents_overview = true,
    ];
    for set in flags {
        let mut actual = cli()?;
        set(&mut actual);
        assert!(matches!(kind(&actual), InitialSessionKind::Selection));
    }
    Ok(())
}

#[test]
fn original_precedence_does_not_weaken_explicit_method_or_overview() -> TestResult {
    let mut actual = cli()?;
    actual.resume_picker = true;
    actual.fork_last = true;
    actual.fork_session_id = Some("actual-explicit-fork".into());
    assert!(matches!(kind(&actual), InitialSessionKind::Fork));
    actual.agents_overview = true;
    assert!(matches!(kind(&actual), InitialSessionKind::Selection));
    actual.agents_overview = false;
    actual.fork_session_id = None;
    assert!(matches!(kind(&actual), InitialSessionKind::Selection));
    Ok(())
}
