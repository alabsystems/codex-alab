// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual App callback registry + same original BottomPane view consumer. A
//! saved form cannot manufacture a reply handle or reopen a resolved request.
use super::*;
use std::io;

#[path = "retained_pending_view_identity.rs"]
pub(crate) mod identity;

impl App {
    /// Capture the actual native view together with IDs borrowed from SAME
    /// original callback registry, before any capture RPC or image transition.
    /// The IDs are DATA for protected Source capture, never callback authority.
    pub(crate) fn alab_capture_retained_pending_views(
        &self,
        session: &AppServerSession,
        bytes: usize,
    ) -> io::Result<Vec<identity::CapturedView>> {
        self.alab_capture_retained_pending_view_owners(session, bytes)
            .map(|(views, _, _)| views)
    }
    pub(in crate::app) fn alab_capture_retained_pending_view_owners(
        &self,
        session: &AppServerSession,
        bytes: usize,
    ) -> io::Result<(
        Vec<identity::CapturedView>,
        Vec<std::sync::Arc<crate::app::retained_original_input::Item>>,
        Box<serde_json::value::RawValue>,
    )> {
        self.alab_validate_pending_view_backend(session)?;
        let mut seen = HashSet::new();
        let mut views = Vec::new();
        let mut actual_inputs = Vec::new();
        let (captured, delayed_clock) =
            self.chat_widget.alab_capture_retained_bound_views(bytes)?;
        for actual in captured {
            let (data, items) = actual.into_parts();
            let requests = data.requests(bytes)?;
            let owners = crate::bottom_pane::alab_retained_views::bound::Owners::from_actual(
                &requests,
                &items,
                &session.request_handle(),
            )?;
            let ids = owners.ids();
            if ids.iter().any(|id| !seen.insert(id.clone())) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "one original callback is displayed by multiple native views",
                ));
            }
            views.push(identity::CapturedView::capture(data, ids, bytes)?);
            drop(owners);
            actual_inputs.extend(items);
        }
        Ok((views, actual_inputs, delayed_clock))
    }

    fn alab_validate_pending_view_backend(
        &self,
        actual_session: &AppServerSession,
    ) -> io::Result<()> {
        if self.app_server_backend_id != Some(actual_session.backend_id()) {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "retained pending view belongs to a different actual backend",
            ));
        }
        Ok(())
    }

    pub(in crate::app) fn alab_install_callback_editing(
        &mut self,
        session: &AppServerSession,
        captured: &identity::CapturedView,
        closed: &HashSet<codex_app_server_protocol::RequestId>,
        max_bytes: usize,
        max_generations: usize,
    ) -> io::Result<()> {
        self.alab_validate_pending_view_backend(session)?;
        let Some((data, ids)) =
            captured.editing_after_closed(closed, max_bytes, max_generations)?
        else {
            return Ok(()); // Never apply a closed request's editor to its successor.
        };
        self.chat_widget.alab_install_matching_bound_view(
            data,
            &ids,
            &session.request_handle(),
            max_bytes,
            max_generations,
        )
    }

    pub(crate) fn alab_install_retained_pending_view(
        &mut self,
        actual_session: &AppServerSession,
        index: usize,
        captured: identity::CapturedView,
        max_bytes: usize,
        max_generations: usize,
    ) -> io::Result<()> {
        self.alab_validate_pending_view_backend(actual_session)?;
        let (data, ids) = captured.into_parts(max_bytes)?;
        self.chat_widget.alab_install_bound_view(
            index,
            data,
            &ids,
            &actual_session.request_handle(),
            max_bytes,
            max_generations,
        )
    }
}
