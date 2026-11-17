// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Original credential-reader routing selected only by actual Native E1 and
//! the mandatory whole-UI decoder. This owns DATA/custody, never UI application,
//! Native permission, a Core, or a model/Goal release.
use super::owned_local_ui_transfer::DecodedUiTransfer;
use codex_alab_tui::frontend_control::EnteredRetainedFrontend;
use codex_config::CloudConfigBundleLoader;
use codex_config::ConfigRequirementsToml;
use std::future::Future;
use std::io;
use std::sync::Arc;

/// Constructed only by decoding the WHOLE input of an actual Entered resource.
/// The original decoded object remains owned until the real UI installer has
/// consumed its native sections with actual destination/continuation owners.
pub struct RetainedFrontendReaders {
    actual: Arc<DecodedUiTransfer>,
    target: super::owned_local_frontend::OwnedLocalTarget,
    // Weak registration links the SAME original pre-await constructor to its
    // real first pump allocation without Reader->Constructor->Reader cycle.
    prepared_constructor: std::sync::Mutex<Option<std::sync::Weak<constructor::Constructor>>>,
}
impl std::fmt::Debug for RetainedFrontendReaders {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("RetainedFrontendReaders(<actual E1 DATA; not UI-applied>)")
    }
}
impl RetainedFrontendReaders {
    /// The caller still owns entry on every decode error. No malformed input
    /// falls through to an original credential constructor or default config.
    pub fn from_entered(entry: &Arc<EnteredRetainedFrontend>) -> io::Result<Arc<Self>> {
        let decoded = DecodedUiTransfer::from_entered(entry.clone()).map_err(io::Error::other)?;
        let target = super::owned_local_frontend_startup::prepare_entered(
            entry.clone(),
            decoded.snapshot().clone(),
        )?;
        let initial = match decoded.selection_data() {
            super::owned_local_ui_transfer::SelectionData::NoPrimary { initial }
            | super::owned_local_ui_transfer::SelectionData::Selected { initial, .. } => initial,
        };
        if let Some(checkpoint) = initial {
            target.restore_initial_operation(checkpoint.clone())?;
        }
        Ok(Arc::new(Self {
            actual: Arc::new(decoded),
            target,
            prepared_constructor: std::sync::Mutex::new(None),
        }))
    }
    pub fn entry(&self) -> &Arc<EnteredRetainedFrontend> {
        self.actual.entry()
    }
    pub(crate) fn decoded(&self) -> &Arc<DecodedUiTransfer> {
        &self.actual
    }
    /// Before original Tui/input initialization. Active editors require their
    /// actual SDK continuation path; decoding never supplies that resource.
    pub fn restore_terminal_before_init(&self) -> io::Result<()> {
        self.actual.restore_terminal_before_init()
    }
    /// Install on the original restored-UI dispatch only. This does not run
    /// ordinary startup/rebuild or any parser by itself. It preserves the SAME
    /// entry scope for original argv and source-seed suppression, and the SAME
    /// decoder owner for real later view/input restoration.
    pub async fn scope<F: Future>(self: &Arc<Self>, original: F) -> F::Output {
        codex_alab_tui::frontend_control::with_retained_frontend(
            self.entry().clone(),
            CURRENT.scope(self.clone(), original),
        )
        .await
    }
}
tokio::task_local! { static CURRENT: Arc<RetainedFrontendReaders>; }
fn active() -> io::Result<Option<Arc<RetainedFrontendReaders>>> {
    let entered = codex_alab_tui::frontend_control::current_retained_frontend();
    let actual = CURRENT.try_with(Arc::clone).ok();
    match (entered, actual) {
        (None, None) => Ok(None),
        (Some(entry), Some(actual)) if Arc::ptr_eq(&entry, actual.entry()) => {
            actual.decoded().snapshot().validate_data()?;
            Ok(Some(actual))
        }
        _ => Err(io::Error::other(
            "actual entered reader scope lost its mandatory whole-E1 decoder",
        )),
    }
}
/// Only actual entered/decoded scope chooses the retained startup target. This
/// has no ordinary selector, backend constructor or Native permission effect.
pub(crate) fn startup_target(
    original: crate::AppServerTarget,
) -> io::Result<crate::AppServerTarget> {
    let Some(actual) = active()? else {
        return Ok(original);
    };
    if original.uses_remote_workspace() {
        return Err(io::Error::other(
            "entered retained local frontend conflicts with original remote selection",
        ));
    }
    Ok(crate::AppServerTarget::OwnedLocal {
        target: actual.target.clone(),
    })
}
pub(crate) fn has_entered_target() -> io::Result<bool> {
    Ok(active()?.is_some())
}
pub(crate) fn frozen_cloud_loader() -> io::Result<Option<CloudConfigBundleLoader>> {
    Ok(active()?.map(|actual| actual.decoded().snapshot().ui_cloud_loader()))
}
pub(crate) async fn ensure_config_server_login(
    original_auth: impl FnOnce() -> io::Result<codex_login::AuthConfig>,
    requirements: &ConfigRequirementsToml,
) -> io::Result<()> {
    let actual = active()?;
    ensure_with_snapshot(
        actual.as_ref().map(|actual| actual.decoded().snapshot()),
        original_auth,
        requirements,
    )
    .await
}
async fn ensure_with_snapshot(
    actual: Option<&super::owned_local_ui_snapshot::UiSnapshot>,
    original_auth: impl FnOnce() -> io::Result<codex_login::AuthConfig>,
    requirements: &ConfigRequirementsToml,
) -> io::Result<()> {
    if let Some(snapshot) = actual {
        snapshot.validate_data()?;
        if snapshot.config_server_data() != requirements.config_server.as_ref() {
            return Err(io::Error::other(
                "original config-server requirements differ from actual E1 policy",
            ));
        }
        // Observe the actual frozen backend result; never construct a local
        // OAuthSessionManager/read/refresh or assume success from entry DATA.
        snapshot
            .ui_cloud_loader()
            .get()
            .await
            .map_err(io::Error::other)?;
        return Ok(());
    }
    codex_cloud_config::ensure_config_server_login(original_auth()?, requirements).await
}

