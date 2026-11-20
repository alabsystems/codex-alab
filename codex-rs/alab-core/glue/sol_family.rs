// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Follow released models within their family using the serving account's catalog.
use std::collections::HashSet;

use chrono::DateTime;
use chrono::Utc;
use codex_protocol::openai_models::ModelPreset;
use codex_protocol::openai_models::ReasoningEffort;

type Release = [u64; 3];

fn release_version(number: &str) -> Option<Release> {
    let mut version = [0; 3];
    for (index, component) in number.split('.').enumerate() {
        if component.is_empty()
            || !component.bytes().all(|byte| byte.is_ascii_digit())
            || (component.len() > 1 && component.starts_with('0'))
        {
            return None;
        }
        *version.get_mut(index)? = component.parse().ok()?;
    }
    Some(version)
}

/// Versioned OpenAI releases have stable family names. An experimental SKU,
/// alias, date or deployment label must not become the next production model.
fn gpt_family(slug: &str) -> Option<(&str, Release)> {
    let tail = slug.strip_prefix("gpt-")?;
    let (number, family) = match tail.split_once('-') {
        Some((number, family)) => {
            if family.split('-').any(|part| {
                part.is_empty()
                    || !part.bytes().all(|byte| byte.is_ascii_lowercase())
                    || matches!(
                        part,
                        "alpha" | "beta" | "preview" | "latest" | "experimental" | "rc"
                    )
            }) {
                return None;
            }
            (number, family)
        }
        None => (tail, ""),
    };
    Some((family, release_version(number)?))
}

fn eligible(model: &ModelPreset, effort: Option<&ReasoningEffort>, now: DateTime<Utc>) -> bool {
    model.show_in_picker
        && effort.is_none_or(|requested| {
            model
                .supported_reasoning_efforts
                .iter()
                .any(|level| &level.effort == requested)
        })
        && model
            .upgrade
            .as_ref()
            .and_then(|upgrade| upgrade.retirement_at)
            .is_none_or(|retirement| retirement > now)
}

fn newest_in_family(
    models: &[ModelPreset],
    namespace: &str,
    family: &str,
    effort: Option<&ReasoningEffort>,
) -> Option<(usize, Release)> {
    let now = Utc::now();
    // max_by_key picks the last tie; reverse to keep the catalog's priority.
    models
        .iter()
        .enumerate()
        .rev()
        .filter_map(|(index, model)| {
            let (candidate, version) = gpt_family(model.model.strip_prefix(namespace)?)?;
            (candidate == family && eligible(model, effort, now)).then_some((index, version))
        })
        .max_by_key(|(_, version)| *version)
}

pub fn newest_eligible_sol(
    models: &[ModelPreset],
    effort: Option<&ReasoningEffort>,
) -> Option<usize> {
    newest_in_family(models, "", "sol", effort).map(|(index, _)| index)
}

/// Follow the serving provider's explicit upgrade chain for models whose IDs
/// do not have first-party version semantics. Only listed targets from this
/// same catalog can be returned. Cycles cannot confer upgrade authority.
pub fn declared_successor(
    models: &[ModelPreset],
    current: &str,
    effort: Option<&ReasoningEffort>,
) -> Option<usize> {
    let mut index = models.iter().position(|model| model.model == current)?;
    let mut seen = HashSet::from([index]);
    let mut selected = None;
    let now = Utc::now();
    while let Some(edge) = &models[index].upgrade {
        let Some(next) = models.iter().position(|model| model.model == edge.id) else {
            break;
        };
        if !seen.insert(next) {
            return None;
        }
        // A provider may deploy GPT models as-is. A recommendation must never
        // move a versioned GPT selection backwards or into another SKU.
        if let Some((family, version)) = gpt_family(&models[index].model) {
            let (next_family, next_version) = gpt_family(&models[next].model)?;
            if next_family != family || next_version <= version {
                return None;
            }
        }
        index = next;
        // A saved thread can be several releases behind. A retired release
        // still carries the provider's edge to its successor.
        if eligible(&models[next], effort, now) {
            selected = Some(next);
        }
    }
    selected
}

