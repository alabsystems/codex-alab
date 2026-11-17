// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Keep the real BottomPane owner private to its original ChatWidget.
use super::*;
impl ChatWidget {
    pub(crate) fn alab_install_matching_retained_pending_view(
        &mut self,
        data: crate::bottom_pane::alab_retained_views::ViewData,
        requests: &[codex_app_server_protocol::ToolRequestUserInputParams],
        owners: &[crate::app::app_server_requests::alab_retained_owners::UserInputOwner<'_>],
        max_bytes: usize,
        max_generations: usize,
    ) -> std::io::Result<()> {
        self.bottom_pane.alab_install_matching_retained_view(
            data,
            requests,
            owners,
            max_bytes,
            max_generations,
        )
    }

    pub(crate) fn alab_capture_retained_pending_views(
        &self,
        max_bytes: usize,
    ) -> std::io::Result<Vec<crate::bottom_pane::alab_retained_views::ViewData>> {
        self.bottom_pane.alab_capture_retained_view_stack(max_bytes)
    }
    pub(crate) fn alab_install_retained_pending_view(
        &mut self,
        index: usize,
        data: crate::bottom_pane::alab_retained_views::ViewData,
        owners: &[crate::app::app_server_requests::alab_retained_owners::UserInputOwner<'_>],
        max_bytes: usize,
        max_generations: usize,
    ) -> std::io::Result<()> {
        self.bottom_pane
            .alab_install_retained_view(index, data, owners, max_bytes, max_generations)
    }
}

impl ChatWidget {
    pub(crate) fn alab_capture_retained_bound_views(
        &self,
        bytes: usize,
    ) -> std::io::Result<(
        Vec<crate::bottom_pane::alab_retained_views::bound::Captured>,
        Box<serde_json::value::RawValue>,
    )> {
        self.bottom_pane.alab_capture_bound_view_stack(bytes)
    }
    pub(crate) fn alab_install_matching_bound_view(
        &mut self,
        data: crate::bottom_pane::alab_retained_views::ViewData,
        ids: &[codex_app_server_protocol::RequestId],
        client: &codex_app_server_client::AppServerRequestHandle,
        bytes: usize,
        generations: usize,
    ) -> std::io::Result<()> {
        self.bottom_pane
            .alab_install_matching_bound_view(data, ids, client, bytes, generations)
    }
}

impl ChatWidget {
    pub(crate) fn alab_install_bound_view(
        &mut self,
        index: usize,
        data: crate::bottom_pane::alab_retained_views::ViewData,
        ids: &[codex_app_server_protocol::RequestId],
        client: &codex_app_server_client::AppServerRequestHandle,
        bytes: usize,
        generations: usize,
    ) -> std::io::Result<()> {
        self.bottom_pane
            .alab_install_bound_view(index, data, ids, client, bytes, generations)
    }
}

impl ChatWidget {
    pub(crate) fn alab_prime_delayed_clock(
        &mut self,
        raw: &serde_json::value::RawValue,
        bytes: usize,
    ) -> std::io::Result<()> {
        crate::bottom_pane::alab_retained_delayed_approval::PreparedDelayed::decode(raw, bytes)?
            .prime_before_original_replay(&mut self.bottom_pane)
    }
}
