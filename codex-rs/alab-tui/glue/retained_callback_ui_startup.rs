// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Same entered graph and genuine pre-App configured owner. No default App,
//! callback recreation, Core lookup, or startup/Hold release is supplied.
use super::*;
use crate::startup_draft::alab_configured::Pending as Configured;

struct Observed {
    ticket: Arc<Ticket>,
    cut: Arc<ConfiguredCallbackObservation>,
}
pub(crate) struct StartupCallbacks {
    owner: Arc<CallbackUi>,
    _replies: crate::app::RetainedReplyOwner,
    transfer: Arc<Transfer>,
    configured: std::sync::Weak<Configured>,
    serial: Semaphore,
    cuts: Mutex<Vec<Observed>>,
    // Retain the actual completion before any endpoint ACK await.
    commit: Mutex<Option<Arc<codex_app_server_client::retained_configured::ConfigurationCommit>>>,
}
impl StartupCallbacks {
    pub(crate) fn limits_data(
        section: &serde_json::value::RawValue,
        service: &str,
    ) -> io::Result<(usize, usize)> {
        let data: TransferData = serde_json::from_str(section.get()).map_err(invalid)?;
        let limits = data.limits;
        if data.schema != 3
            || data.service != service
            || !token(&data.graph)
            || limits.bytes == 0
            || limits.bytes > codex_alab_tui::upgrade_exec_custody::MAX_STATE_BYTES
            || limits.generations == 0
            || limits.generations > limits.bytes
            || section.get().len() > limits.bytes
        {
            return Err(invalid("startup callback section changed actual budgets"));
        }
        let _original = requests(&data.views, &data.replies, limits.bytes)?;
        Ok((limits.bytes, limits.generations))
    }
    /// Called only by the original entered Constructor, synchronously before
    /// the first Source observation. This owns ALL original graph/operations.
    pub(crate) fn restore(
        session: &AppServerSession,
        pending: &Arc<Configured>,
        section: &serde_json::value::RawValue,
        replies: crate::app::RetainedReplyOwner,
    ) -> io::Result<Arc<Self>> {
        if !pending.current_on(session) {
            return Err(invalid("startup callback owner changed configured tenure"));
        }

        let value: serde_json::Value = serde_json::from_str(section.get()).map_err(invalid)?;
        let data: TransferData = serde_json::from_value(value.clone()).map_err(invalid)?;
        let limits = data.limits;
        if limits.bytes == 0
            || limits.bytes > codex_alab_tui::upgrade_exec_custody::MAX_STATE_BYTES
            || limits.generations == 0
            || limits.generations > limits.bytes
            || section.get().len() > limits.bytes
        {
            return Err(invalid("startup callback section changed actual budgets"));
        }
        let owner = replies.callback_owner(session)?;
        let transfer = replies.restore_transfer(session, value, limits)?;
        Ok(Arc::new(Self {
            owner,
            _replies: replies,
            transfer,
            configured: Arc::downgrade(pending),
            serial: Semaphore::new(1),
            cuts: Mutex::new(Vec::new()),
            commit: Mutex::new(None),
        }))
    }
    fn current(&self, session: &AppServerSession, pending: &Arc<Configured>) -> io::Result<()> {
        if !self
            .configured
            .upgrade()
            .is_some_and(|actual| Arc::ptr_eq(&actual, pending))
            || !pending.current_on(session)
        {
            return Err(invalid(
                "startup callback cut lost original configured owner",
            ));
        }
        self.owner.validate(session)?;
        self.transfer.validate(session)
    }
    /// Required after the real configured tail and BEFORE a fresh successful
    /// original StartupDraft draw. Only OWN actually closed cells are accepted.
    /// Saved closed forms are reconciled as closed instead of being reopened;
    /// the original provisional BottomPane/composer/picker remains untouched.
    pub(crate) async fn observe(
        &self,
        session: &AppServerSession,
        pending: &Arc<Configured>,
    ) -> io::Result<()> {
        self.current(session, pending)?;
        let _permit = self.serial.acquire().await.map_err(invalid)?;
        self.transfer.resume_original_inputs(session).await?;
        self.current(session, pending)?;
        let receipt = pending
            .callback_receipt(session)?
            .ok_or_else(|| invalid("startup callbacks precede actual configured tail"))?;
        for ticket in &self.transfer.tickets {
            if self
                .cuts
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .iter()
                .any(|cut| Arc::ptr_eq(&cut.ticket, ticket))
            {
                continue;
            }
            let capture = ticket.capture(session).await?;
            let cut = Arc::new(
                capture
                    .observe_configured(&session.request_handle(), read_request(), &receipt)
                    .await
                    .map_err(io::Error::other)?,
            );
            // The typed Source response, not pending absence, proves closure.
            let _closed = configured::all_closed(ticket, &cut)?;
            self.cuts
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(Observed {
                    ticket: ticket.clone(),
                    cut,
                });
            self.current(session, pending)?;
        }
        Ok(())
    }
    /// Completion alone is insufficient: every original graph ID must have
    /// its actual same-ticket closed Source cut before original draw/UIACK.
    pub(crate) fn reconciled(
        &self,
        session: &AppServerSession,
        pending: &Arc<Configured>,
    ) -> io::Result<()> {
        self.current(session, pending)?;
        let cuts = self.cuts.lock().unwrap_or_else(PoisonError::into_inner);
        if self
            .transfer
            .tickets
            .iter()
            .any(|ticket| !cuts.iter().any(|cut| Arc::ptr_eq(&cut.ticket, ticket)))
        {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "startup callback graph still lacks actual closed cuts",
            ));
        }
        let closed: HashSet<_> = cuts
            .iter()
            .flat_map(|cut| cut.ticket.ids.iter().cloned())
            .collect();
        drop(cuts);
        for view in &self.transfer.views {
            if !view.request_ids_data().iter().all(|id| closed.contains(id)) {
                return Err(invalid(
                    "startup saved form lacks its exact closed Source cell",
                ));
            }
            if view
                .editing_after_closed(
                    &closed,
                    self.transfer.limits.bytes,
                    self.transfer.limits.generations,
                )?
                .is_some()
            {
                return Err(invalid("NoPrimary cannot reopen a captured callback form"));
            }
        }
        Ok(())
    }
    pub(crate) async fn acknowledge(
        &self,
        session: &AppServerSession,
        pending: &Arc<Configured>,
    ) -> io::Result<()> {
        self.reconciled(session, pending)?;
        let _permit = self.serial.acquire().await.map_err(invalid)?;
        let actual = pending.callback_commit(session)?.ok_or_else(|| {
            invalid("startup callback retirement precedes actual ConfigurationCommit")
        })?;
        {
            let mut saved = self.commit.lock().unwrap_or_else(PoisonError::into_inner);
            if saved.is_none() {
                *saved = Some(actual.clone());
            }
        }
        let cuts: Vec<_> = self
            .cuts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(|cut| (cut.ticket.clone(), cut.cut.clone()))
            .collect();
        for (ticket, cut) in cuts {
            if ticket.retired() {
                continue;
            }
            // Preserve the private cells for every original frozen Value until
            // the genuine Source input-publication binding/settlement consumer.
            let ids: Vec<_> = ticket
                .ids
                .iter()
                .filter(|id| {
                    !self
                        .transfer
                        .replies
                        .iter()
                        .any(|reply| reply.request_id_data() == *id)
                })
                .cloned()
                .collect();
            if ids.is_empty() {
                continue;
            }
            let capture = ticket.capture(session).await?;
            let retirement = capture
                .acknowledge_configured(
                    &session.request_handle(),
                    read_request(),
                    &cut,
                    &actual,
                    ids,
                )
                .await
                .map_err(io::Error::other)?;
            ticket
                .progress
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .retirement = Some(retirement);
            self.current(session, pending)?;
        }
        Ok(())
    }
}
