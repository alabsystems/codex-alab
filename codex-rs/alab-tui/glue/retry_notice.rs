// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Thread metadata does not prove a provider request has resumed. Keep its live
//! retry notice until the existing progress or terminal notification path restores it.

use super::ChatWidget;
use codex_alab_tui::interface;
use codex_app_server_protocol::ServerNotification;

impl ChatWidget {
    pub(in crate::chatwidget) fn alab_preserve_retry_notice(
        notification: &ServerNotification,
    ) -> bool {
        interface::active()
            && matches!(
                notification,
                ServerNotification::ThreadNameUpdated(_)
                    | ServerNotification::ThreadTokenUsageUpdated(_)
                    | ServerNotification::ThreadGoalUpdated(_)
                    | ServerNotification::ThreadGoalCleared(_)
                    | ServerNotification::ThreadSettingsUpdated(_)
            )
    }
}

#[cfg(test)]
#[path = "retry_notice_tests.rs"]
mod tests;
