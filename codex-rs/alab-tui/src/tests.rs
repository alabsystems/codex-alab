// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use codex_config::types::NotificationMethod;
use codex_config::types::OtelExporterKind;
use codex_core::config::Config;
use codex_core::config::ConfigBuilder;
use codex_features::Feature;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::SandboxPolicy;
use pretty_assertions::assert_eq;

/// The config a fresh codex-alab install gets: the switch on (as the codex binary's main
/// sets it), an empty CODEX_HOME and no project, so only the packaged defaults (upstream's
/// merged with `alab-core/defaults.toml`) and the built-in ones apply.
async fn fresh_install() -> (tempfile::TempDir, Config) {
    codex_alab_core::activate();
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("scratch CODEX_HOME: {err}"));
    let config = ConfigBuilder::default()
        .loader_overrides(codex_config::LoaderOverrides::without_managed_config_for_tests())
        .codex_home(home.path().to_path_buf())
        .fallback_cwd(Some(home.path().to_path_buf()))
        .build()
        .await
        .unwrap_or_else(|err| panic!("config from packaged defaults: {err}"));
    (home, config)
}

/// Whether `[tui] alab_progress` is on in this config's layers, read as the frame glue reads it.
fn progress_on(config: &Config) -> bool {
    crate::progress::setting(
        config
            .config_layer_stack
            .layers_high_to_low()
            .map(|l| &l.config),
    )
}

/// ALab's packaged layer is read as raw TOML, so an upstream rename of any key below would turn
/// ALab's default into a silent no-op. Each assertion is the key doing its job.
#[tokio::test]
async fn packaged_defaults_reach_the_config() {
    let (_home, config) = fresh_install().await;

    assert!(!config.check_for_update_on_startup, "update check");
    assert_eq!(config.analytics_enabled, Some(false), "analytics");
    assert!(!config.feedback_enabled, "/feedback upload");
    assert_eq!(
        config.otel.metrics_exporter,
        OtelExporterKind::None,
        "Statsig metrics"
    );
    assert_eq!(
        config.history.max_bytes,
        Some(8 * 1024 * 1024),
        "history.jsonl cap"
    );

    for feature in [
        Feature::DaemonAutoStart,
        Feature::Apps,
        Feature::Plugins,
        Feature::SkillSearch,
        Feature::ImageGeneration,
        Feature::ToolSuggest,
    ] {
        assert!(
            !config.features.enabled(feature),
            "feature {feature:?} is off"
        );
    }

    assert!(!config.show_tooltips, "tips and the desktop-app ad");
    assert!(
        !config.tui_show_server_version_notice,
        "server version notice"
    );
    assert!(!config.tui_auto_recap, "automatic recaps");
    // The aterm-only three hold inside aterm, and upstream's own values everywhere else.
    let in_aterm = codex_alab_core::inside_aterm();
    assert_eq!(
        config.disable_paste_burst, in_aterm,
        "aterm's typed input submits, inside aterm only"
    );
    assert_eq!(
        config.tui_notifications.method,
        if in_aterm {
            NotificationMethod::Osc9
        } else {
            NotificationMethod::Auto
        },
        "aterm takes OSC 9; elsewhere upstream's auto picks"
    );
    assert_eq!(
        config.tui_pet.as_deref(),
        None,
        "pets use no prompt space until selected"
    );
    assert_eq!(
        progress_on(&config),
        in_aterm,
        "aterm's OSC 9;4 bar, inside aterm only"
    );
    assert_eq!(
        config.tui_status_line.as_deref(),
        Some(
            [
                "model-with-reasoning",
                "context-remaining",
                "five-hour-limit",
                "weekly-limit",
                "git-branch",
                "thread-name",
            ]
            .map(String::from)
            .as_slice()
        ),
        "status line"
    );
    assert_eq!(
        config.tui_terminal_title.as_deref(),
        Some(
            ["run-state", "thread-name", "project-name"]
                .map(String::from)
                .as_slice()
        ),
        "title without the 10 Hz spinner"
    );
    assert_eq!(
        config.notices.hide_rate_limit_model_nudge,
        Some(true),
        "rate-limit nudge"
    );
    assert_eq!(
        config.notices.hide_gov_cloud_guidance,
        Some(true),
        "GovCloud check"
    );
}

