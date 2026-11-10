// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Atomic control-plane selection of real contexts. This creates no native
//! source, credential cache, readiness, or permission to reopen admission.
use arc_swap::ArcSwap;
use std::collections::BTreeMap;
use std::io;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use tokio::sync::Notify;

pub struct Group<M> {
    contexts: ArcSwap<BTreeMap<u64, Arc<M>>>,
    next: AtomicU64,
    changed: Notify,
}
impl<M> Default for Group<M> {
    fn default() -> Self {
        Self {
            contexts: ArcSwap::from_pointee(BTreeMap::new()),
            next: AtomicU64::new(0),
            changed: Notify::new(),
        }
    }
}
pub struct Host<M>(Arc<Handle<M>>);
struct Handle<M> {
    group: Arc<Group<M>>,
    id: u64,
    closed: AtomicBool,
}
impl<M> Clone for Host<M> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<M> std::fmt::Debug for Host<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ContextHost(<control-plane>)")
    }
}
pub struct Replacement<'a, M> {
    pub host: &'a Host<M>,
    pub expected: &'a Arc<M>,
    pub next: Arc<M>,
}
fn unavailable() -> io::Error {
    io::Error::other("source context selection unavailable")
}
impl<M> Group<M> {
    pub fn register(self: &Arc<Self>, initial: Arc<M>) -> io::Result<Host<M>> {
        let id = self
            .next
            .try_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                value.checked_add(1)
            })
            .map_err(|_| unavailable())?;
        loop {
            let observed = self.contexts.load_full();
            let mut next = (*observed).clone();
            if next.insert(id, initial.clone()).is_some() {
                return Err(unavailable());
            }
            let previous = self.contexts.compare_and_swap(&observed, Arc::new(next));
            if Arc::ptr_eq(&previous, &observed) {
                break;
            }
        }
        Ok(Host(Arc::new(Handle {
            group: self.clone(),
            id,
            closed: AtomicBool::new(false),
        })))
    }
    /// The caller must already own actual ACK/resource/admission proofs. This
    /// is one local pointer transaction, never a native publication or grant.
    pub fn publish(self: &Arc<Self>, replacements: &[Replacement<'_, M>]) -> io::Result<()> {
        if replacements.is_empty() {
            return Err(unavailable());
        }
        loop {
            let observed = self.contexts.load_full();
            let mut next = (*observed).clone();
            let mut seen = std::collections::BTreeSet::new();
            for replacement in replacements {
                let handle = &replacement.host.0;
                if !Arc::ptr_eq(&handle.group, self)
                    || handle.closed.load(Ordering::Acquire)
                    || !seen.insert(handle.id)
                    || !observed
                        .get(&handle.id)
                        .is_some_and(|actual| Arc::ptr_eq(actual, replacement.expected))
                {
                    return Err(unavailable());
                }
                next.insert(handle.id, replacement.next.clone());
            }
            let previous = self.contexts.compare_and_swap(&observed, Arc::new(next));
            if Arc::ptr_eq(&previous, &observed) {
                self.changed.notify_waiters();
                return Ok(());
            }
        }
    }
}
impl<M> Host<M> {
    /// Preserve genuine metadata when registration cannot issue a live host.
    /// The isolated, permanently closed handle cannot join any publication.
    pub fn closed(initial: Arc<M>) -> Self {
        let group = Arc::new(Group {
            contexts: ArcSwap::from_pointee(BTreeMap::from([(0, initial)])),
            next: AtomicU64::new(1),
            changed: Notify::new(),
        });
        Self(Arc::new(Handle {
            group,
            id: 0,
            closed: AtomicBool::new(true),
        }))
    }

