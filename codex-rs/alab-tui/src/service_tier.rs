// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Consistent speed labels for service tiers the current model supports.

use crate::model_layout::model_breaks;
use crate::model_layout::painted_width;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use std::ops::Range;

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
pub fn header_model(
    model: &str,
    reasoning: Option<&str>,
    width: usize,
    effort_style: Style,
) -> Vec<Line<'static>> {
    if !crate::interface::active() {
        return Vec::new();
    }
    let mut label = model.to_owned();
    if let Some(reasoning) = reasoning {
        label.push_str(" (");
        label.push_str(reasoning);
        label.push(')');
    }
    let cells = painted_width(&label);
    let split = model_breaks(&label, cells, width);
    let longest = split.as_ref().map_or(cells, |split| split.longest);
    // Give every row the same indent, restoring the usual five spaces as room permits.
    let margin = "     ";
    let indent = &margin[..width.saturating_sub(longest).min(margin.len())];
    let row = |Range { start, end }| {
        let boundary = model.len().clamp(start, end);
        let mut spans = vec![Span::raw(indent)];
        if start == 0 || start < boundary {
            spans.push(Span::raw(label[start..boundary].to_owned()));
        }
        if boundary < end {
            spans.push(Span::styled(label[boundary..end].to_owned(), effort_style));
        }
        Line::from(spans)
    };
    match split {
        Some(split) => split.rows(label.len()).map(row).collect(),
        None => vec![row(0..label.len())],
    }
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
        assert!(header_model("Model", Some("high"), 20, Style::default()).is_empty());
    }
}
