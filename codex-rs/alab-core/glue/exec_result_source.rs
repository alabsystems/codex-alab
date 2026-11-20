// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Prove that every unconfirmed source writer is still exclusively owned by
//! the actual sealed queues. A claimed append has no transferable receipt.
use super::*;
use std::collections::HashMap;
use std::sync::Weak;

type Identity = Weak<Mutex<Option<MessagePublication>>>;

pub(crate) struct ExecResultGrant {
    counter: Arc<AtomicUsize>,
    original: HashMap<usize, Identity>,
    late: AtomicUsize,
}

struct SourceResult(Weak<ExecResultGrant>);

impl ExecResultGrant {
    fn census<'a>(
        counter: &Arc<AtomicUsize>,
        messages: impl Iterator<Item = &'a PendingAgentMessage>,
    ) -> Result<HashMap<usize, Identity>, String> {
        let mut candidates = HashMap::new();
        for delivery in messages.filter_map(|mail| mail.delivery.as_ref()) {
            let publication = delivery.0.lock().unwrap_or_else(PoisonError::into_inner);
            let Some(publication) = publication.as_ref() else {
                continue;
            };
            let Some(receipt) = &publication.result_write else {
                continue;
            };
            if !Arc::ptr_eq(&receipt.0, counter)
                || !publication.observers.is_empty()
                || uuid::Uuid::parse_str(publication.input.data.level_id()).is_err()
            {
                return Err(
                    "terminal result has a foreign writer or nonportable publication".into(),
                );
            }
            let (_, aliases) = candidates
                .entry(Arc::as_ptr(&delivery.0) as usize)
                .or_insert((delivery, 0usize));
            *aliases += 1;
        }
        if candidates.len() != counter.load(Ordering::Acquire)
            || candidates
                .values()
                .any(|(delivery, aliases)| Arc::strong_count(&delivery.0) != *aliases)
        {
            return Err("terminal result is awaiting its source writer's confirmation".into());
        }
        Ok(candidates
            .into_iter()
            .map(|(id, (delivery, _))| (id, Arc::downgrade(&delivery.0)))
            .collect())
    }

    pub(crate) fn prepare<'a>(
        counter: &Arc<AtomicUsize>,
        messages: impl Iterator<Item = &'a PendingAgentMessage>,
    ) -> Result<Arc<Self>, String> {
        let original = Self::census(counter, messages)?;
        let grant = Arc::new(Self {
            counter: Arc::clone(counter),
            original,
            late: AtomicUsize::default(),
        });
        for weak in grant.original.values() {
            let delivery = weak.upgrade().ok_or("source queued result disappeared")?;
            let publication = delivery.lock().unwrap_or_else(PoisonError::into_inner);
            let publication = publication
                .as_ref()
                .ok_or("source queued result was claimed")?;
            publication
                .input
                .data
                .insert(SourceResult(Arc::downgrade(&grant)));
        }
        Ok(grant)
    }

    pub(crate) fn recheck<'a>(
        &self,
        messages: impl Iterator<Item = &'a PendingAgentMessage>,
    ) -> Result<(), String> {
        let actual = Self::census(&self.counter, messages)?;
        if actual.len() != self.original.len()
            || actual.iter().any(|(id, delivery)| {
                self.original
                    .get(id)
                    .is_none_or(|original| !Weak::ptr_eq(original, delivery))
            })
        {
            return Err("source terminal result ownership changed during preparation".into());
        }
        Ok(())
    }

    pub(crate) fn accounts_for(&self, counter: &Arc<AtomicUsize>) -> bool {
        Arc::ptr_eq(&self.counter, counter)
            && self.original.len() + self.late.load(Ordering::Acquire)
                == counter.load(Ordering::Acquire)
    }

    /// The caller owns the committed custody lock and verified the immutable
    /// original edges and lack of observers before asking for this publication.
    pub(crate) fn publish_late(
        self: &Arc<Self>,
        publish: impl FnOnce() -> Option<MessageDelivery>,
    ) -> Result<MessageDelivery, String> {
        let delivery = publish().ok_or("terminal result publication has no delivery")?;
        {
            let publication = delivery.0.lock().unwrap_or_else(PoisonError::into_inner);
            let publication = publication
                .as_ref()
                .ok_or("terminal result publication was already claimed")?;
            if !publication
                .result_write
                .as_ref()
                .is_some_and(|receipt| Arc::ptr_eq(&self.counter, &receipt.0))
            {
                return Err("terminal result publication has no original source writer".into());
            }
            publication
                .input
                .data
                .insert(SourceResult(Arc::downgrade(self)));
        }
        self.late.fetch_add(1, Ordering::Release);
        Ok(delivery)
    }
}

impl MessageDelivery {
    pub(super) fn has_exec_result_grant(&self, publication: &MessagePublication) -> bool {
        publication
            .input
            .data
            .get::<SourceResult>()
            .and_then(|tag| tag.0.upgrade())
            .zip(publication.result_write.as_ref())
            .is_some_and(|(grant, receipt)| Arc::ptr_eq(&grant.counter, &receipt.0))
    }
}

