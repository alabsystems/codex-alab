// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Read the native checklist cell rather than reconstructing tasks from prose.

impl super::PlanUpdateCell {
    pub(crate) fn alab_task_plan(&self) -> &[super::PlanItemArg] {
        &self.plan
    }
}
