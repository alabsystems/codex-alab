// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The protected runner can host a real loopback WebSocket app-server. The
//! sibling upstream test exercises implicit Unix daemon discovery, which needs
//! the system's protected daemon socket directory outside this runner's scratch.

use super::super::focus_palette::PtyCodex;
use super::super::focus_palette::write_test_config;
use anyhow::Context;
use anyhow::Result;
use anyhow::ensure;
use app_test_support::create_fake_rollout;
use std::io::Write;
use std::net::SocketAddr;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Child;

#[derive(Clone, Copy, Debug)]
enum Backend {
    Embedded,
    WebSocket,
    WebSocketWithProviderOverride,
}

async fn remote_server(codex: &Path, home: &Path) -> Result<(Child, String)> {
    let log = home.join("websocket-listener.log");
    let mut child = tokio::process::Command::new(codex)
        .args([
            "app-server",
            "--listen",
            "ws://127.0.0.1:0",
            "-c",
            "model_provider=\"server-provider\"",
        ])
        .env("CODEX_HOME", home)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(&log)?)
        .kill_on_drop(true)
        .spawn()?;
    let endpoint = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let banner = std::fs::read_to_string(&log)?;
            if let Some(address) = banner
                .split_whitespace()
                .find_map(|word| word.strip_prefix("ws://")?.parse::<SocketAddr>().ok())
            {
                ensure!(
                    address.ip().is_loopback() && address.port() != 0,
                    "unexpected listener {address}"
                );
                return Ok::<_, anyhow::Error>(format!("ws://{address}"));
            }
            ensure!(
                child.try_wait()?.is_none(),
                "app-server exited before announcing its listener: {banner}"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .context("wait for the real loopback app-server")??;
    Ok((child, endpoint))
}

fn provider_history(home: &Path, cwd: &Path) -> Result<()> {
    write_test_config(home, cwd)?;
    let mut config = std::fs::OpenOptions::new()
        .append(true)
        .open(home.join("config.toml"))?;
    writeln!(
        config,
        "\n[model_providers.server-provider]\nname = 'Server provider'\nbase_url = 'http://127.0.0.1:9/v1'\nwire_api = 'responses'\nrequires_openai_auth = false\n[projects.'/']\ntrust_level = 'trusted'"
    )?;
    for (hour, provider, preview) in [
        ("10", "openai", "Client provider history"),
        ("11", "server-provider", "Server provider history"),
    ] {
        create_fake_rollout(
            home,
            &format!("2025-01-02T{hour}-00-00"),
            &format!("2025-01-02T{hour}:00:00Z"),
            preview,
            Some(provider),
            /*git_info*/ None,
        )?;
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn embedded_and_explicit_websocket_resume_and_fork_respect_provider() -> Result<()> {
    let codex = codex_utils_cargo_bin::cargo_bin("codex")?;
    let cwd = codex_utils_cargo_bin::repo_root()?;
    for action in ["resume", "fork"] {
        for backend in [
            Backend::Embedded,
            Backend::WebSocket,
            Backend::WebSocketWithProviderOverride,
        ] {
            for last in [false, true] {
                let home = codex_alab_tui::test_paths::short_tempdir()?;
                provider_history(home.path(), &cwd)?;
                let mut daemon = match backend {
                    Backend::Embedded => None,
                    _ => Some(remote_server(&codex, home.path()).await?),
                };
                let mut args = vec![action, "--all"];
                match &daemon {
                    Some((_, endpoint)) => args.extend(["--remote", endpoint.as_str()]),
                    None => args.push("--no-daemon"),
                }
                if matches!(backend, Backend::WebSocketWithProviderOverride) {
                    args.extend(["-c", "model_provider=\"openai\""]);
                }
                if last {
                    args.push("--last");
                }
                let mut terminal = PtyCodex::start_cli(&cwd, home, &args)?;
                let case = format!("{action} {backend:?}, last={last}");
                let (expected, excluded): (&[&str], Option<&str>) = match (action, backend, last) {
                    ("resume", Backend::WebSocketWithProviderOverride, true) => {
                        (&["Cannot change this thread's model provider"], None)
                    }
                    ("resume", _, false) => (
                        &["Server provider history", "Client provider history"],
                        None,
                    ),
                    ("resume", _, true) => (&["Server provider history"], None),
                    (_, Backend::WebSocket, _) => (
                        &["Server provider history"],
                        Some("Client provider history"),
                    ),
                    _ => (
                        &["Client provider history"],
                        Some("Server provider history"),
                    ),
                };
                for text in expected {
                    terminal.wait_for_screen(text).with_context(|| {
                        format!("{case}; screen:\n{}", terminal.screen_contents())
                    })?;
                }
                if let Some(text) = excluded {
                    ensure!(
                        !terminal.screen_contains(text),
                        "{case} unexpectedly showed {text}"
                    );
                }
                drop(terminal);
                if let Some((child, _)) = &mut daemon {
                    child.kill().await?;
                    child.wait().await?;
                }
            }
        }
    }
    Ok(())
}
