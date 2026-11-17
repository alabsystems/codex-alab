// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Preserve the original already-validated MCP display model, not a fabricated wire result.
use super::*;
use crate::terminal_hyperlinks::alab_retained_text::invalid;
use serde::Deserialize;
use serde::Serialize;
use std::io;
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::history_cell) struct Data {
    content: Vec<Block>,
    is_error: bool,
    has_image: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Block {
    display: Display,
    is_image: bool,
    original_text: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum Display {
    Text { value: String },
    Summary { value: String, borrowed: bool },
    Json { value: String },
}
impl Data {
    pub(in crate::history_cell) fn capture(actual: &McpToolResult) -> Self {
        Self {
            content: actual
                .content
                .iter()
                .map(|block| Block {
                    display: match &block.display {
                        McpContentDisplay::Text(value) => Display::Text {
                            value: value.clone(),
                        },
                        McpContentDisplay::Json(value) => Display::Json {
                            value: value.clone(),
                        },
                        McpContentDisplay::Summary(value) => Display::Summary {
                            value: value.to_string(),
                            borrowed: matches!(value, Cow::Borrowed(_)),
                        },
                    },
                    is_image: block.is_image,
                    original_text: block.original_text.clone(),
                })
                .collect(),
            is_error: actual.is_error,
            has_image: actual.has_image,
        }
    }
    pub(in crate::history_cell) fn restore(self) -> io::Result<McpToolResult> {
        let content = self
            .content
            .into_iter()
            .map(|block| {
                Ok(McpContentBlock {
                    display: match block.display {
                        Display::Text { value } => McpContentDisplay::Text(value),
                        Display::Json { value } => McpContentDisplay::Json(value),
                        Display::Summary {
                            value,
                            borrowed: false,
                        } => McpContentDisplay::Summary(Cow::Owned(value)),
                        Display::Summary {
                            value,
                            borrowed: true,
                        } => McpContentDisplay::Summary(Cow::Borrowed(match value.as_str() {
                            "Returned image" => "Returned image",
                            "<audio content>" => "<audio content>",
                            "<unknown embedded resource>" => "<unknown embedded resource>",
                            _ => return Err(invalid("unknown borrowed native MCP summary")),
                        })),
                    },
                    is_image: block.is_image,
                    original_text: block.original_text,
                })
            })
            .collect::<io::Result<_>>()?;
        Ok(McpToolResult {
            content,
            is_error: self.is_error,
            has_image: self.has_image,
        })
    }
}
