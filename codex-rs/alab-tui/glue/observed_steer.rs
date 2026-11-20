// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::ChatWidget;
use crate::app_command::AppCommand;
use crate::app_server_session::ThreadParamsMode;
use codex_protocol::ThreadId;

impl ChatWidget {
    pub(super) fn alab_bind_observed_steer(&self, mut op: AppCommand) -> AppCommand {
        if let AppCommand::UserTurn {
            turn_trigger: "user",
            alab_observed_steer,
            ..
        } = &mut op
        {
            let lifecycle = &self.turn_lifecycle;
            let observed = lifecycle.admitted_turn_id.as_ref().or_else(|| {
                lifecycle
                    .last_turn_id
                    .as_ref()
                    .filter(|_| lifecycle.agent_turn_running)
            });
            *alab_observed_steer = self.thread_id.zip(observed.cloned());
        }
        op
    }
}

impl AppCommand {
    /// A local server can distinguish a normally finished turn from a held finding
    /// when asked to steer the observed turn. A fresh start loses that distinction.
    pub(crate) fn alab_initial_steer(
        &self,
        thread: ThreadId,
        mode: ThreadParamsMode,
        active: Option<&str>,
    ) -> Option<String> {
        self.alab_observed_steer_for(thread, mode)
            .or(active)
            .map(str::to_owned)
    }

    pub(crate) fn alab_observed_steer_for(
        &self,
        thread: ThreadId,
        mode: ThreadParamsMode,
    ) -> Option<&str> {
        let (source, turn) = self.alab_observed_steer(mode)?;
        (source == thread).then_some(turn)
    }

    pub(crate) fn alab_observed_steer(&self, mode: ThreadParamsMode) -> Option<(ThreadId, &str)> {
        match self {
            Self::UserTurn {
                turn_trigger: "user",
                alab_observed_steer: Some((source, turn)),
                alab_safety_retries: 0,
                ..
            } if mode != ThreadParamsMode::Cloud => Some((*source, turn.as_str())),
            _ => None,
        }
    }
}
