// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The actual queue item owns its reply before an answer can enter AppEvent.
//! Publication is the same Source operation on C1/C2. Endpoint delivery never
//! establishes Core consumption, effects, or Native permission.
use super::*;
use codex_app_server_client::retained_callbacks::OriginalInputPublication;
use codex_app_server_client::retained_callbacks::OriginalInputReceipt;
use codex_app_server_client::retained_callbacks::OriginalInputReservation;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ToolRequestUserInputParams;
use codex_app_server_protocol::ToolRequestUserInputResponse;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::Weak;
use tokio::sync::Semaphore;

#[path = "retained_original_input_request.rs"]
mod request;
use request::Request as InputRequest;
const MAX_ITEMS: usize = 4096;
thread_local! {
    static ACTIVE: RefCell<Option<Arc<Item>>> = const { RefCell::new(None) };
    static RESOLVED: RefCell<Option<RequestId>> = const { RefCell::new(None) };
}
pub(in crate::app) fn resolving(id: &RequestId) -> ResolvedScope {
    ResolvedScope(
        RESOLVED.with(|slot| slot.replace(Some(id.clone()))),
        std::marker::PhantomData,
    )
}
pub(crate) struct ResolvedScope(Option<RequestId>, std::marker::PhantomData<std::rc::Rc<()>>);
impl Drop for ResolvedScope {
    fn drop(&mut self) {
        RESOLVED.with(|slot| *slot.borrow_mut() = self.0.take());
    }
}
fn matches_resolved(item: &Arc<Item>) -> bool {
    RESOLVED.with(|slot| slot.borrow().as_ref() == Some(&item.id()))
}

pub(crate) struct Item {
    owner: Weak<Owner>,
    request: InputRequest,
    native_approval: Mutex<Option<crate::bottom_pane::alab_retained_approval::RequestData>>,
    native_features: Mutex<std::result::Result<Option<codex_features::Features>, ()>>,
    native_faulted: std::sync::atomic::AtomicBool,
    reservation: Mutex<Arc<OriginalInputReservation>>,
    serial: Semaphore,
    progress: Mutex<Progress>,
}
#[derive(Default)]
struct Progress {
    capture: Option<callbacks::input_capture::InputCapture>,
    bound: bool,
    receipt: Option<OriginalInputReceipt>,
    retired: bool,
    closed_unused: bool,
}
impl std::fmt::Debug for Item {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OriginalInputItem")
            .field("request_id", &self.id())
            .finish_non_exhaustive()
    }
}
pub(crate) struct Scope(Option<Arc<Item>>, std::marker::PhantomData<std::rc::Rc<()>>);
impl Drop for Scope {
    fn drop(&mut self) {
        ACTIVE.with(|slot| *slot.borrow_mut() = self.0.take());
    }
}
fn enter(item: Option<Arc<Item>>) -> Scope {
    Scope(
        ACTIVE.with(|slot| slot.replace(item)),
        std::marker::PhantomData,
    )
}
pub(crate) fn current() -> Option<Arc<Item>> {
    ACTIVE.with(|slot| slot.borrow().clone())
}
pub(crate) fn scope_item(item: Option<Arc<Item>>) -> Scope {
    enter(item)
}

