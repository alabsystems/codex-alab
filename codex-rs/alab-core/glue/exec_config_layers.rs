// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Private configuration data for native exec handoff. Managed requirements,
//! service bindings and project trust are always taken from the current host.

use super::*;
use serde::Deserialize;
use serde::Serialize;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    version: u32,
    layers: Vec<Layer>,
    ignore_user_and_project_rules: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Layer {
    source: Source,
    #[serde(with = "toml_document")]
    config: TomlValue,
    disabled_reason: Option<String>,
    raw: Option<(String, AbsolutePathBuf)>,
    hooks_folder: Option<AbsolutePathBuf>,
}

mod toml_document {
    use super::*;

    pub(super) fn serialize<S: serde::Serializer>(
        value: &TomlValue,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let document = toml::to_string(value).map_err(serde::ser::Error::custom)?;
        document.serialize(serializer)
    }

    pub(super) fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<TomlValue, D::Error> {
        let document = String::deserialize(deserializer)?;
        toml::from_str(&document).map_err(serde::de::Error::custom)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Source {
    PackagedDefaults {
        file: AbsolutePathBuf,
    },
    Mdm {
        domain: String,
        key: String,
    },
    System {
        file: AbsolutePathBuf,
    },
    EnterpriseManaged {
        id: String,
        name: String,
    },
    User {
        file: AbsolutePathBuf,
        profile: Option<String>,
    },
    Project {
        dot_codex_folder: AbsolutePathBuf,
    },
    SessionFlags,
    LegacyManagedFile {
        file: AbsolutePathBuf,
    },
    LegacyManagedMdm,
}

impl From<&ConfigLayerSource> for Source {
    fn from(value: &ConfigLayerSource) -> Self {
        match value {
            ConfigLayerSource::PackagedDefaults { file } => {
                Self::PackagedDefaults { file: file.clone() }
            }
            ConfigLayerSource::Mdm { domain, key } => Self::Mdm {
                domain: domain.clone(),
                key: key.clone(),
            },
            ConfigLayerSource::System { file } => Self::System { file: file.clone() },
            ConfigLayerSource::EnterpriseManaged { id, name } => Self::EnterpriseManaged {
                id: id.clone(),
                name: name.clone(),
            },
            ConfigLayerSource::User { file, profile } => Self::User {
                file: file.clone(),
                profile: profile.clone(),
            },
            ConfigLayerSource::Project { dot_codex_folder } => Self::Project {
                dot_codex_folder: dot_codex_folder.clone(),
            },
            ConfigLayerSource::SessionFlags => Self::SessionFlags,
            ConfigLayerSource::LegacyManagedConfigTomlFromFile { file } => {
                Self::LegacyManagedFile { file: file.clone() }
            }
            ConfigLayerSource::LegacyManagedConfigTomlFromMdm => Self::LegacyManagedMdm,
        }
    }
}

impl From<Source> for ConfigLayerSource {
    fn from(value: Source) -> Self {
        match value {
            Source::PackagedDefaults { file } => Self::PackagedDefaults { file },
            Source::Mdm { domain, key } => Self::Mdm { domain, key },
            Source::System { file } => Self::System { file },
            Source::EnterpriseManaged { id, name } => Self::EnterpriseManaged { id, name },
            Source::User { file, profile } => Self::User { file, profile },
            Source::Project { dot_codex_folder } => Self::Project { dot_codex_folder },
            Source::SessionFlags => Self::SessionFlags,
            Source::LegacyManagedFile { file } => Self::LegacyManagedConfigTomlFromFile { file },
            Source::LegacyManagedMdm => Self::LegacyManagedConfigTomlFromMdm,
        }
    }
}

fn managed(source: &ConfigLayerSource) -> bool {
    match source {
        ConfigLayerSource::Mdm { .. }
        | ConfigLayerSource::EnterpriseManaged { .. }
        | ConfigLayerSource::LegacyManagedConfigTomlFromFile { .. }
        | ConfigLayerSource::LegacyManagedConfigTomlFromMdm => true,
        ConfigLayerSource::PackagedDefaults { .. }
        | ConfigLayerSource::System { .. }
        | ConfigLayerSource::User { .. }
        | ConfigLayerSource::Project { .. }
        | ConfigLayerSource::SessionFlags => false,
    }
}

impl ConfigLayerStack {
    /// Capture configuration data, never the authority to apply it.
    pub fn capture_exec_layers(&self) -> std::io::Result<JsonValue> {
        let layers = self
            .layers
            .iter()
            .filter(|layer| !managed(&layer.name))
            .map(|layer| Layer {
                source: Source::from(&layer.name),
                config: layer.config.clone(),
                disabled_reason: layer.disabled_reason.clone(),
                raw: layer
                    .raw_toml
                    .as_ref()
                    .map(|raw| (raw.contents.clone(), raw.base_dir.clone())),
                hooks_folder: layer.hooks_config_folder_override.clone(),
            })
            .collect();
        serde_json::to_value(Snapshot {
            version: 1,
            layers,
            ignore_user_and_project_rules: self.ignore_user_and_project_exec_policy_rules,
        })
        .map_err(|_| std::io::Error::other("configuration snapshot could not be encoded"))
    }

    /// Restore original configuration data under the current host's policy.
    /// A project no longer recognized by the loader cannot lend old trust.
    pub fn restore_exec_layers(value: JsonValue, current: &Self) -> std::io::Result<Self> {
        let snapshot: Snapshot = serde_json::from_value(value)
            .map_err(|_| std::io::Error::other("invalid configuration snapshot"))?;
        if snapshot.version != 1 {
            return Err(std::io::Error::other("unsupported configuration snapshot"));
        }
        let mut layers = Vec::new();
        for saved in snapshot.layers {
            let name = ConfigLayerSource::from(saved.source);
            if managed(&name) {
                return Err(std::io::Error::other(
                    "snapshot cannot supply managed configuration",
                ));
            }
            let live = current.layers.iter().find(|layer| layer.name == name);
            if matches!(name, ConfigLayerSource::Project { .. }) && live.is_none() {
                return Err(std::io::Error::other(
                    "original project configuration is no longer admitted",
                ));
            }
            let disabled_reason = live
                .and_then(|layer| layer.disabled_reason.clone())
                .or(saved.disabled_reason);
            layers.push(ConfigLayerEntry {
                name,
                version: version_for_toml(&saved.config),
                config: saved.config,
                disabled_reason,
                raw_toml: saved
                    .raw
                    .map(|(contents, base_dir)| RawTomlLayer { contents, base_dir }),
                hooks_config_folder_override: saved.hooks_folder,
            });
        }
        layers.extend(
            current
                .layers
                .iter()
                .filter(|layer| managed(&layer.name))
                .cloned(),
        );
        layers.sort_by_key(|layer| layer.name.precedence());
        let mut restored = Self::new(
            layers,
            current.requirements.clone(),
            current.requirements_toml.as_ref().clone(),
        )?;
        restored.cloud_config_binding = current.cloud_config_binding.clone();
        restored.applied_config_server = current.applied_config_server.clone();
        restored.mcp_ema_cancellation = current.mcp_ema_cancellation.clone();
        restored.ignore_user_and_project_exec_policy_rules = snapshot.ignore_user_and_project_rules
            || current.ignore_user_and_project_exec_policy_rules;
        restored.startup_warnings = current.startup_warnings.clone();
        restored.is_projectless = current.is_projectless;
        Ok(restored)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(value: &str) -> AbsolutePathBuf {
        AbsolutePathBuf::from_absolute_path(value).unwrap()
    }
    fn config(value: &str) -> TomlValue {
        toml::from_str(value).unwrap()
    }
    fn stack(layers: Vec<ConfigLayerEntry>) -> ConfigLayerStack {
        ConfigLayerStack::new(
            layers,
            ConfigRequirements::default(),
            ConfigRequirementsToml::default(),
        )
        .unwrap()
    }

    #[test]
    fn handoff_retains_original_data_paths_and_raw_provenance() {
        let source = ConfigLayerSource::User {
            file: path("/original/config.toml"),
            profile: None,
        };
        let entry = ConfigLayerEntry::new_with_raw_toml(
            source.clone(),
            config("model = 'original'"),
            "model = 'original'\n".into(),
            path("/original"),
        )
        .with_hooks_config_folder_override(Some(path("/original/hooks")));
        let original = stack(vec![entry.clone()]);
        let changed = stack(vec![ConfigLayerEntry::new(
            source,
            config("model = 'changed'"),
        )]);
        let restored = ConfigLayerStack::restore_exec_layers(
            original.capture_exec_layers().unwrap(),
            &changed,
        )
        .unwrap();
        assert_eq!(restored.layers, vec![entry]);
    }

    #[test]
    fn handoff_cannot_restore_revoked_or_missing_project_trust() {
        let source = ConfigLayerSource::Project {
            dot_codex_folder: path("/project/.codex"),
        };
        let original = stack(vec![ConfigLayerEntry::new(
            source.clone(),
            config("model = 'original'"),
        )]);
        let revoked = stack(vec![ConfigLayerEntry::new_disabled(
            source,
            config("model = 'changed'"),
            "trust revoked",
        )]);
        let restored = ConfigLayerStack::restore_exec_layers(
            original.capture_exec_layers().unwrap(),
            &revoked,
        )
        .unwrap();
        assert!(restored.layers[0].is_disabled());
        assert!(restored.effective_config().get("model").is_none());
        assert!(
            ConfigLayerStack::restore_exec_layers(
                original.capture_exec_layers().unwrap(),
                &stack(Vec::new())
            )
            .is_err()
        );
    }

    #[test]
    fn handoff_uses_current_managed_layers() {
        let source = ConfigLayerSource::Mdm {
            domain: "fixture".into(),
            key: "config".into(),
        };
        let original = stack(vec![ConfigLayerEntry::new(
            source.clone(),
            config("model = 'old-managed'"),
        )]);
        let current = stack(vec![ConfigLayerEntry::new(
            source,
            config("model = 'current-managed'"),
        )]);
        let restored = ConfigLayerStack::restore_exec_layers(
            original.capture_exec_layers().unwrap(),
            &current,
        )
        .unwrap();
        assert_eq!(restored.layers, current.layers);
    }

    #[test]
    fn handoff_keeps_toml_datetime_distinct_from_text() {
        let original = stack(vec![ConfigLayerEntry::new(
            ConfigLayerSource::SessionFlags,
            config("instant = 2026-11-03T01:02:03Z\ntext = '2026-11-03T01:02:03Z'"),
        )]);
        let restored = ConfigLayerStack::restore_exec_layers(
            original.capture_exec_layers().unwrap(),
            &stack(Vec::new()),
        )
        .unwrap();
        assert!(restored.layers[0].config["instant"].is_datetime());
        assert!(restored.layers[0].config["text"].is_str());
    }
}
