// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Exact frontend/controller custody for local retained IO. Each Native
//! controller incarnation receives at most one F1 delivery. Selection DATA and
//! Source ownership state confer no Native/currentness permission.
use crate::app_server_session::alab_retained_ui::Pending;
use crate::app_server_session::alab_retained_ui::UiStatus;
use crate::app_server_session::alab_retained_ui::kind;
use codex_alab_tui::frontend_control::Controller;
use codex_alab_tui::frontend_control::retained_service as native;
use codex_alab_tui::frontend_control::retained_service::RetainedAttachment;
use codex_alab_tui::frontend_control::retained_service::RetainedStart;
use codex_app_server_client::AppServerClient;
use codex_app_server_client::InProcessClientStartArgs;
use codex_protocol::ThreadId;
use std::collections::HashMap;
use std::io;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;
use tokio::net::UnixStream;
use tokio::sync::Notify;
use tokio::task::JoinHandle;

fn operation() -> String {
    format!(
        "{:016x}{:016x}",
        codex_alab_core::native_source::connection_request_prefix(),
        codex_alab_core::native_source::connection_request_prefix()
    )
}
fn unavailable() -> io::Error {
    io::Error::other("owned local frontend unavailable")
}

pub(crate) struct StartingFrontend {
    controller: Option<Controller>,
    start: Option<RetainedStart>,
}
impl StartingFrontend {
    pub(crate) fn new(
        controller: Controller,
        settings: codex_alab_tui::frontend_control::StartupSettings,
    ) -> io::Result<Self> {
        let start = native::prepare_start(&controller, &operation(), settings)?;
        Ok(Self {
            controller: Some(controller),
            start: Some(start),
        })
    }
    pub(crate) async fn observe(&mut self) -> io::Result<OwnedLocalTarget> {
        match self.start.as_mut().ok_or_else(unavailable)?.observe().await {
            native::ServiceObservation::Observed {
                service,
                phase: native::RetainedPhase::Starting | native::RetainedPhase::Spawned,
            } => Ok(OwnedLocalTarget::new(
                service,
                Some(Creator {
                    _controller: self.controller.take().ok_or_else(unavailable)?,
                    _start: self.start.take().ok_or_else(unavailable)?,
                }),
                None,
            )),
            native::ServiceObservation::Unresolved(kind) => Err(io::Error::new(
                kind,
                "owned local startup remains unresolved",
            )),
            native::ServiceObservation::Observed { .. } => Err(unavailable()),
        }
    }
}
// The original creator is its original intent, never the final F1/Go owner.
struct Creator {
    _controller: Controller,
    _start: RetainedStart,
}
struct Selection {
    service: String,
    _creator: Option<Creator>,
    entered: Option<Arc<codex_alab_tui::frontend_control::EnteredRetainedFrontend>>,
    delivery: Mutex<DeliverySlot>,
    returned: Notify,
    snapshot: std::sync::OnceLock<super::owned_local_ui_snapshot::UiSnapshot>,
    declaration: Mutex<Option<(codex_app_server_protocol::InitializeParams, usize)>>,
    managed_cloud: Mutex<Option<codex_app_server_client::retained_readers::ManagedCloudData>>,
    initial_choice: Mutex<initial_choice::State>,
    side_views: Mutex<side_views::State>,
}
enum DeliverySlot {
    Owned(DeliveryState),
    Borrowed,
}
struct DeliveryState {
    seed: Option<DeliveryController>,
    phase: Option<DeliveryPhase>,
    purpose: Option<EndpointPurpose>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum EndpointPurpose {
    Preparation,
    FinalSession,
}
// The original entered controller remains inside the opaque entry owner.
// Prep deliveries NEVER borrow it; first final F1 does. Later F1 uses a fresh
// controller and asks Native to revalidate the same entered constructor.
enum DeliveryController {
    Owned(Controller),
    Entered(Arc<codex_alab_tui::frontend_control::EnteredRetainedFrontend>),
}
impl std::ops::Deref for DeliveryController {
    type Target = Controller;
    fn deref(&self) -> &Controller {
        match self {
            Self::Owned(actual) => actual,
            Self::Entered(actual) => actual.controller(),
        }
    }
}
fn take_final_seed<T>(purpose: EndpointPurpose, seed: &mut Option<T>) -> Option<T> {
    match purpose {
        EndpointPurpose::Preparation => None,
        EndpointPurpose::FinalSession => seed.take(),
    }
}
enum DeliveryPhase {
    Opening(Opening),
    Ready(Arc<DeliveryOwner>),
    Attempt {
        owner: Arc<DeliveryOwner>,
        attachment: RetainedAttachment,
    },
    Active {
        owner: Arc<DeliveryOwner>,
        role: ActiveRole,
    },
}
#[derive(Clone, Copy)]
enum ActiveRole {
    Preparation,
    SessionUnclaimed,
    SessionClaimed,
}

// Exactly one actual controller withdrawal request remains owned even if its
// observer is cancelled. An uncertain/failed close never permits fresh issue.
struct DeliveryOwner {
    controller: DeliveryController,
    app_owners: Mutex<AppOwners>,
    runtime_frontier: Mutex<Option<codex_alab_tui::frontend_control::FrontendFrontier>>,
    ui_frontier: Mutex<Option<UiSlot>>,
    task_tools: Mutex<HashMap<ThreadId, bool>>,
    configured_begins: Mutex<
        Vec<
            std::sync::Weak<
                crate::owned_local_entered_readers::constructor::configured_begin::Begin,
            >,
        >,
    >,
    close: Mutex<Option<JoinHandle<()>>>,
    completion: tokio::sync::watch::Sender<Option<Result<(), io::ErrorKind>>>,
}
// Weak UI custody cannot turn disappearance into observed completion. The
// separate tiny status allocation survives until actual physical close.
struct UiSlot {
    owner: UiOwner,
    status: Arc<UiStatus>,
}
enum UiOwner {
    Projected(std::sync::Weak<Pending>),
    Configured(std::sync::Weak<crate::startup_draft::alab_configured::Pending>),
}
impl UiSlot {
    fn projected(&self) -> Option<Arc<Pending>> {
        match &self.owner {
            UiOwner::Projected(actual) => actual.upgrade(),
            UiOwner::Configured(_) => None,
        }
    }
    fn same_native(&self, native: &kind::Native) -> bool {
        match &self.owner {
            UiOwner::Projected(actual) => actual
                .upgrade()
                .is_some_and(|actual| actual.same_native(native)),
            UiOwner::Configured(actual) => actual
                .upgrade()
                .is_some_and(|actual| actual.same_native(native)),
        }
    }
    fn permits_startup_submission(&self) -> bool {
        matches!(self.owner, UiOwner::Projected(_)) && self.status.is_complete()
    }
}
#[derive(Clone)]
enum AppPromotion {
    Runtime(codex_alab_tui::frontend_control::AppFrontendOwner),
    Source(codex_alab_tui::frontend_control::FrontendSelectionAppOwner),
}
struct PendingPromotion {
    owner: AppPromotion,
    status: Arc<UiStatus>,
}
#[derive(Default)]
struct AppOwners {
    _committed: Option<AppPromotion>,
    pending: Option<PendingPromotion>,
}
struct CloseGuard(tokio::sync::watch::Sender<Option<Result<(), io::ErrorKind>>>);
impl CloseGuard {
    fn finish(self, result: Result<(), io::ErrorKind>) {
        self.0.send_if_modified(|stored| {
            if stored.is_some() {
                return false;
            }
            *stored = Some(result);
            true
        });
    }
}
impl Drop for CloseGuard {
    fn drop(&mut self) {
        self.0.send_if_modified(|stored| {
            if stored.is_some() {
                return false;
            }
            *stored = Some(Err(io::ErrorKind::Other));
            true
        });
    }
}
impl DeliveryOwner {
    fn new(controller: Controller) -> Arc<Self> {
        Self::with_controller(DeliveryController::Owned(controller))
    }
    fn with_controller(controller: DeliveryController) -> Arc<Self> {
        let (completion, _) = tokio::sync::watch::channel(None);
        Arc::new(Self {
            controller,
            app_owners: Mutex::new(AppOwners::default()),
            runtime_frontier: Mutex::new(None),
            ui_frontier: Mutex::new(None),
            task_tools: Mutex::new(HashMap::new()),
            configured_begins: Mutex::new(Vec::new()),
            close: Mutex::new(None),
            completion,
        })
    }
    fn begin_close(self: &Arc<Self>) -> io::Result<()> {
        let mut close = self.close.lock().unwrap_or_else(PoisonError::into_inner);
        if close.is_none() {
            let runtime = tokio::runtime::Handle::try_current().map_err(|_| unavailable())?;
            let owner = self.clone();
            let guard = CloseGuard(self.completion.clone());
            *close = Some(runtime.spawn(async move {
                // Registration is sealed by the SAME close mutex before this
                // worker starts. Readonly observation cancellation is unrelated.
                let begins = owner
                    .configured_begins
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .iter()
                    .filter_map(std::sync::Weak::upgrade)
                    .collect::<Vec<_>>();
                let mut failure = None;
                for begin in begins {
                    if let Err(error) = begin.drain().await {
                        failure.get_or_insert(error.kind());
                    }
                }
                let closed = owner.controller.close().await.map_err(|error| error.kind());
                let result = failure.map_or(closed, Err);
                guard.finish(result);
            }));
        }
        Ok(())
    }
    fn closing(&self) -> bool {
        self.close
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some()
    }
    async fn close(self: &Arc<Self>) -> io::Result<()> {
        self.begin_close()?;
        let mut completion = self.completion.subscribe();
        loop {
            let observed = *completion.borrow_and_update();
            if let Some(result) = observed {
                return result.map_err(|kind| {
                    io::Error::new(kind, "owned local controller withdrawal unresolved")
                });
            }
            completion.changed().await.map_err(|_| unavailable())?;
        }
    }
}

// An actual open request is owned before polling. Cancellation only removes an
// observer; normal completion retains the exact issued controller, and worker
// loss records Unknown instead of resubmitting a fresh allocation.
struct Opening {
    task: Option<JoinHandle<()>>,
    joined: Option<Result<(), io::ErrorKind>>,
    completion: tokio::sync::watch::Sender<Option<Result<Arc<DeliveryOwner>, io::ErrorKind>>>,
}
struct OpeningGuard(tokio::sync::watch::Sender<Option<Result<Arc<DeliveryOwner>, io::ErrorKind>>>);
impl OpeningGuard {
    fn finish(self, result: Result<Arc<DeliveryOwner>, io::ErrorKind>) {
        let mut result = Some(result);
        self.0.send_if_modified(|stored| {
            if stored.is_some() {
                return false;
            }
            *stored = result.take();
            true
        });
    }
}
impl Drop for OpeningGuard {
    fn drop(&mut self) {
        self.0.send_if_modified(|stored| {
            if stored.is_some() {
                return false;
            }
            *stored = Some(Err(io::ErrorKind::Other));
            true
        });
    }
}
impl Opening {
    fn start() -> io::Result<Self> {
        Self::owned(codex_alab_tui::frontend_control::open())
    }
    fn owned<F>(open: F) -> io::Result<Self>
    where
        F: std::future::Future<Output = io::Result<Controller>> + Send + 'static,
    {
        let runtime = tokio::runtime::Handle::try_current().map_err(|_| unavailable())?;
        let (completion, _) = tokio::sync::watch::channel(None);
        let guard = OpeningGuard(completion.clone());
        let task = runtime.spawn(async move {
            let result = open
                .await
                .map(DeliveryOwner::new)
                .map_err(|error| error.kind());
            guard.finish(result);
        });
        Ok(Self {
            task: Some(task),
            joined: None,
            completion,
        })
    }
    async fn observe(&mut self) -> io::Result<Arc<DeliveryOwner>> {
        if self.joined.is_none() {
            let joined = match self.task.as_mut() {
                Some(task) => task.await.map_err(|_| io::ErrorKind::Other),
                None => Err(io::ErrorKind::Other),
            };
            self.joined = Some(joined);
            drop(self.task.take());
        }
        self.joined
            .ok_or_else(unavailable)?
            .map_err(|kind| io::Error::new(kind, "owned local controller allocation unresolved"))?;
        self.completion
            .borrow()
            .clone()
            .ok_or_else(unavailable)?
            .map_err(|kind| io::Error::new(kind, "owned local controller allocation unresolved"))
    }
}

#[derive(Clone)]
pub(crate) struct OwnedLocalTarget(Arc<Selection>);
impl std::fmt::Debug for OwnedLocalTarget {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("OwnedLocalTarget(<actual controller custody>)")
    }
}
impl PartialEq for OwnedLocalTarget {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for OwnedLocalTarget {}
impl OwnedLocalTarget {
    fn new(service: String, creator: Option<Creator>, seed: Option<Controller>) -> Self {
        Self::with_entry(service, creator, seed.map(DeliveryController::Owned), None)
    }
    fn with_entry(
        service: String,
        creator: Option<Creator>,
        seed: Option<DeliveryController>,
        entered: Option<Arc<codex_alab_tui::frontend_control::EnteredRetainedFrontend>>,
    ) -> Self {
        Self(Arc::new(Selection {
            service,
            _creator: creator,
            entered,
            delivery: Mutex::new(DeliverySlot::Owned(DeliveryState {
                seed,
                phase: None,
                purpose: None,
            })),
            returned: Notify::new(),
            snapshot: std::sync::OnceLock::new(),
            declaration: Mutex::new(None),
            managed_cloud: Mutex::new(None),
            initial_choice: Mutex::new(initial_choice::State::default()),
            side_views: Mutex::new(side_views::State::default()),
        }))
    }
    /// Recovered DATA plus an actual newly issued controller. F1 admission and
    /// replacement Go still require Native; this grants no creator/stop rights.
    pub(crate) fn recover(controller: Controller, service: String) -> io::Result<Self> {
        if service.len() != 32
            || !service
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err(unavailable());
        }
        Ok(Self::new(service, None, Some(controller)))
    }
    /// The mandatory Source UI envelope decoder supplies the actual snapshot
    /// from SAME authenticated E1 input (or a genuine same-backend DATA read).
    /// It must retain/apply the WHOLE E1 UI payload separately before real UI
    /// ACK. This constructor creates no default/rebuilt/bootstrap snapshot.
    pub(crate) fn from_entered(
        entry: Arc<codex_alab_tui::frontend_control::EnteredRetainedFrontend>,
        snapshot: super::owned_local_ui_snapshot::UiSnapshot,
    ) -> io::Result<Self> {
        snapshot.validate_data()?;
        let target = Self::with_entry(
            entry.service_data().into(),
            None,
            Some(DeliveryController::Entered(entry.clone())),
            Some(entry),
        );
        target.install_snapshot(snapshot)?;
        Ok(target)
    }
    /// Every reconnect asks Native again; historical E1 selects an attempt,
    /// never current constructor/F1/UI/Go authority by itself.
    pub(crate) async fn request_native_frontier(
        &self,
        tenure: &FrontendTenure,
    ) -> io::Result<codex_alab_tui::frontend_control::FrontendFrontier> {
        if self != &tenure.target {
            return Err(unavailable());
        }
        match &self.0.entered {
            Some(entry) => entry.request_frontier(tenure.controller()).await,
            None => {
                tenure
                    .controller()
                    .begin_retained_frontier(self.service_data(), None)
                    .await
            }
        }
    }
    pub(crate) fn install_snapshot(
        &self,
        snapshot: super::owned_local_ui_snapshot::UiSnapshot,
    ) -> io::Result<()> {
        self.0.snapshot.set(snapshot).map_err(|_| unavailable())
    }
    pub(crate) fn snapshot(&self) -> io::Result<&super::owned_local_ui_snapshot::UiSnapshot> {
        self.0.snapshot.get().ok_or_else(unavailable)
    }
    pub(crate) fn service_data(&self) -> &str {
        &self.0.service
    }
    /// Latest actual SAME-backend managed observation for E1 configuration
    /// DATA. It says neither that App applied it nor that readers retired.
    pub(crate) fn snapshot_for_transfer(
        &self,
    ) -> io::Result<super::owned_local_ui_snapshot::UiSnapshot> {
        let mut snapshot = self.snapshot()?.clone();
        let managed = self
            .0
            .managed_cloud
            .lock()
            .map_err(|_| unavailable())?
            .clone();
        if let Some(managed) = managed {
            snapshot.replace_managed_data(&managed)?;
        }
        Ok(snapshot)
    }
    pub(crate) fn observe_managed_cloud(
        &self,
        actual: codex_app_server_client::retained_readers::ManagedCloudData,
    ) -> io::Result<()> {
        *self.0.managed_cloud.lock().map_err(|_| unavailable())? = Some(actual);
        Ok(())
    }

