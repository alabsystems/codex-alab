//! The unfocused pane projects assistant replies and turn status while the thread event
//! store continues to own replay.

use super::ThreadEventStore;
use crate::app_event::CapturedSideDisplay;
use crate::app_event::CapturedSideWork;
use crate::git_action_directives::parse_assistant_markdown;
use crate::history_cell::AgentMarkdownCell;
use crate::history_cell::FinalMessageSeparator;
use crate::history_cell::HistoryCell;
use crate::transcript_view::TranscriptView;
use chrono::Local;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadItemPayload;
use codex_app_server_protocol::TurnStatus;
use codex_protocol::items::AsyncUserInputQuestion;
use codex_utils_absolute_path::AbsolutePathBuf;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

pub(super) struct SideCompanion {
    #[cfg(unix)]
    pub(super) alab_identity: Arc<()>, // ALAB(retained-ui): actual parked view incarnation, not a thread selector.
    #[cfg(unix)]
    pub(super) alab_retained: Option<Arc<super::alab_retained_passive::PassiveBinding>>, // ALAB(retained-ui): same accepted passive projection custody.
    #[cfg(unix)] pub(super) alab_command_delivery: super::alab_side_delivery::Applied, // ALAB(retained-side): exact store delivery high-water; no text dedup.
    pub(super) display: CapturedSideDisplay,
    pub(super) cells: Vec<Arc<dyn HistoryCell>>,
    pub(super) view: TranscriptView,
    pub(super) answer: Option<SideAnswer>,
    pub(super) parked_tail: usize,
}

pub(super) struct SideAnswer {
    turn_id: String,
    item_id: String,
    index: usize,
    pending: String,
}

impl SideAnswer {
    pub(super) fn new((turn_id, item_id): (String, String), index: usize) -> Self {
        Self {
            turn_id,
            item_id,
            index,
            pending: String::new(),
        }
    }

    fn matches(&self, turn_id: &str, item_id: &str) -> bool {
        self.turn_id == turn_id && self.item_id == item_id
    }
}

impl SideCompanion {
    pub(super) fn empty(cwd: AbsolutePathBuf) -> Self {
        Self {
            #[cfg(unix)]
            alab_identity: Arc::new(()), // ALAB(retained-ui): minted by actual original view allocation.
            #[cfg(unix)]
            alab_retained: None, // ALAB(retained-ui): no UI/Native receipt inferred from an empty draft.
            #[cfg(unix)] alab_command_delivery: super::alab_side_delivery::Applied::default(), // ALAB(retained-side): no applied delivery inferred.
            display: CapturedSideDisplay {
                #[cfg(unix)]
                alab_history: crate::app::alab_new_side_provenance(), // ALAB(retained-ui): genuine empty companion starts its own DATA provenance.
                rendered_completion_turn_ids: Default::default(),
                active: None,
                auxiliary: Vec::new(),
                draft: Box::default(),
                status_line: None,
                work: None,
                last_completed_agent_message: None,
                cwd,
                visualizations: None,
            },
            cells: Vec::new(),
            view: TranscriptView::default(),
            answer: None,
            parked_tail: 0,
        }
    }

