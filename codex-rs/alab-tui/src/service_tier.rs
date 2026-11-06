// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Consistent speed labels for service tiers the current model supports.

use ratatui::style::Stylize;
use ratatui::text::Line;

/// Normalize known speed labels after the caller has matched the effective tier
/// to its model catalog. Unknown tiers retain the provider's display name.
pub fn display_name<'a>(id: &str, catalog_name: &'a str) -> &'a str {
    display_name_for(crate::interface::active(), id, catalog_name)
}

fn display_name_for<'a>(active: bool, id: &str, catalog_name: &'a str) -> &'a str {
    if active {
        match id {
            "priority" | "fast" => return "Fast",
            "ultrafast" => return "Ultrafast",
            _ => {}
        }
    }
    catalog_name
}

/// Attach the already-resolved processing label to model-only UX surfaces.
pub fn model_name(model: &str, tier: Option<&str>) -> String {
    match tier.filter(|tier| crate::interface::active() && !tier.trim().is_empty()) {
        Some(tier) => format!("{model} · {tier}"),
        None => model.to_owned(),
    }
}

/// The enhanced startup and cleared-screen header expose the same model label
/// as the status card, with the header's matching reasoning effort.
pub fn header_model(model: &str, reasoning: Option<&str>) -> Option<Line<'static>> {
    if !crate::interface::active() {
        return None;
    }
    let mut spans = vec!["     ".into(), model.to_owned().into()];
    if let Some(reasoning) = reasoning {
        spans.push(format!(" ({reasoning})").dim());
    }
    Some(Line::from(spans))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_speeds_have_distinct_labels_without_relabeling_custom_tiers() {
        for (id, expected) in [
            ("priority", "Fast"),
            ("fast", "Fast"),
            ("ultrafast", "Ultrafast"),
        ] {
            assert_eq!(display_name_for(true, id, "provider spelling"), expected);
            assert_eq!(
                display_name_for(false, id, "provider spelling"),
                "provider spelling"
            );
        }
        assert_eq!(display_name_for(true, "flex", "Flex"), "Flex");
        assert_eq!(display_name_for(true, "custom", "Batch Plus"), "Batch Plus");
    }

    #[test]
    fn vanilla_keeps_catalog_spelling_and_model_without_a_tier_suffix() {
        let _scope = crate::interface::scoped(Box::leak(Box::new(crate::interface::State::new())));
        crate::interface::configure(false);
        assert_eq!(display_name("priority", "priority"), "priority");
        assert_eq!(display_name("ultrafast", "ultrafast"), "ultrafast");
        assert_eq!(model_name("Model", Some("Fast")), "Model");
        assert_eq!(model_name("Model", Some("Ultrafast")), "Model");
        assert!(header_model("Model", Some("high")).is_none());
    }
}
