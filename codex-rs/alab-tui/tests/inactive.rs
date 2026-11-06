// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The negative control, in a process of its own: without the switch (every upstream test,
//! every crate but the codex binary), the packaged defaults and the policy are upstream's.

use codex_core::config::ConfigBuilder;
use codex_features::Feature;

#[tokio::test]
async fn without_the_switch_upstream_defaults_stand() {
    assert!(!codex_alab_core::active());
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("scratch CODEX_HOME: {err}"));
    let config = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .fallback_cwd(Some(home.path().to_path_buf()))
        .build()
        .await
        .unwrap_or_else(|err| panic!("config: {err}"));
    assert!(config.check_for_update_on_startup);
    assert_ne!(config.analytics_enabled, Some(false));
    assert!(config.show_tooltips);
    assert!(config.features.enabled(Feature::DaemonAutoStart));
    assert!(codex_alab_tui::announcement_feed());
    assert!(codex_alab_tui::attach_shared_daemon());
    assert!(!codex_alab_tui::is_upsell_cta("open_pricing_dialog"));
    assert!(!codex_alab_tui::skip_optional_model_upgrade());
    assert!(!codex_alab_tui::reload_skills_after_import());
    assert_eq!(
        codex_alab_tui::auth_path_daemon_exclusion_with(
            codex_alab_core::active(),
            Some(home.path().join("auth.json").as_os_str()),
        ),
        None,
    );
}

/// Without the switch the life and session hooks do nothing: no launch snapshot, no stamp,
/// no record, no signal handler, and the exit hooks return.
#[test]
fn without_the_switch_the_life_hooks_are_inert() {
    use codex_alab_tui::session::Frame;
    use codex_alab_tui::session::ProgramState;
    assert!(!codex_alab_core::active());
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("scratch CODEX_HOME: {err}"));
    codex_alab_tui::life::capture_launch();
    codex_alab_tui::life::capture_cli(clap::Command::default);
    codex_alab_tui::life::on_terminate(|| {}, || Ok(()));
    codex_alab_tui::session::observe(&Frame {
        primary_thread: Some("t"),
        displayed_thread: Some("t"),
        rollout_path: None,
        cwd: home.path(),
        model: "m",
        codex_home: home.path(),
        embedded: true,
        state: ProgramState::Idle,
        draft_empty: true,
        queued_input: false,
        popup_open: false,
        editor_open: false,
        pending_user_turn: false,
    });
    codex_alab_tui::session::human_input(true);
    codex_alab_tui::session::human_input(false);
    codex_alab_tui::life::at_exit(/*is_fatal*/ false);
    codex_alab_tui::life::reraise_signal();
    let shared = codex_alab_core::session::shared();
    assert!(shared.launch().is_none());
    assert!(shared.last_human_input_ms().is_none());
    assert!(!shared.is_registered("t"));
    assert!(codex_alab_tui::resume_argv::command().is_none());
    assert!(!home.path().join("alab").exists());
}

/// Without the switch the folder-trust hook never asks aterm, never trusts and shows no notice,
/// even for a launch folder it would otherwise judge.
#[tokio::test]
async fn without_the_switch_folder_trust_is_never_answered() {
    use codex_alab_tui::trust;
    assert!(!codex_alab_core::active());
    assert!(!trust::armed());
    let folder = std::path::PathBuf::from("/Users/_owner/Developer/app");
    let gate = trust::Gate {
        embedded: true,
        restricted: false,
        launch_cwd: Some(folder.clone()),
        folder: folder.clone(),
        trust_target: folder,
    };
    assert_eq!(
        trust::judge(&gate, trust::SETTLE_WITHIN).await,
        trust::Verdict::Show("not codex-alab".to_string())
    );
    assert_eq!(trust::take_notice(), None);
}

/// Without the switch the vendor daemon's packages are never touched (no thread even starts),
/// though the same layout is one `prune` would tidy: the twin below proves it.
#[test]
fn without_the_switch_the_vendor_packages_stay() {
    use codex_alab_tui::daemon_packages;
    assert!(!codex_alab_core::active());
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("scratch CODEX_HOME: {err}"));
    std::fs::create_dir_all(home.path().join("app-server-daemon")).unwrap();
    let root = daemon_packages::package_root(home.path());
    let target = "aarch64-apple-darwin";
    for version in ["0.158.0", "0.159.2"] {
        let release = root.join("releases").join(format!("{version}-{target}"));
        std::fs::create_dir_all(release.join("bin")).unwrap();
        std::fs::write(release.join("bin/codex"), "codex").unwrap();
        std::fs::write(
            release.join("codex-package.json"),
            format!(r#"{{"version":"{version}","target":"{target}","entrypoint":"bin/codex"}}"#),
        )
        .unwrap();
    }
    std::os::unix::fs::symlink(
        root.join(format!("releases/0.159.2-{target}")),
        root.join("current"),
    )
    .unwrap();
    let old = root.join(format!("releases/0.158.0-{target}"));

    assert!(daemon_packages::tidy(home.path(), /*daemon_auto_start*/ false).is_none());
    assert!(
        old.is_dir(),
        "without the switch the superseded release stays"
    );

    let report = daemon_packages::prune(home.path(), &daemon_packages::LiveHost);
    assert_eq!(report.removed, [format!("0.158.0-{target}")], "{report:?}");
    assert!(!old.exists());
}

