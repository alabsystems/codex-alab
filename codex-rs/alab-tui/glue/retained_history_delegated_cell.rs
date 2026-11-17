// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Exact native attribution and Markdown, without reconstructing a sender or task resource.
use super::*;
use serde::Deserialize;
use serde::Serialize;
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::history_cell) struct Data {
    attribution: String,
    markdown: String,
}
impl Data {
    pub(in crate::history_cell) fn capture(actual: &dyn HistoryCell) -> Option<Self> {
        actual
            .as_any()
            .downcast_ref::<DelegatedMessageCell>()
            .map(|actual| Self {
                attribution: actual.attribution.clone(),
                markdown: actual.markdown.clone(),
            })
    }
    pub(in crate::history_cell) fn restore(self) -> Box<dyn HistoryCell> {
        Box::new(DelegatedMessageCell {
            attribution: self.attribution,
            markdown: self.markdown,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal_hyperlinks::alab_retained_text as native;
    use std::io;
    #[test]
    fn original_delegated_attribution_markdown_and_list_spacing_restore_as_native_type()
    -> io::Result<()> {
        let original = DelegatedMessageCell::new("actual-original-thread", "# Exact heading\n\n- first line\n  continuation\n- second [link](https://example.invalid/original)\n".into());
        let data = Data::capture(&original)
            .ok_or_else(|| native::invalid("actual delegated variant missing"))?;
        let data: Data =
            serde_json::from_slice(&serde_json::to_vec(&data).map_err(native::invalid)?)
                .map_err(native::invalid)?;
        let restored = data.restore();
        let actual = restored
            .as_any()
            .downcast_ref::<DelegatedMessageCell>()
            .ok_or_else(|| native::invalid("delegated native type changed"))?;
        assert_eq!(actual.attribution, original.attribution);
        assert_eq!(actual.markdown, original.markdown);
        for width in [0, 8, 24, 97] {
            assert_eq!(restored.display_lines(width), original.display_lines(width));
            assert_eq!(
                restored.transcript_lines(width),
                original.transcript_lines(width)
            );
        }
        assert_eq!(restored.raw_lines(), original.raw_lines());
        Ok(())
    }
}
