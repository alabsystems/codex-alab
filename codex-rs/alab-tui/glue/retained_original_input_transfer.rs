// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Mandatory same-operation E1 DATA. Decoding never makes a callback/reply.
use super::*;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::app) struct Data {
    schema: u8,
    request: InputRequest,
    #[serde(deserialize_with = "required_native_approval")]
    native_approval: Option<crate::bottom_pane::alab_retained_approval::RequestData>,
    operation: Box<serde_json::value::RawValue>,
}
fn required_native_approval<'de, D: serde::Deserializer<'de>>(
    decoder: D,
) -> std::result::Result<Option<crate::bottom_pane::alab_retained_approval::RequestData>, D::Error>
{
    serde::Deserialize::deserialize(decoder)
}
impl Item {
    pub(in crate::app) fn checkpoint(&self) -> io::Result<Data> {
        let reservation = self
            .reservation
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if reservation.pending_publication().is_some()
            && !self
                .progress
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .bound
            && !reservation.binding_data_present()
        {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "frozen original answer must bind its private Source owner before E1 sealing",
            ));
        }
        Ok(Data {
            schema: 2,
            request: self.request.clone(),
            native_approval: self
                .native_approval
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone(),
            operation: serde_json::value::to_raw_value(
                &reservation.checkpoint().map_err(io::Error::other)?,
            )
            .map_err(invalid)?,
        })
    }
    pub(in crate::app) fn retired(&self) -> bool {
        self.progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retired
    }
    pub(in crate::app) async fn resume(
        self: &Arc<Self>,
        session: &AppServerSession,
    ) -> io::Result<()> {
        let reservation = self
            .reservation
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if let Some(publication) = reservation.pending_publication() {
            self.drive(session, &publication).await?;
        }
        Ok(())
    }
}
pub(in crate::app) fn current_items(
    replies: &Replies,
    callbacks: &Arc<callbacks::CallbackUi>,
) -> io::Result<Vec<Arc<Item>>> {
    let owner = replies
        .owner
        .as_ref()
        .ok_or_else(|| invalid("actual original input owner missing"))?;
    if !Arc::ptr_eq(&owner.callbacks, callbacks) {
        return Err(invalid("input checkpoint changed actual callback owner"));
    }
    Ok(owner
        .inputs
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .filter(|item| !item.retired())
        .cloned()
        .collect())
}
/// Called by the actual E1 consumer before original replay. The real final
/// session/Replies owner must already be bound; this constructs no App or Core.
pub(in crate::app) fn restore(replies: &Replies, data: Vec<Data>) -> io::Result<Vec<Arc<Item>>> {
    let owner = replies
        .owner
        .as_ref()
        .ok_or_else(|| invalid("actual final-session reply owner missing before E1 restore"))?;
    if data.len() > MAX_ITEMS {
        return Err(invalid("original input checkpoint exceeds queue capacity"));
    }
    let mut prepared = Vec::new();
    let mut ids = std::collections::HashSet::new();
    for data in data {
        let thread =
            codex_protocol::ThreadId::from_string(data.request.thread()).map_err(invalid)?;
        let reservation = OriginalInputReservation::restore_checkpoint(
            serde_json::from_str(data.operation.get()).map_err(invalid)?,
        )
        .map_err(io::Error::other)?;
        if data.schema != 2
            || thread.to_string() != data.request.thread()
            || !data.request.valid_for(&reservation.request_id_data())
            || data
                .native_approval
                .as_ref()
                .is_some_and(|native| !data.request.allows_native(native))
            || reservation.service_data() != owner.tenure.target.service_data()
            || !ids.insert(reservation.request_id_data())
        {
            return Err(invalid(
                "original input checkpoint changed schema/service/thread/identity",
            ));
        }
        prepared.push(Arc::new(Item {
            owner: Arc::downgrade(owner),
            request: data.request,
            native_approval: Mutex::new(data.native_approval),
            native_features: Mutex::new(Ok(None)),
            native_faulted: std::sync::atomic::AtomicBool::new(false),
            reservation: Mutex::new(reservation),
            serial: Semaphore::new(1),
            progress: Mutex::new(Progress::default()),
        }));
    }
    let mut items = owner.inputs.lock().unwrap_or_else(PoisonError::into_inner);
    let mut actual = Vec::new();
    for item in prepared {
        if let Some(prior) = items.iter().find(|prior| prior.id() == item.id()) {
            // Repeated E1 graph cannot alter an already retained original Value
            // or operation, even when its params happen to be identical.
            let left = prior
                .reservation
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .checkpoint()
                .map_err(io::Error::other)?;
            let right = item
                .reservation
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .checkpoint()
                .map_err(io::Error::other)?;
            if !prior.request.same(&item.request) || left != right {
                return Err(invalid(
                    "repeated original input checkpoint changed same operation",
                ));
            }
            actual.push(prior.clone());
        } else {
            if items.len() >= MAX_ITEMS {
                return Err(invalid("original input owner capacity before E1 replay"));
            }
            items.push(item.clone());
            actual.push(item);
        }
    }
    Ok(actual)
}

impl App {
    /// Retries only already-frozen SAME operations retained before their
    /// original enqueue. A local future drop does not clear their custody.
    pub(crate) async fn alab_flush_original_inputs(
        &mut self,
        session: &AppServerSession,
    ) -> io::Result<()> {
        let Some(owner) = self
            .pending_app_server_requests
            .alab_retained_replies
            .owner
            .clone()
        else {
            return Ok(());
        };
        owner.callbacks.validate(session)?;
        let items = owner
            .inputs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let mut first_error = None;
        for item in items {
            match item.resume(session).await {
                Ok(()) if item.retired() => {
                    self.alab_apply_closed_callback(item.thread_id_data(), &item.id())
                }
                Ok(()) => {}
                Err(error) => {
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
        }
        // Actual retirement is remembered on the SAME resource before removal.
        // No endpoint fact or E1 DATA alone removes an original human Value.
        let removed = {
            let mut items = owner.inputs.lock().unwrap_or_else(PoisonError::into_inner);
            let mut removed = Vec::new();
            let mut index = 0;
            while index < items.len() {
                if items[index].retired() {
                    removed.push(items.remove(index));
                } else {
                    index += 1;
                }
            }
            removed
        };
        drop(removed);
        first_error.map_or(Ok(()), Err)
    }
}

impl Replies {
    pub(crate) async fn flush_original_inputs(&self, session: &AppServerSession) -> io::Result<()> {
        let owner = self
            .owner
            .as_ref()
            .ok_or_else(|| invalid("original input flush has no actual owner"))?;
        owner.callbacks.validate(session)?;
        let items = owner
            .inputs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let mut first_error = None;
        for item in items {
            if let Err(error) = item.resume(session).await
                && first_error.is_none()
            {
                first_error = Some(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }
}
