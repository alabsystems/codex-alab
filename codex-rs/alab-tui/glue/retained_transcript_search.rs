// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Original incremental search state over the one native cell/layout graph.
//! Borrow the actual editor until the enclosing whole-App generation encode;
//! never start a separate register graph or rescan to manufacture a match.
use super::*;
use crate::transcript_view::alab_retained_view::identity::AnchorData;
use crate::transcript_view::alab_retained_view::identity::KeyData;
use crate::transcript_view::alab_retained_view::identity::PositionData;
use crate::transcript_view::alab_retained_view::snapshots;
use crate::transcript_view::alab_retained_view::tables;
use crate::transcript_view::text::alab_retained_layout::LayoutRef;
use serde::Deserialize;
use serde::Serialize;
use std::io;

fn invalid(message: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum ModeData {
    Closed,
    Editing,
    Reading,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum DirectionData {
    Older,
    Newer,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum ProgressData {
    Idle {},
    Restart {},
    Scanning {
        anchor: AnchorData,
        direction: DirectionData,
    },
    AwaitingHistory {},
    Found {},
    Exhausted {},
}
impl ProgressData {
    fn capture(actual: Progress, graph: &tables::Capture<'_>) -> io::Result<Self> {
        Ok(match actual {
            Progress::Idle => Self::Idle {},
            Progress::Restart => Self::Restart {},
            Progress::Scanning(cursor) => Self::Scanning {
                anchor: graph.anchor(cursor.anchor)?,
                direction: match cursor.direction {
                    Direction::Older => DirectionData::Older,
                    Direction::Newer => DirectionData::Newer,
                },
            },
            Progress::AwaitingHistory => Self::AwaitingHistory {},
            Progress::Found => Self::Found {},
            Progress::Exhausted => Self::Exhausted {},
        })
    }
    fn restore(self, graph: &tables::Restore<'_>) -> io::Result<Progress> {
        Ok(match self {
            Self::Idle {} => Progress::Idle,
            Self::Restart {} => Progress::Restart,
            Self::Scanning { anchor, direction } => Progress::Scanning(Cursor {
                anchor: anchor.restore(graph.cells)?,
                direction: match direction {
                    DirectionData::Older => Direction::Older,
                    DirectionData::Newer => Direction::Newer,
                },
            }),
            Self::AwaitingHistory {} => Progress::AwaitingHistory,
            Self::Found {} => Progress::Found,
            Self::Exhausted {} => Progress::Exhausted,
        })
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MatchData {
    anchor: AnchorData,
    end: u64,
    layout: LayoutRef,
}
impl MatchData {
    fn capture(actual: &Match, graph: &mut tables::Capture<'_>) -> io::Result<Self> {
        Ok(Self {
            anchor: graph.anchor(actual.anchor)?,
            end: u64::try_from(actual.end).map_err(invalid)?,
            layout: graph.layout(&actual.layout)?,
        })
    }
    fn restore(self, graph: &tables::Restore<'_>) -> io::Result<Match> {
        let anchor = self.anchor.restore(graph.cells)?;
        let end = usize::try_from(self.end).map_err(invalid)?;
        let layout = graph.layouts.layout(self.layout)?;
        if layout.text().get(anchor.offset..end).is_none() {
            return Err(invalid(
                "native search match is outside its original revision",
            ));
        }
        Ok(Match {
            anchor,
            end,
            layout,
        })
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedData {
    query: String,
    #[serde(deserialize_with = "required")]
    current: Option<MatchData>,
    progress: ProgressData,
    #[serde(deserialize_with = "required")]
    page_start: Option<KeyData>,
}
impl SavedData {
    fn capture(actual: &SavedSelectSearch, graph: &mut tables::Capture<'_>) -> io::Result<Self> {
        Ok(Self {
            query: actual.query.clone(),
            current: actual
                .current
                .as_ref()
                .map(|value| MatchData::capture(value, graph))
                .transpose()?,
            progress: ProgressData::capture(actual.progress, graph)?,
            page_start: actual
                .page_start
                .map(|value| graph.key(value))
                .transpose()?,
        })
    }
    fn restore(self, graph: &tables::Restore<'_>) -> io::Result<SavedSelectSearch> {
        Ok(SavedSelectSearch {
            query: self.query,
            current: self.current.map(|value| value.restore(graph)).transpose()?,
            progress: self.progress.restore(graph)?,
            page_start: self
                .page_start
                .map(|value| value.restore(graph.cells))
                .transpose()?,
        })
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::transcript_view) struct LiveKeyData {
    width: u16,
    cacheable: bool,
    revision: u64,
    is_stream_continuation: bool,
    #[serde(deserialize_with = "required")]
    animation_tick: Option<u64>,
}
impl LiveKeyData {
    pub(in crate::transcript_view) fn capture(
        (width, actual): (u16, ActiveCellTranscriptKey),
    ) -> Self {
        Self {
            width,
            cacheable: actual.cacheable,
            revision: actual.revision,
            is_stream_continuation: actual.is_stream_continuation,
            animation_tick: actual.animation_tick,
        }
    }
    pub(in crate::transcript_view) fn restore(self) -> (u16, ActiveCellTranscriptKey) {
        (
            self.width,
            ActiveCellTranscriptKey {
                cacheable: self.cacheable,
                revision: self.revision,
                is_stream_continuation: self.is_stream_continuation,
                animation_tick: self.animation_tick,
            },
        )
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::transcript_view) struct Data<E = TextArea> {
    schema: u8,
    mode: ModeData,
    editor: E,
    folded_query: String,
    saved_position: PositionData,
    #[serde(deserialize_with = "required")]
    saved_snapshot: Option<snapshots::Data>,
    saved_detailed: bool,
    #[serde(deserialize_with = "required")]
    current: Option<MatchData>,
    #[serde(deserialize_with = "required")]
    saved_select: Option<SavedData>,
    progress: ProgressData,
    #[serde(deserialize_with = "required")]
    scanning_layout: Option<(KeyData, LayoutRef)>,
    #[serde(deserialize_with = "required")]
    page_start: Option<KeyData>,
    query_truncated: bool,
    #[serde(deserialize_with = "required")]
    live: Option<LayoutRef>,
    #[serde(deserialize_with = "required")]
    live_key: Option<LiveKeyData>,
}
impl Data {
    pub(in crate::transcript_view) fn capture<'a>(
        actual: &'a Search,
        graph: &mut tables::Capture<'_>,
    ) -> io::Result<Data<&'a TextArea>> {
        // Register every held cell before projecting keys from any search phase.
        let saved_snapshot = actual
            .saved_snapshot
            .as_ref()
            .map(|snapshot| snapshots::Data::capture(snapshot, graph))
            .transpose()?;
        Ok(Data {
            schema: 1,
            mode: match actual.mode {
                SearchMode::Closed => ModeData::Closed,
                SearchMode::Editing => ModeData::Editing,
                SearchMode::Reading => ModeData::Reading,
            },
            editor: &actual.editor,
            folded_query: actual.folded_query.clone(),
            saved_position: graph.position(actual.saved_position)?,
            saved_snapshot,
            saved_detailed: actual.saved_detailed,
            current: actual
                .current
                .as_ref()
                .map(|value| MatchData::capture(value, graph))
                .transpose()?,
            saved_select: actual
                .saved_select
                .as_ref()
                .map(|value| SavedData::capture(value, graph))
                .transpose()?,
            progress: ProgressData::capture(actual.progress, graph)?,
            scanning_layout: actual
                .scanning_layout
                .as_ref()
                .map(|(key, layout)| Ok::<_, io::Error>((graph.key(*key)?, graph.layout(layout)?)))
                .transpose()?,
            page_start: actual
                .page_start
                .map(|value| graph.key(value))
                .transpose()?,
            query_truncated: actual.query_truncated,
            live: actual
                .live
                .as_ref()
                .map(|value| graph.layout(value))
                .transpose()?,
            live_key: actual.live_key.map(LiveKeyData::capture),
        })
    }
    pub(in crate::transcript_view) fn restore(
        self,
        graph: &tables::Restore<'_>,
        actual_keymap: &RuntimeKeymap,
    ) -> io::Result<Search> {
        if self.schema != 1 {
            return Err(invalid("native incremental search schema"));
        }
        let mut editor = self.editor;
        crate::bottom_pane::alab_retained_editors::configure(&mut editor, actual_keymap);
        Ok(Search {
            mode: match self.mode {
                ModeData::Closed => SearchMode::Closed,
                ModeData::Editing => SearchMode::Editing,
                ModeData::Reading => SearchMode::Reading,
            },
            editor,
            folded_query: self.folded_query,
            saved_position: self.saved_position.restore(graph.cells)?,
            saved_snapshot: self
                .saved_snapshot
                .map(|snapshot| snapshot.restore(graph))
                .transpose()?,
            saved_detailed: self.saved_detailed,
            current: self.current.map(|value| value.restore(graph)).transpose()?,
            saved_select: self
                .saved_select
                .map(|value| value.restore(graph))
                .transpose()?,
            progress: self.progress.restore(graph)?,
            scanning_layout: self
                .scanning_layout
                .map(|(key, layout)| {
                    Ok::<_, io::Error>((key.restore(graph.cells)?, graph.layouts.layout(layout)?))
                })
                .transpose()?,
            page_start: self
                .page_start
                .map(|value| value.restore(graph.cells))
                .transpose()?,
            query_truncated: self.query_truncated,
            live: self
                .live
                .map(|value| graph.layouts.layout(value))
                .transpose()?,
            live_key: self.live_key.map(LiveKeyData::restore),
        })
    }
}
fn required<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    decoder: D,
) -> Result<Option<T>, D::Error> {
    Option::<T>::deserialize(decoder)
}

#[cfg(test)]
#[path = "retained_transcript_search_tests.rs"]
mod tests;

/// Enclosing App root prepass: weak cache/view edges may reference another
/// parked view's held revision before that view's own DATA is captured.
pub(in crate::transcript_view) fn register_roots(
    actual: &Search,
    graph: &mut tables::Capture<'_>,
) -> io::Result<()> {
    if let Some(snapshot) = &actual.saved_snapshot {
        graph.register(&snapshot.cells)?;
    }
    Ok(())
}

/// A fully resolved native field plan, holding no editor or substitute editor.
/// Only the same owning DATA supplies its actual TextArea at the final move.
pub(in crate::transcript_view) struct PreparedFields {
    finish: Box<dyn FnOnce(TextArea) -> Search>,
}
struct WithoutEditor<'a>(&'a Data<TextArea>);
impl Serialize for WithoutEditor<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut fields = serializer.serialize_struct("SearchDataWithoutEditor", 15)?;
        fields.serialize_field("schema", &self.0.schema)?;
        fields.serialize_field("mode", &self.0.mode)?;
        fields.serialize_field("editor", &())?;
        fields.serialize_field("folded_query", &self.0.folded_query)?;
        fields.serialize_field("saved_position", &self.0.saved_position)?;
        fields.serialize_field("saved_snapshot", &self.0.saved_snapshot)?;
        fields.serialize_field("saved_detailed", &self.0.saved_detailed)?;
        fields.serialize_field("current", &self.0.current)?;
        fields.serialize_field("saved_select", &self.0.saved_select)?;
        fields.serialize_field("progress", &self.0.progress)?;
        fields.serialize_field("scanning_layout", &self.0.scanning_layout)?;
        fields.serialize_field("page_start", &self.0.page_start)?;
        fields.serialize_field("query_truncated", &self.0.query_truncated)?;
        fields.serialize_field("live", &self.0.live)?;
        fields.serialize_field("live_key", &self.0.live_key)?;
        fields.end()
    }
}
impl Data<TextArea> {
    /// Borrow the SAME editor while all graph/shape checks remain fallible.
    /// The unit in this private pure DTO is not an allocated/default editor and
    /// starts no enclosing editor encode, register graph or native UI factory.
    pub(in crate::transcript_view) fn prepare_fields(
        &self,
        graph: &tables::Restore<'_>,
    ) -> io::Result<PreparedFields> {
        let bytes = serde_json::to_vec(&WithoutEditor(self)).map_err(invalid)?;
        let data: Data<()> = serde_json::from_slice(&bytes).map_err(invalid)?;
        data.resolve_fields(graph)
    }
    pub(in crate::transcript_view) fn finish_fields(
        self,
        fields: PreparedFields,
        actual_keymap: &RuntimeKeymap,
    ) -> Search {
        let mut editor = self.editor;
        crate::bottom_pane::alab_retained_editors::configure(&mut editor, actual_keymap);
        (fields.finish)(editor)
    }
}
impl Data<()> {
    fn resolve_fields(self, graph: &tables::Restore<'_>) -> io::Result<PreparedFields> {
        if self.schema != 1 {
            return Err(invalid("native incremental search schema"));
        }
        let mode = match self.mode {
            ModeData::Closed => SearchMode::Closed,
            ModeData::Editing => SearchMode::Editing,
            ModeData::Reading => SearchMode::Reading,
        };
        let folded_query = self.folded_query;
        let saved_position = self.saved_position.restore(graph.cells)?;
        let saved_snapshot = self.saved_snapshot.map(|x| x.restore(graph)).transpose()?;
        let saved_detailed = self.saved_detailed;
        let current = self.current.map(|x| x.restore(graph)).transpose()?;
        let saved_select = self.saved_select.map(|x| x.restore(graph)).transpose()?;
        let progress = self.progress.restore(graph)?;
        let scanning_layout = self
            .scanning_layout
            .map(|(key, layout)| -> io::Result<_> {
                Ok((key.restore(graph.cells)?, graph.layouts.layout(layout)?))
            })
            .transpose()?;
        let page_start = self
            .page_start
            .map(|x| x.restore(graph.cells))
            .transpose()?;
        let query_truncated = self.query_truncated;
        let live = self.live.map(|x| graph.layouts.layout(x)).transpose()?;
        let live_key = self.live_key.map(LiveKeyData::restore);
        Ok(PreparedFields {
            finish: Box::new(move |editor| Search {
                mode,
                editor,
                folded_query,
                saved_position,
                saved_snapshot,
                saved_detailed,
                current,
                saved_select,
                progress,
                scanning_layout,
                page_start,
                query_truncated,
                live,
                live_key,
            }),
        })
    }
}

#[cfg(test)]
impl Data<TextArea> {
    pub(in crate::transcript_view) fn editor_for_retained_control(&self) -> &TextArea {
        &self.editor
    }
}