/// The aterm-only defaults reach the config too, whatever terminal runs this test: read as a
/// config file, each key does its job (an upstream rename would make it a silent no-op).
#[tokio::test]
async fn aterm_defaults_reach_the_config() {
    codex_alab_core::activate();
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("scratch CODEX_HOME: {err}"));
    std::fs::write(
        home.path().join("config.toml"),
        codex_alab_core::ATERM_DEFAULTS,
    )
    .unwrap_or_else(|err| panic!("aterm defaults as config: {err}"));
    let config = ConfigBuilder::default()
        .loader_overrides(codex_config::LoaderOverrides::without_managed_config_for_tests())
        .codex_home(home.path().to_path_buf())
        .fallback_cwd(Some(home.path().to_path_buf()))
        .build()
        .await
        .unwrap_or_else(|err| panic!("config: {err}"));
    assert_eq!(
        config.tui_pet.as_deref(),
        None,
        "the packaged defaults leave pets unselected"
    );
    assert_eq!(
        config.tui_notifications.method,
        NotificationMethod::Osc9,
        "aterm takes OSC 9"
    );
    assert!(config.disable_paste_burst, "aterm's typed input submits");
    assert!(progress_on(&config), "aterm's OSC 9;4 bar");
    assert_eq!(
        config.permissions.approval_policy.value(),
        AskForApproval::Never
    );
    assert_eq!(
        config.legacy_sandbox_policy(),
        SandboxPolicy::DangerFullAccess
    );
}

#[tokio::test]
async fn aterm_permission_defaults_preserve_explicit_cli_choices() {
    codex_alab_core::activate();
    let home = tempfile::tempdir().expect("scratch CODEX_HOME");
    std::fs::write(
        home.path().join("config.toml"),
        codex_alab_core::ATERM_DEFAULTS,
    )
    .expect("aterm defaults");
    let config = ConfigBuilder::default()
        .loader_overrides(codex_config::LoaderOverrides::without_managed_config_for_tests())
        .codex_home(home.path().to_path_buf())
        .fallback_cwd(Some(home.path().to_path_buf()))
        .cli_overrides(vec![
            (
                "approval_policy".into(),
                toml::Value::String("on-request".into()),
            ),
            (
                "sandbox_mode".into(),
                toml::Value::String("read-only".into()),
            ),
        ])
        .build()
        .await
        .expect("explicit permissions override the defaults");
    assert_eq!(
        config.permissions.approval_policy.value(),
        AskForApproval::OnRequest
    );
    assert!(matches!(
        config.legacy_sandbox_policy(),
        SandboxPolicy::ReadOnly { .. }
    ));
}

/// `[tui] alab_progress` is ALab's own key: codex reads it without a word (upstream's `[tui]`
/// keeps unknown keys out of its startup warning), and a higher layer's `false` (here `-c`)
/// turns the bar off.
#[tokio::test]
async fn the_progress_key_is_quiet_and_a_person_turns_it_off() {
    codex_alab_core::activate();
    let load = |config_toml: &'static str, cli: Vec<(String, toml::Value)>| async move {
        let home = tempfile::tempdir().unwrap_or_else(|err| panic!("scratch CODEX_HOME: {err}"));
        std::fs::write(home.path().join("config.toml"), config_toml)
            .unwrap_or_else(|err| panic!("config.toml: {err}"));
        let config = ConfigBuilder::default()
            .codex_home(home.path().to_path_buf())
            .fallback_cwd(Some(home.path().to_path_buf()))
            .cli_overrides(cli)
            .build()
            .await
            .unwrap_or_else(|err| panic!("config: {err}"));
        let warned = config
            .startup_warnings
            .iter()
            .any(|w| w.contains("alab_progress"));
        (progress_on(&config), warned, config.startup_warnings)
    };
    let (on, warned, warnings) = load(codex_alab_core::ATERM_DEFAULTS, Vec::new()).await;
    assert!(on);
    assert!(!warned, "{warnings:?}");
    let off = vec![("tui.alab_progress".to_owned(), toml::Value::Boolean(false))];
    let (on, warned, warnings) = load(codex_alab_core::ATERM_DEFAULTS, off).await;
    assert!(!on, "-c tui.alab_progress=false wins");
    assert!(!warned, "{warnings:?}");
    // Negative control: the same key outside `[tui]` is one codex warns about, and is not the
    // switch (only the packaged default, inside aterm, sets it here).
    let (on, warned, warnings) = load("alab_progress = true\n", Vec::new()).await;
    assert!(warned, "{warnings:?}");
    assert_eq!(on, codex_alab_core::inside_aterm());
}

