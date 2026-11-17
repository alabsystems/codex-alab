// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Identity table for editor register generations, never runtime resource owners.
//! Capture strong roots before resolving weak payload keys. Store the table apart
//! from the body so decoding does not depend on checkpoint field order.

use serde::Deserialize;
use serde::Serialize;
use std::sync::Arc;
use std::sync::Weak;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GenerationId(usize);

/// Pins each distinct generation until the whole checkpoint has been encoded.
pub struct Capture<T> {
    generations: Vec<Arc<T>>,
}

impl<T> Default for Capture<T> {
    fn default() -> Self {
        Self {
            generations: Vec::new(),
        }
    }
}

impl<T> Capture<T> {
    pub fn strong(&mut self, generation: &Arc<T>) -> GenerationId {
        if let Some(index) = self
            .generations
            .iter()
            .position(|entry| Arc::ptr_eq(entry, generation))
        {
            return GenerationId(index);
        }
        let id = GenerationId(self.generations.len());
        self.generations.push(Arc::clone(generation));
        id
    }

    /// Weak keys cannot manufacture a strong root absent from the checkpoint.
    pub fn weak(&self, generation: &Weak<T>) -> Result<Option<GenerationId>, &'static str> {
        if let Some(index) = self
            .generations
            .iter()
            .position(|entry| Weak::ptr_eq(&Arc::downgrade(entry), generation))
        {
            return Ok(Some(GenerationId(index)));
        }
        if generation.upgrade().is_some() {
            return Err("live editor register generation missing from strong roots");
        }
        Ok(None)
    }

    pub fn values(&self) -> impl ExactSizeIterator<Item = &T> {
        self.generations.iter().map(Arc::as_ref)
    }
}

/// Allocates each wire generation once, including equal but distinct values.
/// The enclosing decoder must enforce its byte and node budget first.
pub struct Restore<T> {
    generations: Vec<Arc<T>>,
}

impl<T> Restore<T> {
    /// The table's temporary ownership must not keep an otherwise absent root alive.
    pub fn validate_roots(&self) -> Result<(), &'static str> {
        if self
            .generations
            .iter()
            .any(|generation| Arc::strong_count(generation) == 1)
        {
            return Err("editor generation has no restored strong root");
        }
        Ok(())
    }

    /// Read-only alias census; the table does not grant a worker/resource.
    pub fn weak_values(&self) -> impl ExactSizeIterator<Item = Weak<T>> + '_ {
        self.generations.iter().map(Arc::downgrade)
    }

    pub fn new(values: Vec<T>) -> Self {
        Self {
            generations: values.into_iter().map(Arc::new).collect(),
        }
    }

    pub fn strong(&self, id: GenerationId) -> Result<Arc<T>, &'static str> {
        self.generations
            .get(id.0)
            .map(Arc::clone)
            .ok_or("unknown editor register generation")
    }

    pub fn weak(&self, id: GenerationId) -> Result<Weak<T>, &'static str> {
        self.generations
            .get(id.0)
            .map(Arc::downgrade)
            .ok_or("unknown editor register generation")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rich_payload_keys_keep_identity_after_checkpoint_round_trip() {
        let first = Arc::new(vec![String::from("same element")]);
        let second = Arc::new(vec![String::from("same element")]);
        let mut capture = Capture::default();
        let first_id = capture.strong(&first);
        let second_id = capture.strong(&second);
        assert_eq!(capture.strong(&first), first_id);
        assert_ne!(first_id, second_id);
        let wire = serde_json::to_vec(&serde_json::json!({
            "payload": capture.weak(&Arc::downgrade(&first)).unwrap().unwrap(),
            "generations": capture.values().collect::<Vec<_>>(),
            "live": second_id,
            "yank": first_id,
        }))
        .unwrap();
        #[derive(Deserialize)]
        struct Checkpoint {
            payload: GenerationId,
            generations: Vec<Vec<String>>,
            live: GenerationId,
            yank: GenerationId,
        }
        let checkpoint: Checkpoint = serde_json::from_slice(&wire).unwrap();
        let restore = Restore::new(checkpoint.generations);
        let live = restore.strong(checkpoint.live).unwrap();
        let yank = restore.strong(checkpoint.yank).unwrap();
        let payload = restore.weak(checkpoint.payload).unwrap();
        assert!(Weak::ptr_eq(&payload, &Arc::downgrade(&yank)));
        assert!(!Weak::ptr_eq(&payload, &Arc::downgrade(&live)));
        drop(restore);
        assert!(payload.upgrade().is_some());
        drop(yank);
        assert!(payload.upgrade().is_none());
    }

    #[test]
    fn missing_roots_and_unknown_references_are_not_restored() {
        let external = Arc::new(String::from("outside checkpoint"));
        let capture = Capture::<String>::default();
        let dead = Arc::downgrade(&external);
        assert!(capture.weak(&dead).is_err());
        drop(external);
        assert!(capture.weak(&dead).unwrap().is_none());
        let restore = Restore::<String>::new(Vec::new());
        let invalid: GenerationId = serde_json::from_str("42").unwrap();
        assert!(restore.strong(invalid).is_err());
        assert!(restore.weak(invalid).is_err());
    }
}
