//! A saved-child subscription is prepared on its originating connection. Only the
//! latest chooser intent may install it. Release is serialized with reopening that child.

use super::thread_attachment;
use super::thread_attachment::CloseState;
use super::thread_attachment::Incoming;
use super::thread_attachment::Prepared;
use super::*;
use crate::app_server_session::SideChatStorage;
use crate::backend_id::BackendId;
use crate::sidebar_chooser::OpenState;
use crate::sidebar_chooser::Opening;
use uuid::Uuid;

pub(crate) use super::thread_attachment::Target;

struct Flight {
    request: Uuid,
    started: Instant,
    retired: bool,
    // None while releasing an unclaimed subscription.
    incoming: Option<Incoming>,
}

pub(super) struct Openings {
    active: HashMap<(BackendId, ThreadId), Flight>,
    saved_history: saved_history::History, // ALAB(retained-ui): historical observations outlive the failed flight independently of the live channel.
    // The app event channel rotates on reconnect. This pair has the lifetime of the flights.
    tx: AppEventSender,
    completions: mpsc::UnboundedReceiver<AppEvent>,
}

impl Default for Openings {
    fn default() -> Self {
        let (tx, completions) = mpsc::unbounded_channel();
        Self {
            active: HashMap::new(),
            saved_history: Default::default(), // ALAB(retained-ui): no missing original results at startup.
            tx: AppEventSender::new(tx),
            completions,
        }
    }
}

impl Openings {
    pub(super) async fn next_event(
        &mut self,
        events: &mut mpsc::UnboundedReceiver<AppEvent>,
    ) -> Option<AppEvent> {
        tokio::select! {
            event = events.recv() => event,
            event = self.completions.recv() => event,
        }
    }

    pub(super) fn preparing(&self, child: ThreadId, channel: Option<&ThreadEventChannel>) -> bool {
        channel.is_some_and(|channel| {
            channel
                .backend_id
                .and_then(|owner| self.active.get(&(owner, child)))
                .and_then(|flight| flight.incoming.as_ref())
                .is_some_and(|incoming| incoming.uses_channel(channel))
        })
    }

    pub(super) fn abandon(&mut self) {
        for flight in self.active.values_mut() {
            flight.retired = true;
        }
    }

    /// A provisional close is applied only after checking which runtime the resume attached.
    pub(super) fn observe(
        &mut self,
        owner: BackendId,
        child: ThreadId,
        channel: Option<&ThreadEventChannel>,
        notification: &ServerNotification,
    ) -> bool {
        let Some(updates) = self
            .active
            .get_mut(&(owner, child))
            .filter(|flight| !flight.retired)
            .and_then(|flight| flight.incoming.as_mut())
            .filter(|incoming| incoming.owns_channel(channel, owner))
        else {
            return false;
        };
        let updates = &mut updates.received;
        match notification {
            ServerNotification::ThreadSettingsUpdated(n) => {
                updates.settings = Some(n.thread_settings.clone())
            }
            ServerNotification::ThreadReadStateChanged(_) => updates.read_state_changed = true,
            ServerNotification::TurnStarted(_) | ServerNotification::TurnCompleted(_) => {
                updates.turn_changed = true
            }
            ServerNotification::Error(n) if !n.will_retry && n.error.codex_error_info == Some(AppServerCodexErrorInfo::MisalignmentPolicyViolation) => updates.turn_changed = true, // ALAB(misalignment-review): a sparse live refusal also outranks the saved read in flight.
            ServerNotification::ThreadClosed(_) => {
                updates.close = CloseState::Reported;
                return true;
            }
            ServerNotification::ThreadStarted(_) => updates.close = CloseState::Open,
            _ => {}
        }
        false
    }
}

impl App {
    pub(super) fn waiting_saved_chat(&self) -> Option<Target> {
        let opening = self.sidebar()?.opening.as_ref()?;
        matches!(opening.state, OpenState::Waiting).then_some(opening.target)
    }

