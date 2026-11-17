// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual App history and focused-widget graph. This is a required presentation
//! section, not a complete App/E1 frame or an update/paint acknowledgement.
use super::history_ui::alab_retained as tails;
use super::native_history::alab_retained_queue as queue;
use super::turn_tips::alab_retained as tips;
use super::*;
use crate::app_backtrack::alab_retained as backtrack;
use crate::bottom_pane::TextArea;
use crate::chatwidget::alab_retained_streaming as streaming;
use crate::chatwidget::alab_retained_transcript as transcript;
use crate::clipboard_copy::worker::alab_retained_clipboard::BoundClipboard;
use crate::clipboard_copy::worker::alab_retained_clipboard::ScopeCapture;
use crate::clipboard_copy::worker::alab_retained_clipboard::ScopeRestored;
use crate::copy_input_guard::alab_retained_graph as guards;
use crate::history_cell::alab_retained_cells as cells;
use crate::terminal_hyperlinks::alab_retained_text as native;
use crate::transcript_reflow::alab_retained as reflow;
use crate::transcript_view::alab_retained_view as views;
use serde::Deserialize;
use serde::Serialize;
use std::io;
fn invalid(x: impl std::fmt::Display) -> io::Error {
    native::invalid(x)
}
fn required<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<T, D::Error> {
    T::deserialize(d)
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Replay {
    retained_lines: Vec<native::HyperlinkData>,
    render_from_transcript_tail: bool,
    was_truncated: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::app) struct Data<E> {
    schema: u8,
    transcript_cells: Vec<cells::CellRef>,
    native_history: queue::Data,
    turn_tips: tips::Data,
    view: views::complete::Data<E>,
    widget_transcript: transcript::Data,
    widget_streaming: streaming::Data,
    #[serde(deserialize_with = "required")]
    last_rendered_history_tail: Option<tails::TailData>,
    #[serde(deserialize_with = "required")]
    last_thread_usage_status_cell: Option<tails::UsageData>,
    pending_thread_usage_history_refresh: bool,
    deferred_history_lines: Vec<native::HyperlinkData>,
    has_emitted_history_lines: bool,
    transcript_reflow: reflow::Data,
    #[serde(deserialize_with = "required")]
    initial_history_replay_buffer: Option<Replay>,
    pending_thread_switch_resets: usize,
    scrollback_has_older_history: bool,
    backtrack: backtrack::Data,
    backtrack_render_pending: bool,
}
impl App {
    /// Run with all other App/overlay/widget root registrations before weak
    /// caches or cells are exported. This method borrows genuine App owners.
    pub(in crate::app) fn alab_register_transcript_roots(
        &self,
        graph: &mut views::CaptureGraph<'_>,
    ) -> io::Result<()> {
        views::complete::register_roots(&self.transcript_view, &self.transcript_cells, graph)?;
        for companion in self.side_companions.values() {
            views::complete::register_roots(&companion.view, &companion.cells, graph)?;
        }
        if let Some(Overlay::Transcript(actual)) = &self.overlay {
            actual.alab_register_retained_roots(graph)?;
        }
        backtrack::Data::register_roots(&self.backtrack, graph)?;
        graph.with_native(|cells, _text| queue::Data::register_roots(&self.native_history, cells))
    }
    pub(in crate::app) fn alab_capture_transcript<'a>(
        &'a self,
        graph: &mut views::CaptureGraph<'_>,
        scopes: &mut ScopeCapture,
        guards: &mut guards::Capture,
    ) -> io::Result<Data<&'a TextArea>> {
        self.alab_register_transcript_roots(graph)?;
        let roots = self
            .transcript_cells
            .iter()
            .map(|cell| graph.root(cell))
            .collect::<io::Result<_>>()?;
        let (
            widget_transcript,
            widget_streaming,
            native_history,
            turn_tips,
            last_rendered_history_tail,
            last_thread_usage_status_cell,
            deferred_history_lines,
            initial_history_replay_buffer,
        ) = graph.with_native(|cells, text| {
            let replay = self
                .initial_history_replay_buffer
                .as_ref()
                .map(|actual| -> io::Result<Replay> {
                    let InitialHistoryReplayBuffer {
                        retained_lines,
                        render_from_transcript_tail,
                        was_truncated,
                    } = actual;
                    Ok(Replay {
                        retained_lines: retained_lines
                            .iter()
                            .map(|x| text.line(x))
                            .collect::<io::Result<_>>()?,
                        render_from_transcript_tail: *render_from_transcript_tail,
                        was_truncated: *was_truncated,
                    })
                })
                .transpose()?;
            Ok((
                transcript::Data::capture(&self.chat_widget, cells, text)?,
                streaming::Data::capture(&self.chat_widget, text, cells)?,
                queue::Data::capture(&self.native_history, cells)?,
                tips::Data::capture(&self.turn_tips, cells)?,
                self.last_rendered_history_tail
                    .as_ref()
                    .map(|x| tails::TailData::capture(x, cells, text))
                    .transpose()?,
                self.last_thread_usage_status_cell
                    .as_ref()
                    .map(|x| tails::UsageData::capture(x, cells, text))
                    .transpose()?,
                self.deferred_history_lines
                    .iter()
                    .map(|x| text.line(x))
                    .collect::<io::Result<Vec<_>>>()?,
                replay,
            ))
        })?;
        Ok(Data {
            schema: 1,
            transcript_cells: roots,
            native_history,
            turn_tips,
            view: views::complete::Data::capture(&self.transcript_view, graph, scopes, guards)?,
            widget_transcript,
            widget_streaming,
            last_rendered_history_tail,
            last_thread_usage_status_cell,
            pending_thread_usage_history_refresh: self.pending_thread_usage_history_refresh,
            deferred_history_lines,
            has_emitted_history_lines: self.has_emitted_history_lines,
            transcript_reflow: reflow::Data::capture(&self.transcript_reflow)?,
            initial_history_replay_buffer,
            pending_thread_switch_resets: self.pending_thread_switch_resets,
            scrollback_has_older_history: self.scrollback_has_older_history,
            backtrack: backtrack::Data::capture(&self.backtrack, graph)?,
            backtrack_render_pending: self.backtrack_render_pending,
        })
    }
}
pub(in crate::app) struct PrepareBindings<'a> {
    pub(in crate::app) cells: &'a cells::Restored,
    pub(in crate::app) resources: &'a mut cells::RestoreContext,
    pub(in crate::app) theme: &'a crate::render::highlight::alab_retained_image_theme::ThemeStamp,
    pub(in crate::app) renderer: &'a Arc<crate::retained_image_renderer::Binding>,
    pub(in crate::app) tui: &'a crate::tui::Tui,
}
struct History {
    transcript_cells: Vec<Arc<dyn HistoryCell>>,
    native_history: super::native_history::NativeHistory,
    turn_tips: super::turn_tips::TurnTips,
    last_rendered_history_tail: Option<history_ui::RenderedHistoryTail>,
    last_thread_usage_status_cell: Option<history_ui::ThreadUsageStatusHistory>,
    pending_thread_usage_history_refresh: bool,
    deferred_history_lines: Vec<crate::terminal_hyperlinks::HyperlinkLine>,
    has_emitted_history_lines: bool,
    transcript_reflow: crate::transcript_reflow::TranscriptReflowState,
    initial_history_replay_buffer: Option<InitialHistoryReplayBuffer>,
    pending_thread_switch_resets: usize,
    scrollback_has_older_history: bool,
    backtrack_render_pending: bool,
}
pub(in crate::app) struct PreparedLines {
    context: cells::ContextIdentity,
    view: Stage<views::complete::PreparedText<TextArea>, views::complete::Prepared>,
    transcript: transcript::Prepared,
    streaming: streaming::Prepared,
    history: History,
    backtrack: backtrack::Data,
}
enum Stage<P, A> {
    Pending(P),
    Admitted(A),
}
pub(in crate::app) struct Prepared {
    context: cells::ContextIdentity,
    view: Stage<views::complete::Prepared, crate::transcript_view::TranscriptView>,
    transcript: Stage<transcript::Prepared, transcript::Admitted>,
    streaming: Stage<streaming::Prepared, streaming::Admitted>,
    history: History,
    backtrack: crate::app_backtrack::BacktrackState,
}
pub(in crate::app) struct Refused {
    pub(in crate::app) prepared: Prepared,
    pub(in crate::app) error: io::Error,
}

