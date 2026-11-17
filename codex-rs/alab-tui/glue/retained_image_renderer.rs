// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Genuine pre-ACK renderer resources. Config/path/raster DATA cannot mint one.
use crate::app_server_session::AppServerSession;
use crate::app_server_session::alab_retained_ui;
use crate::legacy_core::config::Config;
use crate::owned_local_frontend::FrontendTenure;
use crate::render::highlight::alab_retained_image_theme as theme;
use crate::tui::Tui;
use codex_alab_tui::frontend_control::EnteredRetainedFrontend;
use codex_alab_tui::upgrade_exec_custody::images::ClaimedFiles;
use codex_alab_tui::upgrade_exec_custody::images::SealedLocator;
use codex_alab_tui::upgrade_exec_custody::images::{self};
use std::io;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
enum Frontier {
    Selected {
        owner: Arc<alab_retained_ui::Pending>,
        native: alab_retained_ui::kind::Native,
    },
    Configured {
        owner: Arc<crate::startup_draft::alab_configured::Pending>,
    },
}
/// Restrict-only local observation; it has no reset/constructor outside Binding.
#[derive(Clone)]
pub(crate) struct RestoreWatch(Arc<WatchState>);
struct WatchState {
    valid: AtomicBool,
    changed: tokio::sync::watch::Sender<()>,
}
impl RestoreWatch {
    fn new() -> Self {
        let (changed, _) = tokio::sync::watch::channel(());
        Self(Arc::new(WatchState {
            valid: AtomicBool::new(true),
            changed,
        }))
    }
    pub(crate) fn invalidate(&self) {
        if self.0.valid.swap(false, Ordering::AcqRel) {
            self.changed();
        }
    }
    pub(crate) fn changed(&self) {
        self.0.changed.send_replace(());
    }
    fn valid(&self) -> bool {
        self.0.valid.load(Ordering::Acquire)
    }
}
pub(crate) struct Binding {
    watch: RestoreWatch,
    entry: Arc<EnteredRetainedFrontend>,
    tenure: FrontendTenure,
    book: crate::retained_tui_images::Book,
    _actual_config: Config,
    frontier: Frontier,
    files: Arc<ClaimedFiles>,
    theme_at_entry: theme::ThemeStamp,
    theme_installed: Mutex<Option<(theme::ThemeStamp, u64)>>,
}
impl Binding {
    /// The original complete Config installation has already succeeded. This
    /// does not poll a new ConfigBuilder, file loader, Core or UI ACK.
    pub(crate) fn selected(
        tui: &mut Tui,
        config: &Config,
        session: &AppServerSession,
        owner: &Arc<alab_retained_ui::Pending>,
    ) -> io::Result<Arc<Self>> {
        let native = owner.image_frontier(session)?;
        Self::bind(
            tui,
            config,
            session,
            Frontier::Selected {
                owner: owner.clone(),
                native,
            },
        )
    }
    pub(crate) fn configured(
        tui: &mut Tui,
        config: &Config,
        session: &AppServerSession,
        owner: &Arc<crate::startup_draft::alab_configured::Pending>,
    ) -> io::Result<Arc<Self>> {
        if !owner.current_on(session) {
            return Err(io::Error::other("configured image owner changed delivery"));
        }
        Self::bind(
            tui,
            config,
            session,
            Frontier::Configured {
                owner: owner.clone(),
            },
        )
    }
    fn bind(
        tui: &mut Tui,
        config: &Config,
        session: &AppServerSession,
        frontier: Frontier,
    ) -> io::Result<Arc<Self>> {
        let tenure = session
            .retained_frontend()
            .ok_or_else(|| io::Error::other("image renderer lacks actual final delivery"))?
            .clone();
        let entry = tenure
            .entered_image_resource()?
            .ok_or_else(|| io::Error::other("image renderer lacks genuine entered custody"))?;
        let locator = SealedLocator::from_entered(entry.clone())?;
        let files = images::claim(&locator)?;
        // SAME received fd/unlink owners enter real Tui storage before the
        // transport ledger may release its strong pending claim pin.
        tui.alab_image_owner.bind_files(entry.clone(), &files)?;
        let theme_at_entry = theme::ThemeStamp::capture()?;
        let result = Arc::new(Self {
            watch: RestoreWatch::new(),
            entry,
            tenure,
            book: tui.alab_image_owner.clone(),
            _actual_config: config.clone(),
            frontier,
            files,
            theme_at_entry,
            theme_installed: Mutex::new(None),
        });
        result.check()?;
        Ok(result)
    }
    pub(crate) fn is_selected_for(&self, owner: &Arc<alab_retained_ui::Pending>) -> bool {
        matches!(&self.frontier,Frontier::Selected {owner:actual,..} if Arc::ptr_eq(actual,owner))
    }
    pub(crate) fn changes(&self) -> tokio::sync::watch::Receiver<()> {
        self.watch.0.changed.subscribe()
    }
    pub(crate) fn restore_watch(&self) -> RestoreWatch {
        self.watch.clone()
    }
    pub(crate) fn check(&self) -> io::Result<()> {
        if !self.watch.valid() {
            return Err(io::Error::other(
                "actual image restoration remains unresolved",
            ));
        }
        let actual = self
            .tenure
            .entered_image_resource()?
            .ok_or_else(|| io::Error::other("image renderer lost actual E1"))?;
        if !Arc::ptr_eq(&actual, &self.entry) || !self.files.same_entry(&self.entry) {
            return Err(io::Error::other("image renderer changed actual E1"));
        }
        let valid = match &self.frontier {
            Frontier::Selected { owner, native } => {
                owner.belongs_to(&self.tenure)
                    && owner.same_native(native)
                    && self.tenure.is_ui_frontier(owner)
            }
            Frontier::Configured { owner } => {
                owner.belongs_to(&self.tenure)
                    && self.tenure.is_configured_frontier(owner)
                    && owner
                        .native_frontier()
                        .is_for_controller(self.tenure.controller())
            }
        };
        if valid {
            Ok(())
        } else {
            Err(io::Error::other(
                "image renderer's pre-ACK projection is no longer canonical",
            ))
        }
    }
    pub(crate) fn install_runtime_theme(
        &self,
        tui: &mut Tui,
        captured: &theme::ThemeStamp,
        prepared: Option<syntect::highlighting::Theme>,
    ) -> io::Result<()> {
        self.check()?;
        if !self.belongs_to(tui) {
            return Err(io::Error::other("theme target is a different actual Tui"));
        }
        // Genuine C1 absence does not erase a real C2 theme initialization.
        let Some(prepared) = prepared else {
            return Ok(());
        };
        // Acquire fallible bookkeeping before native mutation. The native helper
        // tests/changes revision under SAME Theme write lock, no check/set gap.
        let mut installed = self
            .theme_installed
            .lock()
            .map_err(|_| io::Error::other("native theme installation interrupted"))?;
        if let Some((original, revision)) = &*installed {
            let current = theme::ThemeStamp::capture()?;
            return if original.semantic_eq(captured)
                && current.semantic_eq(captured)
                && current.revision_data() == *revision
            {
                Ok(())
            } else {
                Err(io::Error::other(
                    "native theme already installed or edited by another original action",
                ))
            };
        }
        let revision = theme::conditional_install(&self.theme_at_entry, prepared)?;
        *installed = Some((captured.clone(), revision));
        Ok(())
    }
    pub(crate) fn files(&self) -> io::Result<&Arc<ClaimedFiles>> {
        self.check()?;
        Ok(&self.files)
    }
    pub(crate) fn belongs_to(&self, tui: &Tui) -> bool {
        self.book.same_owner(&tui.alab_image_owner)
            && tui
                .alab_image_renderer
                .as_ref()
                .is_some_and(|current| std::ptr::eq(current.as_ref(), self))
    }
}

