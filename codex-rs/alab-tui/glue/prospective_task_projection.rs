// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The original complete UI projection and exact daybreak launch override.
//! These are prospective NEW-task request DATA, never an original start claim.
use super::*;
impl AppServerSession {
    pub(crate) fn prospective_task_start_params(&self, config: &Config) -> ThreadStartParams {
        let mut params = thread_start_params_from_config(
            config,
            self.thread_params_mode(),
            self.remote_cwd_override(),
            /*session_start_source*/ None,
        );
        let daybreak_launch_override = config
            .config_layer_stack
            .layers_high_to_low()
            .find(|layer| layer.config.get("daybreak").is_some())
            .is_some_and(|layer| {
                matches!(
                    layer.name,
                    codex_config::ConfigLayerSource::SessionFlags
                        | codex_config::ConfigLayerSource::User {
                            profile: Some(_),
                            ..
                        }
                )
            });
        params.daybreak_enabled = daybreak_launch_override.then_some(config.daybreak_enabled);
        params
    }
}
