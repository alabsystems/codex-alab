// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Preserve delivery identity without replaying publication callbacks.
use super::*;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "state", deny_unknown_fields)]
pub(crate) enum ExecPublication {
    Consumed,
    Pending {
        sender: ThreadId,
        scope_id: uuid::Uuid,
    },
    QueuedResult {
        sender: ThreadId,
        scope_id: uuid::Uuid,
    },
}

// Decoding prepares data only. Binding this marker and creating its receipt
// must wait for the actual destination runtime's accepted input installation.
struct RestoredQueuedResult;

impl MessageDelivery {
    pub(crate) fn capture_exec(&self) -> Result<(usize, ExecPublication), String> {
        let publication = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let saved = match publication.as_ref() {
            None => ExecPublication::Consumed,
            Some(publication) => {
                // Contributors may retain arbitrary ExtensionData at publication.
                // A host must provide a restoration protocol before that data can
                // cross exec; never turn a live delivery into historical mail.
                if !publication.observers.is_empty() {
                    return Err("message publication has nonportable lifecycle observers".into());
                }
                let sender = publication.input.sender;
                let scope_id = uuid::Uuid::parse_str(publication.input.data.level_id())
                    .map_err(|_| "message publication has a nonportable scope identity")?;
                if publication.result_write.is_some() {
                    if !self.has_exec_result_grant(publication) {
                        return Err(
                            "terminal result is awaiting its source writer's confirmation".into(),
                        );
                    }
                    ExecPublication::QueuedResult { sender, scope_id }
                } else if publication
                    .input
                    .data
                    .get::<RestoredQueuedResult>()
                    .is_some()
                {
                    return Err(
                        "queued terminal result has not reached its destination runtime".into(),
                    );
                } else {
                    ExecPublication::Pending { sender, scope_id }
                }
            }
        };
        Ok((Arc::as_ptr(&self.0) as usize, saved))
    }

    #[cfg(test)]
    pub(crate) fn restore_legacy_pending_for_test(sender: ThreadId) -> Self {
        Self::restore_exec(ExecPublication::Pending {
            sender,
            scope_id: uuid::Uuid::now_v7(),
        })
    }

    pub(crate) fn restore_exec(saved: ExecPublication) -> Self {
        let publication = match saved {
            ExecPublication::Consumed => None,
            ExecPublication::Pending { sender, scope_id }
            | ExecPublication::QueuedResult { sender, scope_id } => {
                let data = ExtensionData::new(scope_id.to_string());
                if matches!(saved, ExecPublication::QueuedResult { .. }) {
                    data.insert(RestoredQueuedResult);
                }
                Some(MessagePublication {
                    input: MessageLifecycleInput { sender, data },
                    observers: Box::default(),
                    result_write: None,
                })
            }
        };
        Self(Arc::new(Mutex::new(publication)))
    }

    pub(crate) fn is_restored_queued_result(&self) -> bool {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .is_some_and(|publication| {
                publication
                    .input
                    .data
                    .get::<RestoredQueuedResult>()
                    .is_some()
            })
    }

    /// All fallible owner checks precede the first increment. Ordered locks
    /// serialize concurrent installation of frames sharing an original delivery.
    pub(crate) fn bind_exec_results(
        mut deliveries: Vec<Self>,
        count: &Arc<AtomicUsize>,
    ) -> Result<(), String> {
        deliveries.sort_unstable_by_key(|delivery| Arc::as_ptr(&delivery.0));
        deliveries.dedup_by(|left, right| Arc::ptr_eq(&left.0, &right.0));
        let mut states = deliveries
            .iter()
            .map(|delivery| delivery.0.lock().unwrap_or_else(PoisonError::into_inner))
            .collect::<Vec<_>>();
        for state in &states {
            if let Some(publication) = state.as_ref()
                && let Some(receipt) = publication.result_write.as_ref()
                && !Arc::ptr_eq(&receipt.0, count)
            {
                return Err(
                    "queued terminal result belongs to a different destination runtime".into(),
                );
            }
        }
        for state in &mut states {
            if let Some(publication) = state.as_mut()
                && publication
                    .input
                    .data
                    .get::<RestoredQueuedResult>()
                    .is_some()
                && publication.result_write.is_none()
            {
                count.fetch_add(1, Ordering::Relaxed);
                publication.result_write = Some(ResultWrite(Arc::clone(count)));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;

    #[derive(Default)]
    struct Observer(AtomicUsize);
    impl MessageLifecycleContributor for Observer {
        fn on_publish(&self, _: &MessageLifecycleInput) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
        fn on_consumed(&self, _: &MessageLifecycleInput, _: ThreadId) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn observer_state_is_not_silently_discarded_or_republished() {
        let observer = Arc::new(Observer::default());
        let observers: Vec<Arc<dyn MessageLifecycleContributor>> = vec![observer.clone()];
        let delivery = MessageDelivery::publish(ThreadId::new(), &observers, None).unwrap();
        assert_eq!(observer.0.load(Ordering::SeqCst), 1);
        assert!(delivery.capture_exec().is_err());
        assert_eq!(observer.0.load(Ordering::SeqCst), 1);
        delivery.consumed(ThreadId::new());
        assert_eq!(observer.0.load(Ordering::SeqCst), 2);
        assert!(matches!(
            delivery.capture_exec().unwrap().1,
            ExecPublication::Consumed
        ));
    }
    #[test]
    fn capture_cannot_replace_a_terminal_result_receipt() {
        let count = Arc::new(AtomicUsize::new(0));
        let delivery = MessageDelivery::publish(ThreadId::new(), &[], Some(&count)).unwrap();
        assert!(delivery.capture_exec().is_err());
        assert!(delivery.awaits_result_write());
        let receipt = delivery.consumed(ThreadId::new()).unwrap();
        assert_eq!(count.load(Ordering::Acquire), 1);
        assert!(delivery.consumed(ThreadId::new()).is_none());
        receipt.confirm();
        assert_eq!(count.load(Ordering::Acquire), 0);
        assert_eq!(
            delivery.capture_exec().unwrap().1,
            ExecPublication::Consumed
        );
    }

    #[test]
    fn restored_delivery_keeps_its_scope_identity_and_consumes_all_its_clones_once() {
        let original = MessageDelivery::restore_legacy_pending_for_test(ThreadId::new());
        let (_, saved) = original.capture_exec().unwrap();
        let before = serde_json::to_value(&saved).unwrap();
        let restored =
            MessageDelivery::restore_exec(serde_json::from_value(before.clone()).unwrap());
        assert_eq!(
            serde_json::to_value(restored.capture_exec().unwrap().1).unwrap(),
            before
        );
        let shared = restored.clone();
        restored.consumed(ThreadId::new());
        assert!(matches!(
            shared.capture_exec().unwrap().1,
            ExecPublication::Consumed
        ));
        assert!(matches!(
            original.capture_exec().unwrap().1,
            ExecPublication::Pending { .. }
        ));
    }
}