    pub(super) fn apply_notification(
        &mut self,
        notification: &ServerNotification,
        store: &ThreadEventStore,
        new_status: impl FnOnce() -> crate::status_indicator_widget::StatusIndicatorWidget,
    ) -> bool {
        #[cfg(unix)]
        if let Some(receipt)=store.alab_side_delivery.current(notification).ok().flatten() { // ALAB(retained-side): genuine original input delivery, unchanged ordinary fallback otherwise.
            match self.alab_apply_command(notification,&receipt) {Ok(Some(changed))=>return changed,Err(_)=>{self.alab_command_delivery.fail();return false},_=>{}}
        }
        match notification {
            ServerNotification::TurnStarted(event)
                if self
                    .display
                    .work
                    .as_ref()
                    .is_none_or(|work| work.turn_id != event.turn.id) =>
            {
                self.display.work = Some(CapturedSideWork::new(
                    event.turn.id.clone(),
                    new_status(),
                    Duration::ZERO,
                ));
            }
            ServerNotification::AgentMessageDelta(delta)
                if !delta.delta.is_empty() && !store.is_realtime_delegated_turn(&delta.turn_id) =>
            {
                if self
                    .answer
                    .as_ref()
                    .is_some_and(|answer| !answer.matches(&delta.turn_id, &delta.item_id))
                {
                    self.flush_answer();
                    self.answer = None;
                }
                let answer = self.answer.get_or_insert_with(|| {
                    SideAnswer::new(
                        (delta.turn_id.clone(), delta.item_id.clone()),
                        self.cells.len(),
                    )
                });
                answer.pending.push_str(&delta.delta);
            }
            ServerNotification::ItemCompleted(event) => {
                let item = &event.item.payload;
                if store.is_realtime_delegated_turn(&event.turn_id)
                    && crate::chatwidget::is_private_realtime_agent_item(item)
                {
                    return false;
                }
                return self.finish_item(&event.turn_id, item);
            }
            ServerNotification::TurnCompleted(event)
                if event.turn.status != TurnStatus::InProgress =>
            {
                let turn = &event.turn;
                if self.display.active.is_some() {
                    for item in turn.item_payloads() {
                        if matches!(item, ThreadItemPayload::CommandExecution { .. }) {
                            self.finish_item(&turn.id, item);
                        }
                    }
                }
                if turn.status == TurnStatus::Completed
                    && let Some((item, _)) = crate::chatwidget::final_agent_message(
                        turn,
                        store.is_realtime_delegated_turn(&turn.id),
                    )
                    && crate::chatwidget::needs_agent_message(
                        self.display.last_completed_agent_message.as_ref(),
                        &turn.id,
                        item,
                    )
                {
                    self.finish_item(&turn.id, item);
                }
                if self
                    .answer
                    .as_ref()
                    .is_some_and(|answer| answer.turn_id == turn.id)
                {
                    self.flush_answer();
                    self.answer = None;
                }
                let work = self.display.work.take_if(|work| work.turn_id == turn.id);
                if turn.status == TurnStatus::Completed
                    && (work.is_some() || store.active_turn_id() == Some(turn.id.as_str()))
                    && let Some(completion) = FinalMessageSeparator::from_turn_once(
                        turn,
                        &mut self.display.rendered_completion_turn_ids,
                        work.map(|work| work.timer.elapsed_at(Instant::now())),
                        Some(Local::now()),
                    )
                {
                    self.cells.push(Arc::new(completion));
            #[cfg(unix)]
            self.alab_note_end(&turn.id,self.cells.len()-1); // ALAB(retained-ui): actual completion row retains original turn identity.
                }
            }
            ServerNotification::ThreadClosed(_) => return self.display.work.take().is_some(),
            _ => return false,
        }
        true
    }

    /// History pages may arrive while a reply or command still owns a slot in the parked transcript.
    pub(super) fn splice_older_cells(
        &mut self,
        range: std::ops::Range<usize>,
        cells: Vec<Arc<dyn HistoryCell>>,
    ) {
        let shift = |index: usize| {
            if index >= range.end {
                index - range.len() + cells.len()
            } else {
                index
            }
        };
        self.parked_tail = shift(self.parked_tail);
        if let Some(answer) = &mut self.answer {
            answer.index = shift(answer.index);
        }
        #[cfg(unix)]
        self.display.alab_history.lock().unwrap_or_else(std::sync::PoisonError::into_inner).removed(&self.cells[range.clone()]); // ALAB(retained-ui): exact original splice invalidates only removed cell provenance.
        self.cells.splice(range, cells);
    }

