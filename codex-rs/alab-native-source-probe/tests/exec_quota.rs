// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Run the loopback named-quota fixture through the production exec controller.
//! The test runs the exec controller in a foreground PTY with two scratch logins.

#[cfg(not(debug_assertions))]
compile_error!("the named exec probe requires debug assertions");

use clap::Parser;
use std::ffi::OsString;
use std::io;
use std::net::Ipv4Addr;
use std::net::SocketAddrV4;
use std::path::PathBuf;

const ARGS: &[&str] = &[
    "exec",
    "--json",
    "--color",
    "never",
    "--skip-git-repo-check",
    "--dangerously-bypass-approvals-and-sandbox",
    "Keep this headless capacity smoke task and its completed plan.",
];

fn fixture_config() -> io::Result<()> {
    let invalid = || {
        io::Error::other("expected the named exec fixture's scratch login and loopback provider")
    };
    let cwd = std::env::current_dir()?;
    let root = cwd.parent().ok_or_else(invalid)?;
    let path = |key| std::env::var_os(key).map(PathBuf::from);
    let home = root.join("codex-home");
    if cwd != root.join("work")
        || path("CODEX_HOME").as_ref() != Some(&home)
        || !path("CODEX_AUTH_PATH").is_some_and(|auth| {
            auth == root.join("auth-old/auth.json") || auth == root.join("auth-new/auth.json")
        })
    {
        return Err(invalid());
    }

    let config: toml::Table = std::fs::read_to_string(home.join("config.toml"))?
        .parse()
        .map_err(io::Error::other)?;
    let base = config
        .get("chatgpt_base_url")
        .and_then(toml::Value::as_str)
        .ok_or_else(invalid)?;
    let listener: SocketAddrV4 = base
        .strip_prefix("https://")
        .ok_or_else(invalid)?
        .parse()
        .map_err(io::Error::other)?;
    let endpoint = config
        .get("model_providers")
        .and_then(|providers| providers.get("capacity-smoke"))
        .and_then(|provider| provider.get("base_url"))
        .and_then(toml::Value::as_str);
    if *listener.ip() != Ipv4Addr::LOCALHOST
        || listener.port() == 0
        || config.get("model_provider").and_then(toml::Value::as_str) != Some("capacity-smoke")
        || endpoint != Some(format!("{base}/v1").as_str())
    {
        return Err(invalid());
    }
    Ok(())
}

fn main() -> io::Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args == [OsString::from("--describe-named-exec-probe")] {
        println!(
            "{}",
            serde_json::json!({
                "schema": 1, "feature": "named-exec-probe", "limit": "codex_other",
                "profile": "debug", "source": option_env!("STABLE_GIT_COMMIT"),
            })
        );
        return Ok(());
    }
    if !args.iter().eq(ARGS.iter().map(std::ffi::OsStr::new)) {
        return Err(io::Error::other("expected the named exec fixture command"));
    }
    fixture_config()?;
    let cli = codex_exec::Cli::try_parse_from(args).map_err(io::Error::other)?;
    codex_alab_core::activate();
    codex_alab_core::initialize_running_build!().map_err(io::Error::other)?;
    codex_alab_tui::life::capture_launch();
    codex_arg0::arg0_dispatch_or_else(|paths| async move {
        #[cfg(unix)]
        let _startup = codex_alab_tui::frontend_control::enter_source_when_unretained().await?;
        #[cfg(not(unix))]
        let _startup = codex_alab_core::native_backend_startup::enter().await?;
        Box::pin(codex_exec::run_main(cli, paths)).await
    })
    .map_err(io::Error::other)
}