    pub(crate) fn holds_startup_submission(&self) -> bool {
        let owner = {
            let slot = self
                .0
                .delivery
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let DeliverySlot::Owned(DeliveryState {
                phase:
                    Some(DeliveryPhase::Active {
                        owner,
                        role: ActiveRole::SessionClaimed,
                    }),
                ..
            }) = &*slot
            else {
                return true;
            };
            owner.clone()
        };
        if owner.closing() {
            return true;
        }
        // Never enter Pending.progress while holding any delivery lock.
        owner
            .ui_frontier
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .is_none_or(|slot| !slot.permits_startup_submission())
    }
    pub(crate) fn ui_frontier(
        &self,
    ) -> Option<Arc<crate::app_server_session::alab_retained_ui::Pending>> {
        let slot = self
            .0
            .delivery
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let DeliverySlot::Owned(DeliveryState {
            phase:
                Some(DeliveryPhase::Active {
                    owner,
                    role: ActiveRole::SessionClaimed,
                }),
            ..
        }) = &*slot
        else {
            return None;
        };
        if owner.closing() {
            return None;
        }
        owner
            .ui_frontier
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .and_then(UiSlot::projected)
    }
    async fn lease(&self) -> DeliveryLease {
        loop {
            let returned = self.0.returned.notified();
            tokio::pin!(returned);
            returned.as_mut().enable();
            {
                let mut slot = self
                    .0
                    .delivery
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                if let DeliverySlot::Owned(state) =
                    std::mem::replace(&mut *slot, DeliverySlot::Borrowed)
                {
                    return DeliveryLease {
                        selection: self.0.clone(),
                        state: Some(state),
                    };
                }
            }
            returned.await;
        }
    }
    /// Continue the same pending F1, or open a new actual controller only AFTER
    /// the previous delivered controller's explicit close receipt. Never send a
    /// new operation on a consumed controller incarnation.
    pub(crate) async fn preparation_endpoint(&self) -> io::Result<PreparationEndpoint> {
        self.endpoint(EndpointPurpose::Preparation).await
    }
    async fn endpoint(&self, purpose: EndpointPurpose) -> io::Result<PreparationEndpoint> {
        let mut lease = self.lease().await;
        let state = lease.state.as_mut().ok_or_else(unavailable)?;
        if let Some(DeliveryPhase::Active { owner, .. }) = &state.phase {
            if !owner.closing() {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "owned local endpoint remains owned",
                ));
            }
            owner.close().await?;
            drop(state.phase.take());
            state.purpose = None;
        }
        if state.phase.is_some() && state.purpose != Some(purpose) {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "original endpoint purpose remains unresolved",
            ));
        }
        if state.phase.is_none() {
            state.purpose = Some(purpose);
            state.phase = Some(match take_final_seed(purpose, &mut state.seed) {
                Some(controller) => {
                    DeliveryPhase::Ready(DeliveryOwner::with_controller(controller))
                }
                None => DeliveryPhase::Opening(Opening::start()?),
            });
        }
        if let Some(DeliveryPhase::Opening(opening)) = &mut state.phase {
            let owner = opening.observe().await?;
            state.phase = Some(DeliveryPhase::Ready(owner));
        }
        if let Some(DeliveryPhase::Ready(owner)) = &state.phase {
            // Store the actual issued controller BEFORE fallible callback
            // preparation; an error cannot lose it and mint a fresh one.
            let attachment =
                native::prepare_attachment(&owner.controller, &operation(), &self.0.service)?;
            state.phase = Some(DeliveryPhase::Attempt {
                owner: owner.clone(),
                attachment,
            });
        }
        let Some(DeliveryPhase::Attempt { owner, attachment }) = state.phase.as_mut() else {
            return Err(unavailable());
        };
        match attachment.observe().await {
            native::AttachmentObservation::Confirmed => {
                let stream = attachment.take_stream()?;
                let owner = owner.clone();
                state.phase = Some(DeliveryPhase::Active {
                    owner: owner.clone(),
                    role: ActiveRole::Preparation,
                });
                Ok(PreparationEndpoint {
                    target: self.clone(),
                    stream: Some(stream),
                    owner: Some(owner),
                })
            }
            native::AttachmentObservation::Unresolved { error, .. } => Err(io::Error::new(
                error,
                "owned local attachment remains unresolved",
            )),
        }
    }
    pub(crate) fn session_tenure(&self) -> io::Result<FrontendTenure> {
        let mut slot = self
            .0
            .delivery
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        match &mut *slot {
            DeliverySlot::Owned(DeliveryState {
                phase: Some(DeliveryPhase::Active { owner, role }),
                ..
            }) if matches!(role, ActiveRole::SessionUnclaimed) => {
                *role = ActiveRole::SessionClaimed;
                Ok(FrontendTenure {
                    target: self.clone(),
                    delivery: owner.clone(),
                    _owner: Some(SessionClose(owner.clone())),
                })
            }
            _ => Err(unavailable()),
        }
    }
    async fn close_active(&self) -> io::Result<()> {
        let lease = self.lease().await;
        let owner = match &lease.state.as_ref().ok_or_else(unavailable)?.phase {
            Some(DeliveryPhase::Active { owner, .. }) => owner.clone(),
            _ => return Err(unavailable()),
        };
        drop(lease);
        owner.close().await
    }
    pub(crate) async fn connect(
        &self,
        args: InProcessClientStartArgs,
    ) -> io::Result<AppServerClient> {
        {
            let mut declaration = self
                .0
                .declaration
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if declaration.is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "owned local configuration replacement requires its explicit service transaction",
                ));
            }
            *declaration = Some(args.native_frontend_declaration());
        }
        let mut endpoint = self.endpoint(EndpointPurpose::FinalSession).await?;
        let result = args.connect_native_frontend(endpoint.take_stream()?).await;
        match result {
            Ok(client) => {
                endpoint.into_session()?;
                Ok(AppServerClient::Remote(client))
            }
            Err(error) => {
                let _closed = endpoint.close().await;
                Err(error)
            }
        }
    }
    pub(crate) async fn reconnect(&self) -> io::Result<AppServerClient> {
        let (initialize, capacity) = self
            .0
            .declaration
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or_else(unavailable)?;
        self.close_active().await?;
        let mut endpoint = self.endpoint(EndpointPurpose::FinalSession).await?;
        let result =
            codex_app_server_client::RemoteAppServerClient::connect_native_stream_with_initialize(
                endpoint.take_stream()?,
                initialize,
                capacity,
            )
            .await;
        match result {
            Ok(client) => {
                endpoint.into_session()?;
                Ok(AppServerClient::Remote(client))
            }
            Err(error) => {
                let _closed = endpoint.close().await;
                Err(error)
            }
        }
    }
}
struct DeliveryLease {
    selection: Arc<Selection>,
    state: Option<DeliveryState>,
}
impl Drop for DeliveryLease {
    fn drop(&mut self) {
        {
            let mut slot = self
                .selection
                .delivery
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if let Some(state) = self.state.take() {
                *slot = DeliverySlot::Owned(state);
            }
        }
        self.selection.returned.notify_waiters();
    }
}