    pub(super) fn wants_saved_chat(&self, target: Target) -> bool {
        !self.reconnect.offline
            && self.current_displayed_thread_id() == Some(target.parent)
            && self.chooser_focus() == Some(sidebar::PaneFocus::Sidebar)
            && self
                .sidebar()
                .and_then(|pane| pane.opening.as_ref())
                .is_some_and(|o| o.target == target && !matches!(o.state, OpenState::Failed(_)))
    }

    pub(super) async fn open_saved_chat(
        &mut self,
        tui: &mut tui::Tui,
        server: &mut AppServerSession,
        target: Target,
    ) -> Result<()> {
        if !self.can_select_thread() {
            return Ok(());
        }
        let child = target.child;
        self.saved_openings.alab_retry_saved_history(server, Some(child)); // ALAB(retained-ui): renewed interest may observe an earlier pending reply without replaying its operation.
        let flight = self.saved_openings.active.get(&target.key());
        let state = if let Some(flight) = flight
            && flight.incoming.is_some()
            && !flight.retired
        {
            OpenState::Loading(flight.started)
        } else if flight.is_some() || self.saved_openings.active.len() >= 2 || self.saved_openings.alab_saved_history_full() { // ALAB(retained-ui): reserve space for this original without evicting unresolved history.
            OpenState::Waiting
        } else {
            let side = side::SideThreadState {
                storage: SideChatStorage::Attached,
                ..side::SideThreadState::new(target.parent)
            };
            self.abandoned_side_threads.remove(&child);
            if !self.should_attach_live_thread_for_selection(child)
                && let Some(channel) = self.thread_event_channels.get(&child)
                && channel.backend_id.is_none_or(|id| id == target.backend_id)
                && channel.store.lock().await.session.is_some()
            {
                self.side_threads.insert(child, side);
                return self.select_agent_thread(tui, server, child).await;
            }
            let request = Uuid::new_v4();
            let started = Instant::now();
            let incoming = Incoming::begin(self, server, target, request).await; // ALAB(retained-ui): correlate provisional release with this original opening.
            self.saved_openings.active.insert(
                target.key(),
                Flight {
                    request,
                    started,
                    retired: false,
                    incoming: Some(incoming),
                },
            );
            let view = server.alab_saved_attachment_view(child, request); // ALAB(retained-ui): bind this chooser UUID before its original construction starts.
            let config = self.config.clone();
            let settings = self.local_settings.clone();
            let tx = self.saved_openings.tx.clone();
            server.alab_spawn_original_work(async move { // ALAB(retained-ui): account for the original resume until its completion reaches the chooser.
                let prepared =
                    thread_attachment::prepare_saved(view, &settings, &config, child).await;
                tx.send(AppEvent::SavedChatPrepared {
                    target,
                    request,
                    result: Box::new(prepared),
                });
            });
            OpenState::Loading(started)
        };
        self.sidebar_mut().opening = Some(Opening { target, state });
        tui.frame_requester().schedule_frame();
        Ok(())
    }

