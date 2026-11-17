// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Preserve original chosen tips and exposure; capture never chooses or acknowledges a new tip.
use super::*;
use crate::history_cell::alab_retained_cells as cells;
use crate::streaming::alab_retained::invalid;
use crate::streaming::alab_retained::required;
use codex_alab_tui::upgrade_editor_clock::Snapshot;
use serde::Deserialize;
use serde::Serialize;
use std::io;
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum PhaseData {
    Working {},
    WaitingForHistory {},
    Complete {
        #[serde(deserialize_with = "required")]
        tail: Option<cells::CellRef>,
    },
    Finished {},
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Current {
    thread_id: ThreadId,
    turn_id: String,
    started_at: Snapshot,
    phase: PhaseData,
    has_final_answer: bool,
    #[serde(deserialize_with = "required")]
    template: Option<String>,
    shown: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::app) struct Data {
    starts: usize,
    completions_shown: usize,
    next_completion: usize,
    #[serde(deserialize_with = "required")]
    previous: Option<String>,
    #[serde(deserialize_with = "required")]
    current: Option<Current>,
}
impl Data {
    pub(in crate::app) fn capture(actual: &TurnTips, graph: &cells::Capture) -> io::Result<Self> {
        let TurnTips {
            starts,
            completions_shown,
            next_completion,
            previous,
            current,
        } = actual;
        let current = current
            .as_ref()
            .map(|actual| -> io::Result<Current> {
                let TurnTipState {
                    thread_id,
                    turn_id,
                    started_at,
                    phase,
                    has_final_answer,
                    template,
                    shown,
                } = actual;
                Ok(Current {
                    thread_id: *thread_id,
                    turn_id: turn_id.clone(),
                    started_at: Snapshot::capture(*started_at).map_err(invalid)?,
                    phase: match phase {
                        Phase::Working => PhaseData::Working {},
                        Phase::WaitingForHistory => PhaseData::WaitingForHistory {},
                        Phase::Complete(tail) => PhaseData::Complete {
                            tail: graph.weak_reference(tail)?,
                        },
                        Phase::Finished => PhaseData::Finished {},
                    },
                    has_final_answer: *has_final_answer,
                    template: template.as_deref().map(str::to_owned),
                    shown: *shown,
                })
            })
            .transpose()?;
        Ok(Self {
            starts: *starts,
            completions_shown: *completions_shown,
            next_completion: *next_completion,
            previous: previous.as_deref().map(str::to_owned),
            current,
        })
    }
    pub(in crate::app) fn restore(self, graph: &cells::Restored) -> io::Result<TurnTips> {
        let current = self
            .current
            .map(|x| -> io::Result<TurnTipState> {
                Ok(TurnTipState {
                    thread_id: x.thread_id,
                    turn_id: x.turn_id,
                    started_at: x.started_at.restore().map_err(invalid)?,
                    phase: match x.phase {
                        PhaseData::Working {} => Phase::Working,
                        PhaseData::WaitingForHistory {} => Phase::WaitingForHistory,
                        PhaseData::Complete { tail } => {
                            Phase::Complete(graph.weak_reference(tail)?)
                        }
                        PhaseData::Finished {} => Phase::Finished,
                    },
                    has_final_answer: x.has_final_answer,
                    template: x.template.map(std::borrow::Cow::Owned),
                    shown: x.shown,
                })
            })
            .transpose()?;
        Ok(TurnTips {
            starts: self.starts,
            completions_shown: self.completions_shown,
            next_completion: self.next_completion,
            previous: self.previous.map(std::borrow::Cow::Owned),
            current,
        })
    }
}

#[cfg(test)]
mod controls {
    use super::*;
    use crate::terminal_hyperlinks::alab_retained_text as native;
    #[test]
    fn retained_removed_tip_text_and_completion_exposure_survive_without_reselection_or_acknowledgement()
    -> io::Result<()> {
        let tail: Arc<dyn HistoryCell> =
            Arc::new(crate::history_cell::PlainHistoryCell::new(vec![
                Line::from("actual completion"),
            ]));
        let chosen =
            "A chosen source-build template which is absent from the destination catalogue.";
        let actual = TurnTips {
            starts: 11,
            completions_shown: 1,
            next_completion: 14,
            previous: Some(std::borrow::Cow::Owned("previous removed template".into())),
            current: Some(TurnTipState {
                thread_id: ThreadId::new(),
                turn_id: "same-turn".into(),
                started_at: Instant::now() - Duration::from_secs(43),
                phase: Phase::Complete(Arc::downgrade(&tail)),
                has_final_answer: true,
                template: Some(std::borrow::Cow::Owned(chosen.into())),
                shown: true,
            }),
        };
        let mut cells = cells::Capture::default();
        let tail_ref = cells.intern(&tail)?;
        let data = Data::capture(&actual, &cells)?;
        let mut text = native::Capture::default();
        let bytes = cells.encode(&mut text, 1_048_576)?;
        let mut text = native::Restored::new(text.export()?)?;
        let cells = cells::Restored::decode(
            &bytes,
            1_048_576,
            &mut text,
            &mut cells::RestoreContext::default(),
        )?;
        let mut restored = data.restore(&cells)?;
        let current = restored.current.as_ref().unwrap();
        assert_eq!(current.template.as_deref(), Some(chosen));
        assert!(current.shown);
        assert!(current.started_at.elapsed() >= WORKING_DELAY);
        let Phase::Complete(weak) = &current.phase else {
            panic!("original completion phase lost")
        };
        assert!(Arc::ptr_eq(
            &weak.upgrade().unwrap(),
            &cells.cell(tail_ref)?
        ));
        restored.acknowledge(TipSurface::Completion);
        assert_eq!(restored.completions_shown, 1);
        assert_eq!(restored.next_completion, 14);
        assert_eq!(
            restored.previous.as_deref(),
            Some("previous removed template")
        );
        assert_eq!(
            restored.current.as_ref().unwrap().template.as_deref(),
            Some(chosen)
        );
        Ok(())
    }
}