    fn finish_item(&mut self, turn: &str, item: &ThreadItemPayload) -> bool {
        match item {
            ThreadItemPayload::AgentMessage {
                id,
                text,
                questions,
                ..
            } => {
                let replaces = self
                    .answer
                    .as_ref()
                    .is_some_and(|answer| answer.matches(turn, id));
                if !replaces || text.is_empty() {
                    self.flush_answer();
                }
                let finished = self.answer.take_if(|_| replaces);
                if !text.is_empty() {
                    let index = finished.map_or(self.cells.len(), |answer| answer.index);
                    self.write_answer(
                        index,
                        text.clone(),
                        questions.as_deref().unwrap_or_default(),
                    );
                    #[cfg(unix)]
                    self.alab_note_answer(turn,id,index,Some(item.clone())); // ALAB(retained-ui): bind real completed answer payload to exact original cell.
                }
                self.display.last_completed_agent_message = Some((turn.to_owned(), id.clone()));
            }
            ThreadItemPayload::CommandExecution {
                id,
                status,
                exit_code,
                source,
                aggregated_output,
                duration_ms,
                ..
            } => {
                let Some(cell) = self.display.active.as_mut().and_then(|active| {
                    active
                        .as_any_mut()
                        .downcast_mut::<crate::exec_cell::ExecCell>()
                }) else {
                    return false;
                };
                if *status == codex_app_server_protocol::CommandExecutionStatus::InProgress
                    || !cell.complete_call(
                        id,
                        crate::exec_cell::CommandOutput::from_execution(
                            status.clone(),
                            *exit_code,
                            *source,
                            aggregated_output.clone(),
                        ),
                        duration_ms.map(|ms| Duration::from_millis(ms.max(0) as u64)), // ALAB(shell-output): display measured time only.
                    )
                {
                    return false;
                }
                if !cell.is_active()
                    && let Some(active) = self.display.active.take()
                {
                    // Later answer deltas may already have appended a cell after the parked command.
                    self.cells.insert(self.parked_tail, Arc::from(active));
                    #[cfg(unix)]
                    self.alab_note_answer(turn,id,self.parked_tail,Some(item.clone())); // ALAB(retained-ui): real completed command cell shares actual source identity.
                    if let Some(answer) = &mut self.answer
                        && answer.index >= self.parked_tail
                    {
                        answer.index += 1;
                    }
                }
            }
            _ => return false,
        }
        true
    }

    /// Commit pending deltas at paint time so parsing and wrapping run once per frame.
    pub(super) fn flush_answer(&mut self) {
        let Some(answer) = self
            .answer
            .as_mut()
            .filter(|answer| !answer.pending.is_empty())
        else {
            return;
        };
        let index = answer.index;
        #[cfg(unix)]
        let alab_item=(answer.turn_id.clone(),answer.item_id.clone()); // ALAB(retained-ui): exact original streaming source before write.
        let mut source = self
            .cells
            .get(index)
            .and_then(|cell| cell.copy_source())
            .unwrap_or_default()
            .to_owned();
        source.push_str(&answer.pending);
        answer.pending.clear();
        self.write_answer(index, source, &[]);
        #[cfg(unix)]
        self.alab_note_answer(&alab_item.0,&alab_item.1,index,None); // ALAB(retained-ui): real streaming cell range, no completed payload invented.
    }

    fn write_answer(&mut self, index: usize, source: String, questions: &[AsyncUserInputQuestion]) {
        let cwd = &self.display.cwd;
        let markdown = parse_assistant_markdown(&source, cwd.as_path());
        let cell: Arc<dyn HistoryCell> = Arc::new(AgentMarkdownCell::from_parsed_message(
            markdown.visible_markdown,
            source,
            cwd.as_path(),
            self.display.visualizations.clone(),
            questions,
        ));
        if index == self.cells.len() {
            self.cells.push(cell);
        } else {
            self.view
                .replace_range(&self.cells, index..index + 1, &cell);
            self.cells[index] = cell;
        }
    }
}

#[cfg(unix)]
#[path = "../../../alab-tui/glue/owned_local_side_history_companion.rs"]
mod alab_side_history_companion; // ALAB(retained-ui): actual original cell producer identities, not rendered-text inference.