    pub(super) async fn finish_saved_chat(
        &mut self,
        tui: &mut tui::Tui,
        server: &mut AppServerSession,
        target: Target,
        request: Uuid,
        mut prepared: Prepared,
    ) -> Result<()> {
        let child = target.child;
        let wants = server.backend_id() == target.backend_id
            && self.wants_saved_chat(target)
            && self.conversation_focus_available(tui)
            && self.sidebar_chooser_available()
            && self.can_select_thread();
        let Some(flight) = self
            .saved_openings
            .active
            .get_mut(&target.key())
            .filter(|f| f.request == request)
        else {
            return Ok(());
        };
        let Some(mut incoming) = flight.incoming.take() else {
            return Ok(());
        };
        let current = wants
            && !flight.retired
            && incoming.owns_channel(self.thread_event_channels.get(&child), target.backend_id);
        if current
            && incoming.received.close == CloseState::Reported
            && prepared.result.as_ref().is_ok_and(super::alab_attachment::is_live) // ALAB(source-selection): the same real close check also covers a retained live attachment.
        {
            incoming.received.close = CloseState::Checking;
            flight.incoming = Some(incoming);
            let mut view = server.request_view();
            let tx = self.saved_openings.tx.clone();
            server.alab_spawn_original_work(async move { // ALAB(retained-ui): retain the original close check through publication.
                prepared.close_check = match view.thread_read(child, /*include_turns*/ false).await
                {
                    Ok(thread) => {
                        Ok(thread.status == codex_app_server_protocol::ThreadStatus::NotLoaded)
                    }
                    Err(err) if App::is_terminal_thread_read_error(&err) => Ok(true),
                    Err(err) => Err(err.to_string()),
                };
                tx.send(AppEvent::SavedChatPrepared {
                    target,
                    request,
                    result: Box::new(prepared),
                });
            });
            return Ok(());
        }
        if incoming.received.close == CloseState::Checking {
            match prepared.close_check {
                Ok(true) => incoming.received.close = CloseState::Closed,
                Ok(false) => incoming.received.close = CloseState::Open,
                Err(error) => prepared.result = Err(error),
            }
        }
        if current && let Ok(attachment) = prepared.result {
            self.finish_saved_release(target, request);
            return self
                .install_saved_chat(
                    tui,
                    server,
                    target,
                    attachment,
                    prepared.metadata,
                    &incoming.received,
                )
                .await;
        }
        let release = incoming
            .discard(self, target, prepared.metadata.subscribed || prepared.unreadable_resume || prepared.missing_resume.is_some()) // ALAB(retained-ui): reserve claim-aware C1 cleanup for received or still-unresolved originals.
            .await;
        if let Some(pane) = self.sidebars.get_mut(&Some(target.parent))
            && pane
                .opening
                .as_ref()
                .is_some_and(|o| o.target == target && matches!(o.state, OpenState::Loading(_)))
        {
            pane.opening = if current {
                prepared.result.err().map(|message| Opening {
                    target,
                    state: OpenState::Failed(message),
                })
            } else {
                None
            };
        }
        let release = self.saved_openings.alab_remember_saved_resume(target, request, prepared.missing_resume, release, server, self.reconnect.offline); // ALAB(retained-ui): historical reads keep their C1 candidate until its original outcome is known.
        if let Some(release) = release {
            release.publish(self.saved_openings.tx.clone()); // ALAB(retained-ui): the reserved unsubscribe carries its original target, operation and physical session.
        } else {
            self.finish_saved_release(target, request);
        }
        tui.frame_requester().schedule_frame();
        Ok(())
    }

    pub(super) fn finish_saved_release(&mut self, target: Target, request: Uuid) {
        if self
            .saved_openings
            .active
            .get(&target.key())
            .is_some_and(|flight| flight.request == request)
        {
            self.saved_openings.active.remove(&target.key());
            if self.waiting_saved_chat().is_some() {
                self.saved_openings.tx.send(AppEvent::StartQueuedSavedChat);
            }
        }
    }
}

#[path = "../../../alab-tui/glue/saved_resume_history.rs"]
pub(crate) mod saved_history; // ALAB(retained-ui): exact original outcome lookup across a physical reconnect.

#[cfg(unix)]
#[path = "../../../alab-tui/glue/owned_local_side_history_capture.rs"]
mod alab_retained_side_capture; // ALAB(retained-ui): reuse actual Incoming/Openings observer custody for S2.

#[path = "../../../alab-tui/glue/lifecycle_actor_events.rs"]
mod alab_lifecycle; // ALAB(life): process control belongs to the actor's persistent receiver.

#[cfg(test)]
#[path = "../../../alab-tui/glue/saved_chat_work_checks.rs"]
pub(in crate::app) mod alab_saved_chat_work_checks; // ALAB(retained-ui): observe the original saved-chat worker and its completion queue in the existing integration scenarios.
