// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::Config;
use codex_config::ConfigLayerSource;
use codex_config::ConfigLayerStack;
use codex_models_manager::sol_family::declared_successor;
use codex_protocol::openai_models::ModelPreset;

fn is_saved(source: &ConfigLayerSource) -> bool {
    matches!(
        source,
        ConfigLayerSource::PackagedDefaults { .. } | ConfigLayerSource::User { profile: None, .. }
    )
}

fn winning_layer<'a>(
    layers: &'a ConfigLayerStack,
    key: &str,
) -> Option<&'a codex_config::ConfigLayerEntry> {
    layers
        .layers_high_to_low()
        .find(|layer| layer.config.get(key).is_some())
}

pub(super) fn from_layers(
    layers: &ConfigLayerStack,
    typed_model: Option<&str>,
    configured_model: Option<&str>,
) -> Option<String> {
    if typed_model.is_some() || !is_saved(&winning_layer(layers, "model")?.name) {
        return None;
    }
    configured_model.map(str::to_owned)
}

impl Config {
    /// Embedded app-server reads its own global preference. An RPC model field
    /// is an exact selection and must retain that meaning for external clients.
    pub fn alab_request_model(&self) -> Option<String> {
        if self.alab_follows_saved_model() {
            None
        } else {
            self.model.clone()
        }
    }

    /// A regular fork gets the active model from its parent. Keep a launch pin
    /// exact when carrying it into the fork's request.
    pub fn alab_set_fork_source_model(&mut self, model: &str) {
        let follows = self.alab_follows_saved_model();
        self.model = Some(model.to_owned());
        if follows {
            self.alab_saved_model_can_follow.clone_from(&self.model);
        }
    }

    /// The embedded server shares provider configuration with its TUI. A remote
    /// catalog can communicate migration using its own explicit upgrade records.
    pub fn alab_display_model(&self, models: &[ModelPreset], embedded: bool) -> Option<String> {
        let current = self.model.as_deref()?;
        let successor = if !self.alab_follows_saved_model() {
            None
        } else if embedded
            && (self.alab_follows_explicit_api_catalog()
                || (self.model_provider_id == codex_model_provider_info::AMAZON_BEDROCK_PROVIDER_ID
                    && self.model_provider.base_url.is_none()
                    && self.model_catalog.is_none()
                    && self
                        .model_provider
                        .aws
                        .as_ref()
                        .is_some_and(|aws| aws.region.is_none())))
        {
            // Picker rows lack the credential and origin needed to select a
            // successor for an explicit OpenAI API or implicit Mantle route.
            // Thread admission has the serving credential and catalog.
            None
        } else if embedded {
            self.alab_catalog_successor(models, current, self.model_reasoning_effort.as_ref())
        } else {
            declared_successor(models, current, self.model_reasoning_effort.as_ref())
        };
        Some(successor.map_or_else(|| current.to_owned(), |index| models[index].model.clone()))
    }

    pub(crate) fn alab_enable_recovered_model_following(&mut self) -> bool {
        let scoped = winning_layer(&self.config_layer_stack, "model")
            .is_some_and(|layer| !is_saved(&layer.name));
        if !codex_alab_core::active() || self.model.is_none() || scoped {
            return false;
        }
        self.alab_saved_model_can_follow.clone_from(&self.model);
        true
    }

    pub fn alab_has_scoped_effort(&self) -> bool {
        winning_layer(&self.config_layer_stack, "model_reasoning_effort")
            .is_some_and(|layer| !is_saved(&layer.name))
    }

    pub fn alab_inherited_reasoning_effort(&self) -> bool {
        codex_alab_core::active()
            && winning_layer(&self.config_layer_stack, "model_reasoning_effort").is_some_and(
                |layer| {
                    is_saved(&layer.name)
                        && layer
                            .config
                            .get("model_reasoning_effort")
                            .and_then(toml::Value::as_str)
                            == self
                                .model_reasoning_effort
                                .as_ref()
                                .map(ToString::to_string)
                                .as_deref()
                },
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_config::ConfigLayerEntry;
    use codex_utils_absolute_path::AbsolutePathBuf;

    #[test]
    fn profile_project_flags_and_typed_model_pin_even_the_same_slug() {
        let dir = tempfile::tempdir().expect("config directory");
        let file =
            AbsolutePathBuf::try_from(dir.path().join("config.toml")).expect("absolute config");
        let ordinary = ConfigLayerSource::User {
            file: file.clone(),
            profile: None,
        };
        let layer =
            |name| ConfigLayerEntry::new(name, toml::toml! { model = "gpt-6.1-sol" }.into());
        let layers = |sources: Vec<_>| {
            ConfigLayerStack::new(
                sources.into_iter().map(layer).collect(),
                Default::default(),
                Default::default(),
            )
            .expect("ordered layers")
        };
        let slug = Some("gpt-6.1-sol");
        let user = layers(vec![ordinary.clone()]);
        assert_eq!(from_layers(&user, None, slug).as_deref(), slug);
        assert_eq!(from_layers(&user, slug, slug), None);
        for source in [
            ConfigLayerSource::User {
                file: file.clone(),
                profile: Some("coding".into()),
            },
            ConfigLayerSource::Project {
                dot_codex_folder: file,
            },
            ConfigLayerSource::SessionFlags,
        ] {
            assert_eq!(
                from_layers(&layers(vec![ordinary.clone(), source]), None, slug),
                None
            );
        }
    }
}
