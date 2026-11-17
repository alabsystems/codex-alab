// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! UI DATA provenance attached by original cell producers. Nothing here
//! supplies Core/Native authority or declares a projection acknowledged.
use crate::history_cell::HistoryCell;
use codex_app_server_protocol::ThreadItemPayload;
use color_eyre::eyre::Result;
use color_eyre::eyre::bail;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::collections::HashSet;
use std::ops::Range;
use std::sync::Arc;
use std::sync::Mutex;
pub(crate) type SharedProvenance = Arc<Mutex<Provenance>>;
pub(crate) fn new_provenance() -> SharedProvenance {
    Arc::new(Mutex::new(Provenance::default()))
}
#[derive(Clone, Debug)]
pub(crate) struct CellSource {
    pub(crate) thread: Option<codex_protocol::ThreadId>,
    pub(crate) turn: String,
    pub(crate) payload: ThreadItemPayload,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum Source {
    Item { turn: String, item: String },
    TurnEnd { turn: String },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Checkpoint {
    groups: Vec<CheckpointGroup>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CheckpointGroup {
    sources: Vec<Source>,
    payloads: Vec<ThreadItemPayload>,
    start: usize,
    end: usize,
}
struct Group {
    sources: Vec<Source>,
    payloads: Vec<ThreadItemPayload>,
    cells: Vec<Arc<dyn HistoryCell>>,
}
#[derive(Default)]
pub(crate) struct Provenance {
    groups: Vec<Group>,
    faulted: bool,
    revision: u64,
    changed: HashMap<Source, u64>,
}
impl std::fmt::Debug for Provenance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SideHistoryProvenance")
            .field("groups", &self.groups.len())
            .finish()
    }
}
fn locate(cells: &[Arc<dyn HistoryCell>], group: &[Arc<dyn HistoryCell>]) -> Option<Range<usize>> {
    if group.is_empty() {
        return None;
    }
    cells
        .windows(group.len())
        .position(|window| window.iter().zip(group).all(|(a, b)| Arc::ptr_eq(a, b)))
        .map(|start| start..start + group.len())
}
impl Provenance {
    pub(crate) fn note(
        &mut self,
        sources: Vec<Source>,
        payloads: Vec<ThreadItemPayload>,
        cells: &[Arc<dyn HistoryCell>],
    ) {
        if self.record(sources, payloads, cells).is_err() {
            self.faulted = true;
        }
    }
    /// Called after the ORIGINAL producer has created/updated these exact
    /// cells. IDs only identify projected DATA; range comes from actual Arcs.
    pub(crate) fn record(
        &mut self,
        sources: Vec<Source>,
        payloads: Vec<ThreadItemPayload>,
        produced: &[Arc<dyn HistoryCell>],
    ) -> Result<()> {
        if sources.is_empty()
            || sources.iter().any(|source| match source {
                Source::Item { turn, item } => turn.is_empty() || item.is_empty(),
                Source::TurnEnd { turn } => turn.is_empty(),
            })
            || sources.iter().collect::<HashSet<_>>().len() != sources.len()
        {
            bail!("invalid original side cell provenance");
        }
        if payloads.iter().any(|payload| {
            !sources.iter().any(|key| {
                matches!(key,
            Source::Item{item,..} if item == payload.id())
            })
        }) {
            bail!("payload disagrees with original side source");
        }
        let keys = sources.iter().collect::<HashSet<_>>();
        // A grouped tool cell may own several IDs. Never forget its remaining
        // identities because one new item overlaps it.
        if self.groups.iter().any(|group| {
            group.sources.iter().any(|key| keys.contains(key))
                && group.sources.iter().any(|key| !keys.contains(key))
        }) {
            bail!("original grouped side cells require whole-group reconciliation");
        }
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| color_eyre::eyre::eyre!("side producer revision overflow"))?;
        for key in &sources {
            self.changed.insert(key.clone(), self.revision);
        }
        self.groups
            .retain(|group| !group.sources.iter().any(|key| keys.contains(key)));
        self.groups.push(Group {
            sources,
            payloads,
            cells: produced.to_vec(),
        });
        Ok(())
    }
    pub(crate) fn range(
        &self,
        source: &Source,
        cells: &[Arc<dyn HistoryCell>],
    ) -> Result<Option<Range<usize>>> {
        let Some(group) = self
            .groups
            .iter()
            .find(|group| group.sources.contains(source))
        else {
            return Ok(None);
        };
        if group.cells.is_empty() {
            return Ok(Some(0..0));
        }
        Ok(Some(locate(cells, &group.cells).ok_or_else(|| {
            color_eyre::eyre::eyre!("original side cell range changed without provenance")
        })?))
    }
    /// Locate only whole original producer groups. Replacing one member of a
    /// shared tool cell would discard another real item, so require all keys.
    pub(crate) fn replacement(
        &self,
        sources: &[Source],
        cells: &[Arc<dyn HistoryCell>],
    ) -> Result<Option<Range<usize>>> {
        let keys = sources.iter().collect::<HashSet<_>>();
        let groups = self
            .groups
            .iter()
            .filter(|group| group.sources.iter().any(|key| keys.contains(key)))
            .collect::<Vec<_>>();
        if groups.is_empty() {
            return Ok(None);
        }
        if groups
            .iter()
            .any(|group| group.sources.iter().any(|key| !keys.contains(key)))
        {
            bail!("fresh side snapshot only covers part of an original cell group");
        }
        let ranges = groups
            .iter()
            .filter(|group| !group.cells.is_empty())
            .map(|group| {
                locate(cells, &group.cells)
                    .ok_or_else(|| color_eyre::eyre::eyre!("original side cell range changed"))
            })
            .collect::<Result<Vec<_>>>()?;
        let Some(start) = ranges.iter().map(|range| range.start).min() else {
            return Ok(Some(0..0));
        };
        let end = ranges.iter().map(|range| range.end).max().unwrap_or(start);
        if (start..end).any(|index| !ranges.iter().any(|range| range.contains(&index))) {
            bail!("fresh side projection would replace an unrelated original cell");
        }
        Ok(Some(start..end))
    }
    pub(crate) fn identical(
        &self,
        sources: &[Source],
        payloads: &[ThreadItemPayload],
        cells: &[Arc<dyn HistoryCell>],
    ) -> Result<bool> {
        let Some(group) = self.groups.iter().find(|group| group.sources == sources) else {
            return Ok(false);
        };
        if !group.cells.is_empty() && locate(cells, &group.cells).is_none() {
            bail!("side cell identity changed");
        }
        Ok(group.payloads == payloads)
    }
    pub(crate) fn payloads(&self, source: &Source) -> Option<&[ThreadItemPayload]> {
        self.groups
            .iter()
            .find(|group| group.sources.contains(source))
            .map(|group| group.payloads.as_slice())
    }
    /// Exact original splices retire only the removed real cell identities.
    /// A partially removed grouped cell is retained as unproven, never blessed.
    pub(crate) fn removed(&mut self, removed: &[Arc<dyn HistoryCell>]) {
        let keys = self
            .groups
            .iter()
            .filter(|group| {
                !group.cells.is_empty()
                    && group
                        .cells
                        .iter()
                        .all(|cell| removed.iter().any(|old| Arc::ptr_eq(old, cell)))
            })
            .flat_map(|group| group.sources.iter().cloned())
            .collect::<Vec<_>>();
        if !keys.is_empty() {
            let Some(next) = self.revision.checked_add(1) else {
                self.faulted = true;
                return;
            };
            self.revision = next;
            for key in &keys {
                self.changed.insert(key.clone(), next);
            }
        }
        self.groups
            .retain(|group| !group.sources.iter().any(|key| keys.contains(key)));
    }
    pub(crate) fn revision(&self) -> Result<u64> {
        if self.faulted {
            bail!("side producer provenance is unknown");
        }
        Ok(self.revision)
    }
    /// Actual producer mutations since capture; merely buffered events never
    /// enter this set. Tombstones preserve an original later removal as well.
    pub(crate) fn changed_since(&self, revision: u64) -> Result<HashSet<Source>> {
        if self.faulted || revision > self.revision {
            bail!("side producer revision is unknown");
        }
        Ok(self
            .changed
            .iter()
            .filter(|(_, seen)| **seen > revision)
            .map(|(source, _)| source.clone())
            .collect())
    }
    /// A transfer checkpoint requires every actual non-header cell to have
    /// producer provenance. Missing/expired cells are NOT completion.
    pub(crate) fn checkpoint(
        &self,
        cells: &[Arc<dyn HistoryCell>],
        prefix: usize,
    ) -> Result<Checkpoint> {
        if self.faulted || prefix > cells.len() {
            bail!("side provenance is unknown");
        }
        let mut covered = vec![false; cells.len()];
        let mut groups = Vec::new();
        for group in &self.groups {
            if group.cells.is_empty() {
                continue;
            }
            let range = locate(cells, &group.cells).ok_or_else(|| {
                color_eyre::eyre::eyre!("side provenance no longer names actual cells")
            })?;
            for slot in &mut covered[range.clone()] {
                *slot = true;
            }
            groups.push(CheckpointGroup {
                sources: group.sources.clone(),
                payloads: group.payloads.clone(),
                start: range.start,
                end: range.end,
            });
        }
        if covered.iter().skip(prefix).any(|covered| !covered) {
            bail!("side capture has unproven legacy cells");
        }
        Ok(Checkpoint { groups })
    }
    /// Decoding transfer DATA does not manufacture producer provenance. The
    /// original restore/replay producer must rebind its real resulting Arcs.
    pub(crate) fn validate_transfer(
        checkpoint: &Checkpoint,
        cells: &[Arc<dyn HistoryCell>],
    ) -> Result<()> {
        let mut seen = HashSet::new();
        for group in &checkpoint.groups {
            if group.start > group.end
                || group.end > cells.len()
                || group.sources.is_empty()
                || group.sources.iter().any(|key| !seen.insert(key))
                || group.payloads.iter().any(|payload| {
                    !group
                        .sources
                        .iter()
                        .any(|key| matches!(key,Source::Item{item,..} if item==payload.id()))
                })
            {
                bail!("invalid transferred side provenance range");
            }
        }
        Ok(())
    }
}
