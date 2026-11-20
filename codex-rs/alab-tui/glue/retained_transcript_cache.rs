// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Exact historical cache DATA. Renderer epoch admission/rebinding is separate;
//! decode neither clears/reflows entries nor queries a theme, file or terminal.
use super::*;
use crate::history_cell::alab_retained_cells::CellRef;
use crate::transcript_view::alab_retained_view::tables::Capture;
use crate::transcript_view::alab_retained_view::tables::Restore;
use crate::transcript_view::text::alab_retained_layout::LayoutRef;
use serde::Deserialize;
use serde::Serialize;
use std::io;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum ProtocolData {
    Kitty,
    KittyLocalFile,
    Sixel,
}
impl ProtocolData {
    fn capture(value: crate::pets::ImageProtocol) -> Self {
        match value {
            crate::pets::ImageProtocol::Kitty => Self::Kitty,
            crate::pets::ImageProtocol::KittyLocalFile => Self::KittyLocalFile,
            crate::pets::ImageProtocol::Sixel => Self::Sixel,
        }
    }
    fn restore(self) -> crate::pets::ImageProtocol {
        match self {
            Self::Kitty => crate::pets::ImageProtocol::Kitty,
            Self::KittyLocalFile => crate::pets::ImageProtocol::KittyLocalFile,
            Self::Sixel => crate::pets::ImageProtocol::Sixel,
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum ColorLevel {
    TrueColor,
    Ansi256,
    Ansi16,
    Unknown,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RenderData {
    theme: u64,
    #[serde(deserialize_with = "required")]
    foreground: Option<(u8, u8, u8)>,
    #[serde(deserialize_with = "required")]
    background: Option<(u8, u8, u8)>,
    color_level: ColorLevel,
    #[serde(deserialize_with = "required")]
    cell_size: Option<(u32, u32)>,
}
impl RenderData {
    fn capture(value: &RenderState) -> Self {
        use crate::terminal_palette::StdoutColorLevel as L;
        Self {
            theme: value.theme,
            foreground: value.foreground,
            background: value.background,
            cell_size: value.cell_size,
            color_level: match value.color_level {
                L::TrueColor => ColorLevel::TrueColor,
                L::Ansi256 => ColorLevel::Ansi256,
                L::Ansi16 => ColorLevel::Ansi16,
                L::Unknown => ColorLevel::Unknown,
            },
        }
    }
    fn restore(self) -> RenderState {
        use crate::terminal_palette::StdoutColorLevel as L;
        RenderState {
            theme: self.theme,
            foreground: self.foreground,
            background: self.background,
            cell_size: self.cell_size,
            color_level: match self.color_level {
                ColorLevel::TrueColor => L::TrueColor,
                ColorLevel::Ansi256 => L::Ansi256,
                ColorLevel::Ansi16 => L::Ansi16,
                ColorLevel::Unknown => L::Unknown,
            },
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PresentationData {
    summarized: bool,
    separated: bool,
    shares_prompt_background: bool,
    turn_tip_space: bool,
    expanded: bool,
    disclosure: bool,
    detailed: bool,
    #[serde(deserialize_with = "required")]
    image_protocol: Option<ProtocolData>,
}
impl PresentationData {
    fn capture(value: CellPresentation) -> Self {
        Self {
            summarized: value.summarized,
            separated: value.separated,
            shares_prompt_background: value.shares_prompt_background,
            turn_tip_space: value.turn_tip_space,
            expanded: value.expanded,
            disclosure: value.disclosure,
            detailed: value.detailed,
            image_protocol: value.image_protocol.map(ProtocolData::capture),
        }
    }
    fn restore(self) -> CellPresentation {
        CellPresentation {
            summarized: self.summarized,
            separated: self.separated,
            shares_prompt_background: self.shares_prompt_background,
            turn_tip_space: self.turn_tip_space,
            expanded: self.expanded,
            disclosure: self.disclosure,
            detailed: self.detailed,
            image_protocol: self.image_protocol.map(ProtocolData::restore),
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EntryData {
    // None is an observed expired weak source, never a newly synthesized cell.
    #[serde(deserialize_with = "required")]
    source: Option<CellRef>,
    width: u16,
    presentation: PresentationData,
    layout: LayoutRef,
    rendered_frame: u64,
    #[serde(deserialize_with = "required")]
    animation_tick: Option<u64>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::transcript_view) struct Data {
    schema: u8,
    entries: Vec<EntryData>,
    frame: u64,
    #[serde(deserialize_with = "required")]
    render_state: Option<RenderData>,
}
impl Data {
    pub(in crate::transcript_view) fn capture(
        actual: &LayoutCache,
        graph: &mut Capture<'_>,
    ) -> io::Result<Self> {
        let entries = actual
            .entries
            .iter()
            .map(|entry| -> io::Result<_> {
                Ok(EntryData {
                    source: graph.weak(&entry.source)?,
                    width: entry.width,
                    presentation: PresentationData::capture(entry.presentation),
                    layout: graph.layout(&entry.layout)?,
                    rendered_frame: entry.rendered_frame,
                    animation_tick: entry.animation_tick,
                })
            })
            .collect::<io::Result<_>>()?;
        Ok(Self {
            schema: 1,
            entries,
            frame: actual.frame,
            render_state: actual.render_state.as_ref().map(RenderData::capture),
        })
    }
    pub(in crate::transcript_view) fn restore(
        self,
        graph: &Restore<'_>,
    ) -> io::Result<LayoutCache> {
        if self.schema != 1 || self.entries.len() > MAX_CACHED_ENTRIES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "native layout cache schema/capacity",
            ));
        }
        let mut sources = std::collections::HashSet::new();
        let entries: Vec<CachedLayout> = self
            .entries
            .into_iter()
            .map(|entry| -> io::Result<_> {
                if entry
                    .source
                    .is_some_and(|reference| !sources.insert(reference))
                {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "duplicate actual native cache source",
                    ));
                }
                let source: Weak<dyn HistoryCell> = match entry.source {
                    Some(reference) => Arc::downgrade(&graph.cells.cell(reference)?),
                    None => Weak::<crate::history_cell::PlainHistoryCell>::new(),
                };
                Ok(CachedLayout {
                    source,
                    width: entry.width,
                    presentation: entry.presentation.restore(),
                    layout: graph.layouts.layout(entry.layout)?,
                    rendered_frame: entry.rendered_frame,
                    animation_tick: entry.animation_tick,
                })
            })
            .collect::<io::Result<_>>()?;
        if entries.len() > 1 {
            let bytes = entries
                .iter()
                .flat_map(|entry| {
                    std::iter::once(entry.layout.text().len())
                        .chain(entry.layout.images.iter().map(|image| image.pixels.len()))
                })
                .try_fold(0usize, usize::checked_add)
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "native layout cache byte overflow",
                    )
                })?;
            if bytes > MAX_CACHED_LAYOUT_BYTES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "native layout cache byte capacity",
                ));
            }
        }
        Ok(LayoutCache {
            entries,
            frame: self.frame,
            render_state: self.render_state.map(RenderData::restore),
        })
    }
}
fn required<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    decoder: D,
) -> Result<Option<T>, D::Error> {
    Option::<T>::deserialize(decoder)
}

