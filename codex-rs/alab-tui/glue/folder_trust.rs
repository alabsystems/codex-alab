// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! ALAB(trust) glue, compiled inside codex-tui as `onboarding::directory_trust::alab_trust`
//! (one `#[path]` line at the end of `tui/src/onboarding/directory_trust.rs`): inside aterm,
//! under its harness policy's trust roots, codex-alab answers its own launch folder's trust
//! screen (`codex_alab_tui::trust`, CONTRACT §5 "folder trust").
//!
//! The one call is [`answered`], right before `check_directory_trust` builds the screen. It
//! answers `false`, and the screen shows exactly as upstream's, unless the verdict is to trust
//! AND codex's own write took: the same `write_trusted_project` the screen's "Trust and
//! continue" calls on the embedded app-server (`persist_selected_trust`, no remote key), so the
//! entry is `projects."<trust_target>".trust_level = "trusted"`, and the caller reloads config
//! as it does after the screen.

use super::OnboardingResult;
use crate::AppServerTarget;
use crate::app_server_session::AppServerSession;
use crate::config_update::RemoteProjectTrust;
use crate::config_update::write_trusted_project;
use crate::startup_draft::StartupDraftPump;
use crate::tui::Tui;
use codex_alab_tui::trust;
use codex_protocol::config_types::TrustLevel;

/// Trust `project` without the screen when codex-alab may (module doc): `true` when the trust
/// was written and `consent` says so; `false` to show the screen. Keys typed while aterm is
/// asked go to the startup draft, as during upstream's own lookup.
pub(super) async fn answered(
    tui: &mut Tui,
    app_server: &AppServerSession,
    target: &AppServerTarget,
    project: &RemoteProjectTrust,
    consent: &mut OnboardingResult,
    startup_draft: Option<&mut StartupDraftPump>,
) -> bool {
    if !trust::armed() {
        return false;
    }
    let gate = trust::Gate::at_launch(
        matches!(target, AppServerTarget::Embedded),
        project.trust_level == Some(TrustLevel::Untrusted),
        &project.cwd,
        &project.trust_target,
    );
    let verdict = match startup_draft {
        Some(draft) => match draft.run_until(tui, trust::judge(&gate)).await {
            Ok(verdict) => verdict,
            Err(err) => trust::Verdict::Show(format!("the startup draft stopped: {err}")),
        },
        None => trust::judge(&gate).await,
    };
    let grant = match verdict {
        trust::Verdict::Trust(grant) => grant,
        trust::Verdict::Show(why) => {
            tracing::info!("alab(trust): the folder-trust screen shows: {why}");
            return false;
        }
    };
    if let Err(err) = write_trusted_project(app_server.request_handle(), &grant.trust_target).await
    {
        tracing::warn!(
            "alab(trust): the folder-trust screen shows: writing the trust for {} failed: {err}",
            grant.trust_target.display()
        );
        return false;
    }
    tracing::info!(
        "alab(trust): trusted {} for {} under trust root {} ({})",
        grant.trust_target.display(),
        grant.folder.display(),
        grant.root.as_deref().unwrap_or("- (approve=all)"),
        grant.rule()
    );
    trust::note_trusted(&grant);
    consent.directory_trust_persisted = true;
    true
}
