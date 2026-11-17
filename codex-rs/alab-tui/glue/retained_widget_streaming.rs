// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual ChatWidget streaming fields; a component of the required whole scene.
use super::*;
use crate::history_cell::alab_retained_cells as cells;
use crate::render::highlight::alab_retained_image_theme::ThemeStamp;
use crate::streaming::alab_retained::invalid;
use crate::streaming::alab_retained::required;
use crate::streaming::chunking::alab_retained_chunking as chunking;
use crate::streaming::controller::alab_retained_stream as controllers;
use crate::terminal_hyperlinks::alab_retained_text as native;
use serde::Deserialize;
use serde::Serialize;
use std::io;
use std::sync::Arc;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Data {
    schema: u8,
    adaptive_chunking: chunking::Data,
    #[serde(deserialize_with = "required")]
    stream_controller: Option<controllers::AgentData>,
    #[serde(deserialize_with = "required")]
    plan_stream_controller: Option<controllers::PlanData>,
    pending_stream_consolidations: usize,
}
pub(crate) struct Prepared {
    context: cells::ContextIdentity,
    renderer: Arc<crate::retained_image_renderer::Binding>,
    adaptive_chunking: AdaptiveChunkingPolicy,
    stream_controller: Option<StreamController>,
    plan_stream_controller: Option<PlanStreamController>,
    pending_stream_consolidations: usize,
}
pub(crate) struct Refused {
    pub(crate) prepared: Prepared,
    pub(crate) error: io::Error,
}
/// Local UI admission only. The enclosing scene retains its actual resources
/// and performs final currentness/paint checks before any Native UI acknowledgement.
pub(crate) struct Admitted(Prepared);
/// The complete scene owner retains this displaced actual native state until
/// its original rollback/commit policy has settled. Installation does not drain.
pub(crate) struct Displaced {
    _adaptive_chunking: AdaptiveChunkingPolicy,
    _stream_controller: Option<StreamController>,
    _plan_stream_controller: Option<PlanStreamController>,
    _pending_stream_consolidations: usize,
}
impl Data {
    pub(crate) fn capture(
        actual: &ChatWidget,
        shared: &mut native::Capture,
        cells: &mut cells::Capture,
    ) -> io::Result<Self> {
        Ok(Self {
            schema: 1,
            adaptive_chunking: chunking::Data::capture(&actual.adaptive_chunking)?,
            stream_controller: actual
                .stream_controller
                .as_ref()
                .map(|x| controllers::AgentData::capture(x, shared, cells))
                .transpose()?,
            plan_stream_controller: actual
                .plan_stream_controller
                .as_ref()
                .map(|x| controllers::PlanData::capture(x, shared, cells))
                .transpose()?,
            pending_stream_consolidations: actual.pending_stream_consolidations,
        })
    }
    pub(crate) fn prepare(
        self,
        graph: &cells::Restored,
        shared: &mut native::Restored,
        resources: &cells::RestoreContext,
        theme: &ThemeStamp,
        renderer: &Arc<crate::retained_image_renderer::Binding>,
        tui: &crate::tui::Tui,
    ) -> io::Result<Prepared> {
        if self.schema != 1 || !renderer.belongs_to(tui) {
            return Err(invalid("streaming scene schema or actual renderer changed"));
        }
        graph.require_context(resources)?;
        resources.require_image_renderer(renderer)?;
        Ok(Prepared {
            context: graph.context_identity(),
            renderer: renderer.clone(),
            adaptive_chunking: self.adaptive_chunking.restore()?,
            stream_controller: self
                .stream_controller
                .map(|x| x.restore_bound(shared, resources, theme, renderer))
                .transpose()?,
            plan_stream_controller: self
                .plan_stream_controller
                .map(|x| x.restore_bound(shared, resources, theme, renderer))
                .transpose()?,
            pending_stream_consolidations: self.pending_stream_consolidations,
        })
    }
}
impl Prepared {
    pub(crate) fn admit(
        self,
        tui: &crate::tui::Tui,
        renderer: &crate::retained_image_renderer::Binding,
        clipboard: &crate::clipboard_copy::worker::alab_retained_clipboard::BoundClipboard,
        resources: &cells::RestoreContext,
    ) -> Result<Admitted, Box<Refused>> {
        if let Err(error) = self.validate(tui, renderer, clipboard, resources) {
            return Err(Box::new(Refused {
                prepared: self,
                error,
            }));
        }
        Ok(Admitted(self))
    }
    pub(crate) fn validate(
        &self,
        tui: &crate::tui::Tui,
        renderer: &crate::retained_image_renderer::Binding,
        clipboard: &crate::clipboard_copy::worker::alab_retained_clipboard::BoundClipboard,
        resources: &cells::RestoreContext,
    ) -> io::Result<()> {
        self.context.require(resources)?;
        if !std::ptr::eq(self.renderer.as_ref(), renderer) || !renderer.belongs_to(tui) {
            return Err(invalid(
                "prepared native streaming belongs to another actual renderer",
            ));
        }
        resources.require_image_renderer(renderer)?;
        clipboard.validate_tui(tui)
    }
    /// Caller performs this inside the SAME BoundClipboard.install_graph closure
    /// with all other actual App/session/widget state, before any original drain.
    pub(crate) fn install(
        self,
        actual: &mut ChatWidget,
        tui: &crate::tui::Tui,
        renderer: &crate::retained_image_renderer::Binding,
        clipboard: &crate::clipboard_copy::worker::alab_retained_clipboard::BoundClipboard,
        resources: &cells::RestoreContext,
    ) -> Result<Displaced, Box<Refused>> {
        Ok(self
            .admit(tui, renderer, clipboard, resources)?
            .install(actual))
    }
}
impl Admitted {
    pub(crate) fn validate(
        &self,
        tui: &crate::tui::Tui,
        renderer: &crate::retained_image_renderer::Binding,
        clipboard: &crate::clipboard_copy::worker::alab_retained_clipboard::BoundClipboard,
        resources: &cells::RestoreContext,
    ) -> io::Result<()> {
        self.0.validate(tui, renderer, clipboard, resources)
    }
    /// The complete enclosing graph applies all admitted sections synchronously.
    pub(crate) fn install(self, actual: &mut ChatWidget) -> Displaced {
        let Self(prepared) = self;
        Displaced {
            _adaptive_chunking: std::mem::replace(
                &mut actual.adaptive_chunking,
                prepared.adaptive_chunking,
            ),
            _stream_controller: std::mem::replace(
                &mut actual.stream_controller,
                prepared.stream_controller,
            ),
            _plan_stream_controller: std::mem::replace(
                &mut actual.plan_stream_controller,
                prepared.plan_stream_controller,
            ),
            _pending_stream_consolidations: std::mem::replace(
                &mut actual.pending_stream_consolidations,
                prepared.pending_stream_consolidations,
            ),
        }
    }
}