/// C1 cleanup owner. Local admission remains sealed on Drop/cancellation;
/// the actual reader census and unused seed Native receipt keep their own
/// custody. This is not a Native Go/NoExec/no-effects certificate.
pub struct FrontendReaderRetirement {
    census: codex_alab_core::manager_catalog::Seal,
}
impl FrontendReaderRetirement {
    pub fn begin() -> io::Result<Self> {
        let census = codex_alab_core::manager_catalog::global()
            .seal()
            .map_err(|_| io::Error::other("actual frontend reader census unavailable"))?;
        Ok(Self { census })
    }
    pub async fn drain(&self) -> io::Result<()> {
        self.census.drain_resources().await.map_err(|_| {
            io::Error::other("actual frontend reader retirement remains unobserved")
        })?;
        if let Some(original) =
            codex_alab_core::native_source_launch::retire_unused_startup().await?
        {
            original.closed().await;
        }
        self.census
            .validate()
            .map_err(|_| io::Error::other("actual frontend reader census changed"))
    }
}
#[cfg(test)]
#[path = "owned_local_entered_reader_tests.rs"]
mod tests;

#[path = "retained_startup_constructor.rs"]
pub(crate) mod constructor; // ALAB(retained-ui): retain same mandatory decoded E1 through original UI construction.

fn register_prepared_constructor(
    owner: &Arc<constructor::Constructor>,
    readers: &Arc<RetainedFrontendReaders>,
) -> io::Result<()> {
    let mut slot = readers
        .prepared_constructor
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if slot.is_some() {
        return Err(io::Error::other(
            "entered scope already retains its original prepared constructor",
        ));
    }
    *slot = Some(Arc::downgrade(owner));
    Ok(())
}
pub(crate) fn prepared_entered_constructor() -> io::Result<Option<Arc<constructor::Constructor>>> {
    let Some(readers) = active()? else {
        return Ok(None);
    };
    let actual = readers
        .prepared_constructor
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .and_then(std::sync::Weak::upgrade)
        .ok_or_else(|| {
            io::Error::other("entered startup lost its original prepared constructor")
        })?;
    actual.check_current_scope()?;
    Ok(Some(actual))
}
/// Restrict only redundant constructor surface writes while the genuine
/// NoPrimary E1 surface awaits its real native pump install. No readiness grant.
pub(crate) fn preserves_entered_no_primary_surface() -> bool {
    match active() {
        Ok(None) => false,
        Ok(Some(actual)) => matches!(
            actual.decoded().ui_data(),
            crate::owned_local_ui_transfer::UiData::NoPrimary { .. }
        ),
        Err(_) => true, // Unknown entered ownership can only inhibit surface reset.
    }
}