impl Replies {
    /// Runs synchronously at the original real callback registration, before
    /// either the per-thread FIFO or deferred interrupt can receive its DATA.
    pub(in crate::app) fn pin_user_input(&self, actual: &BufferedServerRequest) -> io::Result<()> {
        let Some(owner) = &self.owner else {
            return Ok(());
        };
        let Some(request) = InputRequest::from_original(&actual.request) else {
            return Ok(());
        };
        let request_id = actual.request.id();
        let mut items = owner.inputs.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(item) = items.iter().find(|item| item.id() == *request_id) {
            if !item.request.same(&request) {
                return Err(invalid("original input callback changed request DATA"));
            }
            let reservation = item
                .reservation
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone();
            if reservation.is_for_reply(&actual.reply) {
                return Ok(());
            }
            // A restored SAME operation may attach its genuinely replayed new
            // reply. The Source capture/cell still authorizes any publication.
            let attached = actual.reply.adopt_original_input(&reservation)?;
            *item
                .reservation
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = attached;
            return Ok(());
        }
        if items.len() >= MAX_ITEMS {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "original input queue owner capacity",
            ));
        }
        if !actual.reply.retained_pending_for_client(&owner.client) {
            return Err(invalid(
                "original input has no actual unclaimed same-client reply",
            ));
        }
        let reservation = actual
            .reply
            .reserve_original_input(owner.tenure.target.service_data())?;
        items.push(Arc::new(Item {
            owner: Arc::downgrade(owner),
            request,
            native_approval: Mutex::new(None),
            native_features: Mutex::new(Ok(None)),
            native_faulted: std::sync::atomic::AtomicBool::new(false),
            reservation: Mutex::new(reservation),
            serial: Semaphore::new(1),
            progress: Mutex::new(Progress::default()),
        }));
        Ok(())
    }
    pub(in crate::app) fn input_scope(&self, request: &ServerRequest) -> io::Result<Scope> {
        let Some(owner) = &self.owner else {
            return Ok(enter(None));
        };
        let Some(kind) = InputRequest::from_original(request) else {
            return Ok(enter(None));
        };
        let request_id = request.id();
        let item = owner
            .inputs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .find(|item| item.id() == *request_id && item.request.same(&kind))
            .cloned()
            .ok_or_else(|| invalid("actual original input owner missing before view queue"))?;
        Ok(enter(Some(item)))
    }
    pub(in crate::app) fn retire_unused_input(&self, thread: &str, id: &RequestId) {
        let Some(owner) = &self.owner else {
            return;
        };
        let removed = {
            let mut items = owner.inputs.lock().unwrap_or_else(PoisonError::into_inner);
            let Some(index) = items
                .iter()
                .position(|item| item.id() == *id && item.thread_id_data() == thread)
            else {
                return;
            };
            let item = &items[index];
            let mut progress = item.progress.lock().unwrap_or_else(PoisonError::into_inner);
            let frozen = item
                .reservation
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .pending_publication()
                .is_some();
            if frozen {
                return;
            }
            progress.closed_unused = true;
            drop(progress);
            items.remove(index)
        };
        drop(removed);
    }
    pub(in crate::app) fn input_for_reply(
        &self,
        reply: &AppServerReplyHandle,
    ) -> Option<Arc<Item>> {
        self.owner
            .as_ref()?
            .inputs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .find(|item| {
                item.reservation
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .is_for_reply(reply)
            })
            .cloned()
    }
}
impl Item {
    pub(crate) fn id(&self) -> RequestId {
        self.reservation
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .request_id_data()
    }
    pub(crate) fn thread_id_data(&self) -> &str {
        self.request.thread()
    }
    pub(crate) fn matches_resolved(&self) -> bool {
        RESOLVED.with(|slot| slot.borrow().as_ref() == Some(&self.id()))
    }
    pub(crate) fn with_scope<T>(self: &Arc<Self>, body: impl FnOnce() -> T) -> T {
        let _scope = enter(Some(self.clone()));
        body()
    }
    pub(in crate::app) fn callback_capture(
        &self,
    ) -> Option<callbacks::input_capture::InputCapture> {
        self.progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .capture
            .clone()
    }
    pub(crate) fn user_params(&self) -> Option<&ToolRequestUserInputParams> {
        self.request.user_input()
    }
    pub(crate) fn needs_binding(&self) -> bool {
        let publication = {
            self.reservation
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .pending_publication()
                .is_some()
        };
        publication
            && !self
                .progress
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .bound
    }
    pub(in crate::app) fn is_for_client(&self, client: &AppServerRequestHandle) -> bool {
        self.reservation
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_for_client(client)
    }
    fn freeze(
        self: &Arc<Self>,
        response: ToolRequestUserInputResponse,
    ) -> io::Result<PublicationEvent> {
        if self.request.user_input().is_none() {
            return Err(invalid("user answer changed actual original callback kind"));
        }
        self.freeze_value(serde_json::to_value(&response).map_err(invalid)?)
    }
    pub(super) fn freeze_value(
        self: &Arc<Self>,
        value: serde_json::Value,
    ) -> io::Result<PublicationEvent> {
        let progress = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
        if progress.closed_unused {
            return Err(invalid("original callback closed before this answer"));
        }
        let reservation = self
            .reservation
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let publication = reservation.freeze(value)?;
        drop(progress);
        Ok(PublicationEvent {
            item: self.clone(),
            publication,
        })
    }
    pub(crate) async fn prepare(self: &Arc<Self>, session: &AppServerSession) -> io::Result<()> {
        let _permit = self.serial.acquire().await.map_err(invalid)?;
        self.prepare_locked(session).await
    }
    async fn prepare_locked(self: &Arc<Self>, session: &AppServerSession) -> io::Result<()> {
        let owner = self
            .owner
            .upgrade()
            .ok_or_else(|| invalid("original input owner retired"))?;
        owner.callbacks.validate(session)?;
        let reservation = self
            .reservation
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if reservation.pending_publication().is_none() || reservation.binding_data_present() {
            return Ok(());
        }
        let prior = {
            self.progress
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .capture
                .clone()
        };
        let capture = match prior {
            Some(actual) => actual,
            None => {
                let actual = callbacks::input_capture::prepare(
                    &owner.callbacks,
                    self.thread_id_data(),
                    &self.id(),
                )?;
                self.progress
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .capture = Some(actual.clone());
                actual
            }
        };
        let actual = capture.capture(session).await?;
        reservation
            .bind_capture(
                &session.request_handle(),
                callbacks::read_request(),
                &actual,
            )
            .await
            .map_err(io::Error::other)?;
        self.progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .bound = true;
        Ok(())
    }
    async fn drive(
        self: &Arc<Self>,
        session: &AppServerSession,
        publication: &Arc<OriginalInputPublication>,
    ) -> io::Result<()> {
        let _permit = self.serial.acquire().await.map_err(invalid)?;
        let current = self
            .reservation
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if !Arc::ptr_eq(publication.reservation(), &current) {
            return Err(invalid("publication changed original queue owner"));
        }
        if self
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retired
        {
            return Ok(());
        }
        self.prepare_locked(session).await?;
        let prior = {
            self.progress
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .receipt
                .clone()
        };
        let receipt = match prior {
            Some(actual) => actual,
            None => publication
                .publish(&session.request_handle(), callbacks::read_request())
                .await
                .map_err(io::Error::other)?,
        };
        // Store the actual result before the retirement await. Cancellation
        // keeps this immutable operation/value and its same terminal receipt.
        {
            let mut progress = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
            progress.bound = true;
            progress.receipt = Some(receipt.clone());
        }
        publication
            .retire(
                &session.request_handle(),
                callbacks::read_request(),
                &receipt,
            )
            .await
            .map_err(io::Error::other)?;
        self.progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retired = true;
        Ok(())
    }
}

