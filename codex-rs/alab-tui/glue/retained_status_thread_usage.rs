// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Independent original estimate and label-reservation Arc identities, not billing authority.
use super::*;
use crate::terminal_hyperlinks::alab_retained_text as native;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::io;
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::status) struct Data {
    estimate: u32,
    label: u32,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::status) struct Graph {
    estimates: Vec<Option<ThreadUsage>>,
    labels: Vec<bool>,
}
#[derive(Default)]
pub(in crate::status) struct Capture {
    estimates: Vec<Arc<RwLock<Option<ThreadUsage>>>>,
    estimate_ids: HashMap<usize, u32>,
    labels: Vec<Arc<AtomicBool>>,
    label_ids: HashMap<usize, u32>,
}
fn intern<T>(
    actual: &Arc<T>,
    held: &mut Vec<Arc<T>>,
    ids: &mut HashMap<usize, u32>,
) -> io::Result<u32> {
    let pointer = Arc::as_ptr(actual) as usize;
    if let Some(id) = ids.get(&pointer) {
        return Ok(*id);
    }
    if held.len() >= native::MAX_NODES {
        return Err(native::invalid("status usage alias capacity"));
    }
    let id = u32::try_from(held.len()).map_err(native::invalid)?;
    held.push(actual.clone());
    ids.insert(pointer, id);
    Ok(id)
}
impl Capture {
    pub(in crate::status) fn reference(&mut self, actual: &StatusThreadUsage) -> io::Result<Data> {
        Ok(Data {
            estimate: intern(
                &actual.estimate,
                &mut self.estimates,
                &mut self.estimate_ids,
            )?,
            label: intern(
                &actual.reserve_label_width,
                &mut self.labels,
                &mut self.label_ids,
            )?,
        })
    }
    pub(in crate::status) fn export(&self) -> io::Result<Graph> {
        Ok(Graph {
            estimates: self
                .estimates
                .iter()
                .map(|actual| {
                    actual
                        .read()
                        .map(|value| value.clone())
                        .map_err(|_| native::invalid("status usage capture lock poisoned"))
                })
                .collect::<io::Result<_>>()?,
            labels: self
                .labels
                .iter()
                .map(|actual| actual.load(Ordering::Relaxed))
                .collect(),
        })
    }
}
#[derive(Default)]
pub(in crate::status) struct Restored {
    estimates: Vec<Arc<RwLock<Option<ThreadUsage>>>>,
    labels: Vec<Arc<AtomicBool>>,
    installed: bool,
}
impl Restored {
    pub(in crate::status) fn install(&mut self, graph: Graph) -> io::Result<()> {
        if self.installed
            || graph.estimates.len() > native::MAX_NODES
            || graph.labels.len() > native::MAX_NODES
        {
            return Err(native::invalid(
                "status usage graph capacity/already installed",
            ));
        }
        self.installed = true;
        self.estimates = graph
            .estimates
            .into_iter()
            .map(|value| Arc::new(RwLock::new(value)))
            .collect();
        self.labels = graph
            .labels
            .into_iter()
            .map(|value| Arc::new(AtomicBool::new(value)))
            .collect();
        Ok(())
    }
    pub(in crate::status) fn usage(&self, data: &Data) -> io::Result<StatusThreadUsage> {
        Ok(StatusThreadUsage {
            estimate: self
                .estimates
                .get(data.estimate as usize)
                .cloned()
                .ok_or_else(|| native::invalid("missing original usage estimate alias"))?,
            reserve_label_width: self
                .labels
                .get(data.label as usize)
                .cloned()
                .ok_or_else(|| native::invalid("missing original usage label alias"))?,
        })
    }
}
#[cfg(test)]
mod controls {
    use super::*;
    #[test]
    fn actual_usage_clones_share_both_original_arcs_and_equal_distinct_values_stay_independent()
    -> io::Result<()> {
        let actual = StatusThreadUsage::default();
        let clone = actual.clone();
        let distinct = StatusThreadUsage::default();
        let mut capture = Capture::default();
        let a = capture.reference(&actual)?;
        let b = capture.reference(&clone)?;
        let c = capture.reference(&distinct)?;
        let mut restored = Restored::default();
        restored.install(capture.export()?)?;
        let a = restored.usage(&a)?;
        let b = restored.usage(&b)?;
        let c = restored.usage(&c)?;
        assert!(Arc::ptr_eq(&a.estimate, &b.estimate));
        assert!(Arc::ptr_eq(&a.reserve_label_width, &b.reserve_label_width));
        assert!(!Arc::ptr_eq(&a.estimate, &c.estimate));
        assert!(!Arc::ptr_eq(&a.reserve_label_width, &c.reserve_label_width));
        a.reserve_label_width();
        assert!(b.reserve_label_width.load(Ordering::Relaxed));
        assert!(!c.reserve_label_width.load(Ordering::Relaxed));
        Ok(())
    }
}
