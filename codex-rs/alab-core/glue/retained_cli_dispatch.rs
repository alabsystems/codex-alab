// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Reuse the ORIGINAL multitool parser/finalizers for an actually bound backend.
//! The private argv prefix requests this route; it creates no Native permission.
//! The frontend selector remains separate and must supply genuine entry/frontier.
use std::ffi::OsString;
use std::io;

#[cfg(any(unix, test))]
const BACKEND: &str = "--alab-retained-service";
#[cfg(any(unix, test))]
fn original_arguments(arguments: &[OsString]) -> Option<Vec<OsString>> {
    if !arguments.get(1).is_some_and(|value| value == BACKEND) {
        return None;
    }
    let mut original = Vec::with_capacity(arguments.len().saturating_sub(1));
    original.push(arguments[0].clone());
    original.extend_from_slice(&arguments[2..]);
    Some(original)
}

pub(super) fn arguments() -> io::Result<Vec<OsString>> {
    #[cfg(unix)]
    if let Ok(arguments) = owned::CURRENT.try_with(|actual| actual.arguments.clone()) {
        return Ok(arguments);
    }
    codex_alab_core::native_backend_startup::arguments()
}
/// Only the real original TUI-producing subcommands may enter this backend.
/// Reject other workflows before their original dispatch/config/auth bodies.
pub(super) fn validate_route(subcommand: Option<&super::Subcommand>) -> anyhow::Result<()> {
    #[cfg(unix)]
    if owned::CURRENT.try_with(|_| ()).is_ok()
        && !matches!(
            subcommand,
            None | Some(
                super::Subcommand::Agents(_)
                    | super::Subcommand::Resume(_)
                    | super::Subcommand::Fork(_)
            )
        )
    {
        anyhow::bail!("retained backend requires the original interactive CLI workflow");
    }
    let _ = subcommand;
    Ok(())
}

#[cfg(unix)]
pub(super) use owned::run_if_requested;
#[cfg(unix)]
pub(super) use owned::start_backend_if_bound;
#[cfg(unix)]
mod owned {
    use super::*;
    use codex_alab_tui::frontend_control::retained_service::BoundBackend;
    use codex_alab_tui::frontend_control::retained_service::{self};
    use codex_tui::OrdinaryBackendRuntime;
    use futures::FutureExt as _;
    use std::panic::AssertUnwindSafe;
    use std::panic::catch_unwind;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::PoisonError;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering;

