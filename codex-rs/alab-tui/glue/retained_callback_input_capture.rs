// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Uses the SAME canonical actual callback ticket as view capture and live ACK.
use super::*;
#[derive(Clone)]
pub(in crate::app) struct InputCapture(pub(super) Arc<Ticket>);
pub(in crate::app) fn prepare(
    owner: &Arc<CallbackUi>,
    thread: &str,
    id: &RequestId,
) -> io::Result<InputCapture> {
    let mut tickets = owner.tickets.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(ticket) = tickets
        .iter()
        .find(|ticket| ticket.ids.contains(id) && !ticket.retired())
    {
        if ticket.thread != thread {
            return Err(invalid("original input changed canonical callback thread"));
        }
        return Ok(InputCapture(ticket.clone()));
    }
    let operation =
        CallbackCaptureOperation::new(owner.tenure.target.service_data(), vec![id.clone()])
            .map_err(io::Error::other)?;
    let ticket = Ticket::new(
        thread.to_owned(),
        vec![id.clone()],
        operation,
        Origin::Original,
    )?;
    tickets.push(ticket.clone());
    Ok(InputCapture(ticket))
}
impl InputCapture {
    pub(in crate::app) async fn capture(
        &self,
        session: &AppServerSession,
    ) -> io::Result<CallbackCapture> {
        self.0.capture(session).await
    }
}