/// Only inert native DTO fields enter this helper. The actual decoded
/// editor is retained in LinePreparation.data.view until the final move.
fn duplicate<T: serde::Serialize + serde::de::DeserializeOwned>(data: &T) -> io::Result<T> {
    let bytes = serde_json::to_vec(data).map_err(invalid)?;
    serde_json::from_slice(&bytes).map_err(invalid)
}
pub(in crate::app) struct LinePreparation {
    data: Data<TextArea>,
    context: Option<cells::ContextIdentity>,
    history: Option<History>,
    transcript: Option<transcript::Prepared>,
    streaming: Option<streaming::Prepared>,
}
pub(in crate::app) struct PrepareLinesRefused {
    pub(in crate::app) input: LinePreparation,
    pub(in crate::app) error: io::Error,
}
impl Data<TextArea> {
    pub(in crate::app) fn prepare_lines(
        self,
        layouts: &mut views::layouts::Restored,
        bindings: &mut PrepareBindings<'_>,
    ) -> std::result::Result<PreparedLines, Box<PrepareLinesRefused>> {
        LinePreparation {
            data: self,
            context: None,
            history: None,
            transcript: None,
            streaming: None,
        }
        .retry(layouts, bindings)
    }
}
impl LinePreparation {
    pub(in crate::app) fn retry(
        mut self,
        layouts: &mut views::layouts::Restored,
        bindings: &mut PrepareBindings<'_>,
    ) -> std::result::Result<PreparedLines, Box<PrepareLinesRefused>> {
        let checked = (|| {
            if self.data.schema != 1 {
                return Err(invalid("App transcript section schema"));
            }
            bindings.cells.require_context(bindings.resources)?;
            if let Some(context) = &self.context {
                context.require(bindings.resources)?;
            }
            Ok(())
        })();
        if let Err(error) = checked {
            return Err(Box::new(PrepareLinesRefused { input: self, error }));
        }
        if self.context.is_none() {
            self.context = Some(bindings.cells.context_identity());
        }
        if self.history.is_none() {
            let prepared = (|| -> io::Result<History> {
                Ok(History {
                    transcript_cells: duplicate(&self.data.transcript_cells)?
                        .into_iter()
                        .map(|id| bindings.cells.cell(id))
                        .collect::<io::Result<_>>()?,
                    native_history: duplicate(&self.data.native_history)?
                        .restore(bindings.cells)?,
                    turn_tips: duplicate(&self.data.turn_tips)?.restore(bindings.cells)?,
                    last_rendered_history_tail: duplicate(&self.data.last_rendered_history_tail)?
                        .map(|x| x.restore(bindings.cells, layouts.lines()))
                        .transpose()?,
                    last_thread_usage_status_cell: duplicate(
                        &self.data.last_thread_usage_status_cell,
                    )?
                    .map(|x| x.restore(bindings.cells, layouts.lines()))
                    .transpose()?,
                    pending_thread_usage_history_refresh: duplicate(
                        &self.data.pending_thread_usage_history_refresh,
                    )?,
                    deferred_history_lines: duplicate(&self.data.deferred_history_lines)?
                        .iter()
                        .map(|x| layouts.lines().line(x))
                        .collect::<io::Result<_>>()?,
                    has_emitted_history_lines: duplicate(&self.data.has_emitted_history_lines)?,
                    transcript_reflow: duplicate(&self.data.transcript_reflow)?.restore()?,
                    initial_history_replay_buffer: duplicate(
                        &self.data.initial_history_replay_buffer,
                    )?
                    .map(|x| -> io::Result<InitialHistoryReplayBuffer> {
                        Ok(InitialHistoryReplayBuffer {
                            retained_lines: x
                                .retained_lines
                                .iter()
                                .map(|x| layouts.lines().line(x))
                                .collect::<io::Result<_>>()?,
                            render_from_transcript_tail: x.render_from_transcript_tail,
                            was_truncated: x.was_truncated,
                        })
                    })
                    .transpose()?,
                    pending_thread_switch_resets: duplicate(
                        &self.data.pending_thread_switch_resets,
                    )?,
                    scrollback_has_older_history: duplicate(
                        &self.data.scrollback_has_older_history,
                    )?,
                    backtrack_render_pending: duplicate(&self.data.backtrack_render_pending)?,
                })
            })();
            match prepared {
                Ok(value) => self.history = Some(value),
                Err(error) => return Err(Box::new(PrepareLinesRefused { input: self, error })),
            }
        }
        if self.transcript.is_none() {
            let prepared = (|| {
                let copy: transcript::Data = duplicate(&self.data.widget_transcript)?;
                copy.prepare(
                    bindings.cells,
                    layouts.lines(),
                    bindings.resources,
                    bindings.theme,
                )
            })();
            match prepared {
                Ok(value) => self.transcript = Some(value),
                Err(error) => return Err(Box::new(PrepareLinesRefused { input: self, error })),
            }
        }
        if self.streaming.is_none() {
            let prepared = (|| {
                let copy: streaming::Data = duplicate(&self.data.widget_streaming)?;
                copy.prepare(
                    bindings.cells,
                    layouts.lines(),
                    bindings.resources,
                    bindings.theme,
                    bindings.renderer,
                    bindings.tui,
                )
            })();
            match prepared {
                Ok(value) => self.streaming = Some(value),
                Err(error) => return Err(Box::new(PrepareLinesRefused { input: self, error })),
            }
        }
        // This pattern consumes no part if an internal stage is incomplete;
        // there is no dummy/native Default used to fill a missing owner.
        let (data, context, history, transcript, streaming) = match self {
            Self {
                data,
                context: Some(context),
                history: Some(history),
                transcript: Some(transcript),
                streaming: Some(streaming),
            } => (data, context, history, transcript, streaming),
            input => {
                return Err(Box::new(PrepareLinesRefused {
                    input,
                    error: invalid("App native line preparation remains incomplete"),
                }));
            }
        };
        let Data {
            schema,
            transcript_cells,
            native_history,
            turn_tips,
            view,
            widget_transcript,
            widget_streaming,
            last_rendered_history_tail,
            last_thread_usage_status_cell,
            pending_thread_usage_history_refresh,
            deferred_history_lines,
            has_emitted_history_lines,
            transcript_reflow,
            initial_history_replay_buffer,
            pending_thread_switch_resets,
            scrollback_has_older_history,
            backtrack,
            backtrack_render_pending,
        } = data;
        let view = match view.prepare_lines(layouts) {
            Ok(view) => view,
            Err(refused) => {
                let views::complete::LinesRefused { data: view, error } = *refused;
                let data = Data {
                    schema,
                    transcript_cells,
                    native_history,
                    turn_tips,
                    view,
                    widget_transcript,
                    widget_streaming,
                    last_rendered_history_tail,
                    last_thread_usage_status_cell,
                    pending_thread_usage_history_refresh,
                    deferred_history_lines,
                    has_emitted_history_lines,
                    transcript_reflow,
                    initial_history_replay_buffer,
                    pending_thread_switch_resets,
                    scrollback_has_older_history,
                    backtrack,
                    backtrack_render_pending,
                };
                return Err(Box::new(PrepareLinesRefused {
                    input: Self {
                        data,
                        context: Some(context),
                        history: Some(history),
                        transcript: Some(transcript),
                        streaming: Some(streaming),
                    },
                    error,
                }));
            }
        };
        Ok(PreparedLines {
            context,
            view: Stage::Pending(view),
            transcript,
            streaming,
            history,
            backtrack,
        })
    }
}

