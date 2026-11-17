// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Original native annotation DATA only; no local file access or permission.
use super::*;
#[test]
fn standalone_link_preserves_original_kind_target_and_columns() -> io::Result<()> {
    for destination_kind in [DestinationKind::Web, DestinationKind::TrustedFile] {
        let original = TerminalHyperlink {
            columns: 2..19,
            destination: "file:///saved/nonexistent/image.png".into(),
            destination_kind,
        };
        let data = LinkData::capture(&original);
        let bytes = serde_json::to_vec(&data).map_err(invalid)?;
        let restored: LinkData = serde_json::from_slice(&bytes).map_err(invalid)?;
        assert_eq!(original, restored.restore()?);
    }
    Ok(())
}
#[test]
fn invalid_standalone_columns_refuse_without_retargeting() {
    let data = LinkData {
        columns: Range { start: 3, end: 1 },
        destination: "https://example.test".into(),
        trusted_file: false,
    };
    assert!(data.restore().is_err());
}