/// Opaque private AppEvent. No turn-ID lookup, JSON decoding, or new reply
/// handle occurs when the actual App later handles this queued answer.
pub(crate) struct PublicationEvent {
    item: Arc<Item>,
    publication: Arc<OriginalInputPublication>,
}
#[cfg(test)]
impl PublicationEvent {
    /// Read only immutable local request/value DATA. This neither observes a
    /// Source receipt nor exposes the private publication capability to tests.
    pub(crate) fn local_ui_original_value_data(
        &self,
    ) -> io::Result<(serde_json::Value, serde_json::Value)> {
        let data = self.publication.checkpoint().map_err(invalid)?;
        let id = data
            .get("requestId")
            .cloned()
            .ok_or_else(|| invalid("original input request DATA missing"))?;
        let value = data
            .get("value")
            .cloned()
            .ok_or_else(|| invalid("original input value DATA missing"))?;
        Ok((id, value))
    }
}
impl std::fmt::Debug for PublicationEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("OriginalInputPublication")
            .field(&self.item)
            .finish()
    }
}
impl App {
    pub(crate) async fn alab_publish_original_input(
        &mut self,
        session: &AppServerSession,
        event: PublicationEvent,
    ) -> io::Result<()> {
        let owner = self
            .pending_app_server_requests
            .alab_retained_replies
            .owner
            .as_ref()
            .ok_or_else(|| invalid("original input event has no actual App owner"))?
            .clone();
        if !event
            .item
            .owner
            .upgrade()
            .is_some_and(|actual| Arc::ptr_eq(&actual, &owner))
        {
            return Err(invalid(
                "original input event belongs to another actual App",
            ));
        }
        event.item.drive(session, &event.publication).await?;
        self.alab_apply_closed_callback(event.item.thread_id_data(), &event.item.id());
        // The actual original SourceResolved notification owns canonical replay
        // updates. Reapplying a turn-based optimistic pop here could remove B
        // when this SAME A event was already drained before a C1 capture.
        self.refresh_pending_thread_approvals().await;
        if let Ok(thread) = codex_protocol::ThreadId::from_string(event.item.thread_id_data()) {
            self.refresh_side_parent_status_from_store(thread).await;
        }

        let removed = {
            let mut items = owner.inputs.lock().unwrap_or_else(PoisonError::into_inner);
            items
                .iter()
                .position(|item| Arc::ptr_eq(item, &event.item))
                .map(|index| items.remove(index))
        };
        drop(removed);
        Ok(())
    }
}

