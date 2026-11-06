// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! ALAB(trust) glue, compiled inside codex-tui as `onboarding::directory_trust::alab_trust`
//! (one `#[path]` line at the end of `tui/src/onboarding/directory_trust.rs`): inside aterm,
//! under its harness policy's trust roots, codex-alab answers its own launch folder's trust
//! screen (`codex_alab_tui::trust`, CONTRACT §5 "folder trust").
//!
//! The first call is [`answered`], right before `check_directory_trust` builds the screen. It
//! answers `Ok(false)`, and the screen shows exactly as upstream's, unless the verdict is to trust
//! AND codex's own write took: the same `write_trusted_project` the screen's "Trust and
//! continue" calls on the embedded app-server (`persist_selected_trust`, no remote key), so the
//! entry is `projects."<trust_target>".trust_level = "trusted"`, and the caller reloads config
//! as it does after the screen. A person who quits the startup draft while aterm is asked
//! (Ctrl-C or Ctrl-D on an empty composer) quits, as during upstream's own lookup: the
//! cancellation is its `Err` (review of d5e2fc5abf, finding 4).
//!
//! The second is [`during`], around the screen's run: a screen [`answered`] left raises
//! codex-alab's attention while it waits for a person, naming its folder and the reason
//! (`codex_alab_tui::waiting`).

use std::future::Future;
use std::sync::Mutex;
use std::sync::PoisonError;

use super::OnboardingResult;
use super::OnboardingScreen;
use super::Step;
use crate::AppServerTarget;
use crate::app_server_session::AppServerSession;
use crate::config_update::RemoteProjectTrust;
use crate::config_update::write_trusted_project;
use crate::startup_draft::StartupCancelled;
use crate::startup_draft::StartupDraftPump;
use crate::tui::Tui;
use codex_alab_tui::trust;
use codex_protocol::config_types::TrustLevel;
use color_eyre::eyre::Result;

/// Trust `project` without the screen when codex-alab may (module doc): `Ok(true)` when the
/// trust was written and `consent` says so; `Ok(false)` to show the screen; the startup
/// draft's cancellation as its `Err`. Keys typed while aterm is asked go to the startup draft,
/// as during upstream's own lookup, and only a draft that draws its composer meanwhile waits
/// out a host busy for a moment (`trust::SETTLE_WITHIN`): anywhere else nothing draws, and one
/// read is made.
pub(super) async fn answered(
    tui: &mut Tui,
    app_server: &AppServerSession,
    target: &AppServerTarget,
    project: &RemoteProjectTrust,
    consent: &mut OnboardingResult,
    startup_draft: Option<&mut StartupDraftPump>,
) -> Result<bool> {
    LEFT.lock().unwrap_or_else(PoisonError::into_inner).take();
    if !trust::armed() {
        return Ok(false);
    }
    let gate = trust::Gate::at_launch(
        matches!(target, AppServerTarget::Embedded),
        project.trust_level == Some(TrustLevel::Untrusted),
        &project.cwd,
        &project.trust_target,
    );
    let verdict = match startup_draft {
        Some(draft) => {
            let settle = if draft.alab_draws() {
                trust::SETTLE_WITHIN
            } else {
                std::time::Duration::ZERO
            };
            match draft.run_until(tui, trust::judge(&gate, settle)).await {
                Ok(verdict) => verdict,
                Err(err) if StartupCancelled::matches(&err) => return Err(err.into()),
                Err(err) => trust::Verdict::Show(format!("the startup draft stopped: {err}")),
            }
        }
        None => trust::judge(&gate, std::time::Duration::ZERO).await,
    };
    let grant = match verdict {
        trust::Verdict::Trust(grant) => grant,
        trust::Verdict::Show(why) => {
            tracing::info!("alab(trust): the folder-trust screen shows: {why}");
            left(why);
            return Ok(false);
        }
    };
    if let Err(err) = write_trusted_project(app_server.request_handle(), &grant.trust_target).await
    {
        tracing::warn!(
            "alab(trust): the folder-trust screen shows: writing the trust for {} failed: {err}",
            grant.trust_target.display()
        );
        left(format!("writing the trust failed: {err}"));
        return Ok(false);
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
    Ok(true)
}

/// Why [`answered`] last left the screen to a person, for the screen it then shows ([`during`]).
static LEFT: Mutex<Option<String>> = Mutex::new(None);

fn left(why: String) {
    *LEFT.lock().unwrap_or_else(PoisonError::into_inner) = Some(why);
}

/// Run `run(screen)` with codex-alab's attention raised while a folder-trust screen
/// [`answered`] left waits for a person (`codex_alab_tui::waiting::during`: inert outside
/// aterm, outside codex-alab, and once a primary thread is registered). A screen [`answered`]
/// never judged (not codex-alab) runs alone. A plain function, so the screen's future goes on
/// the heap as the hook calls in (`codex_alab_tui::waiting`'s module doc).
pub(super) fn during<F, Fut>(screen: OnboardingScreen, run: F) -> impl Future<Output = Fut::Output>
where
    F: FnOnce(OnboardingScreen) -> Fut,
    Fut: Future,
{
    let why = LEFT.lock().unwrap_or_else(PoisonError::into_inner).take();
    let folder = screen.steps.iter().find_map(|step| match step {
        Step::TrustDirectory(widget) => Some(widget.cwd.clone()),
        _ => None,
    });
    let waiting = match (folder, why) {
        (Some(folder), Some(why)) => Some(codex_alab_tui::waiting::Waiting::trust(&folder, &why)),
        _ => None,
    };
    codex_alab_tui::waiting::during_if(waiting, run(screen))
}