/// Mandatory whole-App assembler entry BEFORE image leaf decode/install/paint.
/// An event-loop call later cannot substitute for this original restore boundary.
pub(crate) fn before_selected_restore(
    tui: &mut Tui,
    config: &Config,
    session: &AppServerSession,
    owner: &Arc<alab_retained_ui::Pending>,
) -> io::Result<Arc<Binding>> {
    let actual = Binding::selected(tui, config, session, owner)?;
    let retired = tui.alab_image_renderer.replace(actual.clone());
    if let Some(retired) = retired {
        retired.watch.changed();
    }
    Ok(actual)
}
pub(crate) fn before_configured_restore(
    tui: &mut Tui,
    config: &Config,
    session: &AppServerSession,
    owner: &Arc<crate::startup_draft::alab_configured::Pending>,
) -> io::Result<Arc<Binding>> {
    let actual = Binding::configured(tui, config, session, owner)?;
    let retired = tui.alab_image_renderer.replace(actual.clone());
    if let Some(retired) = retired {
        retired.watch.changed();
    }
    Ok(actual)
}

/// Restrict-only held-draw disposition; neither absence nor a wake is readiness.
pub(crate) fn holds_ui_ack(tui: &Tui) -> bool {
    tui.alab_image_renderer
        .as_ref()
        .is_some_and(|actual| actual.check().is_err())
}

/// A completed earlier frontier does not inhibit a later genuine projection.
/// Only this SAME actual pending restoration can add the local restriction.
pub(crate) fn holds_selected_ui_ack(tui: &Tui, owner: &Arc<alab_retained_ui::Pending>) -> bool {
    tui.alab_image_renderer
        .as_ref()
        .is_some_and(|actual| actual.is_selected_for(owner) && actual.check().is_err())
}

/// Original startup's Config is fully installed. Establish the actual C2
/// binding BEFORE its first image-cut await, and reuse it on held retries.
/// Genuine C1 has no Entered payload; that absence grants no renderer resource.
pub(crate) fn ensure_configured_binding(
    tui: &mut Tui,
    config: &Config,
    session: &AppServerSession,
    owner: &Arc<crate::startup_draft::alab_configured::Pending>,
) -> io::Result<Option<Arc<Binding>>> {
    if !owner.current_on(session) {
        return Err(io::Error::other(
            "configured image ownership changed its actual session",
        ));
    }
    let tenure = session
        .retained_frontend()
        .ok_or_else(|| io::Error::other("configured image ownership lacks actual delivery"))?;
    if tenure.entered_image_resource()?.is_none() {
        if tui.alab_image_renderer.is_some() {
            return Err(io::Error::other(
                "entered image owner cannot become ordinary C1",
            ));
        }
        return Ok(None);
    }
    if let Some(actual) = &tui.alab_image_renderer
        && matches!(&actual.frontier,Frontier::Configured {owner:current} if Arc::ptr_eq(current,owner))
    {
        return Ok(Some(actual.clone()));
    }
    before_configured_restore(tui, config, session, owner).map(Some)
}