/// Without the switch /feedback's per-request tags reach the SQLite log, as upstream's do.
#[test]
fn without_the_switch_the_log_database_keeps_feedback_tags() {
    assert!(!codex_alab_core::active());
    let filter = codex_state::log_db::default_filter();
    assert!(filter.would_enable("feedback_tags", &tracing::Level::INFO));
    assert!(filter.would_enable("codex_core::session", &tracing::Level::TRACE));
    assert!(filter.would_enable("codex_http_client::custom_ca", &tracing::Level::TRACE));
}

/// Without the switch the OSC 7501 hooks leave the report to upstream (`ALAB(turns)`): nothing
/// is written through them, inside aterm or not.
#[test]
fn without_the_switch_upstream_writes_the_program_report() {
    use codex_alab_core::session::ProgramState;
    use codex_alab_tui::turns;
    assert!(!codex_alab_core::active());
    assert!(!turns::enabled());
    let frame = turns::Frame {
        state: ProgramState::Working,
        startup_only: true,
        rest: ProgramState::Idle,
        submitted: false,
        elsewhere: false,
        held: false,
    };
    assert!(turns::frame(frame, |_| panic!("upstream writes the report")).is_none());
    assert!(
        !turns::running_elsewhere(None),
        "no shared state is read for another thread's turn"
    );
    assert!(
        !turns::seen_elsewhere(None),
        "no shared state is read for a turn the TUI saw"
    );
    assert!(
        turns::starting(
            /*asked*/ true,
            |_| panic!("upstream writes the report")
        )
        .is_none()
    );
}

/// Without the switch the account notice is never routed (`ALAB(notices)`): the session is not
/// even looked for, no connection is made though an aterm is there, and upstream's banner
/// shows.
#[tokio::test]
async fn without_the_switch_no_notice_is_routed() {
    use codex_alab_aterm::fake::FakeAterm;
    use codex_alab_aterm::fake::Mode;
    use codex_alab_tui::notices;
    assert!(!codex_alab_core::active());
    let dir = tempfile::tempdir().unwrap_or_else(|err| panic!("scratch dir: {err}"));
    let fake = FakeAterm::start(dir.path(), Mode::Current)
        .await
        .unwrap_or_else(|err| panic!("fake aterm: {err}"));
    let notice = notices::RoutedNotice {
        account: "acct-0123".to_string(),
        user: "user-abcd".to_string(),
        title: "Set up security for Daybreak mode".to_string(),
        description: "Finish setup by October 15.".to_string(),
        url: "https://chatgpt.com/cyber".to_string(),
    };
    let looked = std::cell::Cell::new(false);
    let endpoint = fake.endpoint();
    let locate = || {
        looked.set(true);
        Some(endpoint)
    };
    assert!(!notices::route_from(locate, &notice).await);
    assert!(!looked.get(), "the session is not looked for");
    assert_eq!(fake.connections(), 0, "{:?}", fake.requests());
    assert!(!notices::route(&notice).await);
    assert_eq!(fake.connections(), 0);
    let looked = std::cell::Cell::new(false);
    let locate = || {
        looked.set(true);
        Some(fake.endpoint())
    };
    assert!(!notices::settle_from(locate, &notice.account, &notice.user).await);
    assert!(!looked.get(), "no session is looked for to settle");
    assert!(!notices::settle(&notice.account, &notice.user).await);
    assert_eq!(fake.connections(), 0, "{:?}", fake.requests());
}

/// Without the switch the system skills stay where upstream keeps them (`ALAB(skills)`).
#[test]
fn without_the_switch_system_skills_stay_in_dot_system() {
    assert!(!codex_alab_core::active());
    let home = codex_utils_absolute_path::AbsolutePathBuf::from_absolute_path("/codex-home")
        .unwrap_or_else(|err| panic!("absolute: {err}"));
    assert_eq!(
        codex_skills::system_cache_root_dir(&home).as_path(),
        std::path::Path::new("/codex-home/skills/.system")
    );
}
