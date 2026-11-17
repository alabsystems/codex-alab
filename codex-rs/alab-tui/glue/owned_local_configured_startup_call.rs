// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Child of original app::startup: preserve the WHOLE existing preparation.
use super::*;

/// These are the ORIGINAL required borrows; no config/loader input is rebuilt.
pub(super) struct Inputs<'a> {
    pub(super) config: &'a mut Config,
    pub(super) app_server: &'a mut AppServerSession,
    pub(super) target: &'a AppServerTarget,
    pub(super) cli_kv_overrides: &'a [(String, TomlValue)],
    pub(super) harness_overrides: &'a ConfigOverrides,
    pub(super) environments: &'a EnvironmentManager,
}
pub(super) async fn prepare(
    pump: &mut StartupDraftPump,
    tui: &mut crate::tui::Tui,
    inputs: Inputs<'_>,
) -> std::io::Result<Result<crate::projectless::AppliedDefaults>> {
    let Inputs {
        config,
        app_server,
        target,
        cli_kv_overrides,
        harness_overrides,
        environments,
    } = inputs;
    #[cfg(unix)]
    let pending = if app_server.retained_frontend().is_some() {
        // Retain the actual one-task Native begin BEFORE the first await. The
        // original StartupDraftPump remains the input owner while observing it.
        let original_owner = pump.alab_constructor_owner(|| None)?;
        let _reply_owner = original_owner.prepare_reply_owner(app_server)?;
        if !original_owner.requires_configured_projection()? {
            return pump
                .run_until(
                    tui,
                    prepare_fresh_startup_config(
                        config,
                        app_server,
                        target,
                        cli_kv_overrides,
                        harness_overrides,
                        environments,
                    ),
                )
                .await;
        }
        let begin = original_owner.configured_begin(app_server)?;
        let pending = pump.run_until(tui, begin.observe(app_server)).await??;
        pump.alab_adopt_configured_pending(app_server, pending.clone())?;
        Some(pending)
    } else {
        pump.alab_configured_pending()
    };
    let original = prepare_fresh_startup_config(
        config,
        app_server,
        target,
        cli_kv_overrides,
        harness_overrides,
        environments,
    );
    #[cfg(not(unix))]
    return pump.run_until(tui, original).await;
    #[cfg(unix)]
    {
        let defaults = match pending {
            Some(pending) => pump.run_until(tui, pending.project(original)).await?,
            None => return pump.run_until(tui, original).await,
        };
        let defaults = match defaults {
            Ok(defaults) => defaults,
            Err(error) => return Ok(Err(error)),
        };
        let pending = pump
            .alab_configured_pending()
            .ok_or_else(|| std::io::Error::other("configured owner disappeared"))?;
        // Keep the ORIGINAL provisional editor/input pump live until the actual
        // same-session matching required tail is consumed, never queue emptiness.
        pump.run_until(tui, app_server.alab_wait_configured_tail(&pending))
            .await??;
        loop {
            match pump
                .alab_observe_configured(tui, app_server, config)
                .await?
            {
                crate::startup_draft::alab_configured::Observation::Observed => break,
                crate::startup_draft::alab_configured::Observation::Held(actual) => {
                    // SAME pump/Pending/replay remains live through real input
                    // and native plan changes; no return can pretend UI ACK.
                    pump.run_until(tui, actual.wait()).await??;
                }
            }
        }
        Ok(Ok(defaults))
    }
}
