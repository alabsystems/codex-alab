// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::Config;
use super::ThreadParamsMode;
use super::config_request_overrides_from_config;
use std::collections::HashMap;

pub(super) fn remote_start_preference(
    params: &mut super::ThreadStartParams,
    config: &Config,
    mode: ThreadParamsMode,
    models: &[codex_protocol::openai_models::ModelPreset],
) {
    if mode == ThreadParamsMode::Remote && config.alab_follows_saved_model() {
        params.model = config.alab_display_model(models, false);
    }
}

pub(super) fn alab_saved_model_fork_overrides(
    config: &Config,
    mode: ThreadParamsMode,
) -> Option<HashMap<String, serde_json::Value>> {
    let mut overrides = config_request_overrides_from_config(config, mode)?;
    if config.alab_inherited_reasoning_effort() {
        overrides.remove("model_reasoning_effort");
    }
    (!overrides.is_empty()).then_some(overrides)
}

#[cfg(test)]
use core_test_support::alab_test_process as process;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_server_session::ResumeModelSettings;
    use crate::app_server_session::thread_fork_params_from_config;
    use crate::app_server_session::thread_resume_params_from_config;
    use crate::app_server_session::thread_start_params_from_config;
    use crate::legacy_core::config::ConfigBuilder;
    use crate::legacy_core::config::ConfigOverrides;
    use crate::legacy_core::config::LoaderOverrides;
    use crate::resume_permissions::ResumePermissions;
    use codex_protocol::ThreadId;

    #[tokio::test]
    async fn lifecycle_requests_distinguish_global_preferences_from_invocation_overrides() {
        if !process::child(concat!(
            module_path!(),
            "::lifecycle_requests_distinguish_global_preferences_from_invocation_overrides"
        )) {
            return;
        }
        codex_alab_core::activate();
        let dir = tempfile::tempdir().expect("codex home");
        std::fs::write(
            dir.path().join("config.toml"),
            "model = 'gpt-6.1-sol'\nmodel_reasoning_effort = 'ultra'\n",
        )
        .expect("global preference");
        let builder = || {
            ConfigBuilder::default()
                .codex_home(dir.path().to_owned())
                .loader_overrides(LoaderOverrides::without_managed_config_for_tests())
                .fallback_cwd(Some(dir.path().to_owned()))
        };
        let mode = ThreadParamsMode::Embedded;
        let id = ThreadId::new();
        let global = builder().build().await.expect("global preference loads");
        let start = thread_start_params_from_config(&global, mode, None, None);
        assert_eq!(start.model, None);
        let mut remote_start =
            thread_start_params_from_config(&global, ThreadParamsMode::Remote, None, None);
        remote_start_preference(&mut remote_start, &global, ThreadParamsMode::Remote, &[]);
        assert_eq!(remote_start.model.as_deref(), Some("gpt-6.1-sol"));
        let fork = thread_fork_params_from_config(global, id, mode, None);
        assert_eq!(fork.model, None);
        assert!(
            fork.config
                .as_ref()
                .is_none_or(|config| !config.contains_key("model_reasoning_effort"))
        );

        let effort = builder()
            .cli_overrides(vec![("model_reasoning_effort".into(), "high".into())])
            .build()
            .await
            .expect("effort override loads");
        let resume = thread_resume_params_from_config(
            effort,
            id,
            mode,
            None,
            ResumeModelSettings::OverrideFromCurrentConfig,
            ResumePermissions::CURRENT_CONFIG,
        );
        assert_eq!(resume.model, None);
        assert_eq!(
            resume.config.unwrap().get("model_reasoning_effort"),
            Some(&serde_json::json!("high"))
        );

        let pin = builder()
            .harness_overrides(ConfigOverrides {
                model: Some("gpt-6.1-sol".into()),
                ..Default::default()
            })
            .build()
            .await
            .expect("explicit model loads");
        let start = thread_start_params_from_config(&pin, mode, None, None);
        assert_eq!(start.model.as_deref(), Some("gpt-6.1-sol"));
        let fork = thread_fork_params_from_config(pin, id, mode, None);
        assert_eq!(fork.model.as_deref(), Some("gpt-6.1-sol"));
    }
}
