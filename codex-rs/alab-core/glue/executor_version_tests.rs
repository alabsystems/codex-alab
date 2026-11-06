// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use codex_build_info::BuildInfo;
use codex_http_client::HttpClientFactory;
use codex_http_client::OutboundProxyPolicy;
use tokio::process::Command;
use tokio::sync::mpsc;

use crate::ExecServerRuntimeOptions;
use crate::ExecServerTelemetry;
use crate::discover_v2::capability_manager::CapabilityManager;
use crate::protocol::InitializeParams;
use crate::protocol::InitializeResponse;
use crate::rpc::RpcNotificationSender;
use crate::server::ExecServerHandler;
use crate::server::session_registry::SessionRegistry;
use crate::supports_capability_discovery_v2;

const CHILD: &str = "ALAB_EXECUTOR_VERSION_CHILD";
const COMPLETE: &str = "ALAB_EXECUTOR_VERSION_HANDSHAKE_OK";
const CLIENT: &str = "0.165.0-dev.1+gaaaaaaaaa";
const PRODUCT: &str = "0.1.0-dev.1+codex.0.164.0.gaaaaaaaaa";

fn child_name() -> String {
    let (_, module) = module_path!().split_once("::").expect("test module path");
    format!("{module}::executor_version_handshake_child")
}

#[tokio::test]
async fn executor_handshake_uses_client_identity_without_changing_product_display()
-> anyhow::Result<()> {
    // Each identity owns a process: neither OnceLock nor the active switch can
    // contaminate upstream tests running concurrently in this test binary.
    let name = child_name();
    for mode in [
        "active",
        "inactive",
        "uninitialized",
        "source",
        "malformed",
        "old",
        "prerelease",
    ] {
        let world = tempfile::Builder::new()
            .prefix("alab-executor-version-")
            .tempdir()?;
        let mut command = Command::new(std::env::current_exe()?);
        command
            .args(["--exact", &name, "--nocapture", "--test-threads=1"])
            .env_clear()
            .env(CHILD, mode)
            .env("HOME", world.path())
            .env("CODEX_HOME", world.path().join("codex"))
            .env("TMPDIR", world.path())
            .env("USER", "alab-test")
            .env("LOGNAME", "alab-test")
            .env("PATH", "/usr/bin:/bin")
            .current_dir(world.path())
            .stdin(Stdio::null());
        for (key, directory) in [
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_CACHE_HOME", "cache"),
            ("XDG_DATA_HOME", "data"),
            ("XDG_STATE_HOME", "state"),
            ("XDG_RUNTIME_DIR", "runtime"),
        ] {
            command.env(key, world.path().join(directory));
        }
        // File-backed logs avoid an unbounded pipe drain. One minute detects a
        // stuck child; it is not a latency budget.
        let stdout_path = world.path().join("child.stdout");
        let stderr_path = world.path().join("child.stderr");
        command
            .stdout(std::fs::File::create(&stdout_path)?)
            .stderr(std::fs::File::create(&stderr_path)?)
            .kill_on_drop(true);
        let mut child = command.spawn()?;
        let status = match tokio::time::timeout(Duration::from_secs(60), child.wait()).await {
            Ok(Ok(status)) => Ok(status),
            failed => {
                // Always attempt both operations, including when wait or kill
                // fails, before returning the original failure and diagnostics.
                let killed = child.start_kill();
                let reaped = child.wait().await;
                Err(anyhow::anyhow!(
                    "executor version child {mode} did not finish: {failed:?}; kill={killed:?}; reap={reaped:?}"
                ))
            }
        };
        let stdout_bytes = std::fs::read(stdout_path)?;
        let stderr_bytes = std::fs::read(stderr_path)?;
        let stdout = String::from_utf8_lossy(&stdout_bytes);
        let stderr = String::from_utf8_lossy(&stderr_bytes);
        let status = status.map_err(|error| anyhow::anyhow!("{error}\n{stdout}\n{stderr}"))?;
        assert!(
            status.success(),
            "executor version child {mode}: {stdout}\n{stderr}"
        );
        assert!(stdout.contains(COMPLETE), "child did not run: {stdout}");
        assert!(stdout.contains("1 passed; 0 failed"), "{stdout}");
    }
    Ok(())
}

