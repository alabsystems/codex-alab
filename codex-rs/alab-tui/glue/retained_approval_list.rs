// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Selection DATA for the original approval list. Actions, headers, keymap and
//! actual event senders stay on its real replayed destination. No callbacks run.
use super::*;
use serde::Deserialize;
use serde::Serialize;
use std::io;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ListData {
    schema: u32,
    count: usize,
    selected: Option<usize>,
    scroll_top: usize,
    last_selected: Option<usize>,
    completion: Option<bool>,
    dismiss_after_child_accept: bool,
}
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "original approval list DATA differs from its actual option graph",
    )
}
impl ListData {
    pub(crate) fn completed(&self) -> bool {
        self.completion.is_some()
    }
    pub(crate) fn capture(view: &ListSelectionView) -> io::Result<Self> {
        Self::check_shape(view)?;
        let data = Self {
            schema: 1,
            count: view.items.len(),
            selected: view.state.selected_idx,
            scroll_top: view.state.scroll_top,
            last_selected: view.last_selected_actual_idx,
            completion: view
                .completion
                .map(|value| matches!(value, ViewCompletion::Accepted)),
            dismiss_after_child_accept: view.dismiss_after_child_accept,
        };
        data.validate_data()?;
        Ok(data)
    }
    fn check_shape(view: &ListSelectionView) -> io::Result<()> {
        // These are the original ApprovalOverlay::build_options list contract,
        // not a generic picker codec or an idle/current-selection restriction.
        if view.is_searchable
            || !view.search_query.is_empty()
            || !view.tabs.is_empty()
            || !view.search_items.is_empty()
            || view.active_tab_idx.is_some()
            || view.filtered_indices != (0..view.items.len()).collect::<Vec<_>>()
        {
            return Err(invalid());
        }
        Ok(())
    }
    pub(crate) fn validate_data(&self) -> io::Result<()> {
        if self.schema != 1
            || self.count > 256
            || self.selected.is_some_and(|v| v >= self.count)
            || self.last_selected.is_some_and(|v| v >= self.count)
            || self.scroll_top > self.count
        {
            return Err(invalid());
        }
        Ok(())
    }
    pub(crate) fn validate_destination(&self, view: &ListSelectionView) -> io::Result<()> {
        self.validate_data()?;
        Self::check_shape(view)?;
        if self.count != view.items.len() {
            return Err(invalid());
        }
        Ok(())
    }
    pub(crate) fn install(self, view: &mut ListSelectionView) -> io::Result<()> {
        self.validate_destination(view)?;
        view.state.selected_idx = self.selected;
        view.state.scroll_top = self.scroll_top;
        view.last_selected_actual_idx = self.last_selected;
        view.completion = self.completion.map(|accepted| {
            if accepted {
                ViewCompletion::Accepted
            } else {
                ViewCompletion::Cancelled
            }
        });
        view.dismiss_after_child_accept = self.dismiss_after_child_accept;
        // Paint caches are derived anew for the actual terminal dimensions.
        Ok(())
    }
}
