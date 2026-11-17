// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Allocate genuine cell identities before exporting native bodies through ONE
//! shared rich-line graph. Unique Composite children remain native Boxes.
use super::base::alab_retained::Data as BaseData;
use super::messages::alab_retained_messages::Data as MessageData;
use super::messages::alab_retained_user::UserData;
use super::patches::alab_retained_patches::Data as PatchData;
use super::plans::alab_retained_plans::Data as PlanData;
use super::request_user_input::alab_retained_request_user_input::Data as QuestionResultData;
use super::separators::alab_retained::Data as SeparatorData;
use super::session::alab_retained::Data as SessionData;
use super::startup_warnings::alab_retained_startup_warnings::Data as StartupWarningData;
use super::warnings::alab_retained_warnings::Data as WarningData;
use super::*;

pub(crate) use super::activity_details::alab_retained_details::Data as ActivityDetailsData;
use super::delegated_message::alab_retained_delegated::Data as DelegatedData;
use super::dynamic::alab_retained_dynamic as dynamic;
pub(crate) use super::dynamic::alab_retained_dynamic::Data as DynamicData;
use super::exec::alab_retained_terminal::Data as TerminalData;
use super::hook_cell::alab_retained_hook::Data as HookData;
use super::mcp::alab_retained_mcp::ComputerData;
use super::mcp::alab_retained_mcp::Data as McpData;
use super::notices::alab_retained_notice::Data as NoticeData;
use super::search::alab_retained_web::Data as WebData;
use super::session::AlabRetainedBannerData as BannerCellData;
use crate::app::AlabRetainedAgentStatusData as AgentStatusData;
use crate::chatwidget::AlabRetainedSplitFlapData as SplitFlapData;
use crate::exec_cell::alab_retained_exec::Data as ExecData;
use crate::inline_visualization::InlineVisualizationContext;
use crate::inline_visualization::alab_retained_context as inline;
use crate::status::alab_retained_status as status;
pub(crate) use crate::status::alab_retained_status::StatusRef;
pub(crate) use crate::status::alab_retained_status::SummaryRef;
use crate::terminal_hyperlinks::alab_retained_text as native;
use crate::tui::alab_retained_frames as frames;
pub(crate) use crate::tui::alab_retained_frames::FrameRef;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::io;
use std::sync::Arc;

const MAX_CELLS: usize = 1_000_000;
pub(crate) fn depth(value: usize) -> io::Result<usize> {
    if value >= 256 {
        Err(native::invalid("native history-cell nesting/cycle"))
    } else {
        Ok(value + 1)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct CellRef(u32);

/// Nonserializable shared capture resources. All cells and held layouts use
/// lines() from the SAME original layout Capture; no per-cell alias table.
pub(crate) struct CaptureContext<'a> {
    lines: &'a mut native::Capture,
    inline: &'a mut inline::Capture,
    activity: &'a mut dynamic::Capture,
    status: &'a mut status::Capture,
    frames: &'a mut frames::Capture,
    indices: &'a mut HashMap<usize, CellRef>,
    held: &'a mut Vec<Arc<dyn HistoryCell>>,
}
impl CaptureContext<'_> {
    pub(crate) fn frame(&mut self, actual: &crate::tui::FrameRequester) -> io::Result<FrameRef> {
        self.frames.reference(actual)
    }
    pub(crate) fn cell(&mut self, actual: &Arc<dyn HistoryCell>) -> io::Result<CellRef> {
        intern_cell(self.indices, self.held, actual)
    }
    /// SAME table for actual widget registries and cells; call IDs never mint aliases.
    pub(in crate::history_cell) fn dynamic(&mut self) -> &mut dynamic::Capture {
        self.activity
    }
    pub(in crate::history_cell) fn lines(&mut self) -> &mut native::Capture {
        self.lines
    }
    pub(in crate::history_cell) fn inline_context(
        &mut self,
        actual: &InlineVisualizationContext,
    ) -> io::Result<inline::ContextData> {
        self.inline.context(actual)
    }
}
/// Genuine Config-derived resources supplied by the actual restored widget/session.
/// Missing bindings refuse; decoded cwd/path/cache DATA cannot create a binding.
#[derive(Default)]
pub(crate) struct RestoreContext {
    image_renderer: Option<Arc<crate::retained_image_renderer::Binding>>,
    inline: inline::Bindings,
    activity: dynamic::Restored,
    status: status::Restored,
    frames: frames::Bindings,
    cells: Option<CellRestore>,
    completed_graph: Option<std::sync::Weak<()>>,
}
struct CellRestore {
    pending: Vec<Option<Body>>,
    ready: Vec<Option<Arc<dyn HistoryCell>>>,
}
impl RestoreContext {
    pub(crate) fn bind_image_renderer(
        &mut self,
        actual: &Arc<crate::retained_image_renderer::Binding>,
    ) -> io::Result<()> {
        actual.check()?;
        if self
            .image_renderer
            .as_ref()
            .is_some_and(|original| !Arc::ptr_eq(original, actual))
        {
            return Err(native::invalid(
                "native scene cannot replace its actual image renderer",
            ));
        }
        self.image_renderer = Some(actual.clone());
        Ok(())
    }
    pub(crate) fn require_image_renderer(
        &self,
        actual: &crate::retained_image_renderer::Binding,
    ) -> io::Result<()> {
        if self
            .image_renderer
            .as_ref()
            .is_none_or(|original| !std::ptr::eq(original.as_ref(), actual))
        {
            return Err(native::invalid(
                "native view and cells have different actual renderer bindings",
            ));
        }
        actual.check()
    }
    pub(in crate::history_cell) fn image_renderer(
        &self,
    ) -> io::Result<&Arc<crate::retained_image_renderer::Binding>> {
        self.image_renderer.as_ref().ok_or_else(|| {
            native::invalid("native image cell requires the actual installed renderer")
        })
    }