#[tokio::test]
async fn executor_version_handshake_child() -> anyhow::Result<()> {
    let name = child_name();
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args != ["--exact", &name, "--nocapture", "--test-threads=1"] {
        return Ok(());
    }
    let mode = std::env::var(CHILD)?;
    let client = match mode.as_str() {
        "active" | "inactive" => Some(CLIENT),
        "uninitialized" => None,
        "source" => Some("0.0.0-dev.1+gaaaaaaaaa"),
        "malformed" => Some("not-a-semver"),
        "old" => Some("0.163.0"),
        "prerelease" => Some("0.164.0-alpha.1"),
        _ => anyhow::bail!("unknown executor version child {mode}"),
    };
    assert!(!codex_alab_core::active());
    assert_eq!(codex_alab_core::version::running_version(""), "");
    if let Some(client) = client {
        codex_alab_core::version::initialize_package(client, Some(PRODUCT), None, None)?;
        assert_eq!(codex_alab_core::version::product_version(""), PRODUCT);
    }
    if mode == "active" {
        codex_alab_core::activate();
    }
    assert_eq!(codex_alab_core::active(), mode == "active");

    let sessions = SessionRegistry::new(ExecServerTelemetry::default());
    let (outgoing, _notifications) = mpsc::channel(16);
    let handler = ExecServerHandler::new(
        Arc::clone(&sessions),
        CapabilityManager::new(crate::LocalFileSystem::default()),
        RpcNotificationSender::new(outgoing),
        ExecServerRuntimeOptions::new(
            std::env::current_exe()?,
            /*codex_linux_sandbox_exe*/ None,
        )?,
        HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault),
    );
    // Read the actual initialize handler's response through the wire schema;
    // a helper-only version test would miss a missing or misplaced upstream hook.
    let response = handler
        .initialize(InitializeParams {
            client_name: "alab-executor-version-test".to_owned(),
            resume_session_id: None,
        })
        .await
        .expect("initialize executor session");
    let wire = serde_json::to_value(response)?;
    let decoded: InitializeResponse = serde_json::from_value(wire)?;
    assert!(!decoded.session_id.is_empty());
    let mut info = decoded
        .environment_info
        .expect("initialize environment info");
    let upstream = BuildInfo::get().version().to_string();
    let expected = match mode.as_str() {
        "active" | "inactive" => CLIENT,
        "old" => "0.163.0",
        "prerelease" => "0.164.0-alpha.1",
        _ => upstream.as_str(),
    };
    assert_eq!(info.executor_version, expected);
    assert!(info.capabilities.capability_discovery_v2);
    match mode.as_str() {
        "active" | "inactive" => assert!(supports_capability_discovery_v2(&info)),
        "old" | "prerelease" => assert!(!supports_capability_discovery_v2(&info)),
        _ => {}
    }

    handler.initialized().expect("initialized notification");
    if mode == "active" {
        codex_alab_core::set_enhanced(false);
        assert!(!codex_alab_core::active());
    }
    assert_eq!(
        handler
            .environment_info()
            .expect("initialized environment info")
            .executor_version,
        expected
    );
    if client.is_some() {
        assert_eq!(codex_alab_core::version::product_version(""), PRODUCT);
    } else {
        // Reading executor metadata must not initialize an unstamped executable.
        assert_eq!(codex_alab_core::version::running_version(""), "");
    }

    // The product line reproduces the original discovery failure. A correct
    // client version must still honor an executor's advertised capability bit.
    info.executor_version = PRODUCT.to_owned();
    assert!(!supports_capability_discovery_v2(&info));
    info.executor_version = CLIENT.to_owned();
    info.capabilities.capability_discovery_v2 = false;
    assert!(!supports_capability_discovery_v2(&info));

    handler.shutdown().await;
    sessions.shutdown().await;
    println!("{COMPLETE}");
    Ok(())
}
