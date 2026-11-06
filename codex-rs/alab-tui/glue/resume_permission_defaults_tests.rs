// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Exercise the real TUI resume-selection boundary without activating ALab
//! behavior in the shared upstream test process.

use crate::legacy_core::config::ConfigBuilder;
use crate::legacy_core::config::ConfigOverrides;
use crate::resume_permissions::ResumePermissions;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::SandboxPolicy;
use std::time::Duration;

const CASE: &str = "ALAB_RESUME_PERMISSION_CASE";
const COMPLETE: &str = "ALAB_RESUME_PERMISSIONS_OK";

#[tokio::test]
async fn native_resume_permissions_are_scoped_and_preserve_explicit_choices() {
    assert!(!codex_alab_core::active());
    let (_, module) = module_path!().split_once("::").expect("test module");
    let child = format!("{module}::native_resume_permission_child");
    for case in ["aterm", "aterm-explicit", "aterm-cli", "other", "upstream"] {
        let mut command =
            tokio::process::Command::new(std::env::current_exe().expect("test binary"));
        command
            .args(["--exact", &child, "--nocapture", "--test-threads=1"])
            .env(CASE, case)
            .env(
                "TERM_PROGRAM",
                if case == "other" { "other" } else { "aterm" },
            )
            .kill_on_drop(true);
        let output = tokio::time::timeout(Duration::from_secs(60), command.output())
            .await
            .expect("resume permission child hang detector")
            .expect("resume permission child");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{case}: {stdout}\n{stderr}");
        assert!(stdout.contains(COMPLETE), "{case}: {stdout}\n{stderr}");
    }
    assert!(!codex_alab_core::active());
}

#[tokio::test]
#[expect(
    clippy::print_stdout,
    reason = "parent verifies the isolated test actually ran"
)]
async fn native_resume_permission_child() {
    let (_, module) = module_path!().split_once("::").expect("test module");
    let name = format!("{module}::native_resume_permission_child");
    if std::env::args().skip(1).collect::<Vec<_>>()
        != ["--exact", &name, "--nocapture", "--test-threads=1"]
    {
        return;
    }
    let Ok(case) = std::env::var(CASE) else {
        return;
    };
    assert!(matches!(
        case.as_str(),
        "aterm" | "aterm-explicit" | "aterm-cli" | "other" | "upstream"
    ));
    if case != "upstream" {
        codex_alab_core::activate();
    }
    let home = tempfile::tempdir().expect("private config");
    if case == "aterm-explicit" {
        std::fs::write(
            home.path().join("config.toml"),
            "approval_policy = \"on-request\"\nsandbox_mode = \"read-only\"\n",
        )
        .expect("explicit permission config");
    }
    let mut builder = ConfigBuilder::default()
        .loader_overrides(codex_config::LoaderOverrides::without_managed_config_for_tests())
        .codex_home(home.path().to_path_buf())
        .fallback_cwd(Some(home.path().to_path_buf()));
    if case == "aterm-cli" {
        builder = builder.cli_overrides(vec![
            ("approval_policy".into(), "on-request".into()),
            ("sandbox_mode".into(), "read-only".into()),
        ]);
    }
    let config = builder.build().await.expect("effective config");
    let selected = ResumePermissions::from_overrides(&config, &ConfigOverrides::default());
    let explicit = case == "aterm-cli";
    assert_eq!(selected.approval_policy, explicit);
    assert_eq!(selected.profile, explicit);
    assert!(!selected.approvals_reviewer);
    assert!(!selected.workspace_roots);
    // This is the actual gate used before remote resume/fork, not a copy of its predicate.
    assert_eq!(
        super::has_explicit_resume_permission_override(&config, &ConfigOverrides::default()),
        explicit,
        "native defaults and user-file settings must not reject an ordinary remote resume",
    );
    assert_eq!(
        selected.with_alab_defaults(false),
        selected,
        "remote carry is unchanged"
    );
    let carried = selected.with_alab_defaults(true);
    let native = matches!(case.as_str(), "aterm" | "aterm-explicit" | "aterm-cli");
    assert_eq!(carried.approval_policy, native);
    assert_eq!(carried.profile, native);
    if case == "aterm" {
        assert_eq!(
            config.permissions.approval_policy.value(),
            AskForApproval::Never
        );
        assert_eq!(
            config.legacy_sandbox_policy(),
            SandboxPolicy::DangerFullAccess
        );
    } else if matches!(case.as_str(), "aterm-explicit" | "aterm-cli") {
        assert_eq!(
            config.permissions.approval_policy.value(),
            AskForApproval::OnRequest
        );
        assert!(matches!(
            config.legacy_sandbox_policy(),
            SandboxPolicy::ReadOnly { .. }
        ));
    }
    println!("{COMPLETE}");
}