#[cfg(test)]
impl MessageDelivery {
    pub(crate) fn original_result_identity(&self) -> Option<(ThreadId, String)> {
        let publication = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let p = publication.as_ref()?;
        p.result_write.as_ref()?;
        Some((p.input.sender, p.input.data.level_id().to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_protocol::AgentPath;

    fn mail(counter: &Arc<AtomicUsize>) -> PendingAgentMessage {
        PendingAgentMessage::new(
            InterAgentCommunication::new(
                AgentPath::root(),
                AgentPath::root(),
                Vec::new(),
                "original result".into(),
                false,
            ),
            MessageDelivery::publish(ThreadId::new(), &[], Some(counter)),
        )
    }

    #[test]
    fn missing_late_publication_does_not_create_a_source_writer() {
        let count = Arc::new(AtomicUsize::default());
        let grant = ExecResultGrant::prepare(&count, std::iter::empty()).unwrap();
        assert_eq!(
            grant.publish_late(|| None).unwrap_err(),
            "terminal result publication has no delivery"
        );
        assert_eq!(count.load(Ordering::Acquire), 0);
        assert_eq!(grant.late.load(Ordering::Acquire), 0);
        assert!(grant.accounts_for(&count));
    }

    #[test]
    fn claimed_or_foreign_late_publication_keeps_its_actual_writer_unconfirmed() {
        let count = Arc::new(AtomicUsize::default());
        let grant = ExecResultGrant::prepare(&count, std::iter::empty()).unwrap();
        let sender = ThreadId::new();
        let delivery = MessageDelivery::publish(sender, &[], Some(&count)).unwrap();
        let write = delivery.consumed(sender).unwrap();
        assert_eq!(
            grant.publish_late(|| Some(delivery)).unwrap_err(),
            "terminal result publication was already claimed"
        );
        assert_eq!(grant.late.load(Ordering::Acquire), 0);
        assert_eq!(count.load(Ordering::Acquire), 1);
        assert!(!grant.accounts_for(&count));
        write.confirm();
        assert!(grant.accounts_for(&count));

        let foreign = Arc::new(AtomicUsize::default());
        let delivery = MessageDelivery::publish(sender, &[], Some(&foreign)).unwrap();
        let alias = delivery.clone();
        assert_eq!(
            grant.publish_late(|| Some(delivery)).unwrap_err(),
            "terminal result publication has no original source writer"
        );
        assert!(alias.capture_exec().is_err());
        assert_eq!(foreign.load(Ordering::Acquire), 1);
        assert_eq!(grant.late.load(Ordering::Acquire), 0);
        assert!(grant.accounts_for(&count));
        alias.consumed(sender).unwrap().confirm();
        assert_eq!(foreign.load(Ordering::Acquire), 0);
    }

    #[test]
    fn late_publication_retains_the_original_delivery_until_its_writer_confirms() {
        let count = Arc::new(AtomicUsize::default());
        let grant = ExecResultGrant::prepare(&count, std::iter::empty()).unwrap();
        let sender = ThreadId::new();
        let delivery = grant
            .publish_late(|| MessageDelivery::publish(sender, &[], Some(&count)))
            .unwrap();
        assert!(matches!(
            delivery.capture_exec().unwrap().1,
            ExecPublication::QueuedResult { sender: actual, .. } if actual == sender
        ));
        assert_eq!(grant.late.load(Ordering::Acquire), 1);
        assert!(grant.accounts_for(&count));
        let receipt = delivery.consumed(sender).unwrap();
        assert!(delivery.consumed(sender).is_none());
        assert_eq!(count.load(Ordering::Acquire), 1);
        receipt.confirm();
        assert_eq!(count.load(Ordering::Acquire), 0);
    }

    #[test]
    fn queued_result_source_requires_all_physical_aliases_and_its_actual_writer() {
        let count = Arc::new(AtomicUsize::default());
        let foreign = Arc::new(AtomicUsize::default());
        let original = mail(&count);
        let alias = original.clone();
        assert!(ExecResultGrant::prepare(&count, [&original].into_iter()).is_err());
        assert!(ExecResultGrant::prepare(&foreign, [&original, &alias].into_iter()).is_err());
        let grant = ExecResultGrant::prepare(&count, [&original, &alias].into_iter()).unwrap();
        let delivery = original.delivery.as_ref().unwrap();
        assert!(matches!(
            delivery.capture_exec().unwrap().1,
            ExecPublication::QueuedResult { .. }
        ));
        drop(grant);
        assert!(
            delivery.capture_exec().is_err(),
            "revocation must leave the original receipt attached"
        );
        let claimed = alias.consumed(ThreadId::new()).unwrap();
        assert!(ExecResultGrant::prepare(&count, [&original, &alias].into_iter()).is_err());
        assert_eq!(count.load(Ordering::Acquire), 1);
        claimed.confirm();
        assert_eq!(count.load(Ordering::Acquire), 0);
    }
}
