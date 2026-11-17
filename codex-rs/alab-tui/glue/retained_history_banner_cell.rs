// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Exact native banner, cache and unique header; global harness observations remain original code.
use super::*;
use crate::history_cell::alab_retained_cells::CaptureContext;
use crate::history_cell::alab_retained_cells::RestoreContext;
use crate::history_cell::alab_retained_cells::UniqueBody;
use crate::history_cell::alab_retained_cells::{self as cells};
use crate::terminal_hyperlinks::alab_retained_text as native;
use codex_alab_tui::banner::Mode;
use codex_alab_tui::banner::retained::BannerData;
use serde::Deserialize;
use serde::Serialize;
use std::io;
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::history_cell) struct Data {
    banner: BannerData,
    cache: Option<CacheData>,
    header: Box<UniqueBody>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CacheData {
    width: u16,
    harness: Option<u8>,
    lines: Vec<native::LineData>,
}
fn harness(actual: Harness) -> u8 {
    match actual {
        Harness::On(Mode::Off) => 0,
        Harness::On(Mode::Cruise) => 1,
        Harness::On(Mode::Land) => 2,
        Harness::Unsupervised => 3,
    }
}
fn restore_harness(actual: u8) -> io::Result<Harness> {
    Ok(match actual {
        0 => Harness::On(Mode::Off),
        1 => Harness::On(Mode::Cruise),
        2 => Harness::On(Mode::Land),
        3 => Harness::Unsupervised,
        _ => return Err(native::invalid("unknown original cached harness word")),
    })
}
impl Data {
    pub(in crate::history_cell) fn capture(
        actual: &dyn HistoryCell,
        context: &mut CaptureContext<'_>,
        nesting: usize,
    ) -> io::Result<Option<Self>> {
        let Some(actual) = actual.as_any().downcast_ref::<BannerCell>() else {
            return Ok(None);
        };
        let cache = {
            let cache = actual
                .cache
                .lock()
                .map_err(|_| native::invalid("banner cache capture lock poisoned"))?;
            cache.as_ref().map(|(width, state, lines)| CacheData {
                width: *width,
                harness: state.map(harness),
                lines: lines.iter().map(native::LineData::capture).collect(),
            })
        };
        Ok(Some(Self {
            banner: BannerData::capture(&actual.banner),
            cache,
            header: Box::new(UniqueBody::capture(
                actual.header.as_ref(),
                context,
                cells::depth(nesting)?,
            )?),
        }))
    }
    pub(in crate::history_cell) fn restore(
        self,
        shared: &mut native::Restored,
        resources: &mut RestoreContext,
        nesting: usize,
    ) -> io::Result<Box<dyn HistoryCell>> {
        let cache = self
            .cache
            .map(|cache| -> io::Result<_> {
                Ok((
                    cache.width,
                    cache.harness.map(restore_harness).transpose()?,
                    cache
                        .lines
                        .iter()
                        .map(native::LineData::restore)
                        .collect::<io::Result<_>>()?,
                ))
            })
            .transpose()?;
        Ok(Box::new(BannerCell {
            banner: self.banner.restore()?,
            cache: Mutex::new(cache),
            header: self
                .header
                .restore(shared, resources, cells::depth(nesting)?)?,
        }))
    }
}

#[cfg(test)]
mod controls {
    use super::*;
    use crate::history_cell::AgentMessageCell;
    use crate::history_cell::HistoryCell;
    use ratatui::style::Stylize;
    use std::sync::Arc;
    #[test]
    fn native_banner_cell_retains_unique_header_historical_banner_and_exact_cached_lines()
    -> io::Result<()> {
        let original: Arc<dyn HistoryCell> = Arc::new(BannerCell {
            banner: Banner::new(
                codex_alab_tui::banner::Style::Codex,
                Depth::Mono,
                Theme::from_bg(Some((12, 34, 56))),
                "historical",
            )
            .with_claims(codex_alab_tui::banner::claims::CLAIMS),
            cache: Mutex::new(Some((
                80,
                Some(Harness::Unsupervised),
                vec![Line::from("actual cache field").bold()],
            ))),
            header: Box::new(AgentMessageCell::new(
                vec![Line::from("exact original header")],
                true,
            )),
        });
        let mut graph = cells::Capture::default();
        let root = graph.intern(&original)?;
        let mut text = native::Capture::default();
        let bytes = graph.encode(&mut text, 1_048_576)?;
        let mut shared = native::Restored::new(text.export()?)?;
        let restored = cells::Restored::decode(
            &bytes,
            1_048_576,
            &mut shared,
            &mut RestoreContext::default(),
        )?
        .cell(root)?;
        let before = original
            .as_any()
            .downcast_ref::<BannerCell>()
            .ok_or_else(|| native::invalid("actual banner cell missing"))?;
        let actual = restored
            .as_any()
            .downcast_ref::<BannerCell>()
            .ok_or_else(|| native::invalid("native banner type changed"))?;
        assert_eq!(actual.banner, before.banner);
        assert!(actual.header.as_any().is::<AgentMessageCell>());
        assert_eq!(
            *actual
                .cache
                .lock()
                .map_err(|_| native::invalid("restored cache poisoned"))?,
            *before
                .cache
                .lock()
                .map_err(|_| native::invalid("actual cache poisoned"))?
        );
        assert_eq!(actual.header.raw_lines(), before.header.raw_lines());
        assert_eq!(actual.raw_lines(), before.raw_lines());
        Ok(())
    }
}