pub(in crate::app) struct LinesRefused {
    pub(in crate::app) prepared: PreparedLines,
    pub(in crate::app) error: io::Error,
}
impl PreparedLines {
    pub(in crate::app) fn prepare_owners(
        self,
        graph: &views::RestoreGraph<'_>,
        scopes: &ScopeRestored,
    ) -> std::result::Result<Prepared, Box<LinesRefused>> {
        let backtrack = (|| {
            let bytes = serde_json::to_vec(&self.backtrack).map_err(invalid)?;
            let copy: backtrack::Data = serde_json::from_slice(&bytes).map_err(invalid)?;
            copy.restore(graph)
        })();
        let backtrack = match backtrack {
            Ok(value) => value,
            Err(error) => {
                return Err(Box::new(LinesRefused {
                    prepared: self,
                    error,
                }));
            }
        };
        let Self {
            context,
            view,
            transcript,
            streaming,
            history,
            backtrack: original_backtrack,
        } = self;
        let view = match view {
            Stage::Admitted(view) => view,
            Stage::Pending(view) => match view.prepare_owners(graph, scopes) {
                Ok(view) => view,
                Err(refused) => {
                    let views::complete::OwnerRefused { prepared, error } = *refused;
                    return Err(Box::new(LinesRefused {
                        prepared: Self {
                            context,
                            view: Stage::Pending(prepared),
                            transcript,
                            streaming,
                            history,
                            backtrack: original_backtrack,
                        },
                        error,
                    }));
                }
            },
        };
        Ok(Prepared {
            context,
            view: Stage::Pending(view),
            transcript: Stage::Pending(transcript),
            streaming: Stage::Pending(streaming),
            history,
            backtrack,
        })
    }
}