/// A prep stream and its SAME real F1 controller remain together. Drop closes
/// any locally held stream and starts the same owned close; observation loss
/// cannot turn that into a second close request or a backend-stop claim.
pub(crate) struct PreparationEndpoint {
    target: OwnedLocalTarget,
    stream: Option<UnixStream>,
    owner: Option<Arc<DeliveryOwner>>,
}
impl PreparationEndpoint {
    pub(crate) fn stream_mut(&mut self) -> io::Result<&mut UnixStream> {
        self.stream.as_mut().ok_or_else(unavailable)
    }
    pub(crate) fn take_stream(&mut self) -> io::Result<UnixStream> {
        self.stream.take().ok_or_else(unavailable)
    }
    pub(crate) async fn close(&mut self) -> io::Result<()> {
        drop(self.stream.take());
        self.owner.as_ref().ok_or_else(unavailable)?.close().await
    }
    fn into_session(mut self) -> io::Result<()> {
        if self.stream.is_some() {
            return Err(unavailable());
        }
        let expected = self.owner.as_ref().ok_or_else(unavailable)?;
        {
            let mut slot = self
                .target
                .0
                .delivery
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            match &mut *slot {
                DeliverySlot::Owned(DeliveryState {
                    phase: Some(DeliveryPhase::Active { owner, role }),
                    ..
                }) if Arc::ptr_eq(owner, expected) => *role = ActiveRole::SessionUnclaimed,
                _ => return Err(unavailable()),
            }
        }
        drop(self.owner.take());
        Ok(())
    }
}
impl Drop for PreparationEndpoint {
    fn drop(&mut self) {
        drop(self.stream.take());
        if let Some(owner) = self.owner.take() {
            let _started = owner.begin_close();
        }
    }
}

