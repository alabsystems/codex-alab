// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! One typed native resource table for session aliases. DATA never reconstructs
//! semaphores, permits or active jobs; only SAME original native owners can bind.
use serde::Deserialize;
use serde::Serialize;
use std::io;
use std::sync::Arc;
use std::sync::Weak;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use tokio::sync::Semaphore;
fn invalid(message: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}
const MAX: usize = 16384;
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct GateRef(u32);
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct GenerationRef(u32);
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GateData {
    available: usize,
    closed: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Data {
    schema: u8,
    gates: Vec<GateData>,
    generations: Vec<u64>,
}
/// Weak-only identity pins cannot keep a client, reader, worker, permit or gate
/// owner alive. The enclosing original scene keeps actual resources separately.
pub(crate) struct Capture {
    gates: Vec<Weak<Semaphore>>,
    generations: Vec<Weak<AtomicU64>>,
    data: Data,
}
impl Capture {
    pub(crate) fn new() -> Self {
        Self {
            gates: Vec::new(),
            generations: Vec::new(),
            data: Data {
                schema: 1,
                gates: Vec::new(),
                generations: Vec::new(),
            },
        }
    }
    pub(crate) fn gate(&mut self, actual: &Arc<Semaphore>) -> io::Result<GateRef> {
        let weak = Arc::downgrade(actual);
        if let Some(index) = self
            .gates
            .iter()
            .position(|original| Weak::ptr_eq(original, &weak))
        {
            return Ok(GateRef(u32::try_from(index).map_err(invalid)?));
        }
        if self.gates.len() >= MAX {
            return Err(invalid("native session gate graph capacity"));
        }
        let reference = GateRef(u32::try_from(self.gates.len()).map_err(invalid)?);
        self.data.gates.push(GateData {
            available: actual.available_permits(),
            closed: actual.is_closed(),
        });
        self.gates.push(weak);
        Ok(reference)
    }
    pub(crate) fn generation(&mut self, actual: &Arc<AtomicU64>) -> io::Result<GenerationRef> {
        let weak = Arc::downgrade(actual);
        if let Some(index) = self
            .generations
            .iter()
            .position(|original| Weak::ptr_eq(original, &weak))
        {
            return Ok(GenerationRef(u32::try_from(index).map_err(invalid)?));
        }
        if self.generations.len() >= MAX {
            return Err(invalid("native session generation graph capacity"));
        }
        let reference = GenerationRef(u32::try_from(self.generations.len()).map_err(invalid)?);
        self.data.generations.push(actual.load(Ordering::Acquire));
        self.generations.push(weak);
        Ok(reference)
    }
    pub(crate) fn finish(self) -> (Data, OriginalResources) {
        (
            self.data.clone(),
            OriginalResources {
                gates: self.gates,
                generations: self.generations,
                data: self.data,
            },
        )
    }
}
/// Process-local original resource pins only. No cross-exec issuer exists here.
pub(crate) struct OriginalResources {
    gates: Vec<Weak<Semaphore>>,
    generations: Vec<Weak<AtomicU64>>,
    data: Data,
}
impl OriginalResources {
    pub(crate) fn validate_data(&self, data: &Data) -> io::Result<()> {
        if data.schema != 1
            || data.gates.len() > MAX
            || data.generations.len() > MAX
            || serde_json::to_vec(data).map_err(invalid)?
                != serde_json::to_vec(&self.data).map_err(invalid)?
        {
            return Err(invalid(
                "native session resource DATA differs from original capture",
            ));
        }
        for (weak, saved) in self.gates.iter().zip(&data.gates) {
            let actual = weak
                .upgrade()
                .ok_or_else(|| invalid("original native session gate owner expired"))?;
            if actual.available_permits() != saved.available || actual.is_closed() != saved.closed {
                return Err(invalid("actual native session gate changed since capture"));
            }
        }
        for (weak, saved) in self.generations.iter().zip(&data.generations) {
            if weak
                .upgrade()
                .is_none_or(|actual| actual.load(Ordering::Acquire) != *saved)
            {
                return Err(invalid(
                    "actual native session generation changed or owner expired",
                ));
            }
        }
        Ok(())
    }
    pub(crate) fn gate_for(&self, actual: &Arc<Semaphore>) -> io::Result<GateRef> {
        let weak = Arc::downgrade(actual);
        self.gates
            .iter()
            .position(|original| Weak::ptr_eq(original, &weak))
            .and_then(|index| u32::try_from(index).ok())
            .map(GateRef)
            .ok_or_else(|| invalid("actual native gate is not in original shared resource graph"))
    }
    pub(crate) fn generation_for(&self, actual: &Arc<AtomicU64>) -> io::Result<GenerationRef> {
        let weak = Arc::downgrade(actual);
        self.generations
            .iter()
            .position(|original| Weak::ptr_eq(original, &weak))
            .and_then(|index| u32::try_from(index).ok())
            .map(GenerationRef)
            .ok_or_else(|| {
                invalid("actual native task generation is not in original shared resource graph")
            })
    }
    pub(crate) fn same_gate(&self, reference: GateRef, actual: &Arc<Semaphore>) -> bool {
        self.gates
            .get(reference.0 as usize)
            .is_some_and(|original| Weak::ptr_eq(original, &Arc::downgrade(actual)))
    }
    pub(crate) fn same_generation(
        &self,
        reference: GenerationRef,
        actual: &Arc<AtomicU64>,
    ) -> bool {
        self.generations
            .get(reference.0 as usize)
            .is_some_and(|original| Weak::ptr_eq(original, &Arc::downgrade(actual)))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_gate_aliases_are_shared_but_equal_values_are_distinct() -> io::Result<()> {
        let first = Arc::new(Semaphore::new(1));
        let different = Arc::new(Semaphore::new(1));
        let mut graph = Capture::new();
        let a = graph.gate(&first)?;
        let alias = graph.gate(&first.clone())?;
        let b = graph.gate(&different)?;
        let (data, pins) = graph.finish();
        pins.validate_data(&data)?;
        if a.0 != alias.0
            || a.0 == b.0
            || !pins.same_gate(a, &first)
            || pins.same_gate(a, &different)
        {
            return Err(invalid(
                "actual gate alias graph merged equal values or split original owner",
            ));
        }
        Ok(())
    }
    #[test]
    fn actual_held_permit_and_task_generation_change_invalidate_saved_observation() -> io::Result<()>
    {
        let gate = Arc::new(Semaphore::new(1));
        let generation = Arc::new(AtomicU64::new(7));
        let mut graph = Capture::new();
        let _gate = graph.gate(&gate)?;
        let id = graph.generation(&generation)?;
        let (data, pins) = graph.finish();
        pins.validate_data(&data)?;
        let permit = gate.clone().try_acquire_owned().map_err(invalid)?;
        if pins.validate_data(&data).is_ok() {
            return Err(invalid(
                "actual held permit was replaced by saved count DATA",
            ));
        }
        drop(permit);
        pins.validate_data(&data)?;
        generation.fetch_add(1, Ordering::AcqRel);
        if pins.validate_data(&data).is_ok() || !pins.same_generation(id, &generation) {
            return Err(invalid("actual task generation edit or identity was lost"));
        }
        Ok(())
    }
    #[test]
    fn weak_original_resource_table_cannot_keep_a_gate_or_task_generation_alive() -> io::Result<()>
    {
        let gate = Arc::new(Semaphore::new(1));
        let generation = Arc::new(AtomicU64::new(7));
        let mut graph = Capture::new();
        let _gate = graph.gate(&gate)?;
        let _generation = graph.generation(&generation)?;
        if Arc::strong_count(&gate) != 1 || Arc::strong_count(&generation) != 1 {
            return Err(invalid(
                "metadata graph pinned original native resource strongly",
            ));
        }
        let (data, pins) = graph.finish();
        drop(gate);
        drop(generation);
        if pins.validate_data(&data).is_ok() {
            return Err(invalid(
                "expired original resources were recreated from DATA",
            ));
        }
        Ok(())
    }
}