/// codex-alab's `--strict-config` takes its own off switch, in `config.toml` and through `-c`
/// (the `ALAB(progress)` line in upstream's strict validation, `codex_alab_core::own_tui_key`):
/// the binary that documents `alab_progress = false` does not refuse to start on it.
#[tokio::test]
async fn strict_config_takes_the_progress_off_switch_and_stays_strict() {
    codex_alab_core::activate();
    let load = |config_toml: &'static str, cli: Vec<(String, toml::Value)>| async move {
        let home = tempfile::tempdir().unwrap_or_else(|err| panic!("scratch CODEX_HOME: {err}"));
        std::fs::write(home.path().join("config.toml"), config_toml)
            .unwrap_or_else(|err| panic!("config.toml: {err}"));
        ConfigBuilder::default()
            .codex_home(home.path().to_path_buf())
            .fallback_cwd(Some(home.path().to_path_buf()))
            .loader_overrides(codex_config::LoaderOverrides::without_managed_config_for_tests())
            .cli_overrides(cli)
            .strict_config(/*strict_config*/ true)
            .build()
            .await
            .map(|config| progress_on(&config))
            .map_err(|err| err.to_string())
    };
    let cli = |key: &str| vec![(key.to_owned(), toml::Value::Boolean(false))];
    assert_eq!(
        load("[tui]\nalab_progress = false\n", Vec::new()).await,
        Ok(false)
    );
    assert_eq!(load("", cli("tui.alab_progress")).await, Ok(false));
    // Negative controls: strict stays strict for any other unknown `[tui]` key, a misspelt
    // ALab key included, in the file and through `-c`.
    let typo = load("[tui]\nalab_progres = false\n", Vec::new()).await;
    assert!(
        typo.as_ref().is_err_and(|err| err.contains("alab_progres")),
        "{typo:?}"
    );
    let typo = load("", cli("tui.alab_progres")).await;
    assert!(
        typo.as_ref().is_err_and(|err| err.contains("alab_progres")),
        "{typo:?}"
    );
}

/// A user's own config.toml still wins over every packaged default.
#[tokio::test]
async fn the_users_config_overrides_them() {
    codex_alab_core::activate();
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("scratch CODEX_HOME: {err}"));
    std::fs::write(
        home.path().join("config.toml"),
        "approval_policy = \"on-request\"\nsandbox_mode = \"read-only\"\ncheck_for_update_on_startup = true\n[analytics]\nenabled = true\n[features]\nplugins = true\n[tui]\npet = \"disabled\"\n",
    )
    .unwrap_or_else(|err| panic!("user config: {err}"));
    let config = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .fallback_cwd(Some(home.path().to_path_buf()))
        .build()
        .await
        .unwrap_or_else(|err| panic!("config: {err}"));
    assert!(config.check_for_update_on_startup);
    assert_eq!(config.analytics_enabled, Some(true));
    assert!(config.features.enabled(Feature::Plugins));
    assert_eq!(config.tui_pet.as_deref(), Some("disabled"));
    assert_eq!(
        config.permissions.approval_policy.value(),
        AskForApproval::OnRequest
    );
    assert!(matches!(
        config.legacy_sandbox_policy(),
        SandboxPolicy::ReadOnly { .. }
    ));
}

#[test]
fn plan_ads_go_and_working_buttons_stay() {
    codex_alab_core::activate();
    for action in [
        "open_plus_pricing_web",
        "open_pro_pricing_web",
        "open_pricing_dialog",
    ] {
        assert!(crate::is_upsell_cta(action), "{action}");
    }
    for action in [
        "add_credits",
        "buy_credits",
        "buy_reset",
        "reset_usage",
        "view_usage",
        "notify_owner",
        "request_increase",
        "unknown",
    ] {
        assert!(!crate::is_upsell_cta(action), "{action}");
    }
}

/// The SQLite log database keeps INFO and up, but not /feedback's per-request tags (codex-alab
/// turns /feedback off). Another target's INFO and HTTP-client WARN still go in; TRACE does not.
#[test]
fn the_log_database_keeps_no_feedback_tags() {
    codex_alab_core::activate();
    let filter = codex_state::log_db::default_filter();
    assert!(
        !filter.would_enable("feedback_tags", &tracing::Level::INFO),
        "feedback tags"
    );
    assert!(
        filter.would_enable("codex_core::session", &tracing::Level::INFO),
        "INFO goes in"
    );
    assert!(
        !filter.would_enable("codex_core::session", &tracing::Level::TRACE),
        "TRACE does not"
    );
    assert!(
        filter.would_enable("codex_http_client::custom_ca", &tracing::Level::WARN),
        "a client that failed to build"
    );
}

#[test]
fn admitted_auth_path_excludes_shared_daemon_auto_start() {
    let dir = tempfile::tempdir().unwrap_or_else(|err| panic!("scratch dir: {err}"));
    let auth_file = dir.path().join("acct-1").join("auth.json");
    assert_eq!(
        crate::auth_path_daemon_exclusion_with(/*active*/ true, Some(auth_file.as_os_str()),),
        Some("per-account auth override (CODEX_AUTH_PATH)"),
    );
    assert_eq!(
        crate::auth_path_daemon_exclusion_with(
            /*active*/ true,
            Some(std::ffi::OsStr::new("relative/auth.json")),
        ),
        None,
    );
    assert_eq!(
        crate::auth_path_daemon_exclusion_with(/*active*/ true, None),
        None,
    );
    assert_eq!(
        crate::auth_path_daemon_exclusion_with(/*active*/ false, Some(auth_file.as_os_str()),),
        None,
    );
}