    pub(crate) fn status_handle(
        &self,
        reference: StatusRef,
    ) -> io::Result<crate::status::StatusHistoryHandle> {
        self.status.handle(reference)
    }
    pub(crate) fn status_summary(
        &self,
        reference: SummaryRef,
    ) -> io::Result<Arc<std::sync::RwLock<String>>> {
        self.status.summary(reference)
    }
    pub(crate) fn bind_frame(
        &mut self,
        reference: FrameRef,
        actual: &crate::tui::FrameRequester,
    ) -> io::Result<()> {
        self.frames.bind(reference, actual)
    }
    pub(crate) fn frame(&self, reference: FrameRef) -> io::Result<crate::tui::FrameRequester> {
        self.frames.requester(reference)
    }
    /// Lazy graph resolution preserves a detail/root alias and refuses a decoded cycle.
    pub(crate) fn cell(
        &mut self,
        reference: CellRef,
        shared: &mut native::Restored,
        nesting: usize,
    ) -> io::Result<Arc<dyn HistoryCell>> {
        depth(nesting)?;
        let index = reference.0 as usize;
        let body = {
            let graph = self
                .cells
                .as_mut()
                .ok_or_else(|| native::invalid("native cell resolver not installed"))?;
            if let Some(Some(actual)) = graph.ready.get(index) {
                return Ok(actual.clone());
            }
            graph
                .pending
                .get_mut(index)
                .ok_or_else(|| native::invalid("native cell edge has no graph entry"))?
                .take()
                .ok_or_else(|| native::invalid("native cell cycle or unfinished restoration"))?
        };
        let actual: Arc<dyn HistoryCell> = Arc::from(body.restore(shared, self, nesting)?);
        self.cells
            .as_mut()
            .ok_or_else(|| native::invalid("native cell resolver lost"))?
            .ready[index] = Some(actual.clone());
        Ok(actual)
    }
    /// Restore an actual App dynamic-registry clone using the SAME decoded row table.
    pub(crate) fn dynamic_cell(&self, data: &DynamicData) -> io::Result<DynamicToolCallCell> {
        self.activity.cell(data)
    }
    pub(crate) fn bind_inline(&mut self, actual: &InlineVisualizationContext) -> io::Result<()> {
        self.inline.bind(actual)
    }
    pub(crate) fn inline_context(
        &self,
        data: &inline::ContextData,
    ) -> io::Result<InlineVisualizationContext> {
        self.inline.context(data)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::history_cell) enum Body {
    Image {
        original: Box<super::patches::alab_retained_images::Data>,
    },
    User {
        original: Box<UserData>,
    },
    Message {
        original: Box<MessageData>,
    },
    Exec {
        original: Box<ExecData>,
    },
    Terminal {
        original: Box<TerminalData>,
    },
    Mcp {
        original: Box<McpData>,
    },
    Computer {
        original: Box<ComputerData>,
    },
    Dynamic {
        original: Box<DynamicData>,
    },
    Web {
        original: Box<WebData>,
    },
    Hook {
        original: Box<HookData>,
    },
    Delegated {
        original: Box<DelegatedData>,
    },
    Notice {
        original: Box<NoticeData>,
    },
    AgentStatus {
        original: Box<AgentStatusData>,
    },
    SplitFlap {
        original: Box<SplitFlapData>,
    },
    Banner {
        original: Box<BannerCellData>,
    },
    Status {
        original: Box<status::Data>,
    },
    Base {
        original: Box<BaseData>,
    },
    Session {
        original: Box<SessionData>,
    },
    Separator {
        original: Box<SeparatorData>,
    },
    Plan {
        original: Box<PlanData>,
    },
    Patch {
        original: Box<PatchData>,
    },
    QuestionResult {
        original: Box<QuestionResultData>,
    },
    Warning {
        original: Box<WarningData>,
    },
    StartupWarning {
        original: Box<StartupWarningData>,
    },
}
impl Body {
    pub(in crate::history_cell) fn capture(
        actual: &dyn HistoryCell,
        context: &mut CaptureContext<'_>,
        nesting: usize,
    ) -> io::Result<Self> {
        let _next = depth(nesting)?;
        if let Some(original) =
            super::patches::alab_retained_images::Data::capture(actual, context.lines(), nesting)?
        {
            return Ok(Self::Image {
                original: Box::new(original),
            });
        }

        if let Some(user) = actual.as_any().downcast_ref::<UserHistoryCell>() {
            return Ok(Self::User {
                original: Box::new(UserData::capture(user)),
            });
        }
        if let Some(original) = MessageData::capture(actual, context)? {
            return Ok(Self::Message {
                original: Box::new(original),
            });
        }
        if let Some(original) = ExecData::capture(actual, context)? {
            return Ok(Self::Exec {
                original: Box::new(original),
            });
        }
        if let Some(original) = TerminalData::capture(actual) {
            return Ok(Self::Terminal {
                original: Box::new(original),
            });
        }
        if let Some(original) = McpData::capture(actual)? {
            return Ok(Self::Mcp {
                original: Box::new(original),
            });
        }
        if let Some(original) = ComputerData::capture(actual, context)? {
            return Ok(Self::Computer {
                original: Box::new(original),
            });
        }
        if let Some(original) = DynamicData::capture(actual, context.dynamic())? {
            return Ok(Self::Dynamic {
                original: Box::new(original),
            });
        }
        if let Some(original) = WebData::capture(actual)? {
            return Ok(Self::Web {
                original: Box::new(original),
            });
        }
        if let Some(original) = HookData::capture(actual)? {
            return Ok(Self::Hook {
                original: Box::new(original),
            });
        }
        if let Some(original) = DelegatedData::capture(actual) {
            return Ok(Self::Delegated {
                original: Box::new(original),
            });
        }
        if let Some(original) = NoticeData::capture(actual)? {
            return Ok(Self::Notice {
                original: Box::new(original),
            });
        }
        if let Some(original) = AgentStatusData::capture(actual)? {
            return Ok(Self::AgentStatus {
                original: Box::new(original),
            });
        }
        if let Some(original) = SplitFlapData::capture(actual, context, nesting)? {
            return Ok(Self::SplitFlap {
                original: Box::new(original),
            });
        }
        if let Some(original) = BannerCellData::capture(actual, context, nesting)? {
            return Ok(Self::Banner {
                original: Box::new(original),
            });
        }
        if let Some(original) = status::Data::capture(actual, &mut *context.status)? {
            return Ok(Self::Status {
                original: Box::new(original),
            });
        }
        // Native child codecs own their unique-Box edge increments. Passing the
        // unchanged depth here keeps Composite/SessionInfo and activity details
        // in the SAME graph without charging a dispatcher edge twice.
        if let Some(original) = BaseData::capture(actual, context, nesting)? {
            return Ok(Self::Base {
                original: Box::new(original),
            });
        }
        if let Some(original) = SessionData::capture(actual, context, nesting)? {
            return Ok(Self::Session {
                original: Box::new(original),
            });
        }
        if let Some(original) = SeparatorData::capture(actual, context, nesting)? {
            return Ok(Self::Separator {
                original: Box::new(original),
            });
        }
        if let Some(original) = PlanData::capture(actual, context.lines(), nesting)? {
            return Ok(Self::Plan {
                original: Box::new(original),
            });
        }
        if let Some(original) = PatchData::capture(actual, context.lines(), nesting)? {
            return Ok(Self::Patch {
                original: Box::new(original),
            });
        }
        if let Some(original) = QuestionResultData::capture(actual, context.lines(), nesting)? {
            return Ok(Self::QuestionResult {
                original: Box::new(original),
            });
        }
        if let Some(original) = WarningData::capture(actual, context.lines(), nesting)? {
            return Ok(Self::Warning {
                original: Box::new(original),
            });
        }
        if let Some(original) = StartupWarningData::capture(actual, context.lines(), nesting)? {
            return Ok(Self::StartupWarning {
                original: Box::new(original),
            });
        }
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "history cell requires its exact original native variant codec",
        ))
    }
    pub(in crate::history_cell) fn restore(
        self,
        shared: &mut native::Restored,
        resources: &mut RestoreContext,
        nesting: usize,
    ) -> io::Result<Box<dyn HistoryCell>> {
        let _next = depth(nesting)?;
        match self {
            Self::Image { original } => original.restore(shared, resources, nesting),
            Self::User { original } => original
                .restore()
                .map(|actual| Box::new(actual) as Box<dyn HistoryCell>),
            Self::Message { original } => original.restore(shared, resources),
            Self::Exec { original } => original.restore(shared, resources, _next),
            Self::Terminal { original } => Ok(original.restore()),
            Self::Mcp { original } => original.restore(),
            Self::Computer { original } => original.restore(shared, resources, _next),
            Self::Dynamic { original } => original.restore(&resources.activity),
            Self::Web { original } => original.restore(),
            Self::Hook { original } => original.restore(),
            Self::Delegated { original } => Ok(original.restore()),
            Self::Notice { original } => original.restore(),
            Self::AgentStatus { original } => original.restore(),
            Self::SplitFlap { original } => original.restore(shared, resources, nesting),
            Self::Banner { original } => original.restore(shared, resources, nesting),
            Self::Status { original } => original.restore(&resources.status),
            Self::Base { original } => original.restore(shared, resources, nesting),
            Self::Session { original } => original.restore(shared, resources, nesting),
            Self::Separator { original } => original.restore(shared, resources, nesting),
            Self::Plan { original } => original.restore(shared, nesting),
            Self::Patch { original } => original.restore(shared, nesting),
            Self::QuestionResult { original } => original.restore(shared, nesting),
            Self::Warning { original } => original.restore(shared, nesting),
            Self::StartupWarning { original } => original.restore(shared, nesting),
        }
    }
}
/// Exact unique Box body for private native implementations outside history_cell.
/// The private dispatcher variants stay private; this wrapper carries no resource grant.
#[derive(Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct UniqueBody(Body);
impl UniqueBody {
    pub(crate) fn capture(
        actual: &dyn HistoryCell,
        context: &mut CaptureContext<'_>,
        nesting: usize,
    ) -> io::Result<Self> {
        Body::capture(actual, context, nesting).map(Self)
    }
    pub(crate) fn restore(
        self,
        shared: &mut native::Restored,
        resources: &mut RestoreContext,
        nesting: usize,
    ) -> io::Result<Box<dyn HistoryCell>> {
        self.0.restore(shared, resources, nesting)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Data {
    schema: u8,
    cells: Vec<Body>,
    inline: inline::GraphData,
    activity: dynamic::Table,
    frames: u32,
    status: status::Graph,
}

#[derive(Default)]
pub(crate) struct Capture {
    indices: HashMap<usize, CellRef>,
    held: Vec<Arc<dyn HistoryCell>>,
    inline: inline::Capture,
    activity: dynamic::Capture,
    status: status::Capture,
    frames: frames::Capture,
}
impl Capture {
    /// Allocation only: pin before traversing other original UI graph owners.
    /// A successful intern is never body support, serialization or application.
    pub(crate) fn intern(&mut self, actual: &Arc<dyn HistoryCell>) -> io::Result<CellRef> {
        intern_cell(&mut self.indices, &mut self.held, actual)
    }

    /// Weak UI edges resolve only against already pinned REAL strong App roots.
    pub(crate) fn weak_reference(
        &self,
        actual: &std::sync::Weak<dyn HistoryCell>,
    ) -> io::Result<Option<CellRef>> {
        actual
            .upgrade()
            .map(|owner| {
                let pointer = Arc::as_ptr(&owner).cast::<()>() as usize;
                self.indices.get(&pointer).copied().ok_or_else(|| {
                    native::invalid("live native weak owner has no actual App strong root")
                })
            })
            .transpose()
    }

    /// Capture an actual unique native Box through this SAME queue and resource tables.
    /// No synthetic Arc/root identity is allocated for the unique owner.
    pub(crate) fn unique(
        &mut self,
        actual: &dyn HistoryCell,
        shared: &mut native::Capture,
    ) -> io::Result<UniqueBody> {
        let mut context = CaptureContext {
            lines: shared,
            inline: &mut self.inline,
            activity: &mut self.activity,
            status: &mut self.status,
            frames: &mut self.frames,
            indices: &mut self.indices,
            held: &mut self.held,
        };
        UniqueBody::capture(actual, &mut context, 0)
    }
    /// An actual registry clone shares this table with all history/held-view roots.
    pub(crate) fn dynamic_cell(&mut self, actual: &DynamicToolCallCell) -> io::Result<DynamicData> {
        self.activity.cell(actual)
    }
    /// Original live controllers share the enclosing scene's actual cache aliases.
    pub(crate) fn inline_context(
        &mut self,
        actual: &InlineVisualizationContext,
    ) -> io::Result<inline::ContextData> {
        self.inline.context(actual)
    }
    /// Actual C1 Tui/widget frame handles register before graph traversal.
    pub(crate) fn frame(&mut self, actual: &crate::tui::FrameRequester) -> io::Result<FrameRef> {
        self.frames.reference(actual)
    }
    pub(crate) fn status_handle(
        &mut self,
        actual: &crate::status::StatusHistoryHandle,
    ) -> io::Result<StatusRef> {
        self.status.handle(actual)
    }
    pub(crate) fn status_summary(
        &mut self,
        actual: &Arc<std::sync::RwLock<String>>,
    ) -> io::Result<SummaryRef> {
        self.status.summary(actual)
    }
    pub(crate) fn encode(
        &mut self,
        shared: &mut native::Capture,
        max_bytes: usize,
    ) -> io::Result<Vec<u8>> {
        let mut cells = Vec::new();
        let mut index = 0;
        while index < self.held.len() {
            // Clone only this original root before details can grow the SAME work queue.
            let actual = self.held[index].clone();
            let mut context = CaptureContext {
                lines: shared,
                inline: &mut self.inline,
                activity: &mut self.activity,
                status: &mut self.status,
                frames: &mut self.frames,
                indices: &mut self.indices,
                held: &mut self.held,
            };
            cells.push(Body::capture(actual.as_ref(), &mut context, 0)?);
            index += 1;
        }
        let data = Data {
            schema: 5,
            cells,
            inline: self.inline.export()?,
            activity: self.activity.export()?,
            frames: self.frames.count()?,
            status: self.status.export()?,
        };
        let bytes = serde_json::to_vec(&data).map_err(native::invalid)?;
        if max_bytes == 0 || bytes.len() > max_bytes {
            return Err(native::invalid("native history-cell byte budget"));
        }
        Ok(bytes)
    }
}
fn intern_cell(
    indices: &mut HashMap<usize, CellRef>,
    held: &mut Vec<Arc<dyn HistoryCell>>,
    actual: &Arc<dyn HistoryCell>,
) -> io::Result<CellRef> {
    let pointer = Arc::as_ptr(actual).cast::<()>() as usize;
    if let Some(index) = indices.get(&pointer) {
        return Ok(*index);
    }
    if held.len() >= MAX_CELLS {
        return Err(native::invalid("native history-cell graph capacity"));
    }
    let index = CellRef(u32::try_from(held.len()).map_err(native::invalid)?);
    held.push(actual.clone());
    indices.insert(pointer, index);
    Ok(index)
}
/// Local graph identity only; no native/UI worker/OS permission is represented.
pub(crate) struct ContextIdentity(Arc<()>);
impl ContextIdentity {
    pub(crate) fn require(&self, resources: &RestoreContext) -> io::Result<()> {
        if resources
            .completed_graph
            .as_ref()
            .and_then(std::sync::Weak::upgrade)
            .is_none_or(|actual| !Arc::ptr_eq(&actual, &self.0))
        {
            Err(native::invalid(
                "native cells changed their actual global restore context",
            ))
        } else {
            Ok(())
        }
    }
}
pub(crate) struct Restored {
    cells: Vec<Arc<dyn HistoryCell>>,
    identity: Arc<()>,
}
impl Restored {
    pub(crate) fn decode(
        bytes: &[u8],
        max_bytes: usize,
        shared: &mut native::Restored,
        resources: &mut RestoreContext,
    ) -> io::Result<Self> {
        if max_bytes == 0 || bytes.len() > max_bytes {
            return Err(native::invalid("native history-cell byte budget"));
        }
        let raw: serde_json::Value = serde_json::from_slice(bytes).map_err(native::invalid)?;
        let data: Data = serde_json::from_slice(bytes).map_err(native::invalid)?;
        if raw != serde_json::to_value(&data).map_err(native::invalid)?
            || data.schema != 5
            || data.cells.len() > MAX_CELLS
        {
            return Err(native::invalid(
                "native cell graph schema/required fields/capacity",
            ));
        }
        if resources.cells.is_some() {
            return Err(native::invalid(
                "native cell graph decode already in progress",
            ));
        }
        resources.completed_graph = None;
        resources.inline.install(data.inline)?;
        resources.activity.install(data.activity)?;
        resources.frames.install(data.frames)?;
        resources.status.install(data.status)?;
        let count = data.cells.len();
        resources.cells = Some(CellRestore {
            pending: data.cells.into_iter().map(Some).collect(),
            ready: vec![None; count],
        });
        let restored = (0..count)
            .map(|index| resources.cell(CellRef(index as u32), shared, 0))
            .collect::<io::Result<Vec<_>>>();
        // No stale in-progress resolver can be reused after a failed DATA decode.
        let _ = resources.cells.take();
        let cells = restored?;
        let identity = Arc::new(());
        resources.completed_graph = Some(Arc::downgrade(&identity));
        Ok(Self { cells, identity })
    }
    pub(crate) fn context_identity(&self) -> ContextIdentity {
        ContextIdentity(self.identity.clone())
    }
    pub(crate) fn require_context(&self, resources: &RestoreContext) -> io::Result<()> {
        self.context_identity().require(resources)
    }
    /// Unique native owners may refer to the SAME finished global Arc roots.
    /// The supplied context must be the one that actually decoded this graph.
    pub(crate) fn restore_unique(
        &self,
        body: UniqueBody,
        shared: &mut native::Restored,
        resources: &mut RestoreContext,
    ) -> io::Result<Box<dyn HistoryCell>> {
        self.require_context(resources)?;
        if resources.cells.is_some() {
            return Err(native::invalid(
                "unique cell changed its actual global restore context",
            ));
        }
        resources.cells = Some(CellRestore {
            pending: (0..self.cells.len()).map(|_| None).collect(),
            ready: self.cells.iter().cloned().map(Some).collect(),
        });
        struct Lease<'a>(&'a mut RestoreContext);
        impl Drop for Lease<'_> {
            fn drop(&mut self) {
                self.0.cells = None;
            }
        }
        let lease = Lease(resources);
        body.restore(shared, &mut *lease.0, 0)
    }
    pub(crate) fn weak_reference(
        &self,
        reference: Option<CellRef>,
    ) -> io::Result<std::sync::Weak<dyn HistoryCell>> {
        match reference {
            Some(reference) => self.cell(reference).map(|owner| Arc::downgrade(&owner)),
            None => Ok(std::sync::Weak::<crate::history_cell::PlainHistoryCell>::new()),
        }
    }
    pub(crate) fn cell(&self, index: CellRef) -> io::Result<Arc<dyn HistoryCell>> {
        self.cells.get(index.0 as usize).cloned().ok_or_else(|| {
            native::invalid("native history-cell reference has no actual graph entry")
        })
    }
}

#[cfg(test)]
#[path = "retained_history_cell_graph_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "retained_history_activity_tests.rs"]
mod activity_tests;