    /// Metadata selection is always a real initialized context. Existing
    /// references keep the old context; no cache is transplanted into it.
    pub fn metadata_current(&self) -> io::Result<Arc<M>> {
        self.0
            .group
            .contexts
            .load()
            .get(&self.0.id)
            .cloned()
            .ok_or_else(unavailable)
    }
    pub fn capture(&self) -> io::Result<Arc<M>> {
        if self.0.closed.load(Ordering::Acquire) {
            return Err(unavailable());
        }
        let current = self.metadata_current()?;
        if self.0.closed.load(Ordering::Acquire) {
            return Err(unavailable());
        }
        Ok(current)
    }
    pub fn close(&self) {
        self.0.closed.store(true, Ordering::Release);
        self.0.group.changed.notify_waiters();
    }
    pub async fn changed(&self, previous: &Arc<M>) -> io::Result<()> {
        loop {
            let changed = self.0.group.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            let current = self.capture()?;
            if !Arc::ptr_eq(&current, previous) {
                return Ok(());
            }
            changed.await;
        }
    }
}
impl<M> Drop for Handle<M> {
    fn drop(&mut self) {
        loop {
            let observed = self.group.contexts.load_full();
            let mut next = (*observed).clone();
            next.remove(&self.id);
            let previous = self
                .group
                .contexts
                .compare_and_swap(&observed, Arc::new(next));
            if Arc::ptr_eq(&previous, &observed) {
                self.group.changed.notify_waiters();
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future;
    use std::task::Poll;

    #[tokio::test]
    async fn exhausted_registration_closed_host_keeps_only_genuine_metadata() {
        let group = Arc::new(Group::default());
        let original = Arc::new("original");
        let next = Arc::new("next");
        group.next.store(u64::MAX, Ordering::Release);
        assert!(group.register(original.clone()).is_err());
        assert!(group.contexts.load().is_empty());
        let closed = Host::closed(original.clone());
        assert!(Arc::ptr_eq(&closed.metadata_current().unwrap(), &original));
        assert!(closed.capture().is_err());
        assert!(closed.changed(&original).await.is_err());
        assert!(
            group
                .publish(&[Replacement {
                    host: &closed,
                    expected: &original,
                    next: next.clone(),
                }])
                .is_err()
        );
        assert!(
            closed
                .0
                .group
                .publish(&[Replacement {
                    host: &closed,
                    expected: &original,
                    next,
                }])
                .is_err()
        );
        let observer = closed.clone();
        closed.close();
        drop(closed);
        assert!(observer.capture().is_err());
        assert!(Arc::ptr_eq(
            &observer.metadata_current().unwrap(),
            &original
        ));
        let private = observer.0.group.clone();
        drop(observer);
        assert!(private.contexts.load().is_empty());
    }

    #[test]
    fn missing_registered_context_refuses_metadata_and_capture() {
        let group = Arc::new(Group::default());
        let host = group.register(Arc::new("original")).unwrap();
        group.contexts.store(Arc::new(BTreeMap::new()));
        assert!(host.metadata_current().is_err());
        assert!(host.capture().is_err());
    }

    #[test]
    fn publication_preserves_captured_context_and_rejects_partial_stale_batch() {
        let group = Arc::new(Group::default());
        let a = Arc::new("a");
        let other = Arc::new("other");
        let b = Arc::new("b");
        let first = group.register(a.clone()).unwrap();
        let second = group.register(other.clone()).unwrap();
        let captured = first.capture().unwrap();
        assert!(
            group
                .publish(&[
                    Replacement {
                        host: &first,
                        expected: &a,
                        next: b.clone()
                    },
                    Replacement {
                        host: &second,
                        expected: &a,
                        next: b.clone()
                    },
                ])
                .is_err()
        );
        assert!(Arc::ptr_eq(&first.capture().unwrap(), &a));
        assert!(Arc::ptr_eq(&second.capture().unwrap(), &other));
        group
            .publish(&[
                Replacement {
                    host: &first,
                    expected: &a,
                    next: b.clone(),
                },
                Replacement {
                    host: &second,
                    expected: &other,
                    next: b.clone(),
                },
            ])
            .unwrap();
        let snapshot = group.contexts.load_full();
        assert!(Arc::ptr_eq(snapshot.get(&first.0.id).unwrap(), &b));
        assert!(Arc::ptr_eq(snapshot.get(&second.0.id).unwrap(), &b));
        assert!(Arc::ptr_eq(&captured, &a));
        assert!(
            group
                .publish(&[Replacement {
                    host: &first,
                    expected: &a,
                    next: other
                }])
                .is_err()
        );
    }

    #[tokio::test]
    async fn pointer_change_wakes_with_equal_data_and_closed_capture_refuses() {
        let group = Arc::new(Group::default());
        let a = Arc::new(7);
        let b = Arc::new(7);
        let host = group.register(a.clone()).unwrap();
        let changed = host.changed(&a);
        tokio::pin!(changed);
        assert!(
            std::future::poll_fn(|cx| Poll::Ready(changed.as_mut().poll(cx).is_pending())).await
        );
        group
            .publish(&[Replacement {
                host: &host,
                expected: &a,
                next: b.clone(),
            }])
            .unwrap();
        changed.await.unwrap();
        host.close();
        assert!(host.capture().is_err());
        assert!(host.changed(&b).await.is_err());
        assert!(Arc::ptr_eq(&host.metadata_current().unwrap(), &b));
        assert!(
            group
                .publish(&[Replacement {
                    host: &host,
                    expected: &b,
                    next: a.clone()
                }])
                .is_err()
        );
    }
}
