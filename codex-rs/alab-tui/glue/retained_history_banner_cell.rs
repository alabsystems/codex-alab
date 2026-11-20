// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Retain the native banner and header. A timed card rebuilds its cache from the current host.
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
    #[serde(default)]
    model_label: String,
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
        let model = actual.model.lock().unwrap_or_else(PoisonError::into_inner);
        let cache = if model.dirty {
            None
        } else {
            let cache = actual
                .cache
                .lock()
                .map_err(|_| native::invalid("banner cache capture lock poisoned"))?;
            cache.as_ref().and_then(|(width, state, lines)| {
                // A timed card is derived from this host and local date. Rebuild it on restore.
                let harness = match state {
                    Some(Harness::CruiseUntil(_)) => return None,
                    Some(Harness::On(Mode::Off)) => Some(0),
                    Some(Harness::On(Mode::Cruise)) => Some(1),
                    Some(Harness::On(Mode::Land)) => Some(2),
                    Some(Harness::Unsupervised) => Some(3),
                    None => None,
                };
                Some(CacheData {
                    width: *width,
                    harness,
                    lines: lines.iter().map(native::LineData::capture).collect(),
                })
            })
        };
        Ok(Some(Self {
            model_label: model.label.clone(),
            banner: BannerData::capture(model.resolved_banner.as_ref().unwrap_or(&actual.banner)),
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
            model: std::sync::Arc::new(Mutex::new(BannerModel {
                label: self.model_label,
                ..Default::default()
            })),
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
    fn round_trip(original: &Arc<dyn HistoryCell>) -> io::Result<Arc<dyn HistoryCell>> {
        let mut graph = cells::Capture::default();
        let root = graph.intern(original)?;
        let mut text = native::Capture::default();
        let bytes = graph.encode(&mut text, 1_048_576)?;
        let mut shared = native::Restored::new(text.export()?)?;
        cells::Restored::decode(
            &bytes,
            1_048_576,
            &mut shared,
            &mut RestoreContext::default(),
        )?
        .cell(root)
    }

    #[test]
    fn restored_card_reads_the_current_hosts_landing_time() -> io::Result<()> {
        use codex_alab_tui::session::Autopilot;
        use codex_alab_tui::session::HarnessWord;
        use codex_alab_tui::session::Shared;
        let host = Box::leak(Box::new(Shared::new()));
        let _scope = codex_alab_tui::boxes::scoped(host);
        host.set_autopilot_with_word(Autopilot::Active, HarnessWord::Cruise);
        let now = chrono::Utc::now();
        let earlier = u64::try_from(now.timestamp()).unwrap() + 3_600;
        let later = earlier + 7_200;
        let label = |at| {
            let landing =
                codex_alab_tui::landing::Landing::in_zone(at, &chrono::Local, now).unwrap();
            format!("∞ cruise ⤓{landing}")
        };
        let _clock = codex_alab_tui::fixture_clock::scoped(now);
        let original: Arc<dyn HistoryCell> = Arc::new(BannerCell {
            model: Default::default(),
            banner: Banner::new(
                codex_alab_tui::banner::Style::Codex,
                Depth::Mono,
                Theme::default(),
                "0.11",
            ),
            cache: Mutex::new(None),
            header: Box::new(AgentMessageCell::new(
                vec![Line::from("original header")],
                true,
            )),
        });
        host.set_land_at(Some(earlier));
        assert!(
            original
                .display_lines(80)
                .iter()
                .any(|line| line.to_string().contains(&label(earlier)))
        );
        host.set_land_at(Some(later));
        let restored = round_trip(&original)?;
        let cell = restored.as_any().downcast_ref::<BannerCell>().unwrap();
        assert!(cell.cache.lock().unwrap().is_none());
        assert!(
            cell.transcript_lines(80)
                .iter()
                .any(|line| line.to_string().contains(&label(later)))
        );
        assert_eq!(cell.raw_lines(), original.raw_lines());
        Ok(())
    }

    #[test]
    fn native_banner_cell_retains_unique_header_historical_banner_and_exact_cached_lines()
    -> io::Result<()> {
        let original: Arc<dyn HistoryCell> = Arc::new(BannerCell {
            model: Default::default(),
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
        let restored = round_trip(&original)?;
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
