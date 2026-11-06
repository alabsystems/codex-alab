// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Read the live native picker state so queued refreshes cannot undo newer input.

impl super::BottomPane {
    pub(crate) fn alab_task_view_state(&self) -> Option<(Option<usize>, Option<String>)> {
        let view = self.active_view()?;
        (view.view_id() == Some("alab-tasks")).then(|| {
            (
                view.selected_index(),
                view.search_query().map(str::to_owned),
            )
        })
    }
}