/// Resolve a strictly newer release of the current first-party family. Missing,
/// hidden or retired source rows still carry their identity in the saved slug.
pub fn newest_eligible_successor(
    models: &[ModelPreset],
    current: &str,
    effort: Option<&ReasoningEffort>,
) -> Option<usize> {
    newest_eligible_routed_successor(models, current, "", effort)
}

/// Built-in cloud catalogs can list several routes to the same model release.
/// Compare versions only among entries on the saved route and return its actual row.
pub fn newest_eligible_routed_successor(
    models: &[ModelPreset],
    current: &str,
    namespace: &str,
    effort: Option<&ReasoningEffort>,
) -> Option<usize> {
    let Some((family, current_version)) = gpt_family(current.strip_prefix(namespace)?) else {
        return declared_successor(models, current, effort)
            .filter(|&index| models[index].model.starts_with(namespace));
    };
    let (index, latest_version) = newest_in_family(models, namespace, family, effort)?;
    (latest_version > current_version).then_some(index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_protocol::openai_models::ModelUpgrade;

    fn model(slug: &str) -> ModelPreset {
        let base = crate::bundled_models_response()
            .expect("bundled catalog")
            .models
            .into_iter()
            .find(|model| {
                gpt_family(&model.slug).is_some_and(|(family, _)| family == "sol")
                    && model.visibility == codex_protocol::openai_models::ModelVisibility::List
            })
            .expect("bundled visible Sol");
        let mut preset = ModelPreset::from(base);
        preset.model = slug.into();
        preset.id = slug.into();
        preset.upgrade = None;
        preset
    }

    #[test]
    fn catalog_selects_numeric_sol_releases_and_ignores_other_skus() {
        let models = [
            "gpt-6.9-sol",
            "gpt-6.10.2-sol",
            "gpt-6.10-sol",
            "gpt-22-astra",
            "gpt-23-sol-preview",
            "gpt-24.1-sol-mini",
            "gpt-29-sol-2026-11-17",
        ]
        .map(model);
        assert_eq!(
            models[newest_eligible_sol(&models, Some(&ReasoningEffort::Ultra)).unwrap()].model,
            "gpt-6.10.2-sol"
        );
        assert_eq!(gpt_family("gpt-7-sol"), Some(("sol", [7, 0, 0])));
        for invalid in [
            "gpt-+9-sol",
            "gpt-6..1-sol",
            "gpt-06.1-sol",
            "gpt-6.2.3.4-sol",
            "gpt-6.2-beta-sol",
        ] {
            assert_eq!(gpt_family(invalid), None, "{invalid}");
        }
    }

    #[test]
    fn a_saved_model_only_advances_to_a_newer_eligible_member_of_its_own_family() {
        let mut rows = [
            "gpt-6.2-astra",
            "gpt-7-sol",
            "gpt-6.3-astra",
            "gpt-10-astra-preview",
            "gpt-6.1-terra",
            "gpt-6.2",
        ]
        .map(model);
        rows[2]
            .supported_reasoning_efforts
            .retain(|level| level.effort != ReasoningEffort::Ultra);
        let next =
            |current| newest_eligible_successor(&rows, current, Some(&ReasoningEffort::Ultra));
        assert_eq!(next("gpt-6-astra"), Some(0));
        assert_eq!(next("gpt-6.1-sol"), Some(1));
        assert_eq!(next("gpt-5.5"), Some(5));
        for kept in [
            "gpt-6.2.0-astra",
            "gpt-8-sol",
            "gpt-6.1-terra",
            "gpt-6-luna",
            "gpt-6-astra-preview",
            "provider-deployment",
        ] {
            assert_eq!(next(kept), None, "{kept}");
        }
        assert_eq!(
            newest_eligible_successor(&rows, "gpt-6-astra", None),
            Some(2)
        );
    }

    fn edge(id: &str) -> ModelUpgrade {
        ModelUpgrade {
            id: id.to_owned(),
            migration_config_key: "test".into(),
            retirement_at: None,
            model_link: None,
            upgrade_copy: None,
            migration_markdown: None,
        }
    }

    #[test]
    fn a_provider_declares_successors_for_opaque_models_in_its_own_catalog() {
        let mut rows = ["opaque-blue", "opaque-green", "opaque-gold"].map(model);
        rows[0].upgrade = Some(edge("opaque-green"));
        rows[1].upgrade = Some(edge("opaque-gold"));
        assert_eq!(
            newest_eligible_successor(&rows, "opaque-blue", None),
            Some(2)
        );
        assert_eq!(declared_successor(&rows, "unlisted", None), None);
        rows[2].show_in_picker = false;
        assert_eq!(declared_successor(&rows, "opaque-blue", None), Some(1));
        rows[2].show_in_picker = true;
        rows[2].upgrade = Some(edge("opaque-blue"));
        assert_eq!(declared_successor(&rows, "opaque-blue", None), None);
        rows[0].upgrade = Some(edge("cross-provider-id-absent"));
        assert_eq!(declared_successor(&rows, "opaque-blue", None), None);
    }

    #[test]
    fn saved_model_follows_declared_edges_through_ineligible_releases() {
        let mut rows = [
            "gpt-6-sol",
            "gpt-6.1-sol",
            "gpt-6.2-sol",
            "gpt-6.3-sol",
            "gpt-6.4-sol",
            "gpt-7-astra",
        ]
        .map(model);
        for index in 0..4 {
            rows[index].upgrade = Some(edge(&rows[index + 1].model));
        }
        rows[1].upgrade.as_mut().unwrap().retirement_at = Some(chrono::DateTime::<Utc>::UNIX_EPOCH);
        rows[2].show_in_picker = false;
        rows[3].supported_reasoning_efforts.clear();
        let next = |models: &[ModelPreset]| {
            declared_successor(models, "gpt-6-sol", Some(&ReasoningEffort::High))
        };
        assert_eq!(next(&rows), Some(4));
        assert_eq!(next(&rows[..4]), None);
        rows[3].upgrade = Some(edge("gpt-7-astra"));
        assert_eq!(next(&rows), None);
        rows[3].upgrade = Some(edge("gpt-6.1-sol"));
        assert_eq!(next(&rows), None);
    }

    #[test]
    fn hidden_retired_and_incompatible_models_cannot_win_or_supply_a_fallback() {
        let mut presets = ["gpt-8-sol", "gpt-7.9-sol", "gpt-7.8-sol", "gpt-7.7-sol"].map(model);
        presets[0].show_in_picker = false;
        presets[1]
            .supported_reasoning_efforts
            .retain(|level| level.effort != ReasoningEffort::Ultra);
        presets[2].upgrade = Some(ModelUpgrade {
            id: "gpt-8-sol".into(),
            migration_config_key: "example".into(),
            model_link: None,
            upgrade_copy: None,
            migration_markdown: None,
            retirement_at: Some(chrono::DateTime::<Utc>::UNIX_EPOCH),
        });
        assert_eq!(
            presets[newest_eligible_sol(&presets, Some(&ReasoningEffort::Ultra)).unwrap()].model,
            "gpt-7.7-sol"
        );
        assert!(newest_eligible_sol(&presets[..3], Some(&ReasoningEffort::Ultra)).is_none());
        assert_eq!(
            newest_eligible_sol(&presets, Some(&ReasoningEffort::High)),
            Some(1)
        );
        assert_eq!(newest_eligible_sol(&presets, None), Some(1));
        assert!(newest_eligible_sol(&[], None).is_none());
    }
}