/// Aligned with the original overlay's actual queue, not a turn-ID index.
#[derive(Default)]
pub(crate) struct QueueBindings {
    current: Option<Arc<Item>>,
    queued: VecDeque<Option<Arc<Item>>>,
}
impl QueueBindings {
    pub(crate) fn capture() -> Self {
        Self {
            current: current(),
            queued: VecDeque::new(),
        }
    }
    pub(crate) fn validate_owners(
        &self,
        owners: &[crate::app::app_server_requests::alab_retained_owners::UserInputOwner<'_>],
    ) -> io::Result<()> {
        if owners.is_empty() || owners.len() > self.queued.len() + 1 {
            return Err(invalid("native input queue lost its actual resource count"));
        }
        for (item, owner) in std::iter::once(&self.current)
            .chain(&self.queued)
            .zip(owners)
        {
            match (item, owner.original_input()) {
                (Some(item), Some(actual)) if Arc::ptr_eq(item, &actual) => {}
                (None, None) => {}
                _ => return Err(invalid("native input editing changed original reply owner")),
            }
        }
        Ok(())
    }
    pub(crate) fn push(&mut self) {
        self.queued.push_back(current());
    }
    pub(crate) fn advance(&mut self) {
        if let Some(next) = self.queued.pop_front() {
            self.current = next;
        }
    }
    pub(crate) fn submit(
        &self,
        sender: &crate::app_event_sender::AppEventSender,
        thread: codex_protocol::ThreadId,
        turn: String,
        response: ToolRequestUserInputResponse,
    ) -> bool {
        let Some(item) = &self.current else {
            sender.user_input_answer(thread, turn, response);
            return true;
        };
        let Some(params) = item.request.user_input() else {
            return false;
        };
        if thread.to_string() != params.thread_id
            || turn != params.turn_id
            || sender.voice_only.load(Ordering::Relaxed)
        {
            return false;
        }
        match item.freeze(response) {
            Ok(event) => sender
                .app_event_tx
                .send(crate::app_event::AppEvent::AlabOriginalInput(event))
                .is_ok(),
            Err(error) => {
                tracing::warn!(%error, "original answer remains held on its exact callback");
                false
            }
        }
    }
    pub(crate) fn retain(
        &mut self,
        requests: &mut VecDeque<ToolRequestUserInputParams>,
        call_id: &str,
    ) {
        if self.queued.len() != requests.len() {
            tracing::warn!("original input queue/resource count is held after mismatch");
            return;
        }
        let bindings = std::mem::take(&mut self.queued);
        let old_requests = std::mem::take(requests);
        for (request, binding) in old_requests.into_iter().zip(bindings) {
            let remove =
                request.item_id == call_id && binding.as_ref().is_none_or(matches_resolved);
            if !remove {
                requests.push_back(request);
                self.queued.push_back(binding);
            }
        }
    }
    pub(crate) fn current_matches_resolved(&self) -> bool {
        self.current.as_ref().is_none_or(matches_resolved)
    }
    pub(crate) fn actual_items(&self) -> Vec<Option<Arc<Item>>> {
        std::iter::once(self.current.clone())
            .chain(self.queued.iter().cloned())
            .collect()
    }
}

#[path = "retained_original_input_transfer.rs"]
pub(super) mod transfer;

#[cfg(test)]
#[path = "retained_original_input_driver_tests.rs"]
mod tests;

#[path = "retained_original_approval_driver.rs"]
pub(crate) mod approvals;

#[cfg(test)]
pub(crate) use approvals::tests::item as data_only_approval_item;
