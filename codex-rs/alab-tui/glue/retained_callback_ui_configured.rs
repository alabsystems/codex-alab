// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Closed OWN callback endpoints under the genuine configured/NoPrimary route.
//! The caller must be the actual original configured UI renderer. No synthetic
//! Core, empty App, model release, or Runtime receipt substitutes for that owner.
use super::*;
use crate::startup_draft::alab_configured::Pending as Configured;
use codex_app_server_client::retained_configured::ConfigurationCommit;

pub(super) struct Applied {
    observation: Arc<ConfiguredCallbackObservation>,
    pending: Arc<Configured>,
    commit: Option<Arc<ConfigurationCommit>>,
    closed: Vec<RequestId>,
}
pub(super) fn all_closed(
    ticket: &Ticket,
    observation: &ConfiguredCallbackObservation,
) -> io::Result<Vec<RequestId>> {
    let Some(facts) = observation.facts() else {
        if observation.ticket_retired() {
            return Ok(ticket.ids.clone());
        }
        return Err(invalid("configured callback cut omitted Source facts"));
    };
    if facts.len() != ticket.ids.len() {
        return Err(invalid("configured cut lost a captured ID"));
    }
    for (fact, id) in facts.iter().zip(&ticket.ids) {
        if fact.request_id_data() != id
            || fact.thread_id_data() != Some(ticket.thread.as_str())
            || !matches!(
                fact.outcome_data(),
                Outcome::Response {}
                    | Outcome::ClientError {}
                    | Outcome::Cancelled {}
                    | Outcome::Withdrawn {}
            )
        {
            return Err(invalid(
                "configured observation lacks exact OWN closed cells",
            ));
        }
    }
    Ok(ticket.ids.clone())
}
impl App {
    /// The original App configured renderer calls BEFORE its fresh draw and
    /// Native UIACK. This is a real graph consumer, not an applied-on-read flag.
    /// Original StartupDraft's pre-App flow needs its own same-owner adapter;
    /// calling this on a fabricated/default App is explicitly invalid.
    pub(crate) async fn alab_reconcile_configured_callbacks(
        &mut self,
        session: &AppServerSession,
        pending: &Arc<Configured>,
    ) -> io::Result<bool> {
        let Some(owner) = ui(self, session)? else {
            return Ok(false);
        };
        let Some(receipt) = pending.callback_receipt(session)? else {
            return Ok(false);
        };
        self.alab_flush_original_inputs(session).await?;
        let mut changed = false;
        for transfer in owner.all() {
            transfer.validate(session)?;
            if transfer.original_graph {
                continue;
            }
            if transfer.unknown_application.load(Ordering::Acquire) {
                return Err(invalid("configured callback application remains unknown"));
            }
            let mut cuts = Vec::new();
            for ticket in &transfer.tickets {
                if transfer
                    .configured
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .get(&ticket.key)
                    .is_some_and(|actual| Arc::ptr_eq(&actual.pending, pending))
                {
                    continue;
                }
                let capture = ticket.capture(session).await?;
                let observation = Arc::new(
                    capture
                        .observe_configured(&session.request_handle(), read_request(), &receipt)
                        .await
                        .map_err(io::Error::other)?,
                );
                let closed = all_closed(ticket, &observation)?;
                cuts.push((ticket.clone(), observation, closed));
            }
            if cuts.is_empty() {
                continue;
            }
            let mut guard = ApplicationGuard {
                transfer: &transfer,
                finished: false,
            };
            for (ticket, _, closed) in &cuts {
                for id in closed {
                    self.alab_apply_closed_callback(&ticket.thread, id);
                }
            }
            // Every captured view must be proven closed by its exact Source
            // cell. No form/callback is constructed when the Core has ended.
            let closed: HashSet<_> = cuts
                .iter()
                .flat_map(|(_, _, ids)| ids.iter().cloned())
                .collect();
            for (index, view) in transfer.views.iter().enumerate() {
                if transfer
                    .applied_views
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .contains(&index)
                {
                    continue;
                }
                if !view.request_ids_data().iter().all(|id| closed.contains(id)) {
                    return Err(invalid("NoPrimary graph contains an unobserved callback"));
                }
                self.alab_install_callback_editing(
                    session,
                    view,
                    &closed,
                    transfer.limits.bytes,
                    transfer.limits.generations,
                )?;
                transfer
                    .applied_views
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .insert(index);
            }
            let mut configured = transfer
                .configured
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            for (ticket, observation, closed) in cuts {
                configured.insert(
                    ticket.key.clone(),
                    Applied {
                        observation,
                        pending: pending.clone(),
                        commit: None,
                        closed,
                    },
                );
            }
            guard.finished = true;
            changed = true;
        }
        Ok(changed)
    }
    /// Only after real configured UI application/draw and the actual opaque
    /// ConfigurationCommit. This does NOT release Source Hold/startup input.
    pub(crate) async fn alab_ack_configured_callbacks(
        &mut self,
        session: &AppServerSession,
    ) -> io::Result<()> {
        let Some(owner) = ui(self, session)? else {
            return Ok(());
        };
        for transfer in owner.all() {
            for ticket in &transfer.tickets {
                if ticket.retired() {
                    continue;
                }
                let prepared = transfer
                    .configured
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .get(&ticket.key)
                    .map(|actual| {
                        (
                            actual.observation.clone(),
                            actual.pending.clone(),
                            actual.commit.clone(),
                            actual.closed.clone(),
                        )
                    });
                let Some((observation, pending, prior, mut ids)) = prepared else {
                    continue;
                };
                ids.retain(|id| !self.alab_callback_value_needs_source_binding(&owner, id));
                if ids.is_empty() {
                    continue;
                }
                let commit = match prior {
                    Some(actual) => actual,
                    None => {
                        let Some(actual) = pending.callback_commit(session)? else {
                            continue;
                        };
                        if let Some(applied) = transfer
                            .configured
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .get_mut(&ticket.key)
                        {
                            applied.commit = Some(actual.clone());
                        }
                        actual
                    }
                };
                let capture = ticket.capture(session).await?;
                let retirement = capture
                    .acknowledge_configured(
                        &session.request_handle(),
                        read_request(),
                        &observation,
                        &commit,
                        ids,
                    )
                    .await
                    .map_err(io::Error::other)?;
                ticket
                    .progress
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .retirement = Some(retirement);
                // Original human value/publication owners remain independently held.
            }
        }
        Ok(())
    }
}
