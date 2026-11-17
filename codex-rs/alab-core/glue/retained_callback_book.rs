// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Bounded live ownership; retired intervals authenticate historical ACK without
//! retaining cells or forgetting/reissuing a native permission (none is granted).
use std::collections::BTreeMap;
#[derive(Default)]
pub(super) struct Retired {
    floor: u64,
    ranges: BTreeMap<u64, u64>,
}
impl Retired {
    pub(super) fn contains(&self, id: u64) -> bool {
        id > 0
            && (id <= self.floor
                || self
                    .ranges
                    .range(..=id)
                    .next_back()
                    .is_some_and(|(_, end)| id <= *end))
    }
    pub(super) fn insert(&mut self, id: u64) {
        if id <= self.floor || self.contains(id) {
            return;
        }
        let (mut first, mut last) = (id, id);
        if let Some((&a, &b)) = self.ranges.range(..=id).next_back() {
            if b.checked_add(1) == Some(id) {
                first = a;
                self.ranges.remove(&a);
            }
        }
        if let Some((&a, &b)) = self.ranges.range(id..).next() {
            if id.checked_add(1) == Some(a) {
                last = b;
                self.ranges.remove(&a);
            }
        }
        if self.floor.checked_add(1) == Some(first) {
            self.floor = last;
            while let Some((&a, &b)) = self.ranges.first_key_value() {
                if self.floor.checked_add(1) != Some(a) {
                    break;
                }
                self.floor = b;
                self.ranges.remove(&a);
            }
        } else {
            self.ranges.insert(first, last);
        }
    }
}
pub(super) struct Book<T> {
    pub next: u64,
    pub active: BTreeMap<u64, T>,
    pub operations: BTreeMap<String, u64>,
    pub retired: Retired,
    changed: tokio::sync::watch::Sender<()>,
}
impl<T> Default for Book<T> {
    fn default() -> Self {
        Self {
            next: 0,
            active: BTreeMap::new(),
            operations: BTreeMap::new(),
            retired: Retired::default(),
            changed: tokio::sync::watch::channel(()).0,
        }
    }
}
impl<T> Book<T> {
    pub(super) fn notify_change(&self) {
        self.changed.send_replace(());
    }
    pub(super) fn subscribe(&self) -> tokio::sync::watch::Receiver<()> {
        self.changed.subscribe()
    }
    pub(super) fn admit(&self, limit: usize) -> Result<u64, &'static str> {
        if self.active.len() >= limit {
            return Err("callback ticket capacity unavailable");
        }
        self.next.checked_add(1).ok_or("callback ordinal exhausted")
    }
    pub(super) fn insert(&mut self, id: u64, operation: String, value: T) {
        self.next = id;
        self.operations.insert(operation, id);
        self.active.insert(id, value);
    }
    pub(super) fn retire(&mut self, id: u64, operation: &str) -> Option<T> {
        let value = self.active.remove(&id)?;
        if self.operations.get(operation) == Some(&id) {
            self.operations.remove(operation);
        }
        self.retired.insert(id);
        self.notify_change();
        Some(value)
    }
}
#[cfg(test)]
#[path = "retained_callback_book_tests.rs"]
mod tests;
