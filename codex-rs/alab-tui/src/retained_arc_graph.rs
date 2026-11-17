// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Local identity DATA for exact native UI Arcs, including unsized text/slices.
//! Strong capture roots prevent pointer ABA. Equal values never merge identities.
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::io;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Id(pub usize);

pub struct Capture<T: ?Sized> {
    roots: Vec<Arc<T>>,
    indices: HashMap<usize, Id>,
    limit: usize,
}
impl<T: ?Sized> Capture<T> {
    pub fn new(limit: usize) -> Self {
        Self {
            roots: Vec::new(),
            indices: HashMap::new(),
            limit,
        }
    }
    /// The bool reports first admission, so payload encoding happens once.
    pub fn intern(&mut self, value: &Arc<T>) -> io::Result<(Id, bool)> {
        let pointer = Arc::as_ptr(value).cast::<()>() as usize;
        if let Some(id) = self.indices.get(&pointer) {
            return Ok((*id, false));
        }
        if self.roots.len() >= self.limit {
            return Err(io::Error::other("native Arc graph capacity"));
        }
        let id = Id(self.roots.len());
        self.roots.push(value.clone());
        self.indices.insert(pointer, id);
        Ok((id, true))
    }
    pub fn len(&self) -> usize {
        self.roots.len()
    }
    pub fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }
    pub fn get(&self, id: Id) -> io::Result<Arc<T>> {
        self.roots
            .get(id.0)
            .cloned()
            .ok_or_else(|| io::Error::other("unknown native Arc identity"))
    }
}

pub struct Restored<T: ?Sized> {
    roots: Vec<Arc<T>>,
}
impl<T: ?Sized> Restored<T> {
    /// Caller has already decoded and budgeted each native payload once.
    pub fn new(roots: Vec<Arc<T>>) -> Self {
        Self { roots }
    }
    pub fn get(&self, id: Id) -> io::Result<Arc<T>> {
        self.roots
            .get(id.0)
            .cloned()
            .ok_or_else(|| io::Error::other("unknown native Arc identity"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unsized_equal_values_keep_distinct_aliases_and_capture_pins_roots() -> io::Result<()> {
        let first: Arc<str> = String::from("same").into();
        let other: Arc<str> = String::from("same").into();
        let weak = Arc::downgrade(&first);
        let mut capture = Capture::new(2);
        let (a, fresh) = capture.intern(&first)?;
        assert!(fresh);
        assert_eq!(capture.intern(&first)?, (a, false));
        let (b, _) = capture.intern(&other)?;
        assert_ne!(a, b);
        drop(first);
        assert!(weak.upgrade().is_some());
        let restored = Restored::new(vec![capture.get(a)?, capture.get(b)?]);
        assert!(Arc::ptr_eq(&restored.get(a)?, &restored.get(a)?));
        assert!(!Arc::ptr_eq(&restored.get(a)?, &restored.get(b)?));
        assert!(capture.intern(&Arc::<str>::from("overflow")).is_err());
        assert!(restored.get(Id(99)).is_err());
        Ok(())
    }
}
