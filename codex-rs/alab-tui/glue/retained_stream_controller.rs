// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Complete native stream controllers within the SAME enclosing rich-line graph.
use super::*;
use crate::history_cell::alab_retained_cells as cells;
use crate::inline_visualization::alab_retained_context as inline;
use crate::render::highlight::alab_retained_image_theme::CacheGeneration;
use crate::render::highlight::alab_retained_image_theme::ThemeStamp;
use crate::streaming::alab_retained::PathData;
use crate::streaming::alab_retained::invalid;
use crate::streaming::alab_retained::required;
use crate::streaming::alab_retained_state as state;
use crate::streaming::prose_preview::alab_retained_preview as preview;
use crate::streaming::render::alab_retained_render as render;
use crate::streaming::table_holdback::alab_retained_holdback as holdback;
use crate::terminal_hyperlinks::alab_retained_text as native;
use serde::Deserialize;
use serde::Serialize;
use std::io;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CoreData {
    state: state::Data,
    #[serde(deserialize_with = "required")]
    width: Option<usize>,
    render: render::Data,
    preview: preview::Data,
    enqueued_stable_len: usize,
    emitted_stable_len: usize,
    cwd: PathData,
    #[serde(deserialize_with = "required")]
    inline_visualization_context: Option<inline::ContextData>,
    raw: bool,
    #[serde(deserialize_with = "required")]
    stable_prefix_len_cache: Option<(usize, Option<usize>, usize)>,
    holdback_scanner: holdback::Data,
}
impl CoreData {
    fn capture(
        actual: &StreamCore,
        shared: &mut native::Capture,
        cells: &mut cells::Capture,
    ) -> io::Result<Self> {
        let StreamCore {
            state,
            width,
            render,
            preview,
            enqueued_stable_len,
            emitted_stable_len,
            cwd,
            inline_visualization_context,
            render_mode,
            stable_prefix_len_cache,
            holdback_scanner,
        } = actual;
        Ok(Self {
            state: state::Data::capture(state, shared)?,
            width: *width,
            render: render::Data::capture(render, state.collector.committed_source(), shared)?,
            preview: preview::Data::capture(preview, shared)?,
            enqueued_stable_len: *enqueued_stable_len,
            emitted_stable_len: *emitted_stable_len,
            cwd: PathData::capture(cwd),
            inline_visualization_context: inline_visualization_context
                .as_ref()
                .map(|x| cells.inline_context(x))
                .transpose()?,
            raw: matches!(render_mode, HistoryRenderMode::Raw),
            stable_prefix_len_cache: stable_prefix_len_cache
                .as_ref()
                .map(|x| (x.source_start, x.width, x.stable_prefix_len)),
            holdback_scanner: holdback::Data::capture(holdback_scanner),
        })
    }
    fn restore_native(
        self,
        shared: &mut native::Restored,
        inline: Option<InlineVisualizationContext>,
        revision: Option<u64>,
    ) -> io::Result<StreamCore> {
        if self.inline_visualization_context.is_some() != inline.is_some() {
            return Err(invalid("streaming visualization is not genuinely bound"));
        }
        let state = self.state.restore(shared)?;
        if self
            .enqueued_stable_len
            .checked_sub(self.emitted_stable_len)
            != Some(state.queued_len())
        {
            return Err(invalid(
                "streaming FIFO differs from actual enqueued/emitted boundaries",
            ));
        }
        let source = state.collector.committed_source();
        let render = self.render.restore(source, shared, revision)?;
        if self.emitted_stable_len > self.enqueued_stable_len
            || self.enqueued_stable_len > render.lines.len()
        {
            return Err(invalid(
                "streaming emitted/enqueued boundary outside original render",
            ));
        }
        let stable_prefix_len_cache = self
            .stable_prefix_len_cache
            .map(
                |(source_start, width, stable_prefix_len)| -> io::Result<StablePrefixLenCache> {
                    crate::streaming::alab_retained::boundary(source, source_start)?;
                    Ok(StablePrefixLenCache {
                        source_start,
                        width,
                        stable_prefix_len,
                    })
                },
            )
            .transpose()?;
        let preview = self
            .preview
            .restore(state.collector.pending_source(), shared)?;
        let holdback_scanner = self.holdback_scanner.restore(source)?;
        Ok(StreamCore {
            state,
            width: self.width,
            render,
            preview,
            enqueued_stable_len: self.enqueued_stable_len,
            emitted_stable_len: self.emitted_stable_len,
            cwd: self.cwd.restore()?,
            inline_visualization_context: inline,
            render_mode: if self.raw {
                HistoryRenderMode::Raw
            } else {
                HistoryRenderMode::Rich
            },
            stable_prefix_len_cache,
            holdback_scanner,
        })
    }
    fn restore_bound(
        self,
        shared: &mut native::Restored,
        resources: &cells::RestoreContext,
        captured_theme: &ThemeStamp,
        renderer: &crate::retained_image_renderer::Binding,
    ) -> io::Result<StreamCore> {
        resources.require_image_renderer(renderer)?;
        let revision = self
            .render
            .fence_revision()
            .map(|old| captured_theme.cache_generation(old, renderer))
            .transpose()?
            .and_then(|x| match x {
                CacheGeneration::Equivalent(x) => Some(x),
                CacheGeneration::PendingInvalidation => None,
            });
        let inline = self
            .inline_visualization_context
            .as_ref()
            .map(|x| resources.inline_context(x))
            .transpose()?;
        self.restore_native(shared, inline, revision)
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AgentData {
    core: CoreData,
    header_emitted: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PlanData {
    core: CoreData,
    header_emitted: bool,
    top_padding_emitted: bool,
}
impl AgentData {
    pub(crate) fn capture(
        actual: &StreamController,
        shared: &mut native::Capture,
        cells: &mut cells::Capture,
    ) -> io::Result<Self> {
        let StreamController {
            core,
            header_emitted,
        } = actual;
        Ok(Self {
            core: CoreData::capture(core, shared, cells)?,
            header_emitted: *header_emitted,
        })
    }
    pub(crate) fn restore_bound(
        self,
        shared: &mut native::Restored,
        resources: &cells::RestoreContext,
        theme: &ThemeStamp,
        renderer: &crate::retained_image_renderer::Binding,
    ) -> io::Result<StreamController> {
        Ok(StreamController {
            core: self
                .core
                .restore_bound(shared, resources, theme, renderer)?,
            header_emitted: self.header_emitted,
        })
    }
}
impl PlanData {
    pub(crate) fn capture(
        actual: &PlanStreamController,
        shared: &mut native::Capture,
        cells: &mut cells::Capture,
    ) -> io::Result<Self> {
        let PlanStreamController {
            core,
            header_emitted,
            top_padding_emitted,
        } = actual;
        Ok(Self {
            core: CoreData::capture(core, shared, cells)?,
            header_emitted: *header_emitted,
            top_padding_emitted: *top_padding_emitted,
        })
    }
    pub(crate) fn restore_bound(
        self,
        shared: &mut native::Restored,
        resources: &cells::RestoreContext,
        theme: &ThemeStamp,
        renderer: &crate::retained_image_renderer::Binding,
    ) -> io::Result<PlanStreamController> {
        Ok(PlanStreamController {
            core: self
                .core
                .restore_bound(shared, resources, theme, renderer)?,
            header_emitted: self.header_emitted,
            top_padding_emitted: self.top_padding_emitted,
        })
    }
}
#[cfg(test)]
#[path = "retained_stream_controller_tests.rs"]
mod tests;
