// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! CPU parser cache replay through the original constructor, never a render or RPC.
use super::*;
use crate::streaming::alab_retained::invalid;
use serde::Deserialize;
use serde::Serialize;
use sha2::Digest;
use sha2::Sha256;
use std::io;
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum Data {
    Plain {},
    Highlighted {
        bytes: usize,
        lines: usize,
        theme_revision: u64,
    },
}
pub(crate) fn syntax_identity() -> io::Result<[u8; 32]> {
    static IDENTITY: std::sync::OnceLock<[u8; 32]> = std::sync::OnceLock::new();
    if let Some(id) = IDENTITY.get() {
        return Ok(*id);
    }
    fn canonical(x: serde_json::Value) -> serde_json::Value {
        match x {
            serde_json::Value::Object(map) => serde_json::Value::Object(
                map.into_iter()
                    .map(|(k, v)| (k, canonical(v)))
                    .collect::<std::collections::BTreeMap<_, _>>()
                    .into_iter()
                    .collect(),
            ),
            serde_json::Value::Array(xs) => {
                serde_json::Value::Array(xs.into_iter().map(canonical).collect())
            }
            x => x,
        }
    }
    let mut digest = Sha256::new();
    digest.update(b"ALAB native streaming syntax and implementation cohort v1\0");
    // These are the actual compiler inputs, never runtime path/build labels.
    // Alias mapping, replay code or dependency changes invalidate only the
    // optional append cache, even if the serialized grammar database is equal.
    const INPUTS: [(&str, &[u8]); 8] = [
        (
            "highlight",
            include_bytes!("../../tui/src/render/highlight.rs"),
        ),
        (
            "highlighter",
            include_bytes!("../../tui/src/render/highlight_streaming.rs"),
        ),
        (
            "fence",
            include_bytes!("../../tui/src/streaming/code_fence.rs"),
        ),
        (
            "render",
            include_bytes!("../../tui/src/streaming/render.rs"),
        ),
        (
            "controller",
            include_bytes!("../../tui/src/streaming/controller.rs"),
        ),
        ("lock", include_bytes!("../../Cargo.lock")),
        ("replay", include_bytes!("retained_stream_highlighter.rs")),
        (
            "retained-fence",
            include_bytes!("retained_stream_code_fence.rs"),
        ),
    ];
    for (label, bytes) in INPUTS {
        digest.update((label.len() as u64).to_be_bytes());
        digest.update(label.as_bytes());
        digest.update((bytes.len() as u64).to_be_bytes());
        digest.update(bytes);
    }
    let value = canonical(serde_json::to_value(super::super::syntax_set()).map_err(invalid)?);
    let bytes = serde_json::to_vec(&value).map_err(invalid)?;
    digest.update((bytes.len() as u64).to_be_bytes());
    digest.update(bytes);
    let id: [u8; 32] = digest.finalize().into();
    let _ = IDENTITY.set(id);
    Ok(*IDENTITY
        .get()
        .ok_or_else(|| invalid("streaming syntax identity was not retained"))?)
}
impl Data {
    pub(crate) fn capture(
        actual: &StreamingCodeHighlighter,
        code: &str,
        language: &str,
    ) -> io::Result<Self> {
        let StreamingCodeHighlighter { state } = actual;
        let Some(HighlightedCode {
            bytes,
            lines,
            theme_revision,
            syntax,
        }) = state
        else {
            return Ok(Self::Plain {});
        };
        if *bytes != code.len() || *lines != code.lines().count() {
            return Err(invalid(
                "original syntax cache does not cover its committed code",
            ));
        }
        // A stale cache is already destined for the original canonical fallback.
        // For a current one prove that the existing native replay reproduces it.
        if *theme_revision == super::super::syntax_theme_revision() {
            let rebuilt = StreamingCodeHighlighter::new(code, language, *theme_revision)
                .ok_or_else(|| invalid("original highlighted cache cannot replay"))?;
            let Some(rebuilt) = rebuilt.state else {
                return Err(invalid("highlighted syntax cache replay became plain"));
            };
            if rebuilt.bytes != *bytes || rebuilt.lines != *lines || rebuilt.syntax != *syntax {
                return Err(invalid("original syntax cache replay changed parser state"));
            }
        }
        Ok(Self::Highlighted {
            bytes: *bytes,
            lines: *lines,
            theme_revision: *theme_revision,
        })
    }
    /// A None result invalidates only this optional append fast path. Existing
    /// cached output stays untouched until the original next update/reflow.
    pub(crate) fn restore(
        self,
        code: &str,
        language: &str,
        original_revision: u64,
        current_revision: u64,
    ) -> io::Result<Option<StreamingCodeHighlighter>> {
        match self {
            Self::Plain {} => Ok(Some(StreamingCodeHighlighter { state: None })),
            Self::Highlighted {
                bytes,
                lines,
                theme_revision,
            } => {
                if bytes != code.len()
                    || lines != code.lines().count()
                    || theme_revision != original_revision
                {
                    return Err(invalid(
                        "retained highlighted cache boundaries differ from its fence",
                    ));
                }
                let Some(actual) = StreamingCodeHighlighter::new(code, language, current_revision)
                else {
                    return Ok(None);
                };
                let Some(state) = &actual.state else {
                    return Ok(None);
                };
                if state.bytes != bytes
                    || state.lines != lines
                    || state.theme_revision != current_revision
                {
                    return Err(invalid(
                        "native highlighted replay changed its input metadata",
                    ));
                }
                Ok(Some(actual))
            }
        }
    }
}