    pub(super) struct State {
        pub(super) arguments: Vec<OsString>,
        bound: BoundBackend,
        started: AtomicBool,
        // A lease moves a real owner out only while the original dispatcher
        // polls it. Drop restores it even if that whole future unwinds.
        owners: Mutex<Vec<OrdinaryBackendRuntime>>,
    }
    tokio::task_local! { pub(super) static CURRENT: Arc<State>; }
    struct RuntimeLease {
        state: Arc<State>,
        owner: Option<OrdinaryBackendRuntime>,
    }
    impl RuntimeLease {
        fn take(state: Arc<State>) -> io::Result<Self> {
            let owner = state
                .owners
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .pop()
                .ok_or_else(|| io::Error::other("ordinary backend owner unavailable"))?;
            Ok(Self {
                state,
                owner: Some(owner),
            })
        }
        fn runtime(&self) -> io::Result<&OrdinaryBackendRuntime> {
            self.owner
                .as_ref()
                .ok_or_else(|| io::Error::other("ordinary backend owner unavailable"))
        }
    }
    impl Drop for RuntimeLease {
        fn drop(&mut self) {
            if let Some(owner) = self.owner.take() {
                self.state
                    .owners
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(owner);
            }
        }
    }
    pub(crate) async fn run_if_requested(
        arg0_paths: codex_arg0::Arg0DispatchPaths,
        remote_control_disabled: bool,
    ) -> anyhow::Result<bool> {
        let raw = std::env::args_os().collect::<Vec<_>>();
        let Some(arguments) = original_arguments(&raw) else {
            return Ok(false);
        };
        // The explicit debug bound fixture has its own qualified early branch
        // before this hook. It must never be parsed as ordinary user defaults.
        #[cfg(all(feature = "alab-native-source-proof", debug_assertions))]
        if raw
            .get(2)
            .is_some_and(|value| value == "--alab-native-bound-proof")
        {
            return Ok(false);
        }
        if codex_alab_tui::upgrade_state::pending().is_some() {
            anyhow::bail!("retained backend cannot restore a source-exec forest");
        }
        let controller = codex_alab_tui::frontend_control::open().await?;
        let bound = retained_service::bind_backend(controller).await?;
        let state = Arc::new(State {
            arguments,
            bound,
            started: AtomicBool::new(false),
            owners: Mutex::new(Vec::new()),
        });
        let mut work = Box::pin(
            AssertUnwindSafe(CURRENT.scope(state.clone(), async move {
                // Same original source constructor, AFTER actual physical bind and
                // BEFORE parser-side profile/config/auth work. It is idempotent;
                // the bound original preparation retains its existing stage too.
                codex_alab_core::native_source_launch::enter().await?;
                if let Some(tui) = Box::pin(super::super::dispatch_cli(
                    arg0_paths,
                    remote_control_disabled,
                ))
                .await?
                {
                    tui.await?;
                }
                Ok::<_, anyhow::Error>(())
            }))
            .catch_unwind(),
        );
        let result = work.as_mut().await;
        let destroyed = catch_unwind(AssertUnwindSafe(|| drop(work)));
        state.bound.incoming().stop();
        let source_shutdown = state.bound.begin_source_shutdown();
        let owners =
            std::mem::take(&mut *state.owners.lock().unwrap_or_else(PoisonError::into_inner));
        // The original same-gate writer is released BEFORE real registry/
        // Native retirement; final shutdown withdraws only after all owners.
        let mut cleanup = source_shutdown.drain().await;
        for mut owner in owners {
            if let Err(error) = owner.shutdown().await {
                if cleanup.is_ok() {
                    cleanup = Err(error);
                }
            }
        }
        let backend = state.bound.shutdown().await;
        match (result, destroyed) {
            (Ok(result), Ok(())) => {
                result?;
                cleanup?;
                backend?;
                Ok(true)
            }
            _ => {
                state.bound.incoming().fail(io::Error::other(
                    "ordinary backend dispatcher outcome unobserved",
                ));
                Err(io::Error::other("ordinary backend dispatcher outcome unobserved").into())
            }
        }
    }
    pub(crate) async fn start_backend_if_bound(
        cli: &super::super::TuiCli,
        remote: Option<&str>,
        remote_auth_token_env: Option<&str>,
        arg0_paths: &codex_arg0::Arg0DispatchPaths,
    ) -> io::Result<Option<codex_tui::AppExitInfo>> {
        let Some(state) = CURRENT.try_with(Arc::clone).ok() else {
            return Ok(None);
        };
        if remote.is_some() || remote_auth_token_env.is_some() {
            return Err(io::Error::other(
                "retained local backend cannot select a remote workspace",
            ));
        }
        if state.started.swap(true, Ordering::AcqRel) {
            return Err(io::Error::other(
                "ordinary backend construction already attempted",
            ));
        }
        let owner = codex_tui::prepare_retained_local_backend_bound(
            cli.clone(),
            arg0_paths.clone(),
            codex_config::LoaderOverrides::default(),
            state.bound.clone(),
        )
        .await?
        .start()
        .await?;
        state
            .owners
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(owner);
        let lease = RuntimeLease::take(state)?;
        let completion = lease.runtime()?.runtime().wait().await;
        completion.as_ref().as_ref().map_err(|error| {
            io::Error::new(error.kind(), "ordinary retained Source runtime failed")
        })?;
        // The main owner above performs all original cloud/log/DB/telemetry
        // cleanup. No frontend EOF, label or this exit summary proves Native
        // effects/cohort settlement.
        Ok(Some(codex_tui::AppExitInfo {
            token_usage: Default::default(),
            thread_id: None,
            resume_hint: None,
            disconnect_info: None,
            update_action: None,
            exit_reason: codex_tui::ExitReason::UserRequested,
        }))
    }
}

#[cfg(test)]
#[path = "retained_cli_dispatch_tests.rs"]
mod tests;
