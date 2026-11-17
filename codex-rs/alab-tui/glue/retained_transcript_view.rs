// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Native transcript presentation over the same whole-App cell graph. The
//! identity leaf does not replace native cells with rendered text. Full view,
//! selection, pinned-layout and search codecs must compose before activation.
use super::*;
#[path = "retained_transcript_identity.rs"]
pub(crate) mod identity;

#[path = "retained_transcript_disclosure.rs"]
pub(in crate::transcript_view) mod disclosures;

#[cfg(test)]
fn restore_test_cells(
    cells: &mut crate::history_cell::alab_retained_cells::Capture,
    max: usize,
) -> std::io::Result<crate::history_cell::alab_retained_cells::Restored> {
    use crate::history_cell::alab_retained_cells::RestoreContext;
    use crate::terminal_hyperlinks::alab_retained_text as native;
    let mut text = native::Capture::default();
    let bytes = cells.encode(&mut text, max)?;
    let mut text = native::Restored::new(text.export()?)?;
    crate::history_cell::alab_retained_cells::Restored::decode(
        &bytes,
        max,
        &mut text,
        &mut RestoreContext::default(),
    )
}

#[path = "retained_transcript_snapshot.rs"]
pub(in crate::transcript_view) mod snapshots;
#[path = "retained_transcript_tables.rs"]
pub(in crate::transcript_view) mod tables;

#[path = "retained_transcript_complete.rs"]
pub(crate) mod complete;

// One App graph facade; upstream transcript implementation modules stay private.
pub(crate) use super::bookmark::alab_retained_bookmark::Data as BookmarkData;
pub(crate) use super::text::alab_retained_layout as layouts;
pub(crate) use tables::Capture as CaptureGraph;
pub(crate) use tables::Data as CapturedTables;
pub(crate) use tables::Restore as RestoreGraph;
