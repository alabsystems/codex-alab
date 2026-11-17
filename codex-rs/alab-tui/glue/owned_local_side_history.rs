// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Consume the genuine fresh S2 side snapshot in the ORIGINAL passive store
//! and companion. This is UI application; Native/Source completion remains
//! the existing matching tail -> End -> actual paint/input -> real Commit.
use super::super::thread_attachment::CloseState;
use super::super::thread_attachment::Incoming;
use super::super::thread_attachment::Target;
use super::*;
use crate::app_server_session::SideChatStorage;
use crate::app_server_session::alab_retained_ui::ProjectionReplay;
use crate::app_server_session::alab_retained_ui::original_resume::existing::ReconciledSide;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadHistoryMode;
use color_eyre::eyre::Result;
use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::PoisonError;

use crate::app::alab_side_history_data as provenance;
use provenance::CellSource;
use provenance::Source;

/// Owned by original App saved-openings before first S2 await. It carries
/// actual Incoming/store identity, not a supplied readiness/subscriber flag.
pub(crate) struct Capture {
    target: Target,
    request: uuid::Uuid,
    history: Option<(crate::app::AlabSideProvenance, u64)>,
}
/// Kept by the original SidebarFork/CreationInstall with its nonclone replay.
pub(crate) struct Snapshot {
    capture: Capture,
    current: Thread,
    boundary: Option<codex_app_server_client::source_selection::SideDisplayBoundary>,
    metadata: Option<crate::app_server_session::AttachmentMetadata>,
    incoming: Option<Incoming>,
    store: Option<Arc<tokio::sync::Mutex<ThreadEventStore>>>,
    prefix: Vec<(
        ThreadBufferedEvent,
        Option<Arc<crate::app::alab_side_delivery::Receipt>>,
    )>,
    store_installed: bool,
    prefix_applied: bool,
}
struct ProjectedGroup {
    sources: Vec<Source>,
    payloads: Vec<ThreadItemPayload>,
    cells: Vec<Arc<dyn HistoryCell>>,
}
#[derive(Default)]
pub(crate) struct ProjectionCapture {
    active: Option<(ThreadId, Vec<ProjectedGroup>)>,
}
impl App {
    /// Caller retains returned capture through the actual existing project
    /// future. Original Openings owns SAME Incoming while UI keeps receiving.
    pub(crate) async fn alab_capture_side_history(
        &mut self,
        server: &mut AppServerSession,
        original: &crate::app_server_session::SidebarFork,
    ) -> Result<Capture> {
        if original.boundary.is_err() {
            bail!("original side boundary failed");
        }
        let child = original.thread.session.thread_id;
        let parent = original
            .thread
            .session
            .forked_from_id
            .ok_or_else(|| eyre!("original side construction has no source"))?;
        let target = Target {
            backend_id: server.backend_id(),
            parent,
            child,
        };
        let history = if let Some(pane) = self.side_companions.get(&child) {
            Some(pane.display.alab_history.clone())
        } else if self.chat_widget.thread_id() == Some(child) {
            Some(self.chat_widget.alab_history.clone())
        } else {
            None
        };
        let history = history
            .map(|ledger| {
                let revision = ledger
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .revision()?;
                Ok::<_, color_eyre::eyre::Report>((ledger, revision))
            })
            .transpose()?;
        let request = self.alab_begin_side_history_capture(server, target).await?;
        Ok(Capture {
            target,
            request,
            history,
        })
    }
}
impl Capture {
    pub(crate) fn into_sidebar(
        self,
        app: &mut App,
        server: &AppServerSession,
        reconciled: ReconciledSide,
    ) -> Result<crate::app_server_session::SidebarFork> {
        let (original, replay, history) = self.reconcile(app, server, reconciled)?;
        original.with_retained_side_history(server, replay, history)
    }
    /// This consumes the actual peer carrier, never a reconstructed Resume.
    /// Attached paginated display history already passes the original store
    /// lineage filter. Other modes require a real original boundary provider.
    pub(crate) fn reconcile(
        self,
        app: &mut App,
        server: &AppServerSession,
        reconciled: ReconciledSide,
    ) -> Result<(
        crate::app_server_session::SidebarFork,
        ProjectionReplay,
        Snapshot,
    )> {
        let (original, response, metadata, replay) = reconciled.into_parts();
        let current = response.thread;
        let mut outcome = SideOutcome {
            pending: replay.pending().clone(),
            complete: false,
        };
        let boundary = replay.side_display_boundary()?;
        let attached = original.storage == SideChatStorage::Attached;
        // The old attached/paginated route retains its actual store lineage.
        // Other original modes require the real response-owned occurrence cut.
        if boundary.is_none()
            && (!attached
                || current.history_mode != ThreadHistoryMode::Paginated
                || current.ephemeral)
        {
            bail!("side snapshot lacks its actual child-visible history cut");
        }
        if boundary.as_ref().is_some_and(|data| !data.valid())
            || current.id != self.target.child.to_string()
            || original.thread.session.thread_id != self.target.child
            || current.forked_from_id.as_deref() != Some(self.target.parent.to_string().as_str())
            || (attached
                && current.attached_to_thread_id.as_deref()
                    != Some(self.target.parent.to_string().as_str()))
            || (!attached && current.attached_to_thread_id.is_some())
            || !metadata.subscribed
            || replay.is_primary_presentation()?
            || replay.pending().projection_target() != Some((current.id.clone(), false))
            || !replay.pending().current_on(server)
            || server.backend_id() != self.target.backend_id
        {
            bail!("fresh side snapshot changed original capture or presentation");
        }
        // Observation does not remove the real opening; take happens in the
        // inline original apply path, so notifications keep original tracking.
        app.alab_check_side_history_capture(&self.target, self.request)?;
        outcome.complete = true;
        Ok((
            original,
            replay,
            Snapshot {
                capture: self,
                current,
                boundary,
                metadata: Some(metadata),
                incoming: None,
                store: None,
                prefix: Vec::new(),
                store_installed: false,
                prefix_applied: false,
            },
        ))
    }
}
struct SideOutcome {
    pending: Arc<crate::app_server_session::alab_retained_ui::Pending>,
    complete: bool,
}
impl Drop for SideOutcome {
    fn drop(&mut self) {
        if !self.complete {
            self.pending.abandon_ui();
        }
    }
}
impl Snapshot {
    pub(crate) fn begin_install(
        &mut self,
        app: &mut App,
        server: &mut AppServerSession,
    ) -> Result<()> {
        if self.incoming.is_some() || self.store_installed {
            bail!("side history install already entered");
        }
        let incoming =
            app.alab_take_side_history_capture(&self.capture.target, self.capture.request)?;
        if incoming.received.close != CloseState::Open {
            bail!("side history capture observed a close");
        }
        let channel = app
            .thread_event_channels
            .get(&self.capture.target.child)
            .ok_or_else(|| eyre!("original side capture channel missing"))?;
        if !incoming.owns_channel(Some(channel), server.backend_id()) {
            bail!("side history store changed");
        }
        self.store = Some(channel.store.clone());
        let metadata = self
            .metadata
            .take()
            .ok_or_else(|| eyre!("original hydration metadata already consumed"))?;
        server.install_attachment_metadata(
            self.capture.target.child,
            metadata,
            incoming.received.read_state_changed,
        );
        self.incoming = Some(incoming);
        Ok(())
    }
    /// Runs in the ORIGINAL store lock at its original installation point.
    pub(in crate::app) fn install_store(
        &mut self,
        actual: &Arc<tokio::sync::Mutex<ThreadEventStore>>,
        store: &mut ThreadEventStore,
        session: crate::session_state::ThreadSessionState,
        original_turns: Vec<Turn>,
    ) -> Result<()> {
        let incoming = self
            .incoming
            .as_ref()
            .ok_or_else(|| eyre!("actual side Incoming missing"))?;
        if self.store_installed
            || store.attachment_prefix.is_none()
            || self
                .store
                .as_ref()
                .is_none_or(|expected| !Arc::ptr_eq(actual, expected))
        {
            bail!("side snapshot lost original store/prefix boundary");
        }
        let live = incoming.received.turn_changed.then(|| {
            (
                store.active_turn_id.clone(),
                store.latest_turn_id.clone(),
                store.evicted_active_turn.take(),
                store.active_reasoning_item.clone(),
            )
        });
        let mut session = session;
        if let Some(settings) = incoming.received.settings.as_ref() {
            super::super::thread_settings::apply_thread_settings_to_session(&mut session, settings);
        }
        // Preserve the original side UI boundary, then apply ONLY the fresh
        // child-visible display view from the same captured Core.
        App::install_side_thread_snapshot(store, session, original_turns);
        store.set_turns(self.current.turns.clone());
        if let Some((active, latest, evicted, reasoning)) = live {
            store.active_turn_id = active;
            store.latest_turn_id = latest;
            store.evicted_active_turn = evicted;
            store.active_reasoning_item = reasoning;
        }
        store.rebase_buffer_after_session_refresh();
        self.prefix = store
            .buffer
            .iter()
            .map(|event| {
                let receipt = store.alab_side_delivery.stored(event)?;
                let mut event = event.clone();
                if let Some(boundary) = &self.boundary
                    && !side_event_visible(&mut event, boundary)
                {
                    return Ok(None);
                }
                Ok(Some((event, receipt)))
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect();
        self.store_installed = true;
        Ok(())
    }
    /// BEFORE original prefix delivery, required tail, End, or paint witness.
    pub(crate) async fn apply_companion(
        &mut self,
        app: &mut App,
        tui: &mut crate::tui::Tui,
        server: &AppServerSession,
        identity: &Arc<()>,
        fresh: bool,
    ) -> Result<()> {
        let thread = self.capture.target.child;
        if !self.store_installed || server.backend_id() != self.capture.target.backend_id {
            bail!("fresh side history has not reached original store");
        }
        let pane = app
            .side_companions
            .get(&thread)
            .ok_or_else(|| eyre!("actual passive companion missing"))?;
        if !Arc::ptr_eq(&pane.alab_identity, identity)
            || app.chat_widget.thread_id() == Some(thread)
        {
            bail!("fresh side history changed its passive companion");
        }
        let actual_history = pane.display.alab_history.clone();
        let baseline = match &self.capture.history {
            Some((original, revision)) if Arc::ptr_eq(original, &actual_history) => *revision,
            Some(_) => bail!("original side provenance owner changed"),
            None if fresh => 0,
            None => bail!("preexisting side view was not captured before projection"),
        };
        if fresh {
            let actual = self
                .store
                .as_ref()
                .ok_or_else(|| eyre!("actual side store missing"))?
                .clone();
            let guard = actual.lock().await;
            if guard.active
                || guard
                    .session
                    .as_ref()
                    .is_none_or(|state| state.thread_id != thread)
            {
                bail!("passive side no longer owns its original store");
            }
            let pane = app
                .side_companions
                .get_mut(&thread)
                .ok_or_else(|| eyre!("same companion disappeared"))?;
            for (event, receipt) in &self.prefix {
                if let ThreadBufferedEvent::Notification(notification) = event {
                    let command = if let Some(receipt) = receipt {
                        pane.alab_apply_command(notification, receipt)?
                    } else {
                        None
                    };
                    if command.is_none() {
                        pane.apply_notification(notification, &guard, || {
                            app.chat_widget.new_status_indicator()
                        });
                    }
                }
            }
            pane.flush_answer();
            self.prefix_applied = true;
        }
        // Actual active surface is preserved and painted by the original
        // SideCompanion. Its real command prefixes/deltas above are consumed
        // through original ExecCell, once per actual stored delivery.
        app.side_companions
            .get(&thread)
            .ok_or_else(|| eyre!("actual companion disappeared"))?
            .alab_command_delivery
            .check()?;
        let prefix_keys = actual_history
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .changed_since(baseline)?;
        let runs = {
            let history = actual_history
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            ordered_runs(
                &self.current.turns,
                &prefix_keys,
                &history,
                &app.side_companions
                    .get(&thread)
                    .ok_or_else(|| eyre!("actual companion disappeared"))?
                    .cells,
            )?
        };
        let hidden = hidden_review_item_ids(&self.current.turns);
        let visibility = if app.config.show_raw_agent_reasoning {
            RawReasoningVisibility::Visible
        } else {
            RawReasoningVisibility::Hidden
        };
        app.remove_hidden_review_cells(
            tui,
            &self.current.turns,
            &hidden,
            thread,
            &self.current.cwd,
            visibility,
        );
        for run in runs {
            if app.alab_side_history_projection.active.is_some() {
                bail!("another original side projection is active");
            }
            app.alab_side_history_projection.active = Some((thread, Vec::new()));
            // A genuinely rendered producer group is a chronological boundary,
            // like an already installed page. Never join missing A/C across B.
            let _cells = app.project_older_history_cells(
                run.items,
                &self.current.turns,
                &hidden,
                thread,
                &self.current.cwd,
                visibility,
            );
            let (_, groups) = app
                .alab_side_history_projection
                .active
                .take()
                .ok_or_else(|| eyre!("side projector capture missing"))?;
            let pane = app
                .side_companions
                .get_mut(&thread)
                .ok_or_else(|| eyre!("side companion moved during synchronous projection"))?;
            apply_ordered_groups(pane, run.after.as_ref(), run.before.as_ref(), groups)?;
        }
        let pane = app
            .side_companions
            .get_mut(&thread)
            .ok_or_else(|| eyre!("actual companion disappeared"))?;
        let actual_history = pane.display.alab_history.clone();
        // Every original retained cell must have genuine producer provenance;
        // a missing legacy capture is Unknown, never an empty reset.
        let _checkpoint = actual_history
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .checkpoint(&pane.cells, history_prefix_len(&pane.cells))?;
        pane.view.history = if server.has_older_history(thread) {
            TranscriptHistoryState::Partial
        } else {
            TranscriptHistoryState::Complete
        };
        tui.frame_requester().schedule_frame();
        Ok(())
    }
}
struct OrderedRun {
    after: Option<Source>,
    before: Option<Source>,
    items: Vec<HistoryPageItem>,
}
fn ordered_runs(
    turns: &[Turn],
    changed: &HashSet<Source>,
    history: &provenance::Provenance,
    cells: &[Arc<dyn HistoryCell>],
) -> Result<Vec<OrderedRun>> {
    let mut runs = Vec::new();
    let mut items = Vec::new();
    let mut after = None;
    for turn in turns {
        for item in &turn.items {
            let source = Source::Item {
                turn: turn.id.clone(),
                item: item.id().to_owned(),
            };
            if changed.contains(&source) {
                // A removed/hidden item has no current rendered anchor. It stays
                // suppressed without inventing a physical position for it.
                if history
                    .range(&source, cells)?
                    .is_some_and(|range| !range.is_empty())
                {
                    if !items.is_empty() {
                        runs.push(OrderedRun {
                            after: after.clone(),
                            before: Some(source.clone()),
                            items: std::mem::take(&mut items),
                        });
                    }
                    after = Some(source);
                }
            } else {
                items.push(HistoryPageItem {
                    turn_id: turn.id.clone(),
                    payload: item.payload.clone(),
                });
            }
        }
    }
    if !items.is_empty() {
        runs.push(OrderedRun {
            after,
            before: None,
            items,
        });
    }
    Ok(runs)
}
fn apply_ordered_groups(
    pane: &mut super::super::side_companion::SideCompanion,
    after: Option<&Source>,
    before: Option<&Source>,
    groups: Vec<ProjectedGroup>,
) -> Result<()> {
    let actual_history = pane.display.alab_history.clone();
    let mut insertion = {
        let history = actual_history
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(before) = before {
            history
                .range(before, &pane.cells)?
                .filter(|range| !range.is_empty())
                .ok_or_else(|| eyre!("actual following side anchor moved"))?
                .start
        } else if let Some(after) = after {
            history
                .range(after, &pane.cells)?
                .filter(|range| !range.is_empty())
                .ok_or_else(|| eyre!("actual preceding side anchor moved"))?
                .end
        } else {
            history_prefix_len(&pane.cells)
        }
    };
    for group in groups {
        let existing = actual_history
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .replacement(&group.sources, &pane.cells)?;
        let range = existing
            .filter(|range| !range.is_empty())
            .unwrap_or(insertion..insertion);
        {
            let history = actual_history
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let following = before
                .map(|key| history.range(key, &pane.cells))
                .transpose()?
                .flatten();
            let preceding = after
                .map(|key| history.range(key, &pane.cells))
                .transpose()?
                .flatten();
            if (before.is_some() && following.is_none_or(|anchor| range.end > anchor.start))
                || (after.is_some() && preceding.is_none_or(|anchor| range.start < anchor.end))
            {
                bail!("fresh side snapshot crosses an actual later producer group");
            }
        }
        if let Some(first) = group.cells.first() {
            pane.view.replace_range(&pane.cells, range.clone(), first);
        } else if !range.is_empty() {
            pane.view.restart_search();
        }
        pane.alab_rebase_answer(&group.sources);
        pane.splice_older_cells(range.clone(), group.cells.clone());
        let inserted = range.start..range.start + group.cells.len();
        pane.view.history_loaded(&pane.cells, inserted.clone());
        actual_history
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .record(group.sources, group.payloads, &pane.cells[inserted.clone()])?;
        insertion = inserted.end;
    }
    Ok(())
}
impl Snapshot {
    pub(crate) fn prefix_applied(&self) -> bool {
        self.prefix_applied
    }
}

impl App {
    /// Hook immediately after ORIGINAL projector created these exact Arcs.
    pub(crate) fn alab_record_side_history_group(
        &mut self,
        thread: ThreadId,
        items: &[HistoryPageItem],
        cells: &[Arc<dyn HistoryCell>],
    ) {
        if items.is_empty() {
            return;
        }
        let sources = items
            .iter()
            .map(|item| Source::Item {
                turn: item.turn_id.clone(),
                item: item.payload.id().to_owned(),
            })
            .collect::<Vec<_>>();
        let payloads = items
            .iter()
            .map(|item| item.payload.clone())
            .collect::<Vec<_>>();
        if let Some((target, groups)) = self.alab_side_history_projection.active.as_mut()
            && *target == thread
        {
            groups.push(ProjectedGroup {
                sources,
                payloads,
                cells: cells.to_vec(),
            });
            return;
        }
        let ledger = if let Some(pane) = self.side_companions.get(&thread) {
            Some(pane.display.alab_history.clone())
        } else if self.chat_widget.thread_id() == Some(thread) {
            Some(self.chat_widget.alab_history.clone())
        } else {
            None
        };
        if let Some(ledger) = ledger {
            ledger
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .note(sources, payloads, cells);
        }
    }
    pub(crate) fn alab_record_side_history_end(
        &mut self,
        thread: ThreadId,
        turn: &str,
        cell: &Arc<dyn HistoryCell>,
    ) {
        let sources = vec![Source::TurnEnd {
            turn: turn.to_owned(),
        }];
        if let Some((target, groups)) = self.alab_side_history_projection.active.as_mut()
            && *target == thread
        {
            groups.push(ProjectedGroup {
                sources,
                payloads: Vec::new(),
                cells: vec![cell.clone()],
            });
            return;
        }
        let ledger = if let Some(pane) = self.side_companions.get(&thread) {
            Some(pane.display.alab_history.clone())
        } else if self.chat_widget.thread_id() == Some(thread) {
            Some(self.chat_widget.alab_history.clone())
        } else {
            None
        };
        if let Some(ledger) = ledger {
            ledger.lock().unwrap_or_else(PoisonError::into_inner).note(
                sources,
                Vec::new(),
                std::slice::from_ref(cell),
            );
        }
    }
}

impl App {
    pub(crate) fn alab_insert_sourced_side_history(
        &mut self,
        tui: &mut crate::tui::Tui,
        source: CellSource,
        cell: Box<dyn HistoryCell>,
    ) {
        let before = self.transcript_cells.len();
        self.insert_history_cell(tui, cell); // exact original insertion/dedup/image/scrollback policy
        if source.thread == self.chat_widget.thread_id() {
            self.chat_widget
                .alab_history
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .note(
                    vec![Source::Item {
                        turn: source.turn,
                        item: source.payload.id().to_owned(),
                    }],
                    vec![source.payload],
                    &self.transcript_cells[before..],
                );
        }
    }
}

impl App {
    pub(crate) fn alab_record_side_cell_change(
        &mut self,
        source: Option<CellSource>,
        before: &[Arc<dyn HistoryCell>],
    ) {
        let Some(source) = source.filter(|source| source.thread == self.chat_widget.thread_id())
        else {
            return;
        };
        let created = self
            .transcript_cells
            .iter()
            .filter(|cell| !before.iter().any(|old| Arc::ptr_eq(old, cell)))
            .cloned()
            .collect::<Vec<_>>();
        self.chat_widget
            .alab_history
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .note(
                vec![Source::Item {
                    turn: source.turn,
                    item: source.payload.id().to_owned(),
                }],
                vec![source.payload],
                &created,
            );
    }
}

#[cfg(test)]
#[path = "owned_local_side_history_tests.rs"]
mod tests;

fn side_event_visible(
    event: &mut ThreadBufferedEvent,
    boundary: &codex_app_server_client::source_selection::SideDisplayBoundary,
) -> bool {
    let ThreadBufferedEvent::Notification(notification) = event else {
        return true;
    };
    match notification.as_mut() {
        ServerNotification::ItemStarted(item) => {
            !boundary.excludes(&item.turn_id, item.item.payload.id())
        }
        ServerNotification::ItemCompleted(item) => {
            !boundary.excludes(&item.turn_id, item.item.payload.id())
        }
        ServerNotification::AgentMessageDelta(item) => {
            !boundary.excludes(&item.turn_id, &item.item_id)
        }
        ServerNotification::CommandExecutionOutputDelta(item) => {
            !boundary.excludes(&item.turn_id, &item.item_id)
        }
        ServerNotification::ReasoningTextDelta(item) => {
            !boundary.excludes(&item.turn_id, &item.item_id)
        }
        ServerNotification::ReasoningSummaryTextDelta(item) => {
            !boundary.excludes(&item.turn_id, &item.item_id)
        }
        ServerNotification::TurnCompleted(item) => {
            item.turn
                .items
                .retain(|entry| !boundary.excludes(&item.turn.id, entry.payload.id()));
            true
        }
        _ => true,
    }
}