/// The owning live session closes its client first, then this EXACT final
/// delivery controller. Views retain custody, not a new permission or owner.
pub(crate) struct FrontendTenure {
    pub(crate) target: OwnedLocalTarget,
    delivery: Arc<DeliveryOwner>,
    _owner: Option<SessionClose>,
}
impl Clone for FrontendTenure {
    fn clone(&self) -> Self {
        Self {
            target: self.target.clone(),
            delivery: self.delivery.clone(),
            _owner: None,
        }
    }
}
struct SessionClose(Arc<DeliveryOwner>);
impl Drop for SessionClose {
    fn drop(&mut self) {
        let _started = self.0.begin_close();
    }
}
impl FrontendTenure {
    /// Keep the actual Runtime constructor context on THIS physical delivery.
    /// It is historical resource custody; every later Source begin calls Native.
    pub(crate) fn retain_runtime_frontier(
        &self,
        frontier: &codex_alab_tui::frontend_control::FrontendFrontier,
    ) -> io::Result<()> {
        if !frontier.is_for_controller(&self.delivery.controller)
            || frontier.service_data() != self.target.service_data()
            || self.delivery.closing()
        {
            return Err(unavailable());
        }
        let retired = {
            // Same lock order as install: delivery UI slot, then its SDK-data
            // resource. No Progress guard is acquired in either direction.
            let ui = self
                .delivery
                .ui_frontier
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let mut slot = self
                .delivery
                .runtime_frontier
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if let Some(old) = &*slot {
                if old.frontier_data() == frontier.frontier_data()
                    && old.go_data() == frontier.go_data()
                {
                    return Ok(());
                }
                if ui.as_ref().is_none_or(|old| !old.status.is_complete()) {
                    return Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        "prior Runtime context has no completed UI transition",
                    ));
                }
            }
            slot.replace(frontier.clone())
        };
        drop(retired);
        Ok(())
    }
    pub(crate) fn runtime_frontier(
        &self,
    ) -> io::Result<codex_alab_tui::frontend_control::FrontendFrontier> {
        if self.delivery.closing() {
            return Err(unavailable());
        }
        self.delivery
            .runtime_frontier
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or_else(unavailable)
    }
    fn promotion_status(&self, native: &kind::Native) -> io::Result<Arc<UiStatus>> {
        if !native.is_for_controller(&self.delivery.controller)
            || native.service_data() != self.target.service_data()
            || self.delivery.closing()
        {
            return Err(unavailable());
        }
        let slot = self
            .delivery
            .ui_frontier
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let slot = slot.as_ref().ok_or_else(unavailable)?;
        if !slot.status.is_active() || !slot.same_native(native) {
            return Err(unavailable());
        }
        Ok(slot.status.clone())
    }
    /// Stage the NEW actual owner before any async current/final commit query.
    /// Preserve the prior committed keeper through uncertainty; no label grants
    /// a new constructor or permits replacement of an active pending promotion.
    pub(crate) fn retain_app_owner(
        &self,
        frontier: &codex_alab_tui::frontend_control::FrontendFrontier,
        owner: codex_alab_tui::frontend_control::AppFrontendOwner,
    ) -> io::Result<codex_alab_tui::frontend_control::AppFrontendOwner> {
        if frontier.go_data().is_none() || !owner.same_frontier(frontier) {
            return Err(unavailable());
        }
        let status = self.promotion_status(&kind::Native::Runtime(frontier.clone()))?;
        let mut owners = self
            .delivery
            .app_owners
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if !status.is_active() {
            return Err(unavailable());
        }
        if let Some(pending) = &owners.pending {
            if Arc::ptr_eq(&pending.status, &status)
                && let AppPromotion::Runtime(actual) = &pending.owner
                && actual.same_frontier(frontier)
            {
                return Ok(actual.clone());
            }
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "actual App promotion remains pending",
            ));
        }
        owners.pending = Some(PendingPromotion {
            owner: AppPromotion::Runtime(owner.clone()),
            status,
        });
        Ok(owner)
    }
    pub(crate) fn retain_source_app_owner(
        &self,
        frontier: &codex_alab_tui::frontend_control::FrontendSelectionFrontier,
        owner: codex_alab_tui::frontend_control::FrontendSelectionAppOwner,
    ) -> io::Result<codex_alab_tui::frontend_control::FrontendSelectionAppOwner> {
        if frontier.go_data().is_none() || !owner.same_frontier(frontier) {
            return Err(unavailable());
        }
        let status = self.promotion_status(&kind::Native::Source(frontier.clone()))?;
        let mut owners = self
            .delivery
            .app_owners
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if !status.is_active() {
            return Err(unavailable());
        }
        if let Some(pending) = &owners.pending {
            if Arc::ptr_eq(&pending.status, &status)
                && let AppPromotion::Source(actual) = &pending.owner
                && actual.same_frontier(frontier)
            {
                return Ok(actual.clone());
            }
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "actual Source App promotion remains pending",
            ));
        }
        owners.pending = Some(PendingPromotion {
            owner: AppPromotion::Source(owner.clone()),
            status,
        });
        Ok(owner)
    }
    pub(crate) fn install_ui_frontier(
        &self,
        pending: &Arc<Pending>,
    ) -> color_eyre::eyre::Result<()> {
        if !pending.belongs_to(self) || self.delivery.closing() {
            return Err(color_eyre::eyre::eyre!(
                "retained UI changed physical delivery"
            ));
        }
        if let Some(runtime) = pending.runtime_frontier() {
            self.retain_runtime_frontier(&runtime)?;
        }
        let status = pending.ui_status();
        if !status.is_active() {
            return Err(color_eyre::eyre::eyre!("new UI projection is not active"));
        }
        let retired = {
            let mut slot = self
                .delivery
                .ui_frontier
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if slot.as_ref().is_some_and(|old| !old.status.is_complete()) {
                return Err(color_eyre::eyre::eyre!(
                    "prior UI projection lacks actual completion"
                ));
            }
            slot.replace(UiSlot {
                owner: UiOwner::Projected(Arc::downgrade(pending)),
                status,
            })
        };
        drop(retired); // Even expired Weak never implied completion above.
        Ok(())
    }
    pub(crate) fn is_ui_frontier(&self, pending: &Pending) -> bool {
        if !pending.belongs_to(self) || self.delivery.closing() {
            return false;
        }
        let status = pending.ui_status();
        self.delivery
            .ui_frontier
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .is_some_and(|slot| {
                Arc::ptr_eq(&slot.status, &status)
                    && slot
                        .projected()
                        .is_some_and(|actual| std::ptr::eq(Arc::as_ref(&actual), pending))
            })
    }
    pub(crate) fn finish_ui_frontier(&self, pending: &Arc<Pending>) -> io::Result<()> {
        if !pending.belongs_to(self) || self.delivery.closing() {
            return Err(unavailable());
        }
        let status = pending.ui_status();
        let retired = {
            let slot = self
                .delivery
                .ui_frontier
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let slot = slot.as_ref().ok_or_else(unavailable)?;
            if !Arc::ptr_eq(&slot.status, &status)
                || !status.is_active()
                || !slot
                    .projected()
                    .is_some_and(|actual| Arc::ptr_eq(&actual, pending))
            {
                return Err(unavailable());
            }
            let mut owners = self
                .delivery
                .app_owners
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if pending.expects_promotion() {
                if !owners
                    .pending
                    .as_ref()
                    .is_some_and(|owner| Arc::ptr_eq(&owner.status, &status))
                {
                    return Err(unavailable());
                }
                let promoted = owners.pending.take().ok_or_else(unavailable)?;
                owners._committed.replace(promoted.owner)
            } else {
                if owners.pending.is_some() {
                    return Err(unavailable());
                }
                None
            }
        };
        drop(retired); // Only AFTER real commit, outside progress/delivery/App locks.
        Ok(())
    }
    /// Exact-tenure completion DATA overrides stale historical/disk true.
    pub(crate) fn task_tools_observation(&self, thread: ThreadId) -> Option<bool> {
        self.delivery
            .task_tools
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&thread)
            .copied()
    }
    pub(crate) fn observe_task_tools(
        &self,
        native: &kind::Native,
        receipt: &kind::Receipt,
    ) -> io::Result<()> {
        if !native.matches(receipt)
            || !native.is_for_controller(&self.delivery.controller)
            || native.service_data() != self.target.service_data()
            || self.delivery.closing()
        {
            return Err(unavailable());
        }
        let thread = ThreadId::from_string(receipt.thread_id_data()).map_err(|_| unavailable())?;
        self.delivery
            .task_tools
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(thread, receipt.task_tools_available());
        Ok(())
    }
    /// Same physical delivery custody, never a Native permission observation.
    pub(crate) fn same_delivery(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.delivery, &other.delivery)
    }
    pub(crate) fn controller(&self) -> &Controller {
        &self.delivery.controller
    }
    pub(crate) async fn close(&self) -> io::Result<()> {
        self.delivery.close().await
    }
}

#[cfg(test)]
#[path = "owned_local_frontend_opening_tests.rs"]
mod opening_tests;

#[path = "owned_local_initial_choice.rs"]
mod initial_choice;
pub(crate) use initial_choice::ConstructionObservation;
pub(crate) use initial_choice::ConstructionRecord;

#[cfg(test)]
#[path = "owned_local_entered_controller_tests.rs"]
mod entered_controller_tests;

#[path = "owned_local_configured_tenure.rs"]
mod configured_tenure;

// ALAB(side-boundary): installed Side view/close DATA remains owned across observer loss.
#[path = "owned_local_side_views.rs"]
mod side_views;

pub(crate) use side_views::SideViewCheckpoint;

#[path = "retained_image_frontend.rs"]
mod image_delivery;

#[path = "retained_backend_tenure.rs"]
mod backend_aggregate; // ALAB(retained-ui): actual open tenure binds backend slots to original service DATA.