/// Every fallible native body/resource operation finishes before App fields move.
/// The complete scene keeps this owner together with ALL other App sections.
pub(in crate::app) struct Admitted {
    context: cells::ContextIdentity,
    view: crate::transcript_view::TranscriptView,
    transcript: transcript::Admitted,
    streaming: streaming::Admitted,
    history: History,
    backtrack: crate::app_backtrack::BacktrackState,
}
pub(in crate::app) struct Displaced {
    _history: History,
    _view: crate::transcript_view::TranscriptView,
    _transcript: transcript::Displaced,
    _streaming: streaming::Displaced,
    _backtrack: crate::app_backtrack::BacktrackState,
}
impl Prepared {
    pub(in crate::app) fn admit(
        self,
        bindings: &views::complete::RestoreBindings<'_, '_>,
    ) -> std::result::Result<Admitted, Box<Refused>> {
        if let Err(error) = self.context.require(bindings.cell_resources) {
            return Err(Box::new(Refused {
                prepared: self,
                error,
            }));
        }
        let Self {
            context,
            view,
            transcript,
            streaming,
            history,
            backtrack,
        } = self;
        let view = match view {
            Stage::Admitted(view) => {
                if let Err(error) = views::complete::validate_bound(
                    &view,
                    bindings.cell_resources,
                    bindings.tui,
                    bindings.renderer,
                    bindings.clipboard,
                ) {
                    return Err(Box::new(Refused {
                        prepared: Self {
                            context,
                            view: Stage::Admitted(view),
                            transcript,
                            streaming,
                            history,
                            backtrack,
                        },
                        error,
                    }));
                }
                view
            }
            Stage::Pending(prepared) => match prepared.restore_bound(bindings) {
                Ok(view) => view,
                Err(refused) => {
                    let views::complete::Refused { prepared, error } = *refused;
                    return Err(Box::new(Refused {
                        prepared: Self {
                            context,
                            view: Stage::Pending(prepared),
                            transcript,
                            streaming,
                            history,
                            backtrack,
                        },
                        error,
                    }));
                }
            },
        };
        let transcript = match transcript {
            Stage::Admitted(transcript) => {
                if let Err(error) = transcript.validate(
                    bindings.tui,
                    bindings.renderer,
                    bindings.clipboard,
                    bindings.cell_resources,
                ) {
                    return Err(Box::new(Refused {
                        prepared: Self {
                            context,
                            view: Stage::Admitted(view),
                            transcript: Stage::Admitted(transcript),
                            streaming,
                            history,
                            backtrack,
                        },
                        error,
                    }));
                }
                transcript
            }
            Stage::Pending(prepared) => match prepared.admit(
                bindings.tui,
                bindings.renderer,
                bindings.clipboard,
                bindings.cell_resources,
            ) {
                Ok(transcript) => transcript,
                Err(refused) => {
                    let transcript::Refused { original, error } = *refused;
                    return Err(Box::new(Refused {
                        prepared: Self {
                            context,
                            view: Stage::Admitted(view),
                            transcript: Stage::Pending(original),
                            streaming,
                            history,
                            backtrack,
                        },
                        error,
                    }));
                }
            },
        };
        let streaming = match streaming {
            Stage::Admitted(streaming) => {
                if let Err(error) = streaming.validate(
                    bindings.tui,
                    bindings.renderer,
                    bindings.clipboard,
                    bindings.cell_resources,
                ) {
                    return Err(Box::new(Refused {
                        prepared: Self {
                            context,
                            view: Stage::Admitted(view),
                            transcript: Stage::Admitted(transcript),
                            streaming: Stage::Admitted(streaming),
                            history,
                            backtrack,
                        },
                        error,
                    }));
                }
                streaming
            }
            Stage::Pending(prepared) => match prepared.admit(
                bindings.tui,
                bindings.renderer,
                bindings.clipboard,
                bindings.cell_resources,
            ) {
                Ok(streaming) => streaming,
                Err(refused) => {
                    let streaming::Refused { prepared, error } = *refused;
                    return Err(Box::new(Refused {
                        prepared: Self {
                            context,
                            view: Stage::Admitted(view),
                            transcript: Stage::Admitted(transcript),
                            streaming: Stage::Pending(prepared),
                            history,
                            backtrack,
                        },
                        error,
                    }));
                }
            },
        };
        Ok(Admitted {
            context,
            view,
            transcript,
            streaming,
            history,
            backtrack,
        })
    }
}
impl Admitted {
    pub(in crate::app) fn validate(
        &self,
        resources: &cells::RestoreContext,
        tui: &crate::tui::Tui,
        renderer: &crate::retained_image_renderer::Binding,
        clipboard: &BoundClipboard,
    ) -> io::Result<()> {
        self.context.require(resources)?;
        views::complete::validate_bound(&self.view, resources, tui, renderer, clipboard)?;
        self.transcript
            .validate(tui, renderer, clipboard, resources)?;
        self.streaming.validate(tui, renderer, clipboard, resources)
    }
    pub(in crate::app) fn install(self, app: &mut App) -> Displaced {
        let Self {
            context: _,
            view,
            transcript,
            streaming,
            history,
            backtrack,
        } = self;
        let old = History {
            transcript_cells: std::mem::replace(
                &mut app.transcript_cells,
                history.transcript_cells,
            ),
            native_history: std::mem::replace(&mut app.native_history, history.native_history),
            turn_tips: std::mem::replace(&mut app.turn_tips, history.turn_tips),
            last_rendered_history_tail: std::mem::replace(
                &mut app.last_rendered_history_tail,
                history.last_rendered_history_tail,
            ),
            last_thread_usage_status_cell: std::mem::replace(
                &mut app.last_thread_usage_status_cell,
                history.last_thread_usage_status_cell,
            ),
            pending_thread_usage_history_refresh: std::mem::replace(
                &mut app.pending_thread_usage_history_refresh,
                history.pending_thread_usage_history_refresh,
            ),
            deferred_history_lines: std::mem::replace(
                &mut app.deferred_history_lines,
                history.deferred_history_lines,
            ),
            has_emitted_history_lines: std::mem::replace(
                &mut app.has_emitted_history_lines,
                history.has_emitted_history_lines,
            ),
            transcript_reflow: std::mem::replace(
                &mut app.transcript_reflow,
                history.transcript_reflow,
            ),
            initial_history_replay_buffer: std::mem::replace(
                &mut app.initial_history_replay_buffer,
                history.initial_history_replay_buffer,
            ),
            pending_thread_switch_resets: std::mem::replace(
                &mut app.pending_thread_switch_resets,
                history.pending_thread_switch_resets,
            ),
            scrollback_has_older_history: std::mem::replace(
                &mut app.scrollback_has_older_history,
                history.scrollback_has_older_history,
            ),
            backtrack_render_pending: std::mem::replace(
                &mut app.backtrack_render_pending,
                history.backtrack_render_pending,
            ),
        };
        Displaced {
            _history: old,
            _view: std::mem::replace(&mut app.transcript_view, view),
            _transcript: transcript.install(&mut app.chat_widget),
            _streaming: streaming.install(&mut app.chat_widget),
            _backtrack: std::mem::replace(&mut app.backtrack, backtrack),
        }
    }
}

#[cfg(test)]
#[path = "retained_app_transcript_tests.rs"]
mod controls;
