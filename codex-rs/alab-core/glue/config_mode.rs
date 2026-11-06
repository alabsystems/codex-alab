// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Resolve the mode from enabled config layers before choosing packaged defaults.
//! Compiled in codex-config to keep its private layer representation out of alab-core.

use crate::ConfigLayerEntry;
#[cfg(test)]
use crate::ConfigLayerSource;
use codex_utils_absolute_path::AbsolutePathBuf;
use std::path::Path;

pub(super) fn resolve(
    layers: &mut [ConfigLayerEntry],
    embedded: bool,
    home: &Path,
) -> std::io::Result<()> {
    if !codex_alab_core::is_alab_binary() {
        return Ok(());
    }
    let enhanced = selected_mode(layers);
    codex_alab_core::configure_enhanced(enhanced);
    if embedded {
        replace_defaults(layers, home)?;
    }
    Ok(())
}

fn selected_mode(layers: &[ConfigLayerEntry]) -> bool {
    layers
        .iter()
        .rev()
        .filter(|layer| !layer.is_disabled())
        .find_map(|layer| {
            layer
                .config
                .get("tui")?
                .get(codex_alab_core::INTERFACE_KEY)?
                .as_bool()
        })
        .unwrap_or(true)
}

fn replace_defaults(layers: &mut [ConfigLayerEntry], home: &Path) -> std::io::Result<()> {
    let raw = codex_alab_core::packaged_defaults(include_str!("../../config/defaults.toml"));
    let config = toml::from_str(raw).map_err(std::io::Error::other)?;
    let name = layers[0].name.clone();
    layers[0] = ConfigLayerEntry::new_with_raw_toml(
        name,
        config,
        raw.to_owned(),
        AbsolutePathBuf::from_absolute_path(home)?,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer(on: bool) -> ConfigLayerEntry {
        ConfigLayerEntry::new(
            ConfigLayerSource::SessionFlags,
            toml::toml! { [tui] alab_interface = on }.into(),
        )
    }

    #[test]
    fn disabled_layers_cannot_select_the_experience() {
        let mut ignored = layer(false);
        ignored.disabled_reason = Some("untrusted project".into());
        assert!(selected_mode(&[layer(true), ignored]));
        assert!(!selected_mode(&[layer(true), layer(false)]));
        assert!(selected_mode(&[layer(false), layer(true)]));
    }

    #[test]
    fn higher_precedence_choice_overrides_packaged_defaults() {
        let mut packaged = layer(true);
        packaged.name = ConfigLayerSource::PackagedDefaults {
            file: AbsolutePathBuf::from_absolute_path("/codex").unwrap(),
        };
        assert!(!selected_mode(&[packaged, layer(false)]));
        assert!(selected_mode(&[]));
    }
}