#[cfg(test)]
#[path = "retained_transcript_cache_tests.rs"]
mod tests;

/// Hold historical cache bytes/objects unchanged until the actual renderer is bound.
pub(in crate::transcript_view) struct Prepared {
    native: LayoutCache,
    theme: crate::render::highlight::alab_retained_image_theme::ThemeStamp,
}
impl Data {
    pub(in crate::transcript_view) fn prepare_bound(
        self,
        graph: &Restore<'_>,
        theme: &crate::render::highlight::alab_retained_image_theme::ThemeStamp,
    ) -> io::Result<Prepared> {
        theme.validate_data()?;
        Ok(Prepared {
            native: self.restore(graph)?,
            theme: theme.clone(),
        })
    }
}
impl Prepared {
    pub(in crate::transcript_view) fn install(
        mut self,
        renderer: &crate::retained_image_renderer::Binding,
        tui: &crate::tui::Tui,
    ) -> io::Result<LayoutCache> {
        use crate::render::highlight::alab_retained_image_theme::CacheGeneration;
        renderer.check()?;
        if !renderer.belongs_to(tui) {
            return Err(io::Error::other("native view cache changed actual Tui"));
        }
        if let Some(state) = &mut self.native.render_state {
            let generation = self.theme.cache_generation(state.theme, renderer)?;
            let same_terminal = state.foreground == crate::terminal_palette::default_fg()
                && state.background == crate::terminal_palette::default_bg()
                && state.color_level == crate::terminal_palette::stdout_color_level()
                && state.cell_size == crate::transcript_image::terminal_cell_size();
            match generation {
                CacheGeneration::Equivalent(revision) if same_terminal => state.theme = revision,
                _ => {
                    // Keep exact entries until ORIGINAL begin_frame invalidates
                    // under the genuinely changed actual renderer epoch. A
                    // coincidentally equal process-local counter cannot reuse them.
                    self.native.render_state = None;
                }
            }
        }
        Ok(self.native)
    }
}
