// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Compiled inside the native goal runtime. ALab's automatic plain input uses the
//! same mutation boundary as the goal runtime's own idle continuation.

impl super::GoalRuntimeHandle {
    /// Keep the last goal read and automatic native admission ordered with external
    /// set/clear. Native turn-start accounting uses its separate accounting permit.
    pub async fn alab_automatic_input_permit(
        &self,
    ) -> Result<tokio::sync::SemaphorePermit<'_>, String> {
        self.goal_state_permit().await
    }
}
